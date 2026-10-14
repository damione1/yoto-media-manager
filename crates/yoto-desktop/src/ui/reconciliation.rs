use yoto_domain::Playlist;

pub(super) enum CanonicalUpdate {
    Present(Box<Playlist>),
    Deleted,
}
impl CanonicalUpdate {
    pub(super) fn into_playlist(self) -> Option<Playlist> {
        match self {
            Self::Present(p) => Some(*p),
            Self::Deleted => None,
        }
    }
}
/// Canonical updates continue arriving during a save. Both success and failure
/// must drain the latest update before another save or hydration is allowed.
#[derive(Default)]
pub(super) struct SaveUpdates {
    saving: bool,
    queued: Option<CanonicalUpdate>,
}
impl SaveUpdates {
    #[cfg(test)]
    pub(super) fn is_saving(&self) -> bool {
        self.saving
    }
    pub(super) fn begin(&mut self) {
        self.saving = true;
    }
    pub(super) fn receive(&mut self, canonical: Option<Playlist>) -> Option<CanonicalUpdate> {
        let update = match canonical {
            Some(p) => CanonicalUpdate::Present(Box::new(p)),
            None => CanonicalUpdate::Deleted,
        };
        if self.saving {
            if let Some(CanonicalUpdate::Present(current)) = &self.queued
                && let CanonicalUpdate::Present(incoming) = &update
                && incoming.revision < current.revision
            {
                return None;
            }
            self.queued = Some(update);
            None
        } else {
            Some(update)
        }
    }
    pub(super) fn finish(&mut self) -> Option<CanonicalUpdate> {
        self.saving = false;
        self.queued.take()
    }
}

/// The draft to keep once a write to Yoto has completed while the parent kept editing.
///
/// `sent` is what the write was based on, `canonical` what Yoto now holds and `local` the
/// draft in the editor. Every value the parent changed since `sent` is kept (the latest local
/// state wins); everything else follows `canonical`, which carries what Yoto filled in (such
/// as uploaded audio). Chapters and tracks are matched by id, so a track moved or retitled
/// locally keeps what Yoto changed in it.
pub(super) fn merge_local_edits(
    sent: &Playlist,
    canonical: &Playlist,
    local: &Playlist,
) -> Playlist {
    let mut local = local.clone();
    local.revision = sent.revision;
    let (Ok(sent_value), Ok(canonical_value), Ok(local_value)) = (
        serde_json::to_value(sent),
        serde_json::to_value(canonical),
        serde_json::to_value(&local),
    ) else {
        return canonical.clone();
    };
    let index = |value: &Value| {
        let mut found = BTreeMap::new();
        index_ids(value, &mut found);
        found
    };
    let ids = Ids {
        sent: index(&sent_value),
        canonical: index(&canonical_value),
    };
    let merged = merge(Some(&sent_value), &canonical_value, &local_value, &ids);
    match serde_json::from_value::<Playlist>(merged) {
        Ok(mut playlist) => {
            playlist.revision = canonical.revision;
            playlist
        }
        Err(_) => canonical.clone(),
    }
}

use serde_json::Value;
use std::collections::BTreeMap;

/// Every object with an `id` in `sent` and `canonical`, wherever it sits, so an element moved
/// to another chapter is still merged with its own earlier versions.
struct Ids {
    sent: BTreeMap<String, Value>,
    canonical: BTreeMap<String, Value>,
}
fn element_id(value: &Value) -> Option<&str> {
    value.get("id").and_then(Value::as_str)
}
fn index_ids(value: &Value, found: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) => {
            if let Some(id) = element_id(value) {
                found.insert(id.to_owned(), value.clone());
            }
            for child in map.values() {
                index_ids(child, found);
            }
        }
        Value::Array(items) => {
            for child in items {
                index_ids(child, found);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}
/// A list of objects that all carry an id (chapters, tracks).
fn identified(items: &[Value]) -> bool {
    !items.is_empty() && items.iter().all(|item| element_id(item).is_some())
}
fn merge(sent: Option<&Value>, canonical: &Value, local: &Value, ids: &Ids) -> Value {
    if sent == Some(local) {
        return canonical.clone();
    }
    // Containers are still walked: an element moved here from elsewhere must pick up what
    // Yoto changed in it at its old place.
    if sent == Some(canonical) && !matches!(local, Value::Object(_) | Value::Array(_)) {
        return local.clone();
    }
    match (sent, canonical, local) {
        (Some(Value::Object(s)), Value::Object(c), Value::Object(l)) => {
            let mut merged = serde_json::Map::new();
            for key in c.keys().chain(l.keys()) {
                if merged.contains_key(key) {
                    continue;
                }
                let value = match (s.get(key), c.get(key), l.get(key)) {
                    (s, Some(c), Some(l)) => Some(merge(s, c, l, ids)),
                    // Removed on one side: kept only if the other side changed it.
                    (Some(s), Some(c), None) => (s != c).then(|| c.clone()),
                    (Some(s), None, Some(l)) => (s != l).then(|| l.clone()),
                    (None, Some(c), None) => Some(c.clone()),
                    (None, None, Some(l)) => Some(l.clone()),
                    (_, None, None) => None,
                };
                if let Some(value) = value {
                    merged.insert(key.clone(), value);
                }
            }
            Value::Object(merged)
        }
        (Some(Value::Array(s)), Value::Array(c), Value::Array(l))
            if identified(l) || identified(c) =>
        {
            merge_identified(s, c, l, ids)
        }
        // Both changed the same value, or the shapes differ: the latest local state wins.
        _ => local.clone(),
    }
}
/// Chapters or tracks: local order and membership win when the parent changed them;
/// elements Yoto added are kept at the end.
fn merge_identified(sent: &[Value], canonical: &[Value], local: &[Value], ids: &Ids) -> Value {
    let order = |items: &[Value]| -> Vec<String> {
        items
            .iter()
            .filter_map(element_id)
            .map(str::to_owned)
            .collect()
    };
    let (sent_order, canonical_order, local_order) = (order(sent), order(canonical), order(local));
    let find = |items: &[Value], id: &str| {
        items
            .iter()
            .find(|item| element_id(item) == Some(id))
            .cloned()
    };
    let mut chosen: Vec<String> = if local_order != sent_order {
        local_order
            .iter()
            .filter(|id| !sent_order.contains(id) || canonical_order.contains(id))
            .cloned()
            .collect()
    } else {
        canonical_order
            .iter()
            .filter(|id| local_order.contains(id) || !sent_order.contains(id))
            .cloned()
            .collect()
    };
    for id in &canonical_order {
        if !chosen.contains(id) && !sent_order.contains(id) && !local_order.contains(id) {
            chosen.push(id.clone());
        }
    }
    Value::Array(
        chosen
            .iter()
            .filter_map(|id| {
                let local = find(local, id);
                let canonical = find(canonical, id).or_else(|| ids.canonical.get(id).cloned());
                let sent = find(sent, id).or_else(|| ids.sent.get(id).cloned());
                match (canonical, local) {
                    (Some(c), Some(l)) => Some(merge(sent.as_ref(), &c, &l, ids)),
                    (Some(c), None) => Some(c),
                    (None, Some(l)) => Some(l),
                    (None, None) => None,
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_snapshot_is_retained_until_both_save_outcomes_and_deletion() {
        use std::sync::{Arc, Barrier, Mutex};
        for success in [true, false] {
            let state = Arc::new(Mutex::new(SaveUpdates::default()));
            state.lock().unwrap().begin();
            let arrived = Arc::new(Barrier::new(2));
            let release = Arc::new(Barrier::new(2));
            let receiver = state.clone();
            let a = arrived.clone();
            let r = release.clone();
            let external = std::thread::spawn(move || {
                let mut canonical = Playlist::new("External".into()).unwrap();
                canonical.touch().unwrap();
                assert!(receiver.lock().unwrap().receive(Some(canonical)).is_none());
                a.wait();
                r.wait();
            });
            arrived.wait();
            // This is the callback ordering used for either a successful CAS or
            // a failed CAS: the external version has already been consumed by AppView.
            assert!(state.lock().unwrap().is_saving());
            let stale = Playlist::new("Stale snapshot".into()).unwrap();
            assert!(state.lock().unwrap().receive(Some(stale)).is_none());
            let canonical = state
                .lock()
                .unwrap()
                .finish()
                .unwrap()
                .into_playlist()
                .unwrap();
            assert_eq!(canonical.title, "External");
            let base = if success { 2 } else { 1 };
            assert!(canonical.revision.0 >= base);
            release.wait();
            external.join().unwrap();
            state.lock().unwrap().begin();
            assert!(state.lock().unwrap().receive(None).is_none());
            assert!(
                state
                    .lock()
                    .unwrap()
                    .finish()
                    .unwrap()
                    .into_playlist()
                    .is_none()
            );
        }
    }
}

#[cfg(test)]
mod merge_tests {
    use super::merge_local_edits;
    use yoto_domain::*;
    fn track(title: &str, url: &str) -> Track {
        Track {
            id: TrackId::new(),
            key: TrackKey::new(format!("k{}", TrackId::new().to_string().replace('-', "")))
                .unwrap(),
            title: title.into(),
            icon: None,
            events: None,
            extra: Extra::new(),
            media: TrackMedia::Stream {
                url: url.into(),
                format: AudioFormat::Mp3,
            },
        }
    }
    fn chapter(title: &str, tracks: Vec<Track>) -> Chapter {
        Chapter {
            id: ChapterId::new(),
            key: ChapterKey::new(format!(
                "c{}",
                ChapterId::new().to_string().replace('-', "")
            ))
            .unwrap(),
            title: title.into(),
            icon: None,
            extra: Extra::new(),
            tracks,
        }
    }
    /// A playlist as sent, and the same one as Yoto confirmed it: a newer revision, and
    /// the audio of the first track replaced by Yoto's copy.
    fn sent_and_confirmed() -> (Playlist, Playlist) {
        let mut sent = Playlist::new("Histoires".into()).unwrap();
        sent.chapters = vec![
            chapter("Un", vec![track("Un", "https://local.test/1.mp3")]),
            chapter("Deux", vec![track("Deux", "https://local.test/2.mp3")]),
            chapter(
                "Trois",
                vec![
                    track("Trois A", "https://local.test/3a.mp3"),
                    track("Trois B", "https://local.test/3b.mp3"),
                ],
            ),
        ];
        let mut confirmed = sent.clone();
        confirmed.revision = Revision(sent.revision.0 + 1);
        confirmed.chapters[0].tracks[0].media = TrackMedia::Stream {
            url: "https://yoto.test/1.mp3".into(),
            format: AudioFormat::Mp3,
        };
        (sent, confirmed)
    }
    fn urls(p: &Playlist) -> Vec<String> {
        p.chapters
            .iter()
            .flat_map(|c| &c.tracks)
            .map(|t| match &t.media {
                TrackMedia::Stream { url, .. } => url.clone(),
                TrackMedia::Local { .. } | TrackMedia::RemoteAudio { .. } => String::new(),
            })
            .collect()
    }

    #[test]
    fn without_local_edits_the_confirmed_version_is_kept() {
        let (sent, confirmed) = sent_and_confirmed();
        let merged = merge_local_edits(&sent, &confirmed, &sent);
        assert_eq!(
            serde_json::to_value(&merged).unwrap(),
            serde_json::to_value(&confirmed).unwrap()
        );
    }

    #[test]
    fn edits_made_during_the_write_survive_and_take_what_yoto_filled_in() {
        let (sent, confirmed) = sent_and_confirmed();
        let mut local = sent.clone();
        local.title = "Histoires du soir".into();
        local.metadata.author = "Mamie".into();
        local.chapters[0].tracks[0].title = "Le début".into();
        local.chapters.swap(0, 1);
        let merged = merge_local_edits(&sent, &confirmed, &local);
        assert_eq!(merged.revision, confirmed.revision);
        assert_eq!(merged.title, "Histoires du soir");
        assert_eq!(merged.metadata.author, "Mamie");
        let titles: Vec<_> = merged.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Deux", "Un", "Trois"]);
        assert_eq!(merged.chapters[1].tracks[0].title, "Le début");
        // The retitled, moved track still carries Yoto's audio.
        assert_eq!(
            urls(&merged),
            [
                "https://local.test/2.mp3",
                "https://yoto.test/1.mp3",
                "https://local.test/3a.mp3",
                "https://local.test/3b.mp3"
            ]
        );
    }

    #[test]
    fn a_track_moved_to_another_chapter_keeps_yotos_changes() {
        let (sent, confirmed) = sent_and_confirmed();
        let mut local = sent.clone();
        let moved = local.chapters[0].tracks.remove(0);
        local.chapters.remove(0);
        local.chapters[1].tracks.push(moved);
        let merged = merge_local_edits(&sent, &confirmed, &local);
        assert_eq!(merged.chapters.len(), 2);
        assert_eq!(
            urls(&merged),
            [
                "https://local.test/2.mp3",
                "https://local.test/3a.mp3",
                "https://local.test/3b.mp3",
                "https://yoto.test/1.mp3"
            ]
        );
    }

    #[test]
    fn tracks_yoto_added_meanwhile_are_kept_next_to_local_removals() {
        let (sent, mut confirmed) = sent_and_confirmed();
        confirmed.chapters.push(chapter(
            "Quatre",
            vec![track("Quatre", "https://yoto.test/4.mp3")],
        ));
        let mut local = sent.clone();
        local.chapters[2].tracks.remove(1);
        let merged = merge_local_edits(&sent, &confirmed, &local);
        let titles: Vec<_> = merged.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Un", "Deux", "Trois", "Quatre"]);
        assert_eq!(merged.chapters[2].tracks.len(), 1);
    }

    #[test]
    fn when_both_changed_a_value_the_local_one_wins() {
        let (sent, mut confirmed) = sent_and_confirmed();
        confirmed.metadata.description = "Yoto".into();
        let mut local = sent.clone();
        local.metadata.description = "Ici".into();
        let merged = merge_local_edits(&sent, &confirmed, &local);
        assert_eq!(merged.metadata.description, "Ici");
    }
}
