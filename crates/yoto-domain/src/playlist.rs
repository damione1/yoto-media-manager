use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Extra = BTreeMap<String, serde_json::Value>;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Metadata {
    pub description: String,
    pub author: String,
    pub narrator: String,
    pub languages: Vec<String>,
    pub category: Category,
    pub min_age: Option<u8>,
    pub max_age: Option<u8>,
    pub cover: Option<AssetId>,
    pub extra: Extra,
}
impl Metadata {
    pub fn empty() -> Self {
        Self {
            description: String::new(),
            author: String::new(),
            narrator: String::new(),
            languages: vec![],
            category: Category::None,
            min_age: None,
            max_age: None,
            cover: None,
            extra: Extra::new(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Playback {
    pub autoadvance: Autoadvance,
    pub resume_seconds: u64,
    pub online_only: bool,
    pub shuffle: Vec<ShuffleGroup>,
    pub kind: PlaybackType,
    pub extra: Extra,
}
impl Playback {
    pub fn standard() -> Self {
        Self {
            autoadvance: Autoadvance::Next,
            resume_seconds: 2_592_000,
            online_only: false,
            shuffle: vec![],
            kind: PlaybackType::Linear,
            extra: Extra::new(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ShuffleGroup {
    pub start: usize,
    pub end: usize,
    pub limit: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Track {
    pub id: TrackId,
    pub key: TrackKey,
    pub title: String,
    pub icon: Option<IconReference>,
    pub media: TrackMedia,
    #[serde(default)]
    pub events: Option<TrackEvents>,
    pub extra: Extra,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Chapter {
    pub id: ChapterId,
    pub key: ChapterKey,
    pub title: String,
    pub icon: Option<IconReference>,
    pub tracks: Vec<Track>,
    pub extra: Extra,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RemoteBase {
    pub account: AccountId,
    pub card: CardId,
    pub document: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum DraftOrigin {
    New,
    Existing { base: RemoteBase },
}
/// State of the local draft relative to its account-scoped cloud publication.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum PlaylistSyncState {
    LocalOnly,
    CloudOnly,
    Synced,
    Modified,
    Publishing,
    AuthenticationRequired,
    Conflict,
    Uncertain,
    Failed { failure: Failure },
}
impl PlaylistSyncState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::LocalOnly => "Sur cet ordinateur",
            Self::CloudOnly => "Sur Yoto",
            Self::Synced => "Sur Yoto",
            Self::Modified => "Modification à confirmer sur Yoto",
            Self::Publishing => "Envoi vers Yoto…",
            Self::AuthenticationRequired => "Reconnectez votre compte Yoto",
            Self::Conflict => "Modifiée sur Yoto · à vérifier",
            Self::Uncertain => "Publication à vérifier",
            Self::Failed { .. } => "Envoi échoué · voir les détails",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Playlist {
    pub id: PlaylistId,
    pub title: String,
    pub metadata: Metadata,
    pub playback: Playback,
    pub chapters: Vec<Chapter>,
    pub revision: Revision,
    pub origin: DraftOrigin,
    pub extra: Extra,
    pub content_extra: Extra,
}
impl Playlist {
    pub fn new(title: String) -> Result<Self> {
        Rules::title(&title)?;
        Ok(Self {
            id: PlaylistId::new(),
            title,
            metadata: Metadata::empty(),
            playback: Playback::standard(),
            chapters: vec![],
            revision: Revision(1),
            origin: DraftOrigin::New,
            extra: Extra::new(),
            content_extra: Extra::new(),
        })
    }
    pub fn touch(&mut self) -> Result<()> {
        self.revision = self.revision.next()?;
        Ok(())
    }
    pub fn image_assets(&self) -> std::collections::BTreeSet<AssetId> {
        let mut assets = std::collections::BTreeSet::new();
        assets.extend(self.metadata.cover);
        for chapter in &self.chapters {
            for icon in std::iter::once(&chapter.icon).chain(chapter.tracks.iter().map(|t| &t.icon))
            {
                if let Some(IconReference::Local { asset }) = icon {
                    assets.insert(*asset);
                }
            }
        }
        assets
    }
    /// Add a media version as one chapter holding one track, both with the media's icon.
    pub fn add_media(&mut self, media: &MediaItem, recipe: RecipeId) -> Result<()> {
        let track = Track {
            id: TrackId::new(),
            key: TrackKey::new(uuid::Uuid::new_v4().simple().to_string())?,
            title: media.title.clone(),
            icon: media.icon.clone(),
            events: None,
            media: TrackMedia::Local {
                media: media.id,
                recipe,
            },
            extra: Extra::new(),
        };
        self.chapters.push(Chapter {
            id: ChapterId::new(),
            key: ChapterKey::new(uuid::Uuid::new_v4().simple().to_string())?,
            title: media.title.clone(),
            icon: media.icon.clone(),
            tracks: vec![track],
            extra: Extra::new(),
        });
        self.touch()
    }
    /// Add a long audio cut into parts: one chapter titled like the media, holding one
    /// track per part recipe, in order, each titled with [`part_title_with`] from
    /// `template` ([`PART_TITLE`] when `None`). The chapter and every part carry the
    /// media's icon.
    pub fn add_media_parts(
        &mut self,
        media: &MediaItem,
        parts: &[RecipeId],
        template: Option<&str>,
    ) -> Result<()> {
        let tracks = parts
            .iter()
            .enumerate()
            .map(|(index, recipe)| {
                Ok(Track {
                    id: TrackId::new(),
                    key: TrackKey::new(uuid::Uuid::new_v4().simple().to_string())?,
                    title: part_title_with(
                        template.unwrap_or(PART_TITLE),
                        &media.title,
                        index + 1,
                        parts.len(),
                    ),
                    icon: media.icon.clone(),
                    events: None,
                    media: TrackMedia::Local {
                        media: media.id,
                        recipe: *recipe,
                    },
                    extra: Extra::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.chapters.push(Chapter {
            id: ChapterId::new(),
            key: ChapterKey::new(uuid::Uuid::new_v4().simple().to_string())?,
            title: media.title.clone(),
            icon: media.icon.clone(),
            tracks,
            extra: Extra::new(),
        });
        self.touch()
    }
    pub fn validate(&self) -> Result<()> {
        Rules::title(&self.title)?;
        if self.chapters.is_empty() {
            return Err(Failure::Validation(crate::messages::PLAYLIST_EMPTY.into()));
        }
        let tracks: usize = self.chapters.iter().map(|c| c.tracks.len()).sum();
        if tracks > Rules::TRACKS_MAX {
            return Err(Failure::Validation(
                crate::messages::PLAYLIST_TRACKS_MAX.into(),
            ));
        }
        self.validate_draft()?;
        for c in &self.chapters {
            Rules::title(&c.title)?;
            if c.tracks.is_empty() {
                return Err(Failure::Validation(crate::messages::CHAPTER_EMPTY.into()));
            }
            for t in &c.tracks {
                Rules::title(&t.title)?;
            }
        }
        for group in &self.playback.shuffle {
            if group.start > group.end
                || group.end >= self.chapters.len()
                || group.limit == 0
                || group.limit > group.end - group.start + 1
            {
                return Err(Failure::Validation(
                    crate::messages::SHUFFLE_RANGE_INVALID.into(),
                ));
            }
        }
        Ok(())
    }
    /// Imported MYO cards can be empty or have titles still being edited.
    /// Retain them as editable drafts; publication performs full validation.
    pub fn validate_draft(&self) -> Result<()> {
        Rules::title(&self.title)?;
        // Wire keys are paths: chapter.key, then track.key inside that chapter.
        // They are distinct namespaces; local UUID identities remain globally unique.
        let mut chapter_keys = std::collections::HashSet::new();
        for c in &self.chapters {
            if !chapter_keys.insert(c.key.as_str()) {
                return Err(Failure::Validation(
                    crate::messages::DUPLICATE_CHAPTER_KEY.into(),
                ));
            }
            let mut track_keys = std::collections::HashSet::new();
            for t in &c.tracks {
                if !track_keys.insert(t.key.as_str()) {
                    return Err(Failure::Validation(
                        crate::messages::DUPLICATE_TRACK_KEY.into(),
                    ));
                }
            }
        }
        Ok(())
    }
    /// Yoto permits an empty MYO card, which can subsequently receive audio.
    pub fn validate_remote_write(&self) -> Result<()> {
        if self.chapters.is_empty() {
            self.validate_draft()
        } else {
            self.validate()
        }
    }
    /// Retain local entity identities when a new authoritative document is read.
    /// Track wire keys are compared only within their chapter namespace.
    pub fn retain_ids_from(&mut self, previous: &Self) {
        self.id = previous.id;
        for chapter in &mut self.chapters {
            if let Some(old) = previous.chapters.iter().find(|c| c.key == chapter.key) {
                chapter.id = old.id;
                for track in &mut chapter.tracks {
                    if let Some(old) = old.tracks.iter().find(|t| t.key == track.key) {
                        track.id = old.id;
                    }
                }
            }
        }
    }
    pub fn duplicate(&self) -> Result<Self> {
        let mut p = self.clone();
        p.id = PlaylistId::new();
        p.origin = DraftOrigin::New;
        p.revision = Revision(1);
        p.title = format!("{} (copie)", self.title);
        Rules::title(&p.title)?;
        for c in &mut p.chapters {
            c.id = ChapterId::new();
            for t in &mut c.tracks {
                t.id = TrackId::new();
            }
        }
        Ok(p)
    }
    /// Structural transformations must preserve the same logical shuffle targets. Open
    /// event parameters are intentionally gated because their reference semantics are unknown.
    pub fn validate_structure_against(&self, old: &Self) -> Result<()> {
        let layout = |p: &Self| {
            p.chapters
                .iter()
                .map(|c| {
                    (
                        c.key.clone(),
                        c.tracks.iter().map(|t| t.key.clone()).collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        };
        if layout(self) == layout(old) {
            return Ok(());
        }
        for t in old.chapters.iter().flat_map(|c| &c.tracks) {
            if t.extra.contains_key("events") {
                return Err(Failure::Compatibility(
                    crate::messages::UNPARSED_EVENTS.into(),
                ));
            }
            if let Some(events) = &t.events {
                for event in [&events.on_end, &events.on_lhb, &events.on_rhb]
                    .into_iter()
                    .flatten()
                {
                    if event.cmd == EventCommand::Goto
                        || event.params.as_ref().is_some_and(|p| !p.is_empty())
                    {
                        return Err(Failure::Compatibility(
                            crate::messages::REFERENCE_EVENTS.into(),
                        ));
                    }
                }
            }
        }
        for (i, group) in old.playback.shuffle.iter().enumerate() {
            let before = old
                .chapters
                .get(group.start..=group.end)
                .ok_or_else(|| Failure::Validation(crate::messages::SHUFFLE_INVALID.into()))?
                .iter()
                .map(|c| c.key.clone())
                .collect::<std::collections::BTreeSet<_>>();
            let Some(new) = self.playback.shuffle.get(i) else {
                continue;
            }; // Explicitly removing a group is safe.
            let after = self
                .chapters
                .get(new.start..=new.end)
                .ok_or_else(|| Failure::Validation(crate::messages::SHUFFLE_INVALID.into()))?
                .iter()
                .map(|c| c.key.clone())
                .collect::<std::collections::BTreeSet<_>>();
            if before != after {
                return Err(Failure::Compatibility(
                    crate::messages::SHUFFLE_TARGETS_CHANGED.into(),
                ));
            }
        }
        Ok(())
    }
    fn remap_shuffle(&mut self, old: &Self) -> Result<()> {
        for (group, previous) in self.playback.shuffle.iter_mut().zip(&old.playback.shuffle) {
            let targets = old
                .chapters
                .get(previous.start..=previous.end)
                .ok_or_else(|| Failure::Validation(crate::messages::SHUFFLE_INVALID.into()))?;
            let mut positions = targets
                .iter()
                .map(|c| {
                    self.chapters
                        .iter()
                        .position(|new| new.key == c.key)
                        .ok_or_else(|| {
                            Failure::Compatibility(crate::messages::SHUFFLE_CHAPTER_REMOVED.into())
                        })
                })
                .collect::<Result<Vec<_>>>()?;
            positions.sort_unstable();
            let start = *positions.first().ok_or(Failure::Conflict)?;
            let end = *positions.last().ok_or(Failure::Conflict)?;
            if end - start + 1 != positions.len() {
                return Err(Failure::Compatibility(
                    crate::messages::SHUFFLE_NOT_CONTIGUOUS.into(),
                ));
            }
            group.start = start;
            group.end = end;
        }
        self.validate_structure_against(old)
    }
    pub fn remove_track(&mut self, id: TrackId) -> Result<()> {
        let old = self.clone();
        for c in &mut self.chapters {
            c.tracks.retain(|t| t.id != id);
        }
        self.chapters.retain(|c| !c.tracks.is_empty());
        if let Err(error) = self.remap_shuffle(&old) {
            *self = old;
            return Err(error);
        }
        self.touch()
    }
    pub fn move_chapter(&mut self, id: ChapterId, delta: isize) -> Result<()> {
        let i = self
            .chapters
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| Failure::Validation(crate::messages::CHAPTER_ABSENT.into()))?;
        let j = i
            .checked_add_signed(delta)
            .filter(|j| *j < self.chapters.len())
            .ok_or_else(|| Failure::Validation(crate::messages::CHAPTER_BOUNDARY.into()))?;
        let old = self.clone();
        self.chapters.swap(i, j);
        if let Err(error) = self.remap_shuffle(&old) {
            *self = old;
            return Err(error);
        }
        self.touch()
    }
    pub fn move_track(&mut self, id: TrackId, destination: ChapterId, index: usize) -> Result<()> {
        let (ci, ti) = self
            .chapters
            .iter()
            .enumerate()
            .find_map(|(ci, c)| c.tracks.iter().position(|t| t.id == id).map(|ti| (ci, ti)))
            .ok_or_else(|| Failure::Validation(crate::messages::TRACK_ABSENT.into()))?;
        let di = self
            .chapters
            .iter()
            .position(|c| c.id == destination)
            .ok_or_else(|| Failure::Validation(crate::messages::DESTINATION_ABSENT.into()))?;
        let max = self.chapters[di].tracks.len() - usize::from(ci == di);
        if index > max {
            return Err(Failure::Validation(crate::messages::TRACK_BOUNDARY.into()));
        }
        if ci != di
            && self.chapters[di]
                .tracks
                .iter()
                .any(|t| t.key == self.chapters[ci].tracks[ti].key)
        {
            return Err(Failure::Compatibility(
                crate::messages::TRACK_KEY_COLLISION.into(),
            ));
        }
        let old = self.clone();
        let t = self.chapters[ci].tracks.remove(ti);
        self.chapters[di].tracks.insert(index, t);
        self.chapters.retain(|c| !c.tracks.is_empty());
        if let Err(error) = self.remap_shuffle(&old) {
            *self = old;
            return Err(error);
        }
        self.touch()
    }
    pub fn ungroup(&mut self, id: ChapterId) -> Result<()> {
        let i = self
            .chapters
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| Failure::Validation(crate::messages::CHAPTER_ABSENT.into()))?;
        let old = self.clone();
        let c = self.chapters.remove(i);
        let mut chapters = vec![];
        for t in c.tracks {
            chapters.push(Chapter {
                id: ChapterId::new(),
                key: ChapterKey::new(uuid::Uuid::new_v4().simple().to_string())?,
                title: t.title.clone(),
                icon: None,
                tracks: vec![t],
                extra: Extra::new(),
            });
        }
        self.chapters.splice(i..i, chapters);
        if let Err(error) = self.remap_shuffle(&old) {
            *self = old;
            return Err(error);
        }
        self.touch()
    }
}
