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
