//! Keeps the YouTube downloader (yt-dlp) current between application releases.
//!
//! The application bundles a pinned yt-dlp. When YouTube changes, a newer stable release
//! can be fetched from yt-dlp's official GitHub releases into an app-managed directory.
//! A download is never executed before its SHA2-256 digest matches the checksum file
//! published with the same release, and the bundled copy stays the fallback whenever the
//! managed one is missing, altered or fails to start.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Largest release binary accepted. Current builds weigh 20–40 MB.
pub const MAX_BINARY_BYTES: u64 = 200_000_000;
/// Largest checksum file accepted; the real one lists a few dozen assets.
pub const MAX_SUMS_BYTES: u64 = 64_000;
/// Largest release description accepted from the GitHub API.
pub const MAX_RELEASE_BYTES: u64 = 2_000_000;

/// A yt-dlp release version such as `2026.08.19` or `2026.08.19.1`, compared part by part.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ToolVersion(Vec<u32>);
impl ToolVersion {
    /// Read a stable release tag. Anything but three to four dot-separated numbers, the
    /// first being a four-digit year, is refused, so a tag can safely appear in a URL or
    /// a directory name.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let parts: Vec<&str> = text.split('.').collect();
        if !(3..=4).contains(&parts.len()) || parts[0].len() != 4 {
            return None;
        }
        parts
            .iter()
            .map(|part| {
                if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
                    None
                } else {
                    part.parse().ok()
                }
            })
            .collect::<Option<Vec<u32>>>()
            .map(Self)
    }
}
impl std::fmt::Display for ToolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, part) in self.0.iter().enumerate() {
            match index {
                0 => write!(f, "{part}")?,
                // Month and day keep their leading zero, as in the release tags.
                1 | 2 => write!(f, ".{part:02}")?,
                _ => write!(f, ".{part}")?,
            }
        }
        Ok(())
    }
}

/// Name of the single-file release asset for this platform, if yt-dlp publishes one.
pub fn release_asset() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("yt-dlp_macos")
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        Some("yt-dlp.exe")
    } else {
        None
    }
}

/// A SHA2-256 digest.
pub type Digest256 = [u8; 32];

/// Find `asset`'s digest in a `SHA2-256SUMS` file (`<hex>  <name>` per line, `*` marking
/// binary mode). The name must match exactly and appear once.
pub fn checksum_for(sums: &str, asset: &str) -> Option<Digest256> {
    let mut found = None;
    for line in sums.lines() {
        let line = line.trim_end_matches('\r');
        let Some((hex_digest, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let name = name.trim_start();
        let name = name.strip_prefix('*').unwrap_or(name);
        if name != asset {
            continue;
        }
        if found.is_some() || hex_digest.len() != 64 {
            return None;
        }
        let mut digest = [0u8; 32];
        hex::decode_to_slice(hex_digest, &mut digest).ok()?;
        found = Some(digest);
    }
    found
}

/// SHA2-256 of a file, read in chunks.
pub fn file_digest(path: &std::path::Path) -> std::io::Result<Digest256> {
    use std::io::Read;
    let mut input = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hash.finalize().into())
}

/// Which copy of the YouTube tool imports use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolOrigin {
    /// The copy shipped inside the application.
    Bundled,
    /// A newer copy downloaded and verified by the application.
    Downloaded,
}
/// Why the last update attempt did not install a new version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolProblem {
    /// GitHub could not be reached or answered with an error.
    Network,
    /// The download did not match its published checksum, or no checksum was published.
    Integrity,
    /// The downloaded tool did not start or reported another version.
    Unusable,
    /// The tool could not be written to this computer.
    Storage,
    /// No release is published for this computer.
    Unsupported,
}
/// What the updater is doing or last did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolActivity {
    Idle,
    Checking,
    /// The tool in use is the latest stable release.
    Current,
    /// A newer version was installed and is now in use.
    Updated,
    Failed(ToolProblem),
}
/// The YouTube tool in use and the updater's state, as shown in « Compte ».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YoutubeToolStatus {
    /// Version of the tool imports use, when it could be read.
    pub version: Option<String>,
    pub origin: ToolOrigin,
    pub activity: ToolActivity,
    /// When the last update check finished, in seconds since the epoch.
    pub checked: Option<u64>,
}
/// What a client asks of the YouTube tool updater.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
pub enum YoutubeToolAction {
    /// Report the version in use and the updater's state.
    State,
    /// Look for a newer stable release now and install it when there is one.
    Update,
}

/// Record of the managed copy in use, rewritten atomically after each verified install.
#[derive(Debug, Serialize, Deserialize)]
struct Pointer {
    version: String,
    sha256: String,
}
const POINTER: &str = "current.json";
const LAST_CHECK: &str = "last-check";
/// Ask a tool for its version; `None` when it does not start or answers anything else.
pub type Smoke<'a> = &'a dyn Fn(&std::path::Path) -> Option<ToolVersion>;

/// Run `path --version` within a bounded time and read the release it reports.
pub fn smoke_test(path: &std::path::Path) -> Option<ToolVersion> {
    let mut command = crate::media::command(path);
    command.arg("--version");
    let output =
        crate::process::output(command, 4_000, std::time::Duration::from_secs(120)).ok()?;
    if !output.status.success() {
        return None;
    }
    ToolVersion::parse(&String::from_utf8_lossy(&output.stdout))
}

/// The managed copy recorded in `dir`, provided its bytes still match the recorded digest
/// and it starts and reports the recorded version.
pub fn load_managed(
    dir: &std::path::Path,
    asset: &str,
    smoke: Smoke,
) -> Option<(std::path::PathBuf, ToolVersion)> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(dir.join(POINTER))
        .ok()?
        .take(4_000)
        .read_to_string(&mut text)
        .ok()?;
    let pointer: Pointer = serde_json::from_str(&text).ok()?;
    let version = ToolVersion::parse(&pointer.version)?;
    // Only a validated tag names the directory, so the pointer cannot lead elsewhere.
    let path = dir.join(pointer.version.trim()).join(asset);
    if hex::encode(file_digest(&path).ok()?) != pointer.sha256 {
        return None;
    }
    (smoke(&path)? == version).then_some((path, version))
}

/// Make a verified download runnable: executable bit and, on macOS, no quarantine flag.
fn make_runnable(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(path.as_os_str().as_bytes())?;
        // SAFETY: both arguments are NUL-terminated strings that outlive the call.
        unsafe {
            libc::removexattr(name.as_ptr(), c"com.apple.quarantine".as_ptr(), 0);
        }
    }
    Ok(())
}

/// Install a downloaded release: check its digest, then make it runnable, check it starts
/// and reports `tag`, move it into place and only then switch the pointer. Returns the
/// installed path. Nothing is executed before the digest matches.
pub fn install(
    dir: &std::path::Path,
    asset: &str,
    tag: &str,
    download: tempfile::NamedTempFile,
    expected: &Digest256,
    smoke: Smoke,
) -> std::result::Result<std::path::PathBuf, ToolProblem> {
    let version = ToolVersion::parse(tag).ok_or(ToolProblem::Integrity)?;
    let tag = tag.trim();
    let actual = file_digest(download.path()).map_err(|_| ToolProblem::Storage)?;
    if &actual != expected {
        return Err(ToolProblem::Integrity);
    }
    let staging = tempfile::Builder::new()
        .prefix(".staging-")
        .tempdir_in(dir)
        .map_err(|_| ToolProblem::Storage)?;
    let staged = staging.path().join(asset);
    download
        .persist(&staged)
        .map_err(|_| ToolProblem::Storage)?;
    make_runnable(&staged).map_err(|_| ToolProblem::Storage)?;
    if smoke(&staged).as_ref() != Some(&version) {
        return Err(ToolProblem::Unusable);
    }
    let target = dir.join(tag);
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|_| ToolProblem::Storage)?;
    }
    std::fs::rename(staging.keep(), &target).map_err(|_| ToolProblem::Storage)?;
    let pointer = serde_json::to_vec(&Pointer {
        version: tag.into(),
        sha256: hex::encode(actual),
    })
    .map_err(|_| ToolProblem::Storage)?;
    let mut next = tempfile::NamedTempFile::new_in(dir).map_err(|_| ToolProblem::Storage)?;
    std::io::Write::write_all(&mut next, &pointer).map_err(|_| ToolProblem::Storage)?;
    next.as_file()
        .sync_all()
        .map_err(|_| ToolProblem::Storage)?;
    next.persist(dir.join(POINTER))
        .map_err(|_| ToolProblem::Storage)?;
    remove_others(dir, tag);
    Ok(target.join(asset))
}

/// Remove earlier versions and abandoned staging directories. A copy still running
/// (Windows) cannot be removed; it is retried next time.
fn remove_others(dir: &std::path::Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let stale = name.starts_with(".staging-")
            || (name != keep && ToolVersion::parse(&name).is_some())
            || (name.starts_with(".tmp") && entry.path().is_file());
        if stale {
            let path = entry.path();
            let _ = if path.is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            };
        }
    }
}

const RELEASE_API: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const RELEASE_DOWNLOADS: &str = "https://github.com/yt-dlp/yt-dlp/releases/download";
/// Automatic checks happen at most this often.
const DAILY: u64 = 24 * 60 * 60;
/// After an import fails because the tool is outdated, another automatic check waits this long.
const OUTDATED_PAUSE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Read a response body, refusing more than `limit` bytes.
async fn bounded(
    response: reqwest::Response,
    limit: u64,
) -> std::result::Result<Vec<u8>, ToolProblem> {
    let mut response = response
        .error_for_status()
        .map_err(|_| ToolProblem::Network)?;
    if response.content_length().is_some_and(|n| n > limit) {
        return Err(ToolProblem::Integrity);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ToolProblem::Network)? {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err(ToolProblem::Integrity);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Tag of the latest stable yt-dlp release.
async fn latest_tag(client: &reqwest::Client) -> std::result::Result<String, ToolProblem> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        #[serde(default)]
        draft: bool,
        #[serde(default)]
        prerelease: bool,
    }
    let response = client
        .get(RELEASE_API)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| ToolProblem::Network)?;
    let body = bounded(response, MAX_RELEASE_BYTES).await?;
    let release: Release = serde_json::from_slice(&body).map_err(|_| ToolProblem::Network)?;
    if release.draft || release.prerelease || ToolVersion::parse(&release.tag_name).is_none() {
        return Err(ToolProblem::Integrity);
    }
    Ok(release.tag_name.trim().into())
}

/// Download `asset` of release `tag` into `dir`, hashing it on the way, together with the
/// digest the release publishes for it. Nothing is verified or executed here.
async fn download(
    client: &reqwest::Client,
    dir: &std::path::Path,
    tag: &str,
    asset: &str,
) -> std::result::Result<(tempfile::NamedTempFile, Digest256), ToolProblem> {
    let sums = client
        .get(format!("{RELEASE_DOWNLOADS}/{tag}/SHA2-256SUMS"))
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|_| ToolProblem::Network)?;
    let sums = bounded(sums, MAX_SUMS_BYTES).await?;
    let expected =
        checksum_for(&String::from_utf8_lossy(&sums), asset).ok_or(ToolProblem::Integrity)?;
    let mut response = client
        .get(format!("{RELEASE_DOWNLOADS}/{tag}/{asset}"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| ToolProblem::Network)?;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BINARY_BYTES)
    {
        return Err(ToolProblem::Integrity);
    }
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|_| ToolProblem::Storage)?;
    let mut received = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|_| ToolProblem::Network)? {
        received += chunk.len() as u64;
        if received > MAX_BINARY_BYTES {
            return Err(ToolProblem::Integrity);
        }
        std::io::Write::write_all(&mut file, &chunk).map_err(|_| ToolProblem::Storage)?;
    }
    file.as_file()
        .sync_all()
        .map_err(|_| ToolProblem::Storage)?;
    Ok((file, expected))
}

/// What prompted an update check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// The once-a-day background check.
    Daily,
    /// An import failed because YouTube changed.
    Outdated,
    /// The person asked for it in « Compte ».
    Manual,
}
#[derive(Debug, Clone)]
struct Active {
    path: std::path::PathBuf,
    version: Option<ToolVersion>,
    origin: ToolOrigin,
}
#[derive(Debug, Clone)]
struct Progress {
    activity: ToolActivity,
    checked: Option<u64>,
    automatic: Option<std::time::Instant>,
}
/// Chooses the YouTube tool imports use and keeps it current.
pub struct YoutubeTool {
    dir: std::path::PathBuf,
    bundled: std::path::PathBuf,
    active: std::sync::RwLock<Active>,
    progress: std::sync::Mutex<Progress>,
    gate: tokio::sync::Mutex<()>,
    /// Automatic checks are off until the owning application starts them, so tests and
    /// diagnostic runs never reach the network on their own.
    automatic: std::sync::atomic::AtomicBool,
    /// Whether the bundled and managed copies were compared since start.
    activated: std::sync::atomic::AtomicBool,
    client: std::sync::OnceLock<reqwest::Client>,
}
impl YoutubeTool {
    /// `dir` holds downloaded copies; `bundled` is the copy shipped with the application.
    pub fn new(dir: std::path::PathBuf, bundled: std::path::PathBuf) -> Self {
        Self {
            dir,
            active: std::sync::RwLock::new(Active {
                path: bundled.clone(),
                version: None,
                origin: ToolOrigin::Bundled,
            }),
            bundled,
            progress: std::sync::Mutex::new(Progress {
                activity: ToolActivity::Idle,
                checked: None,
                automatic: None,
            }),
            gate: tokio::sync::Mutex::new(()),
            automatic: false.into(),
            activated: false.into(),
            client: std::sync::OnceLock::new(),
        }
    }
    /// Path of the tool imports should run now.
    pub fn path(&self) -> std::path::PathBuf {
        self.active
            .read()
            .map(|a| a.path.clone())
            .unwrap_or_else(|_| self.bundled.clone())
    }
    pub fn status(&self) -> YoutubeToolStatus {
        let active = self.active.read().ok().map(|a| a.clone());
        let progress = self.progress.lock().ok().map(|p| p.clone());
        YoutubeToolStatus {
            version: active
                .as_ref()
                .and_then(|a| a.version.as_ref())
                .map(ToString::to_string),
            origin: active.map_or(ToolOrigin::Bundled, |a| a.origin),
            activity: progress
                .as_ref()
                .map_or(ToolActivity::Idle, |p| p.activity.clone()),
            checked: progress.and_then(|p| p.checked),
        }
    }
    fn set_activity(&self, activity: ToolActivity, finished: bool) {
        if let Ok(mut progress) = self.progress.lock() {
            progress.activity = activity;
            if finished {
                progress.checked = Some(crate::now());
            }
        }
    }
    /// Choose between the bundled and the managed copy: the managed one only when it is
    /// intact, starts, and is newer than the bundled one.
    pub fn activate(&self, smoke: Smoke) {
        let bundled = smoke(&self.bundled);
        let managed = release_asset().and_then(|asset| load_managed(&self.dir, asset, smoke));
        let chosen = match managed {
            Some((path, version)) if bundled.as_ref().is_none_or(|b| version > *b) => Active {
                path,
                version: Some(version),
                origin: ToolOrigin::Downloaded,
            },
            Some(_) | None => Active {
                path: self.bundled.clone(),
                version: bundled,
                origin: ToolOrigin::Bundled,
            },
        };
        if let Ok(mut active) = self.active.write() {
            *active = chosen;
        }
        self.activated
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut progress) = self.progress.lock() {
            progress.checked = std::fs::read_to_string(self.dir.join(LAST_CHECK))
                .ok()
                .and_then(|t| t.trim().parse().ok());
        }
    }
    /// Pick the tool to use, then, when `daily`, check for a newer release once a day.
    /// Also allows the check that follows an « outdated » import failure.
    pub fn start(self: &std::sync::Arc<Self>, daily: bool) {
        self.automatic
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let tool = self.clone();
        tokio::spawn(async move {
            let chooser = tool.clone();
            let _ = tokio::task::spawn_blocking(move || chooser.activate(&smoke_test)).await;
            if daily {
                tool.update(Trigger::Daily).await;
            }
        });
    }
    /// Answer a client: report the status, or start an update in the background.
    pub fn request(self: &std::sync::Arc<Self>, action: YoutubeToolAction) -> YoutubeToolStatus {
        match action {
            YoutubeToolAction::State => {}
            YoutubeToolAction::Update => {
                self.set_activity(ToolActivity::Checking, false);
                let tool = self.clone();
                tokio::spawn(async move {
                    tool.update(Trigger::Manual).await;
                });
            }
        }
        self.status()
    }
    fn client(&self) -> std::result::Result<&reqwest::Client, ToolProblem> {
        if let Some(client) = self.client.get() {
            return Ok(client);
        }
        let client = reqwest::Client::builder()
            .user_agent(format!(
                "YotoMediaManager/{}",
                yoto_updater::CURRENT_VERSION
            ))
            .https_only(true)
            .redirect(reqwest::redirect::Policy::limited(5))
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|_| ToolProblem::Network)?;
        Ok(self.client.get_or_init(|| client))
    }
    /// Look for a newer stable release and install it. Returns the resulting activity;
    /// a check already running is waited for rather than repeated.
    pub async fn update(self: &std::sync::Arc<Self>, trigger: Trigger) -> ToolActivity {
        let _gate = self.gate.lock().await;
        let automatic = self.automatic.load(std::sync::atomic::Ordering::SeqCst);
        let skip =
            match trigger {
                Trigger::Manual => false,
                Trigger::Daily => {
                    !automatic
                        || std::fs::read_to_string(self.dir.join(LAST_CHECK))
                            .ok()
                            .and_then(|t| t.trim().parse::<u64>().ok())
                            .is_some_and(|at| crate::now().saturating_sub(at) < DAILY)
                }
                Trigger::Outdated => {
                    !automatic
                        || self.progress.lock().ok().is_none_or(|p| {
                            p.automatic.is_some_and(|at| at.elapsed() < OUTDATED_PAUSE)
                        })
                }
            };
        if skip {
            return self.status().activity;
        }
        if trigger == Trigger::Outdated
            && let Ok(mut progress) = self.progress.lock()
        {
            progress.automatic = Some(std::time::Instant::now());
        }
        self.set_activity(ToolActivity::Checking, false);
        let activity = match self.fetch().await {
            Ok(true) => ToolActivity::Updated,
            Ok(false) => ToolActivity::Current,
            Err(problem) => ToolActivity::Failed(problem),
        };
        let _ = std::fs::write(self.dir.join(LAST_CHECK), crate::now().to_string());
        self.set_activity(activity.clone(), true);
        activity
    }
    /// Install the latest release when it is newer than the tool in use.
    /// Returns whether a new version is now in use.
    async fn fetch(self: &std::sync::Arc<Self>) -> std::result::Result<bool, ToolProblem> {
        let asset = release_asset().ok_or(ToolProblem::Unsupported)?;
        std::fs::create_dir_all(&self.dir).map_err(|_| ToolProblem::Storage)?;
        if !self.activated.load(std::sync::atomic::Ordering::SeqCst) {
            // The version in use must be known before deciding a download is newer.
            let tool = self.clone();
            tokio::task::spawn_blocking(move || tool.activate(&smoke_test))
                .await
                .map_err(|_| ToolProblem::Storage)?;
        }
        let client = self.client()?;
        let tag = latest_tag(client).await?;
        let latest = ToolVersion::parse(&tag).ok_or(ToolProblem::Integrity)?;
        let current = self.active.read().ok().and_then(|a| a.version.clone());
        if current.is_some_and(|current| current >= latest) {
            return Ok(false);
        }
        let (file, expected) = download(client, &self.dir, &tag, asset).await?;
        let dir = self.dir.clone();
        let path = tokio::task::spawn_blocking(move || {
            install(&dir, asset, &tag, file, &expected, &smoke_test)
        })
        .await
        .map_err(|_| ToolProblem::Storage)??;
        if let Ok(mut active) = self.active.write() {
            *active = Active {
                path,
                version: Some(latest),
                origin: ToolOrigin::Downloaded,
            };
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_versions_compare_numerically() {
        let v = |s| ToolVersion::parse(s).unwrap();
        assert!(v("2026.10.02") > v("2026.08.19"));
        assert!(v("2026.08.19.1") > v("2026.08.19"));
        assert!(v("2027.01.01") > v("2026.12.31"));
        assert!(v("2026.8.19") == v("2026.08.19"));
        assert_eq!(v("2026.08.09").to_string(), "2026.08.09");
        assert_eq!(v("2026.08.19.1").to_string(), "2026.08.19.1");
    }
    #[test]
    fn unsafe_or_unstable_tags_are_refused() {
        for tag in [
            "",
            "latest",
            "2026.08",
            "26.08.19",
            "2026.08.19.1.2",
            "2026.08.19-nightly",
            "2026.08.19/../x",
            "2026..19",
            "v2026.08.19",
            "2026.08.1234567890",
        ] {
            assert!(ToolVersion::parse(tag).is_none(), "{tag}");
        }
        assert!(ToolVersion::parse(" 2026.08.19\n").is_some());
    }
    #[test]
    fn checksums_are_read_for_the_exact_asset_only() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let sums =
            format!("{a}  yt-dlp\n{b}  yt-dlp_macos\n{a} *yt-dlp.exe\r\n{b}  yt-dlp_macos.zip\n");
        assert_eq!(checksum_for(&sums, "yt-dlp_macos"), Some([0xbb; 32]));
        assert_eq!(checksum_for(&sums, "yt-dlp.exe"), Some([0xaa; 32]));
        assert_eq!(checksum_for(&sums, "yt-dlp_linux"), None);
        assert_eq!(checksum_for(&sums, "yt-dlp_mac"), None);
    }
    #[test]
    fn malformed_or_duplicated_checksums_are_refused() {
        let a = "a".repeat(64);
        assert_eq!(checksum_for(&format!("{}  tool", &a[..63]), "tool"), None);
        assert_eq!(checksum_for(&format!("{}zz  tool", &a[..62]), "tool"), None);
        assert_eq!(
            checksum_for(&format!("{a}  tool\n{}  tool", "b".repeat(64)), "tool"),
            None
        );
        assert_eq!(checksum_for("", "tool"), None);
    }
    /// A download holding `bytes`, created where a real download would be.
    fn downloaded(dir: &std::path::Path, bytes: &[u8]) -> (tempfile::NamedTempFile, Digest256) {
        let mut file = tempfile::NamedTempFile::new_in(dir).unwrap();
        std::io::Write::write_all(&mut file, bytes).unwrap();
        (file, Sha256::digest(bytes).into())
    }
    /// A smoke test that reads the version from the file's own content.
    fn reads_content(path: &std::path::Path) -> Option<ToolVersion> {
        ToolVersion::parse(&std::fs::read_to_string(path).ok()?)
    }
    #[test]
    fn a_mismatched_download_is_never_run_or_activated() {
        let dir = tempfile::tempdir().unwrap();
        let (file, _) = downloaded(dir.path(), b"2026.10.02");
        let ran = std::cell::Cell::new(false);
        let smoke = |_: &std::path::Path| {
            ran.set(true);
            None
        };
        let result = install(dir.path(), "tool", "2026.10.02", file, &[7; 32], &smoke);
        assert_eq!(result, Err(ToolProblem::Integrity));
        assert!(!ran.get());
        assert!(load_managed(dir.path(), "tool", &reads_content).is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn a_verified_download_replaces_the_previous_one() {
        let dir = tempfile::tempdir().unwrap();
        let (file, digest) = downloaded(dir.path(), b"2026.08.19");
        install(
            dir.path(),
            "tool",
            "2026.08.19",
            file,
            &digest,
            &reads_content,
        )
        .unwrap();
        let (file, digest) = downloaded(dir.path(), b"2026.10.02");
        let path = install(
            dir.path(),
            "tool",
            "2026.10.02",
            file,
            &digest,
            &reads_content,
        )
        .unwrap();
        assert_eq!(path, dir.path().join("2026.10.02/tool"));
        let (loaded, version) = load_managed(dir.path(), "tool", &reads_content).unwrap();
        assert_eq!((loaded, version.to_string()), (path, "2026.10.02".into()));
        assert!(!dir.path().join("2026.08.19").exists());
    }
    #[test]
    fn a_download_that_does_not_start_keeps_the_previous_one() {
        let dir = tempfile::tempdir().unwrap();
        let (file, digest) = downloaded(dir.path(), b"2026.08.19");
        install(
            dir.path(),
            "tool",
            "2026.08.19",
            file,
            &digest,
            &reads_content,
        )
        .unwrap();
        // Reports another version than its tag, as a broken or mislabelled build would.
        let (file, digest) = downloaded(dir.path(), b"2026.08.20");
        let result = install(
            dir.path(),
            "tool",
            "2026.10.02",
            file,
            &digest,
            &reads_content,
        );
        assert_eq!(result, Err(ToolProblem::Unusable));
        let (_, version) = load_managed(dir.path(), "tool", &reads_content).unwrap();
        assert_eq!(version.to_string(), "2026.08.19");
        assert!(!dir.path().join("2026.10.02").exists());
    }
    #[test]
    fn an_altered_managed_copy_falls_back_to_the_bundled_one() {
        let Some(asset) = release_asset() else { return };
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("managed");
        std::fs::create_dir(&dir).unwrap();
        let bundled = root.path().join("bundled");
        std::fs::write(&bundled, "2026.08.19").unwrap();
        let (file, digest) = downloaded(&dir, b"2026.10.02");
        install(&dir, asset, "2026.10.02", file, &digest, &reads_content).unwrap();
        let tool = YoutubeTool::new(dir.clone(), bundled.clone());
        tool.activate(&reads_content);
        assert_eq!(tool.path(), dir.join("2026.10.02").join(asset));
        assert_eq!(tool.status().origin, ToolOrigin::Downloaded);
        std::fs::write(dir.join("2026.10.02").join(asset), "2026.10.03").unwrap();
        tool.activate(&reads_content);
        assert_eq!(tool.path(), bundled);
        let status = tool.status();
        assert_eq!(status.origin, ToolOrigin::Bundled);
        assert_eq!(status.version.as_deref(), Some("2026.08.19"));
    }
    #[test]
    fn a_newer_bundled_copy_wins_over_an_older_download() {
        let Some(asset) = release_asset() else { return };
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("managed");
        std::fs::create_dir(&dir).unwrap();
        let bundled = root.path().join("bundled");
        std::fs::write(&bundled, "2026.11.01").unwrap();
        let (file, digest) = downloaded(&dir, b"2026.10.02");
        install(&dir, asset, "2026.10.02", file, &digest, &reads_content).unwrap();
        let tool = YoutubeTool::new(dir, bundled.clone());
        tool.activate(&reads_content);
        assert_eq!(tool.path(), bundled);
    }
    #[cfg(unix)]
    #[test]
    fn the_smoke_test_runs_the_installed_tool() {
        let dir = tempfile::tempdir().unwrap();
        let script = b"#!/bin/sh\n[ \"$1\" = --version ] && echo 2026.10.02\n";
        let (file, digest) = downloaded(dir.path(), script);
        let path = install(dir.path(), "tool", "2026.10.02", file, &digest, &smoke_test).unwrap();
        assert_eq!(smoke_test(&path).unwrap().to_string(), "2026.10.02");
        let (file, digest) = downloaded(dir.path(), b"#!/bin/sh\nexit 3\n");
        let result = install(dir.path(), "tool", "2026.10.03", file, &digest, &smoke_test);
        assert_eq!(result, Err(ToolProblem::Unusable));
    }
    #[test]
    fn file_digests_match_sha256() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            hex::encode(file_digest(&path).unwrap()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
