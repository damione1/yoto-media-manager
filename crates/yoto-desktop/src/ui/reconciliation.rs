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
