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
/// One Yoto track to make from a media of the library: its prepared version and its title.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediaTrack {
    pub recipe: RecipeId,
    pub title: String,
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
        self.add_media_tracks(
            media,
            &media.title,
            vec![MediaTrack {
                recipe,
                title: media.title.clone(),
            }],
            None,
        )
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
            .map(|(index, recipe)| MediaTrack {
                recipe: *recipe,
                title: part_title_with(
                    template.unwrap_or(PART_TITLE),
                    &media.title,
                    index + 1,
                    parts.len(),
                ),
            })
            .collect();
        self.add_media_tracks(media, &media.title, tracks, None)
    }
    /// Add prepared versions of `media` as tracks, each titled as given and carrying the
    /// media's icon: appended to chapter `into` when one is named, otherwise as a new
    /// chapter titled `title` with the media's icon.
    pub fn add_media_tracks(
        &mut self,
        media: &MediaItem,
        title: &str,
        tracks: Vec<MediaTrack>,
        into: Option<ChapterId>,
    ) -> Result<()> {
        if tracks.is_empty() {
            return Err(Failure::Validation(crate::messages::CHAPTER_EMPTY.into()));
        }
        let tracks = tracks
            .into_iter()
            .map(|track| Track {
                id: TrackId::new(),
                key: TrackKey::fresh(),
                title: track.title,
                icon: media.icon.clone(),
                events: None,
                media: TrackMedia::Local {
                    media: media.id,
                    recipe: track.recipe,
                },
                extra: Extra::new(),
            })
            .collect::<Vec<_>>();
        match into {
            Some(id) => {
                let chapter = self
                    .chapters
                    .iter_mut()
                    .find(|c| c.id == id)
                    .ok_or_else(|| {
                        Failure::Validation(crate::messages::DESTINATION_ABSENT.into())
                    })?;
                chapter.tracks.extend(tracks);
            }
            None => self.chapters.push(Chapter {
                id: ChapterId::new(),
                key: ChapterKey::fresh(),
                title: title.to_owned(),
                icon: media.icon.clone(),
                tracks,
                extra: Extra::new(),
            }),
        }
        self.touch()
    }
    /// Shorten keys that Yoto would reject. Earlier versions gave new chapters and tracks
    /// 32-character keys; those were never accepted by Yoto, so no remote reference uses them.
    pub fn fit_wire_keys(&mut self) {
        for chapter in &mut self.chapters {
            if chapter.key.as_str().chars().count() > WIRE_KEY_MAX
                && let Ok(key) = ChapterKey::new(fit_wire_key(chapter.key.as_str()))
            {
                chapter.key = key;
            }
            for track in &mut chapter.tracks {
                if track.key.as_str().chars().count() > WIRE_KEY_MAX
                    && let Ok(key) = TrackKey::new(fit_wire_key(track.key.as_str()))
                {
                    track.key = key;
                }
            }
        }
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
            if c.key.as_str().chars().count() > WIRE_KEY_MAX
                || c.tracks
                    .iter()
                    .any(|t| t.key.as_str().chars().count() > WIRE_KEY_MAX)
            {
                return Err(Failure::Validation(crate::messages::WIRE_KEY_LENGTH.into()));
            }
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
            if let Some(old) = previous
                .chapters
                .iter()
                .find(|c| fit_wire_key(c.key.as_str()) == chapter.key.as_str())
            {
                chapter.id = old.id;
                for track in &mut chapter.tracks {
                    if let Some(old) = old
                        .tracks
                        .iter()
                        .find(|t| fit_wire_key(t.key.as_str()) == track.key.as_str())
                    {
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
        // Chapters removed from the playlist leave their range; a range left empty is dropped.
        let present = self
            .chapters
            .iter()
            .map(|c| &c.key)
            .collect::<std::collections::BTreeSet<_>>();
        let mut groups = self.playback.shuffle.iter();
        for group in &old.playback.shuffle {
            let before = old
                .chapters
                .get(group.start..=group.end)
                .ok_or_else(|| Failure::Validation(crate::messages::SHUFFLE_INVALID.into()))?
                .iter()
                .filter(|c| present.contains(&c.key))
                .map(|c| c.key.clone())
                .collect::<std::collections::BTreeSet<_>>();
            if before.is_empty() {
                continue;
            }
            let Some(new) = groups.next() else {
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
    /// Follows each shuffle range to its chapters' new positions. With `removal`, chapters
    /// that left the playlist leave their range too, its limit shrinks to fit, and a range
    /// with no chapter left is dropped; otherwise a missing chapter refuses the edit.
    fn remap_shuffle(&mut self, old: &Self, removal: bool) -> Result<()> {
        let mut groups = vec![];
        for (group, previous) in self.playback.shuffle.iter().zip(&old.playback.shuffle) {
            let targets = old
                .chapters
                .get(previous.start..=previous.end)
                .ok_or_else(|| Failure::Validation(crate::messages::SHUFFLE_INVALID.into()))?;
            let mut positions = vec![];
            for c in targets {
                match self.chapters.iter().position(|new| new.key == c.key) {
                    Some(position) => positions.push(position),
                    None if removal => {}
                    None => {
                        return Err(Failure::Compatibility(
                            crate::messages::SHUFFLE_CHAPTER_REMOVED.into(),
                        ));
                    }
                }
            }
            positions.sort_unstable();
            let (Some(&start), Some(&end)) = (positions.first(), positions.last()) else {
                continue;
            };
            if end - start + 1 != positions.len() {
                return Err(Failure::Compatibility(
                    crate::messages::SHUFFLE_NOT_CONTIGUOUS.into(),
                ));
            }
            groups.push(ShuffleGroup {
                start,
                end,
                limit: group.limit.min(positions.len()),
            });
        }
        groups.extend(
            self.playback
                .shuffle
                .iter()
                .skip(old.playback.shuffle.len())
                .cloned(),
        );
        self.playback.shuffle = groups;
        self.validate_structure_against(old)
    }
    pub fn remove_track(&mut self, id: TrackId) -> Result<()> {
        // Removing nothing must not pass for a removal waiting to be written.
        if !self
            .chapters
            .iter()
            .flat_map(|c| &c.tracks)
            .any(|t| t.id == id)
        {
            return Err(Failure::Validation(crate::messages::TRACK_ABSENT.into()));
        }
        let old = self.clone();
        for c in &mut self.chapters {
            c.tracks.retain(|t| t.id != id);
        }
        self.chapters.retain(|c| !c.tracks.is_empty());
        if let Err(error) = self.remap_shuffle(&old, true) {
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
        if let Err(error) = self.remap_shuffle(&old, false) {
            *self = old;
            return Err(error);
        }
        self.touch()
    }
    /// Put a track at `index` among the tracks of chapter `destination`. A track leaving
    /// its chapter keeps the icon it showed there; a chapter left empty leaves the playlist.
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
        // A track changing chapter keeps the icon it showed there.
        let t = if ci == di {
            t
        } else {
            inherit_icon(t, &self.chapters[ci].icon)
        };
        self.chapters[di].tracks.insert(index, t);
        self.chapters.retain(|c| !c.tracks.is_empty());
        if let Err(error) = self.remap_shuffle(&old, false) {
            *self = old;
            return Err(error);
        }
        self.touch()
    }
    /// Make every track of a chapter a chapter of its own, in order and at the same
    /// place. A track without an icon keeps the chapter's, so nothing changes on the player.
    pub fn ungroup(&mut self, id: ChapterId) -> Result<()> {
        let i = self
            .chapters
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| Failure::Validation(crate::messages::CHAPTER_ABSENT.into()))?;
        let old = self.clone();
        let c = self.chapters.remove(i);
        let chapters: Vec<_> = c
            .tracks
            .into_iter()
            .map(|t| solo_chapter(t, &c.icon))
            .collect();
        self.chapters.splice(i..i, chapters);
        if let Err(error) = self.remap_shuffle(&old, false) {
            *self = old;
            return Err(error);
        }
        self.touch()
    }
    /// Put a chapter at `position` among the chapters (from 0), the others closing up.
    pub fn move_chapter_to(&mut self, id: ChapterId, position: usize) -> Result<()> {
        let i = self
            .chapters
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| Failure::Validation(crate::messages::CHAPTER_ABSENT.into()))?;
        if position >= self.chapters.len() {
            return Err(Failure::Validation(
                crate::messages::CHAPTER_BOUNDARY.into(),
            ));
        }
        if position == i {
            return Ok(());
        }
        let old = self.clone();
        let c = self.chapters.remove(i);
        self.chapters.insert(position, c);
        if let Err(error) = self.remap_shuffle(&old, false) {
            *self = old;
            return Err(error);
        }
        self.touch()
    }
    /// Take a track out of its chapter into a new chapter of its own, titled like the
    /// track and put at `position` among the chapters once the track has left (from 0;
    /// `None` puts it right after the chapter it leaves). A track without an icon keeps
    /// its chapter's. The only track of a chapter just moves that chapter.
    pub fn extract_track(&mut self, id: TrackId, position: Option<usize>) -> Result<ChapterId> {
        let (ci, ti) = self
            .chapters
            .iter()
            .enumerate()
            .find_map(|(ci, c)| c.tracks.iter().position(|t| t.id == id).map(|ti| (ci, ti)))
            .ok_or_else(|| Failure::Validation(crate::messages::TRACK_ABSENT.into()))?;
        if self.chapters[ci].tracks.len() == 1 {
            let chapter = self.chapters[ci].id;
            self.move_chapter_to(chapter, position.unwrap_or(ci))?;
            return Ok(chapter);
        }
        if position.is_some_and(|p| p > self.chapters.len()) {
            return Err(Failure::Validation(
                crate::messages::CHAPTER_BOUNDARY.into(),
            ));
        }
        let old = self.clone();
        let t = self.chapters[ci].tracks.remove(ti);
        let chapter = solo_chapter(t, &self.chapters[ci].icon);
        let id = chapter.id;
        self.chapters.insert(position.unwrap_or(ci + 1), chapter);
        if let Err(error) = self.remap_shuffle(&old, false) {
            *self = old;
            return Err(error);
        }
        self.touch()?;
        Ok(id)
    }
    /// Cut a chapter before a track: this track and the ones after it form a new chapter,
    /// titled like this track, right after the one they leave. Tracks without an icon keep
    /// the chapter's. The first track of a chapter has nothing before it to cut.
    pub fn split_chapter(&mut self, before: TrackId) -> Result<ChapterId> {
        let (ci, ti) = self
            .chapters
            .iter()
            .enumerate()
            .find_map(|(ci, c)| {
                c.tracks
                    .iter()
                    .position(|t| t.id == before)
                    .map(|ti| (ci, ti))
            })
            .ok_or_else(|| Failure::Validation(crate::messages::TRACK_ABSENT.into()))?;
        if ti == 0 {
            return Err(Failure::Validation(crate::messages::TRACK_BOUNDARY.into()));
        }
        let old = self.clone();
        let icon = self.chapters[ci].icon.clone();
        let tracks: Vec<_> = self.chapters[ci]
            .tracks
            .drain(ti..)
            .map(|t| inherit_icon(t, &icon))
            .collect();
        let chapter = Chapter {
            id: ChapterId::new(),
            key: ChapterKey::fresh(),
            title: tracks[0].title.clone(),
            icon,
            tracks,
            extra: Extra::new(),
        };
        let id = chapter.id;
        self.chapters.insert(ci + 1, chapter);
        if let Err(error) = self.remap_shuffle(&old, false) {
            *self = old;
            return Err(error);
        }
        self.touch()?;
        Ok(id)
    }
    /// Append the tracks of chapter `source` to chapter `target`, which keeps its title
    /// and icon; `source` leaves the playlist. Its tracks without an icon keep its icon,
    /// so nothing changes on the player. A shuffle range that held `source` shrinks.
    pub fn merge_chapters(&mut self, target: ChapterId, source: ChapterId) -> Result<()> {
        let ti = self
            .chapters
            .iter()
            .position(|c| c.id == target)
            .ok_or_else(|| Failure::Validation(crate::messages::DESTINATION_ABSENT.into()))?;
        let si = self
            .chapters
            .iter()
            .position(|c| c.id == source)
            .ok_or_else(|| Failure::Validation(crate::messages::CHAPTER_ABSENT.into()))?;
        if ti == si {
            return Ok(());
        }
        if self.chapters[si]
            .tracks
            .iter()
            .any(|s| self.chapters[ti].tracks.iter().any(|t| t.key == s.key))
        {
            return Err(Failure::Compatibility(
                crate::messages::TRACK_KEY_COLLISION.into(),
            ));
        }
        let old = self.clone();
        let from = self.chapters.remove(si);
        let into = ti - usize::from(si < ti);
        self.chapters[into]
            .tracks
            .extend(from.tracks.into_iter().map(|t| inherit_icon(t, &from.icon)));
        if let Err(error) = self.remap_shuffle(&old, true) {
            *self = old;
            return Err(error);
        }
        self.touch()
    }
}
/// A track given its chapter's icon when it has none, so it looks the same elsewhere.
fn inherit_icon(mut track: Track, icon: &Option<IconReference>) -> Track {
    if track.icon.is_none() {
        track.icon = icon.clone();
    }
    track
}
/// A new chapter holding one track, titled and illustrated like it.
fn solo_chapter(track: Track, icon: &Option<IconReference>) -> Chapter {
    let icon = track.icon.clone().or_else(|| icon.clone());
    Chapter {
        id: ChapterId::new(),
        key: ChapterKey::fresh(),
        title: track.title.clone(),
        icon,
        tracks: vec![track],
        extra: Extra::new(),
    }
}
