pub mod api;
pub mod assets;
pub mod auth;
pub mod db;
pub mod media;
pub mod players;
pub mod process;
pub mod service;
pub mod sources;
pub mod transcode;
pub use service::{Command, Reply, Service, Snapshot};
use yoto_domain::Failure;
fn storage(e: impl std::fmt::Display) -> Failure {
    Failure::Storage(e.to_string())
}
fn media_error(e: impl std::fmt::Display) -> Failure {
    Failure::Media(e.to_string())
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub mod bridge;
pub fn default_profile() -> yoto_domain::Result<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("YOTO_PROFILE") {
        return Ok(path.into());
    }
    let base = std::env::var_os(if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "HOME"
    })
    .ok_or_else(|| yoto_domain::Failure::Storage("User data directory unavailable".into()))?;
    Ok(if cfg!(windows) {
        std::path::PathBuf::from(base).join("Yoto")
    } else {
        std::path::PathBuf::from(base).join("Library/Application Support/Yoto")
    })
}

pub mod session;
pub use session::Session;

pub mod diagnostics;

#[cfg(test)]
mod tests;

/// Resolve aliases before choosing the lock, SQLCipher key, OAuth namespace or IPC endpoint.
pub fn canonical_profile(profile: &std::path::Path) -> yoto_domain::Result<std::path::PathBuf> {
    std::fs::create_dir_all(profile).map_err(storage)?;
    std::fs::canonicalize(profile).map_err(storage)
}
pub fn profile_namespace(profile: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(profile.as_os_str().as_encoded_bytes()))
}

pub mod recovery;

pub mod projection;
