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
    /// The 16×16 icon given to the chapter and tracks this media becomes in a playlist.
    /// Changing it later leaves playlists that already hold the media as they are.
    #[serde(default)]
    pub icon: Option<IconReference>,
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
                crate::messages::TRIM_RANGE_INVALID.into(),
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
            Self::Loudness => "Volume égalisé",
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
    /// Set when this recipe is one part of a longer version cut for Yoto's track limit.
    /// Parts are not versions of the media: they are absent from `MediaItem::recipes`.
    #[serde(default)]
    pub part: Option<RecipePart>,
}
/// Where a part recipe belongs: part `index` (from 1) of `count` of version `of`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecipePart {
    pub of: RecipeId,
    pub index: u16,
    pub count: u16,
}
/// Cut `range` into the fewest equal parts no longer than [`Rules::PART_MICROS_MAX`].
/// A range that already fits is returned whole.
pub fn plan_parts(range: TrimRange) -> Vec<TrimRange> {
    let start = range.start().micros();
    let length = range.duration().micros();
    let count = length.div_ceil(Rules::PART_MICROS_MAX).max(1);
    let bound = |k: u64| {
        // Integer boundaries: the last part ends exactly where the range does.
        let offset = (u128::from(length) * u128::from(k) / u128::from(count)) as u64;
        MediaTime::try_from(start + offset).expect("within the validated range")
    };
    (0..count)
        .map(|k| {
            TrimRange::new(bound(k), bound(k + 1), range.end()).expect("non-empty ordered part")
        })
        .collect()
}
/// Title of each part of a long audio; `{title}`, `{part}` and `{parts}` are replaced.
/// Clients may pass their own wording of it (see [`part_title_with`]).
pub const PART_TITLE: &str = "{title} — partie {part}/{parts}";
/// Title for part `part` of `parts` with [`PART_TITLE`].
pub fn part_title(title: &str, part: usize, parts: usize) -> String {
    part_title_with(PART_TITLE, title, part, parts)
}
/// Title for part `part` of `parts` from `template`, shortening `title` so the whole fits
/// [`Rules::TITLE_MAX`]. A template missing one of its three placeholders falls back to
/// [`PART_TITLE`], so a part is always numbered.
pub fn part_title_with(template: &str, title: &str, part: usize, parts: usize) -> String {
    let template = if ["{title}", "{part}", "{parts}"]
        .iter()
        .all(|placeholder| template.contains(placeholder))
    {
        template
    } else {
        PART_TITLE
    };
    let fill = |title: &str| {
        template
            .replace("{part}", &part.to_string())
            .replace("{parts}", &parts.to_string())
            .replace("{title}", title)
    };
    let room = Rules::TITLE_MAX.saturating_sub(fill("").chars().count());
    let title = title.trim();
    if title.chars().count() <= room {
        fill(title)
    } else {
        let short: String = title.chars().take(room.saturating_sub(1)).collect();
        fill(&format!("{}…", short.trim_end()))
    }
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
