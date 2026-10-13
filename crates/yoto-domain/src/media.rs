use crate::*;
/// Images returned to a client remain rooted until attached, explicitly released,
/// or this expiry. Expiry is epoch seconds; the default lease lasts 24 hours.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DraftAssetLease {
    pub asset: AssetId,
    /// Unattached account-derived images are purged with their account; local images have no owner.
    #[serde(default)]
    pub account: Option<AccountId>,
    pub expires: u64,
}
impl DraftAssetLease {
    pub const LIFETIME_SECONDS: u64 = 24 * 60 * 60;
}
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SourceKind {
    LocalFile,
    Youtube,
}
impl SourceKind {
    pub const ALL: &[Self] = &[Self::LocalFile, Self::Youtube];
    pub fn label(self) -> &'static str {
        match self {
            Self::LocalFile => "Fichier audio",
            Self::Youtube => "YouTube",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum Provenance {
    LocalFile { name: String },
    Youtube { url: String, video_id: String },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AudioInfo {
    pub duration: MediaTime,
    pub bytes: u64,
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Asset {
    pub id: AssetId,
    pub sha256: LocalSha256,
    pub bytes: u64,
    pub extension: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediaItem {
    pub id: MediaId,
    pub title: String,
    pub author: String,
    pub tags: Vec<String>,
    pub original: AssetId,
    pub info: AudioInfo,
    pub provenance: Provenance,
    pub recipes: Vec<RecipeId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "(MediaTime, MediaTime)", into = "(MediaTime, MediaTime)")]
/// Two-element [start, end] array in microseconds; start is inclusive and end exclusive.
/// The service additionally verifies end against the original media duration before processing.
pub struct TrimRange {
    start: MediaTime,
    end: MediaTime,
}
impl TrimRange {
    pub fn new(start: MediaTime, end: MediaTime, duration: MediaTime) -> Result<Self> {
        if start >= end || end > duration {
            return Err(Failure::Validation(
                "Choose 0 ≤ start < end ≤ source duration.".into(),
            ));
        }
        Ok(Self { start, end })
    }
    pub fn start(self) -> MediaTime {
        self.start
    }
    pub fn end(self) -> MediaTime {
        self.end
    }
    pub fn duration(self) -> MediaTime {
        MediaTime::try_from(self.end.micros() - self.start.micros()).expect("validated range")
    }
}
impl TryFrom<(MediaTime, MediaTime)> for TrimRange {
    type Error = Failure;
    fn try_from(v: (MediaTime, MediaTime)) -> Result<Self> {
        Self::new(v.0, v.1, v.1)
    }
}
impl From<TrimRange> for (MediaTime, MediaTime) {
    fn from(v: TrimRange) -> Self {
        (v.start, v.end)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Normalization {
    Preserve,
    Loudness,
}
impl Normalization {
    pub const ALL: &[Self] = &[Self::Preserve, Self::Loudness];
    pub fn label(self) -> &'static str {
        match self {
            Self::Preserve => "Volume original",
            Self::Loudness => "Normaliser (-16 LUFS)",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediaRecipe {
    pub id: RecipeId,
    pub media: MediaId,
    pub original: AssetId,
    pub trim: TrimRange,
    pub normalization: Normalization,
    pub result: Option<AssetId>,
    pub info: Option<AudioInfo>,
    pub tool_version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Waveform {
    pub levels: Vec<Vec<(f32, f32)>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum ImportSelection {
    LocalFile { path: PathBuf },
    Youtube { url: String, entry: Option<String> },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceEntry {
    pub id: String,
    pub title: String,
    pub duration: Option<MediaTime>,
    pub url: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SourceShape {
    Video,
    Playlist,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceInspection {
    pub kind: SourceShape,
    pub entries: Vec<SourceEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum TrackMedia {
    Local {
        media: MediaId,
        recipe: RecipeId,
    },
    RemoteAudio {
        account: AccountId,
        media: YotoMediaId,
        info: AudioInfo,
    },
    Stream {
        url: String,
        format: AudioFormat,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum IconReference {
    Local {
        asset: AssetId,
    },
    Yoto {
        account: AccountId,
        media: YotoMediaId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediaMetadata {
    pub title: String,
    pub author: String,
    pub tags: Vec<String>,
}
/// Exact 16×16 RGBA pixels. Components range from 0 to 255; alpha 0 is transparent.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PixelArt {
    pub pixels: [[[u8; 4]; 16]; 16],
}
impl PixelArt {
    pub fn blank() -> Self {
        Self {
            pixels: [[[0; 4]; 16]; 16],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub enum ReferencePolicy {
    /// Refuse removal while a playlist references the media/version.
    RejectReferenced,
    /// Explicitly remove referring tracks too; active jobs always prevent removal.
    RemovePlaylistTracks,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatalogueSort {
    Title,
    Author,
    Duration,
    RecentlyImported,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum PreviewStatus {
    Stopped,
    Playing { media: MediaId, position: MediaTime },
}
