pub mod device;
pub mod ids;
pub mod jobs;
pub mod media;
pub mod messages;
pub mod playlist;
pub use device::*;
pub use ids::*;
pub use jobs::*;
pub use media::*;
pub use playlist::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
pub use yoto_contracts::*;

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize, JsonSchema)]
pub enum Failure {
    #[error("{0}")]
    Validation(String),
    #[error("Storage: {0}")]
    Storage(String),
    #[error("Media processing: {0}")]
    Media(String),
    #[error("The connection is unavailable.")]
    Connection,
    #[error("Connect your Yoto account to continue.")]
    Authentication,
    #[error("Permission required: {0}")]
    Permission(String),
    #[error("Yoto rate limit; retry after {retry_after_seconds} seconds.")]
    RateLimit { retry_after_seconds: u64 },
    #[error("Yoto contract is incompatible: {0}")]
    Compatibility(String),
    #[error("The content changed since this draft was opened. Reload or resolve the conflict.")]
    Conflict,
    #[error("The remote operation may have completed. Check its result before trying again.")]
    Ambiguous,
    #[error("The operation was cancelled.")]
    Cancelled,
    #[error("Tool unavailable: {0}")]
    Tool(String),
    #[error("Yoto could not transcode « {track} » ({problem:?}).")]
    Transcode {
        track: String,
        problem: TranscodeProblem,
        /// Last progress Yoto reported for this track, when it reported any.
        percent: Option<u8>,
    },
}
/// Why Yoto's conversion of an uploaded track did not finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TranscodeProblem {
    /// Yoto reported that the conversion failed or was cancelled.
    Failed,
    /// Progress stopped moving, even after sending the file again.
    Stalled,
    /// The conversion did not finish within the time allowed for this track.
    TimedOut,
}
impl Failure {
    /// A write may have reached the server when transport or receipt decoding fails.
    pub fn uncertain_write(self) -> Self {
        match self {
            Self::Connection | Self::Compatibility(_) | Self::Storage(_) => Self::Ambiguous,
            other @ (Self::Validation(_)
            | Self::Media(_)
            | Self::Authentication
            | Self::Permission(_)
            | Self::RateLimit { .. }
            | Self::Conflict
            | Self::Ambiguous
            | Self::Cancelled
            | Self::Tool(_)
            | Self::Transcode { .. }) => other,
        }
    }
    pub fn retry_seconds(&self) -> Option<u64> {
        match self {
            Self::Connection => Some(5),
            Self::RateLimit {
                retry_after_seconds,
            } => Some(*retry_after_seconds),
            Self::Transcode { problem, .. } => match problem {
                TranscodeProblem::Stalled | TranscodeProblem::TimedOut => Some(60),
                TranscodeProblem::Failed => None,
            },
            Self::Validation(_)
            | Self::Storage(_)
            | Self::Media(_)
            | Self::Authentication
            | Self::Permission(_)
            | Self::Compatibility(_)
            | Self::Conflict
            | Self::Ambiguous
            | Self::Cancelled
            | Self::Tool(_) => None,
        }
    }
}
pub type Result<T> = std::result::Result<T, Failure>;

pub struct Rules;
impl Rules {
    pub const TITLE_MAX: usize = 140;
    pub const TRACKS_MAX: usize = 100;
    pub const TRACK_BYTES_MAX: u64 = 100_000_000;
    pub const CARD_BYTES_MAX: u64 = 500_000_000;
    pub const TRACK_MICROS_MAX: u64 = 3_600_000_000;
    pub const CARD_MICROS_MAX: u64 = 18_000_000_000;
    pub const MP3_BITRATE: u32 = 192;
    pub const SAMPLE_RATE: u32 = 44_100;
    pub const MAX_ATTEMPTS: u32 = 5;
    pub const WAVEFORM_BUCKETS: usize = 2048;
    pub fn title(value: &str) -> Result<()> {
        if value.trim().is_empty() || value.chars().count() > Self::TITLE_MAX {
            return Err(Failure::Validation(messages::TITLE_LENGTH.into()));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Route {
    Library,
    Playlists,
    Imports,
    Players,
    Account,
}
impl Route {
    pub const ALL: &[Self] = &[
        Self::Library,
        Self::Playlists,
        Self::Imports,
        Self::Players,
        Self::Account,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Library => "Médiathèque",
            Self::Playlists => "Playlists",
            Self::Imports => "Imports",
            Self::Players => "Lecteurs",
            Self::Account => "Compte",
        }
    }
}

#[cfg(test)]
mod tests;
