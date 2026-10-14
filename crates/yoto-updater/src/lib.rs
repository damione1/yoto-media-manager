//! Signed, bounded desktop updates. This client never uses the user's Yoto credentials.
mod install;
mod network;
#[cfg(test)]
mod tests;
mod version;
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

/// The version shown to people: the release tag's `X.Y.Z`, or `nightly` for a build made
/// without `YOTO_APP_VERSION` (local and development builds).
pub const CURRENT_VERSION: &str = match option_env!("YOTO_APP_VERSION") {
    Some(value) if !value.is_empty() => value,
    _ => version::NIGHTLY,
};
/// What the updater compares against the published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppVersion {
    Release(Version),
    /// A local or development build: never reported as up to date, never replaced in place.
    Nightly,
}
impl AppVersion {
    pub fn current() -> Self {
        Self::from_build(CURRENT_VERSION)
    }
    fn from_build(text: &str) -> Self {
        match version::parse_release(text) {
            Some((major, minor, patch)) => Self::Release(Version::new(major, minor, patch)),
            None => Self::Nightly,
        }
    }
    pub fn is_nightly(&self) -> bool {
        matches!(self, Self::Nightly)
    }
}
const PUBLIC_KEY: &str = include_str!("../../../distribution/updater.pub");
const MANIFEST_URL: &str =
    "https://github.com/damione1/yoto-media-manager/releases/latest/download/latest.json";
const RELEASE_DOWNLOADS: &str = "https://github.com/damione1/yoto-media-manager/releases/download";
/// The application's file name: `YotoMediaManager.app` on macOS, the
/// `YotoMediaManager` folder on Windows, and the prefix of update archives.
const APP_NAME: &str = "YotoMediaManager";
/// The name releases used before the rename. Archives and installations still carrying
/// it are accepted during the transition; a macOS `Yoto.app` is renamed when updated.
const LEGACY_APP_NAME: &str = "Yoto";
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
    /// Update archive file names a release may publish, current name first.
    fn archive_names(self, version: &Version) -> [String; 2] {
        let (platform, extension) = match self {
            Self::MacArm => ("darwin-aarch64", "tar.gz"),
            Self::MacIntel => ("darwin-x86_64", "tar.gz"),
            Self::Windows => ("windows-x86_64", "zip"),
        };
        [APP_NAME, LEGACY_APP_NAME].map(|name| format!("{name}-{version}-{platform}.{extension}"))
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
impl Release {
    /// The archive file name, one of `Platform::archive_names` since `select_release`
    /// accepted its URL. The signature's trusted comment must name this same file.
    fn archive_name(&self) -> &str {
        self.artifact
            .url
            .rsplit_once('/')
            .map_or("", |(_, name)| name)
    }
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
    version: AppVersion,
    prepared: std::sync::Mutex<Option<Arc<PreparedUpdate>>>,
    state: watch::Sender<Arc<UpdateState>>,
    gate: Mutex<()>,
    cancel: AtomicBool,
    client: reqwest::Client,
}
impl Updater {
    pub fn new() -> Result<Arc<Self>, UpdateError> {
        Self::for_version(AppVersion::current())
    }
    fn for_version(version: AppVersion) -> Result<Arc<Self>, UpdateError> {
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
            version,
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
        let result = network::check(&self.client, &self.version).await;
        match result {
            Ok(Some(release)) => self.set(UpdateState::Available(Arc::new(release))),
            // A nightly is never "up to date": with no published release, nothing is claimed.
            Ok(None) if self.version.is_nightly() => self.set(UpdateState::Idle),
            Ok(None) => self.set(UpdateState::Current),
            Err(error) => self.set(UpdateState::Failed(Arc::new(error))),
        }
    }
    pub async fn download(&self, release: Arc<Release>) {
        let Ok(_gate) = self.gate.try_lock() else {
            return;
        };
        if self.version.is_nightly() {
            // A nightly may run from a source tree or a development bundle with its own
            // identity: replacing it in place could clobber either, so the release is
            // installed by hand.
            self.failed(UpdateError::Install(
                "Development builds are not replaced by the updater".into(),
            ));
            return;
        }
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
    current: &AppVersion,
    platform: Platform,
) -> Result<Option<Release>, UpdateError> {
    if !manifest.version.pre.is_empty() || !manifest.version.build.is_empty() {
        return Err(UpdateError::Manifest(
            "Only stable semantic versions are supported".into(),
        ));
    }
    // Any published release is offered to a nightly; it is never "current" nor a downgrade
    // that happens on its own, since installing it stays a manual step.
    if let AppVersion::Release(current) = current
        && manifest.version <= *current
    {
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
    let folder = format!("{RELEASE_DOWNLOADS}/v{}/", manifest.version);
    let accepted = platform.archive_names(&manifest.version);
    if !artifact
        .url
        .strip_prefix(&folder)
        .is_some_and(|name| accepted.iter().any(|candidate| candidate == name))
    {
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
