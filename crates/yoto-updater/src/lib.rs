//! Signed, bounded desktop updates. This client never uses the user's Yoto credentials.
mod install;
mod network;
#[cfg(test)]
mod tests;
pub use install::{PreparedUpdate, apply_pending_update};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Mutex, watch};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const PUBLIC_KEY: &str = include_str!("../../../distribution/updater.pub");
const MANIFEST_URL: &str =
    "https://github.com/damione1/yoto-media-manager/releases/latest/download/latest.json";
const MAX_ARCHIVE: u64 = 1_500_000_000;
const MAX_EXPANDED: u64 = 4_000_000_000;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Platform {
    #[serde(rename = "darwin-aarch64")]
    MacArm,
    #[serde(rename = "darwin-x86_64")]
    MacIntel,
    #[serde(rename = "windows-x86_64")]
    Windows,
}
impl Platform {
    pub fn current() -> Result<Self, UpdateError> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => Ok(Self::MacArm),
            ("macos", "x86_64") => Ok(Self::MacIntel),
            ("windows", "x86_64") => Ok(Self::Windows),
            _ => Err(UpdateError::UnsupportedPlatform),
        }
    }
    fn archive_name(self, version: &Version) -> String {
        let (platform, extension) = match self {
            Self::MacArm => ("darwin-aarch64", "tar.gz"),
            Self::MacIntel => ("darwin-x86_64", "tar.gz"),
            Self::Windows => ("windows-x86_64", "zip"),
        };
        format!("Yoto-{version}-{platform}.{extension}")
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: Version,
    pub notes: String,
    pub platforms: BTreeMap<Platform, Artifact>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub url: String,
    pub signature: String,
    pub size: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: Version,
    pub notes: String,
    pub platform: Platform,
    artifact: Artifact,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateState {
    Idle,
    Checking,
    Current,
    Available(Arc<Release>),
    Downloading {
        release: Arc<Release>,
        received: u64,
        total: u64,
    },
    Verifying(Arc<Release>),
    Ready {
        release: Arc<Release>,
        issue: Option<String>,
    },
    Installing,
    Failed(Arc<UpdateError>),
}
#[derive(Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum UpdateError {
    #[error("Unable to contact the update server: {0}")]
    Network(String),
    #[error("Invalid update manifest: {0}")]
    Manifest(String),
    #[error("The update signature is invalid.")]
    Signature,
    #[error("The update archive is unsafe: {0}")]
    Archive(String),
    #[error("The update could not be installed: {0}")]
    Install(String),
    #[error("Updates are not available for this platform.")]
    UnsupportedPlatform,
    #[error("The download was cancelled.")]
    Cancelled,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
pub enum UpdateAction {
    Check,
    Download,
    Cancel,
    State,
}
pub struct Updater {
    prepared: std::sync::Mutex<Option<Arc<PreparedUpdate>>>,
    state: watch::Sender<Arc<UpdateState>>,
    gate: Mutex<()>,
    cancel: AtomicBool,
    client: reqwest::Client,
}
impl Updater {
    pub fn new() -> Result<Arc<Self>, UpdateError> {
        let client = reqwest::Client::builder()
            .user_agent(format!("YotoMediaManager/{CURRENT_VERSION}"))
            .https_only(true)
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(std::time::Duration::from_secs(600))
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| UpdateError::Network(e.to_string()))?;
        let (state, _) = watch::channel(Arc::new(UpdateState::Idle));
        Ok(Arc::new(Self {
            prepared: std::sync::Mutex::new(None),
            state,
            gate: Mutex::new(()),
            cancel: AtomicBool::new(false),
            client,
        }))
    }
    pub fn state(&self) -> Arc<UpdateState> {
        self.state.borrow().clone()
    }
    pub fn request(self: &Arc<Self>, action: UpdateAction) {
        match action {
            UpdateAction::Check => {
                let updater = self.clone();
                tokio::spawn(async move {
                    updater.check().await;
                });
            }
            UpdateAction::Download => {
                if let UpdateState::Available(release) = self.state().as_ref() {
                    let updater = self.clone();
                    let release = release.clone();
                    tokio::spawn(async move {
                        updater.download(release).await;
                    });
                }
            }
            UpdateAction::Cancel => self.cancel(),
            UpdateAction::State => {}
        }
    }
    pub fn prepared(&self) -> Result<Arc<PreparedUpdate>, UpdateError> {
        self.prepared
            .lock()
            .map_err(|e| UpdateError::Install(e.to_string()))?
            .clone()
            .ok_or_else(|| UpdateError::Install("No verified update is ready".into()))
    }
    pub fn subscribe(&self) -> watch::Receiver<Arc<UpdateState>> {
        self.state.subscribe()
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    fn set(&self, state: UpdateState) {
        self.state.send_replace(Arc::new(state));
    }
    pub async fn check(&self) {
        let Ok(_gate) = self.gate.try_lock() else {
            return;
        };
        // A fresh check must not discard a prepared download or installation.
        match self.state.borrow().as_ref() {
            UpdateState::Downloading { .. }
            | UpdateState::Verifying(_)
            | UpdateState::Ready { .. }
            | UpdateState::Installing => return,
            UpdateState::Idle
            | UpdateState::Checking
            | UpdateState::Current
            | UpdateState::Available(_)
            | UpdateState::Failed(_) => {}
        }
        self.set(UpdateState::Checking);
        let result = network::check(&self.client).await;
        match result {
            Ok(Some(release)) => self.set(UpdateState::Available(Arc::new(release))),
            Ok(None) => self.set(UpdateState::Current),
            Err(error) => self.set(UpdateState::Failed(Arc::new(error))),
        }
    }
    pub async fn download(&self, release: Arc<Release>) {
        let Ok(_gate) = self.gate.try_lock() else {
            return;
        };
        self.cancel.store(false, Ordering::SeqCst);
        self.set(UpdateState::Downloading {
            release: release.clone(),
            received: 0,
            total: release.artifact.size,
        });
        let result = network::download(self, release.clone()).await;
        match result {
            Ok(prepared) => match self.prepared.lock() {
                Ok(mut slot) => {
                    *slot = Some(Arc::new(prepared));
                    self.set(UpdateState::Ready {
                        release,
                        issue: None,
                    });
                }
                Err(error) => self.failed(UpdateError::Install(error.to_string())),
            },
            Err(UpdateError::Cancelled) => self.set(UpdateState::Available(release)),
            Err(error) => self.set(UpdateState::Failed(Arc::new(error))),
        }
    }
    pub fn install_blocked(&self, reason: String) {
        if let UpdateState::Ready { release, .. } = self.state().as_ref() {
            self.set(UpdateState::Ready {
                release: release.clone(),
                issue: Some(reason),
            });
        }
    }
    pub fn installing(&self) {
        self.set(UpdateState::Installing);
    }
    pub fn failed(&self, error: UpdateError) {
        self.set(UpdateState::Failed(Arc::new(error)));
    }
}
fn select_release(
    manifest: Manifest,
    current: &Version,
    platform: Platform,
) -> Result<Option<Release>, UpdateError> {
    if !manifest.version.pre.is_empty() || !manifest.version.build.is_empty() {
        return Err(UpdateError::Manifest(
            "Only stable semantic versions are supported".into(),
        ));
    }
    if manifest.version <= *current {
        return Ok(None);
    }
    let artifact = manifest
        .platforms
        .get(&platform)
        .cloned()
        .ok_or_else(|| UpdateError::Manifest("Missing platform artifact".into()))?;
    if artifact.size == 0 || artifact.size > MAX_ARCHIVE {
        return Err(UpdateError::Manifest("Invalid archive size".into()));
    }
    let expected = format!(
        "https://github.com/damione1/yoto-media-manager/releases/download/v{}/{}",
        manifest.version,
        platform.archive_name(&manifest.version)
    );
    if artifact.url != expected {
        return Err(UpdateError::Manifest(
            "Unexpected release download URL".into(),
        ));
    }
    Ok(Some(Release {
        version: manifest.version,
        notes: manifest.notes,
        platform,
        artifact,
    }))
}
fn verify_identity(signature: &str, expected: &str) -> Result<(), UpdateError> {
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(signature.trim())
        .map_err(|_| UpdateError::Signature)?;
    let text = std::str::from_utf8(&decoded).map_err(|_| UpdateError::Signature)?;
    let signature = minisign_verify::Signature::decode(text).map_err(|_| UpdateError::Signature)?;
    if signature.trusted_comment() != expected {
        return Err(UpdateError::Signature);
    }
    Ok(())
}
fn verify(bytes: &[u8], signature: &str, key: &str) -> Result<(), UpdateError> {
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(signature.trim())
        .map_err(|_| UpdateError::Signature)?;
    let text = std::str::from_utf8(&decoded).map_err(|_| UpdateError::Signature)?;
    let signature = minisign_verify::Signature::decode(text).map_err(|_| UpdateError::Signature)?;
    let key = minisign_verify::PublicKey::decode(key).map_err(|_| UpdateError::Signature)?;
    key.verify(bytes, &signature, false)
        .map_err(|_| UpdateError::Signature)
}
