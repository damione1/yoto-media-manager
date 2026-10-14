use super::*;
use gpui_kit::prelude::FluentBuilder;
#[path = "playlist_media_picker.rs"]
mod media_picker;
#[path = "playlist_move_picker.rs"]
mod move_picker;
use super::autosave::{Autosave, Change};
use super::wording::autosave as text;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{WindowExt, spinner::Spinner};
pub(super) use media_picker::{MediaChoices, prepare_media_choices};
use std::time::Instant;
/// What a write to Yoto was started for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CommitTarget {
    /// The grouped autosave of the editor's draft.
    Editor,
    /// Media added from the library or the « Ajouter des médias » picker.
    MediaPicker,
    /// Deleting the playlist from the Yoto account.
    Delete,
}
enum PlaylistOperation {
    Submitting {
        target: CommitTarget,
    },
    Pending {
        job: JobId,
        state: JobState,
        target: CommitTarget,
    },
    Failed(Failure),
}
/// Where the open playlist stands with Yoto, as its tags show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyncState {
    /// Changes wait for their write, or a write is on its way to Yoto.
    Sending,
    /// Something needs the parent: a blocked upload, a newer or deleted Yoto version.
    ToCheck,
    /// The last changes could not be written (invalid entry or failed write).
    Unsaved,
    /// What is shown is what Yoto has.
    OnYoto,
    /// The last write to Yoto failed; what is shown is still what Yoto has.
    Failed,
}
impl SyncState {
    /// A running save wins over anything to check, which wins over changes left unsaved,
    /// which win over « on Yoto ».
    fn of(sending: bool, needs_attention: bool, unsaved: bool) -> Self {
        if sending {
            Self::Sending
        } else if needs_attention {
            Self::ToCheck
        } else if unsaved {
            Self::Unsaved
        } else {
            Self::OnYoto
        }
    }
    fn track_label(self) -> &'static str {
        match self {
            Self::Sending => text::track_sending(),
            Self::ToCheck => text::to_check(),
            Self::Unsaved => text::unsaved(),
            // A failed write leaves the tracks as Yoto holds them.
            Self::OnYoto | Self::Failed => text::track_on_yoto(),
        }
    }
    fn playlist_label(self) -> &'static str {
        match self {
            Self::Sending => text::saving(),
            Self::ToCheck | Self::Unsaved => self.track_label(),
            Self::OnYoto => text::saved(),
            Self::Failed => text::send_failed(),
        }
    }
    fn family(self) -> &'static theme::Family {
        match self {
            Self::Sending => &theme::BUTTER,
            Self::ToCheck | Self::Unsaved | Self::Failed => &theme::ROSE,
            Self::OnYoto => &theme::MINT,
        }
    }
}
fn changed_on_yoto() -> &'static str {
    tr!(
        "La playlist a changé sur Yoto. Choisissez « Recharger la version Yoto » avant de continuer."
    )
}
fn deleted_on_yoto() -> &'static str {
    tr!("Cette playlist a été supprimée sur Yoto. Choisissez une autre playlist dans la liste.")
}
/// Status line while a save is on its way to Yoto.
fn pending_status(state: &JobState) -> String {
    match state {
        JobState::Queued => tr!("Envoi sur Yoto en attente…").into(),
        JobState::Running { phase } => format!("{}…", phase_text(phase)),
        JobState::Retry { failure, .. } => {
            tr!(
                "Nouvel essai automatique : {reason}",
                reason = failure_text(failure)
            )
        }
        JobState::WaitingForAuthentication => {
            tr!("Reconnectez le compte Yoto pour terminer l’envoi.").into()
        }
        JobState::Conflict { .. } => {
            tr!("La playlist a changé sur Yoto. Vérifiez la version distante avant de continuer.")
                .into()
        }
        JobState::Reconciling => tr!(
            "Yoto a peut-être reçu la modification. Vérification requise avant un nouvel envoi."
        )
        .into(),
        JobState::Completed { .. } | JobState::Failed { .. } | JobState::Cancelled => {
            wording::labels::job_state(state).into()
        }
    }
}
/// Where an editor stands when the parent quits the app.
pub(super) enum QuitState {
    /// Nothing waits to be written.
    Settled,
    /// A write is starting, or local changes wait for the one in flight.
    Waiting,
    /// Changes could not be written; the reason, in plain French.
    Unsaved(String),
}
pub struct PlaylistEditor {
    pub draft: Playlist,
    parent: WeakEntity<AppView>,
    replacement: Option<TrackId>,
    icon_previews: BTreeMap<AssetId, PixelArt>,
    pub(super) service: Arc<Service>,
    pub(super) runtime: Arc<tokio::runtime::Runtime>,
    title: Entity<InputState>,
    description: Entity<InputState>,
    author: Entity<InputState>,
    narrator: Entity<InputState>,
    languages: Entity<InputState>,
    resume: Entity<InputState>,
    min_age: Entity<InputState>,
    max_age: Entity<InputState>,
    shuffle: Entity<InputState>,
    shuffle_projection: String,
    chapter_titles: BTreeMap<ChapterId, Entity<InputState>>,
    track_titles: BTreeMap<TrackId, Entity<InputState>>,
    event_params: BTreeMap<(TrackId, EventTrigger), Entity<InputState>>,
    stream: Entity<InputState>,
    tasks: Vec<Task<()>>,
    pub(super) status: String,
    base_revision: Revision,
    base: Playlist,
    external: Option<Playlist>,
    deleted: bool,
    save_updates: reconciliation::SaveUpdates,
    operation: Option<PlaylistOperation>,
    /// When local changes are written; one write in flight at a time.
    autosave: Autosave,
    /// Only the latest scheduled autosave timer acts.
    timer_generation: u64,
    /// The playlist the write in flight was based on, to merge edits made meanwhile.
    sent: Option<Playlist>,
    /// Media to add once the pending changes are written.
    queued_media: Option<Vec<(MediaId, RecipeId)>>,
    /// Why the last changes could not be written, until a later write succeeds.
    unsaved: Option<String>,
    /// The publication job that failed last, which « Réessayer » sends again.
    failed_job: Option<(JobId, CommitTarget)>,
    /// Tell the parent in a toast when the next write fails (they are leaving the playlist).
    report_failures: bool,
    /// Yoto's icon list to refresh once a new icon is written.
    refresh_icons: Option<AccountId>,
    /// The playlist is being deleted on Yoto.
    deleting: bool,
    picker_window: Option<AnyWindowHandle>,
    /// Why the icon chosen in the open icon picker was refused, shown inside the picker.
    icon_error: Option<String>,
    _subscriptions: Vec<Subscription>,
    options_open: bool,
    advanced_open: bool,
    expanded_tracks: std::collections::BTreeSet<TrackId>,
    /// The version playing on this computer from a track's « Écouter » button.
    previewing: Option<RecipeId>,
    preview_generation: u64,
    /// Mirrors « Mode avancé »: shows interactive playback, shuffle ranges, streams and
    /// raw event parameters.
    pub(super) advanced: bool,
}
impl PlaylistEditor {
    /// Input placeholders, again in the interface language after it changed.
    pub fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholders = [
            (&self.description, tr!("Description")),
            (&self.author, tr!("Auteur")),
            (&self.narrator, tr!("Narrateur")),
            (&self.languages, tr!("Langues : fr, en…")),
            (&self.min_age, tr!("Âge minimum")),
            (&self.max_age, tr!("Âge maximum")),
            (
                &self.shuffle,
                tr!("Aléatoire : début-fin-limite (chapitres dès 1)"),
            ),
            (&self.stream, tr!("URL audio HTTPS de streaming")),
        ];
        let events = self
            .event_params
            .values()
            .map(|input| (input, tr!("Paramètres de l’événement")));
        for (input, placeholder) in placeholders.into_iter().chain(events) {
            input.update(cx, |s, cx| s.set_placeholder(placeholder, window, cx));
        }
    }
    pub fn new(
        mut draft: Playlist,
        parent: WeakEntity<AppView>,
        service: Arc<Service>,
        runtime: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Hydrate older catalogue documents which retained events in the extension map.
        for c in &mut draft.chapters {
            for t in &mut c.tracks {
                if t.events.is_none()
                    && let Some(old) = t.extra.get("events")
                    && let Ok(events) = serde_json::from_value(old.clone())
                {
                    t.events = Some(events);
                    t.extra.remove("events");
                }
            }
        }
        let title = cx.new(|cx| InputState::new(window, cx).default_value(draft.title.clone()));
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!("Description"))
                .default_value(draft.metadata.description.clone())
        });
        let author = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!("Auteur"))
                .default_value(draft.metadata.author.clone())
        });
        let narrator = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!("Narrateur"))
                .default_value(draft.metadata.narrator.clone())
        });
        let languages = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!("Langues : fr, en…"))
                .default_value(draft.metadata.languages.join(", "))
        });
        let resume = cx.new(|cx| {
            InputState::new(window, cx).default_value(draft.playback.resume_seconds.to_string())
        });
        let min_age = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!("Âge minimum"))
                .default_value(age_text(draft.metadata.min_age))
        });
        let max_age = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!("Âge maximum"))
                .default_value(age_text(draft.metadata.max_age))
        });
        let shuffle = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!("Aléatoire : début-fin-limite (chapitres dès 1)"))
                .default_value(shuffle_text(&draft))
        });
        let stream = cx
            .new(|cx| InputState::new(window, cx).placeholder(tr!("URL audio HTTPS de streaming")));
        let mut chapter_titles = BTreeMap::new();
        let mut track_titles = BTreeMap::new();
        for c in &draft.chapters {
            chapter_titles.insert(
                c.id,
                cx.new(|cx| InputState::new(window, cx).default_value(c.title.clone())),
            );
            for t in &c.tracks {
                track_titles.insert(
                    t.id,
                    cx.new(|cx| InputState::new(window, cx).default_value(t.title.clone())),
                );
            }
        }
        let mut event_params = BTreeMap::new();
        for c in &draft.chapters {
            for t in &c.tracks {
                for trigger in EventTrigger::ALL {
                    let value = t
                        .events
                        .as_ref()
                        .and_then(|e| event(e, *trigger).as_ref())
                        .and_then(|e| e.params.as_ref())
                        .map(|p| serde_json::to_string(p).expect("event params"))
                        .unwrap_or_else(|| "{}".into());
                    event_params.insert(
                        (t.id, *trigger),
                        cx.new(|cx| {
                            InputState::new(window, cx)
                                .default_value(value)
                                .placeholder(tr!("Paramètres de l’événement"))
                        }),
                    );
                }
            }
        }
        let mut view = Self {
            parent,
            replacement: None,
            icon_previews: BTreeMap::new(),
            event_params,
            base_revision: draft.revision,
            base: draft.clone(),
            external: None,
            deleted: false,
            save_updates: Default::default(),
            operation: None,
            autosave: Autosave::default(),
            timer_generation: 0,
            sent: None,
            queued_media: None,
            unsaved: None,
            failed_job: None,
            report_failures: false,
            refresh_icons: None,
            deleting: false,
            picker_window: None,
            icon_error: None,
            _subscriptions: vec![],
            options_open: false,
            advanced_open: false,
            expanded_tracks: Default::default(),
            previewing: None,
            preview_generation: 0,
            advanced: false,
            draft,
            service,
            runtime,
            title,
            description,
            author,
            narrator,
            languages,
            resume,
            min_age,
            max_age,
            shuffle_projection: shuffle.read(cx).value().to_string(),
            shuffle,
            chapter_titles,
            track_titles,
            stream,
            tasks: vec![],
            status: String::new(),
        };
        let icons: Vec<_> = view
            .draft
            .chapters
            .iter()
            .flat_map(|c| std::iter::once(&c.icon).chain(c.tracks.iter().map(|t| &t.icon)))
            .filter_map(|i| match i {
                Some(IconReference::Local { asset }) => Some(*asset),
                Some(IconReference::Yoto { .. }) | None => None,
            })
            .collect();
        for asset in icons {
            view.cache_icon(asset, cx);
        }
        view.subscribe_inputs(window, cx);
        view
    }
    pub fn reconcile(&mut self, canonical: Option<Playlist>, cx: &mut Context<Self>) {
        let Some(update) = self.save_updates.receive(canonical) else {
            return;
        };
        let canonical = update.into_playlist();
        if canonical
            .as_ref()
            .is_some_and(|p| p.revision == self.base_revision)
        {
            return;
        }
        let clean = self.unchanged(cx);
        self.deleted = canonical.is_none();
        self.external = canonical.clone();
        if clean && let Some(canonical) = canonical {
            let entity = cx.entity();
            cx.defer(move |cx| {
                if let Some(window) = cx.windows().first().copied() {
                    let _ = cx.update_window(window, |_, window, cx| {
                        entity.update(cx, |v, cx| {
                            // Deferred hydration rechecks the draft: a keystroke between
                            // reconcile and this callback must never be discarded.
                            let newer = v
                                .external
                                .as_ref()
                                .is_some_and(|p| p.revision > canonical.revision);
                            if canonical.revision <= v.base_revision || newer {
                                return;
                            }
                            if v.unchanged(cx) {
                                v.adopt(canonical, window, cx);
                            } else {
                                v.external = Some(canonical);
                                v.status = changed_on_yoto().into();
                                cx.notify();
                            }
                        })
                    });
                }
            });
        } else {
            self.status = if self.deleted {
                deleted_on_yoto()
            } else {
                changed_on_yoto()
            }
            .into();
        }
        cx.notify();
    }
    /// Whether the draft differs from what was last written or is on its way (a draft that
    /// does not validate counts as edited).
    fn edited_since_last_write(&mut self, cx: &App) -> bool {
        let valid = self.gather(cx).is_ok();
        let reference = self.sent.as_ref().unwrap_or(&self.base);
        !valid || !same_content(&self.draft, reference)
    }
    /// Whether the draft still equals the last saved playlist (a draft that does not
    /// validate counts as changed).
    fn unchanged(&mut self, cx: &App) -> bool {
        let valid = self.gather(cx).is_ok();
        self.draft.revision = self.base_revision;
        valid && serde_json::to_value(&self.draft).ok() == serde_json::to_value(&self.base).ok()
    }
    fn adopt(&mut self, draft: Playlist, window: &mut Window, cx: &mut Context<Self>) {
        // Sections the parent opened stay open across the reload that follows each save.
        let open = (
            self.options_open,
            self.advanced_open,
            std::mem::take(&mut self.expanded_tracks),
        );
        let preview = (self.previewing, self.preview_generation);
        let advanced = self.advanced;
        *self = Self::new(
            draft,
            self.parent.clone(),
            self.service.clone(),
            self.runtime.clone(),
            window,
            cx,
        );
        (self.options_open, self.advanced_open, self.expanded_tracks) = open;
        // A track still playing keeps its « Arrêter » button and its follower.
        (self.previewing, self.preview_generation) = preview;
        self.advanced = advanced;
        self.status = tr!("Playlist actualisée depuis Yoto.").into();
        cx.notify();
    }
    fn gather(&mut self, cx: &App) -> yoto_domain::Result<()> {
        self.draft.title = self.title.read(cx).value().to_string();
        Rules::title(&self.draft.title)?;
        self.draft.metadata.description = self.description.read(cx).value().to_string();
        self.draft.metadata.author = self.author.read(cx).value().to_string();
        self.draft.metadata.narrator = self.narrator.read(cx).value().to_string();
        self.draft.metadata.languages = self
            .languages
            .read(cx)
            .value()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        self.draft.playback.resume_seconds =
            self.resume.read(cx).value().parse().map_err(|_| {
                Failure::Validation(
                    tr!("La durée de reprise doit être un nombre entier de secondes.").into(),
                )
            })?;
        let age = |input: &Entity<InputState>| {
            let s = input.read(cx).value().to_string();
            if s.trim().is_empty() {
                Ok(None)
            } else {
                s.parse::<u8>()
                    .map(Some)
                    .map_err(|_| Failure::Validation(tr!("Âge entier attendu").into()))
            }
        };
        self.draft.metadata.min_age = age(&self.min_age)?;
        self.draft.metadata.max_age = age(&self.max_age)?;
        if let (Some(a), Some(b)) = (self.draft.metadata.min_age, self.draft.metadata.max_age)
            && a > b
        {
            return Err(Failure::Validation(
                tr!("Âge minimum supérieur au maximum").into(),
            ));
        }
        let projection = self.shuffle.read(cx).value().to_string();
        if projection != self.shuffle_projection {
            self.draft.playback.shuffle = parse_shuffle(&projection)?;
            self.shuffle_projection = projection;
        }
        let base_chapters: BTreeMap<_, _> = self
            .base
            .chapters
            .iter()
            .map(|chapter| (chapter.id, chapter))
            .collect();
        for c in &mut self.draft.chapters {
            if let Some(input) = self.chapter_titles.get(&c.id) {
                c.title = input.read(cx).value().to_string();
            }
            for t in &mut c.tracks {
                if let Some(events) = &mut t.events {
                    for trigger in EventTrigger::ALL {
                        if let Some(e) = event_mut(events, *trigger).as_mut()
                            && let Some(input) = self.event_params.get(&(t.id, *trigger))
                        {
                            e.params = Some(
                                serde_json::from_str(&input.read(cx).value()).map_err(|_| {
                                    Failure::Validation(
                                        tr!("Les paramètres doivent être un objet JSON").into(),
                                    )
                                })?,
                            );
                        }
                    }
                }
                if let Some(input) = self.track_titles.get(&t.id) {
                    t.title = input.read(cx).value().to_string();
                }
            }
            coalesce_single_track_title(c, base_chapters.get(&c.id).copied());
        }
        self.draft.touch()
    }
    pub(super) fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let rt = self.runtime.clone();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move { service.execute(command).await })
                .await;
            let _ = weak.update(cx, |view, cx| {
                view.status = match joined(result) {
                    Ok(_) => tr!("Opération terminée.").into(),
                    Err(e) => failure_text(&e),
                };
                cx.notify();
            });
        }));
    }
    /// Listen to a track's version on this computer, or stop it when it is already playing.
    /// The button follows the player until playback ends, wherever it was stopped from.
    fn toggle_preview(&mut self, recipe: RecipeId, media: MediaId, cx: &mut Context<Self>) {
        self.preview_generation = self.preview_generation.wrapping_add(1);
        let generation = self.preview_generation;
        let service = self.service.clone();
        let rt = self.runtime.clone();
        if self.previewing == Some(recipe) {
            self.previewing = None;
            cx.notify();
            rt.spawn(async move {
                let _ = service.execute(Command::StopPreview).await;
            });
            return;
        }
        self.previewing = Some(recipe);
        cx.notify();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let started = {
                let service = service.clone();
                rt.spawn(async move { service.execute(Command::Preview(recipe)).await })
                    .await
            };
            let failure = match started {
                Ok(Ok(_)) => None,
                Ok(Err(e)) => Some(e),
                Err(e) => Some(Failure::Storage(e.to_string())),
            };
            if let Some(failure) = failure {
                let _ = weak.update(cx, |v, cx| {
                    if v.preview_generation == generation {
                        v.previewing = None;
                        v.status = failure_text(&failure);
                        cx.notify();
                    }
                });
                return;
            }
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                let service = service.clone();
                let playing = match rt
                    .spawn(async move { service.execute(Command::PreviewStatus).await })
                    .await
                {
                    Ok(Ok(Reply::Data(data))) => matches!(
                        serde_json::from_value::<PreviewStatus>(data),
                        Ok(PreviewStatus::Playing { media: id, .. }) if id == media
                    ),
                    _ => false,
                };
                let current = weak
                    .update(cx, |v, cx| {
                        if v.preview_generation != generation {
                            return false;
                        }
                        if !playing {
                            v.previewing = None;
                            cx.notify();
                        }
                        playing
                    })
                    .unwrap_or(false);
                if !current {
                    break;
                }
            }
        }));
    }
    /// The draft changed: write it once the burst of edits is over.
    fn changed(&mut self, change: Change, cx: &mut Context<Self>) {
        self.autosave.record(change, Instant::now());
        self.schedule(cx);
        cx.notify();
    }
    /// Write pending changes now (a field lost focus, Enter was pressed). Changes not yet
    /// recorded, such as a failed write's, are picked up from the draft.
    pub(super) fn flush(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        if !self.autosave.has_pending() && !self.deleting && self.edited_since_last_write(cx) {
            self.autosave.record(Change::Discrete, now);
        }
        self.autosave.flush(now);
        self.schedule(cx);
        cx.notify();
    }
    /// The parent leaves this playlist (another playlist, another page, quitting): write
    /// what is pending now, and report a failure in a toast since the editor is out of sight.
    pub(super) fn flush_before_leaving(&mut self, cx: &mut Context<Self>) {
        self.report_failures = true;
        self.flush(cx);
    }
    /// Where this playlist stands while the app waits to quit.
    pub(super) fn quit_state(&self) -> QuitState {
        let pending = self.autosave.has_pending() || self.queued_media.is_some();
        if pending && self.held() {
            return QuitState::Unsaved(if self.status.is_empty() {
                text::needs_check().into()
            } else {
                self.status.clone()
            });
        }
        if pending || matches!(self.operation, Some(PlaylistOperation::Submitting { .. })) {
            return QuitState::Waiting;
        }
        match &self.unsaved {
            Some(reason) => QuitState::Unsaved(reason.clone()),
            None => QuitState::Settled,
        }
    }
    /// The playlist's title on Yoto, to name it in messages.
    pub(super) fn title(&self) -> &str {
        &self.base.title
    }
    /// Writes wait while something needs the parent first.
    fn held(&self) -> bool {
        self.blocked() || self.external.is_some() || self.deleted
    }
    /// Run [`Self::pump`] when the next write is due. Only the latest timer acts, and it
    /// always runs later on its own, never inside the caller's update.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        self.timer_generation = self.timer_generation.wrapping_add(1);
        let generation = self.timer_generation;
        let now = Instant::now();
        let due = if self.held() {
            None
        } else if self.queued_media.is_some() && !self.autosave.in_flight() {
            Some(now)
        } else {
            self.autosave.next_due()
        };
        let Some(due) = due else {
            return;
        };
        let delay = due.saturating_duration_since(now);
        cx.spawn(async move |weak, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let _ = weak.update(cx, |v, cx| {
                if v.timer_generation == generation {
                    v.pump(cx);
                }
            });
        })
        .detach();
    }
    /// Start whatever write is due: the grouped changes first, then media waiting for them.
    fn pump(&mut self, cx: &mut Context<Self>) {
        if self.held() {
            return;
        }
        let now = Instant::now();
        if self.queued_media.is_some() {
            self.autosave.flush(now);
        }
        if self.autosave.take_due(now) {
            self.write_draft(cx);
        }
        if !self.autosave.in_flight()
            && !self.autosave.has_pending()
            && let Some(media) = self.queued_media.take()
        {
            self.write_media(media, cx);
        }
        self.schedule(cx);
        cx.notify();
    }
    fn abandon_images(&mut self, cx: &mut Context<Self>) {
        let saved = self.base.image_assets();
        let abandoned: Vec<_> = self
            .draft
            .image_assets()
            .difference(&saved)
            .copied()
            .collect();
        if self
            .draft
            .metadata
            .cover
            .is_some_and(|a| abandoned.contains(&a))
        {
            self.draft.metadata.cover = self.base.metadata.cover;
        }
        for chapter in &mut self.draft.chapters {
            let original = self.base.chapters.iter().find(|c| c.id == chapter.id);
            if matches!(&chapter.icon, Some(IconReference::Local { asset }) if abandoned.contains(asset))
            {
                chapter.icon = original.and_then(|c| c.icon.clone());
            }
            for track in &mut chapter.tracks {
                if matches!(&track.icon, Some(IconReference::Local { asset }) if abandoned.contains(asset))
                {
                    track.icon = original
                        .and_then(|c| c.tracks.iter().find(|t| t.id == track.id))
                        .and_then(|t| t.icon.clone());
                }
            }
        }
        for asset in abandoned {
            self.send(Command::ReleaseAsset(asset), cx);
        }
        self.status =
            tr!("Images non enregistrées abandonnées. Les autres saisies sont conservées.").into();
        cx.notify();
    }
    fn notify_parent(&self, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |_, cx| cx.notify());
        });
    }
    pub(super) fn owning_remote(&self, cx: &App) -> yoto_domain::Result<(AccountId, CardId)> {
        let (account, card) = match &self.base.origin {
            DraftOrigin::Existing { base } => (base.account.clone(), base.card.clone()),
            DraftOrigin::New => {
                return Err(Failure::Validation(
                    tr!("Ouvrez une playlist de votre compte Yoto.").into(),
                ));
            }
        };
        let active = self
            .parent
            .upgrade()
            .and_then(|p| p.read(cx).snapshot.active_account.clone());
        if active.as_ref() != Some(&account) {
            return Err(Failure::Authentication);
        }
        Ok((account, card))
    }
    /// Something needs the parent before this playlist can change again: an uncertain or
    /// conflicting write, a sign-in to renew, or its deletion under way.
    pub(super) fn blocked(&self) -> bool {
        if self.deleting {
            return true;
        }
        match &self.operation {
            Some(PlaylistOperation::Pending { state, .. }) => matches!(
                state,
                JobState::WaitingForAuthentication
                    | JobState::Conflict { .. }
                    | JobState::Reconciling
            ),
            Some(PlaylistOperation::Failed(failure)) => {
                matches!(failure, Failure::Ambiguous | Failure::Conflict)
            }
            Some(PlaylistOperation::Submitting { .. }) | None => false,
        }
    }
    /// A write is on its way to Yoto.
    fn write_running(&self) -> bool {
        match &self.operation {
            Some(PlaylistOperation::Submitting { .. }) => true,
            Some(PlaylistOperation::Pending { state, .. }) => match state {
                JobState::Queued | JobState::Running { .. } | JobState::Retry { .. } => true,
                JobState::WaitingForAuthentication
                | JobState::Conflict { .. }
                | JobState::Reconciling
                | JobState::Completed { .. }
                | JobState::Failed { .. }
                | JobState::Cancelled => false,
            },
            Some(PlaylistOperation::Failed(_)) | None => false,
        }
    }
    /// Why the icon chosen in the open icon picker was refused.
    pub(super) fn icon_error(&self) -> Option<&str> {
        self.icon_error.as_deref()
    }
    /// Media are being added: the media picker waits for them.
    pub(super) fn busy(&self) -> bool {
        let adding = match &self.operation {
            Some(
                PlaylistOperation::Submitting { target }
                | PlaylistOperation::Pending { target, .. },
            ) => *target == CommitTarget::MediaPicker,
            Some(PlaylistOperation::Failed(_)) | None => false,
        };
        self.queued_media.is_some() || (adding && self.write_running())
    }
    /// Write the draft as it stands. `autosave` already counts this write as in flight.
    fn write_draft(&mut self, cx: &mut Context<Self>) {
        let ready = self.owning_remote(cx).and_then(|(account, _)| {
            self.gather(cx)?;
            Ok(account)
        });
        let account = match ready {
            Ok(account) => account,
            Err(failure) => {
                self.autosave.finish();
                self.not_written(failure_text(&failure), cx);
                return;
            }
        };
        self.draft.revision = self.base_revision;
        if same_content(&self.draft, &self.base) {
            self.autosave.finish();
            self.unsaved = None;
            return;
        }
        if let Err(failure) = self.draft.touch() {
            self.autosave.finish();
            self.not_written(failure_text(&failure), cx);
            return;
        }
        let sent = self.draft.clone();
        self.start_operation(
            Command::UpdateYotoPlaylist {
                account,
                draft: Box::new(sent.clone()),
                expected: self.base_revision,
            },
            CommitTarget::Editor,
            sent,
            cx,
        );
    }
    /// Add media from the library on Yoto, now that nothing else is being written.
    fn write_media(&mut self, media: Vec<(MediaId, RecipeId)>, cx: &mut Context<Self>) {
        let (account, card) = match self.owning_remote(cx) {
            Ok(remote) => remote,
            Err(failure) => {
                self.status = failure_text(&failure);
                cx.notify();
                return;
            }
        };
        self.autosave.begin();
        // Yoto adds them to the version it holds, which is the last one written.
        let sent = self.base.clone();
        self.start_operation(
            Command::AddYotoMedia {
                account,
                card,
                media,
                part_title: Some(wording::part_title_template().into()),
            },
            CommitTarget::MediaPicker,
            sent,
            cx,
        );
    }
    /// Changes could not be written: say why in the editor, and in a toast when the parent
    /// has left the playlist.
    fn not_written(&mut self, reason: String, cx: &mut Context<Self>) {
        if self.report_failures {
            self.report(text::not_saved(&self.base.title, &reason), cx);
        }
        self.status = reason.clone();
        self.unsaved = Some(reason);
        cx.notify();
    }
    /// A failure toast that stays until the parent dismisses it.
    fn report(&self, message: String, cx: &mut Context<Self>) {
        let parent = self.parent.clone();
        cx.defer(move |cx| {
            let _ = parent.update(cx, |view, cx| view.alert(message, cx));
        });
    }
    /// Close the picker dialog this editor opened, if one is open.
    fn close_picker(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.picker_window.take() {
            self.replacement = None;
            cx.defer(move |cx| {
                let _ = cx.update_window(handle, |_, window, cx| window.close_dialog(cx));
            });
        }
    }
    fn start_operation(
        &mut self,
        command: Command,
        target: CommitTarget,
        sent: Playlist,
        cx: &mut Context<Self>,
    ) {
        self.operation = Some(PlaylistOperation::Submitting { target });
        self.failed_job = None;
        self.sent = Some(sent);
        self.save_updates.begin();
        if target != CommitTarget::Editor {
            self.status = text::writing().into();
        }
        self.notify_parent(cx);
        cx.notify();
        let service = self.service.clone();
        let rt = self.runtime.clone();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move { service.execute(command).await })
                .await;
            let _ = weak.update(cx, |v, cx| {
                match joined(result).and_then(Reply::into_operation) {
                    Ok((_, job)) => {
                        v.operation = Some(PlaylistOperation::Pending {
                            job,
                            state: JobState::Queued,
                            target,
                        });
                        if target != CommitTarget::Editor {
                            v.status = text::writing_and_checking().into();
                        }
                    }
                    Err(e) => v.fail_operation(e, cx),
                }
                if let Some(parent) = v.parent.upgrade() {
                    let snapshot = parent.read(cx).snapshot.clone();
                    v.apply_snapshot(&snapshot, cx);
                }
                cx.notify();
            });
        }));
    }
    fn fail_operation(&mut self, failure: Failure, cx: &mut Context<Self>) {
        let reason = failure_text(&failure);
        self.status = reason.clone();
        let needs_verification = matches!(failure, Failure::Ambiguous | Failure::Conflict);
        let target = match &self.operation {
            Some(
                PlaylistOperation::Submitting { target }
                | PlaylistOperation::Pending { target, .. },
            ) => Some(*target),
            Some(PlaylistOperation::Failed(_)) | None => None,
        };
        self.operation = Some(PlaylistOperation::Failed(failure));
        self.draft = self.base.clone();
        self.sent = None;
        self.deleting = false;
        self.queued_media = None;
        self.autosave.finish();
        if target == Some(CommitTarget::Editor) {
            self.report(text::not_saved(&self.base.title, &reason), cx);
            self.unsaved = Some(reason);
        }
        let queued = self.save_updates.finish();
        if let Some(update) = queued {
            if needs_verification {
                self.external = update.into_playlist();
            } else {
                self.reconcile(update.into_playlist(), cx);
            }
        }
        self.schedule(cx);
        self.notify_parent(cx);
        cx.notify();
    }
    pub fn apply_snapshot(&mut self, snapshot: &Snapshot, cx: &mut Context<Self>) {
        let (job, target) = match &self.operation {
            Some(PlaylistOperation::Pending { job, target, .. }) => (*job, *target),
            Some(PlaylistOperation::Submitting { .. } | PlaylistOperation::Failed(_)) | None => {
                return;
            }
        };
        let Some(job) = snapshot.jobs.iter().find(|j| j.id == job) else {
            return;
        };
        match &job.state {
            JobState::Completed {
                output: JobOutput::Published { .. },
            } => {
                let Some(canonical) = snapshot
                    .playlists
                    .iter()
                    .find(|p| p.id == self.base.id)
                    .cloned()
                else {
                    return;
                };
                self.complete(canonical, target, cx);
            }
            JobState::Completed {
                output: JobOutput::Imported { .. } | JobOutput::Encoded { .. },
            } => self.fail_operation(
                Failure::Compatibility(tr!("Résultat de playlist inattendu.").into()),
                cx,
            ),
            JobState::Failed { failure } => {
                let failure = failure.clone();
                self.fail_operation(failure, cx);
                self.failed_job = Some((job.id, target));
            }
            JobState::Cancelled => self.fail_operation(Failure::Cancelled, cx),
            state @ (JobState::Queued
            | JobState::Running { .. }
            | JobState::Retry { .. }
            | JobState::WaitingForAuthentication
            | JobState::Conflict { .. }
            | JobState::Reconciling) => {
                let attention = matches!(
                    state,
                    JobState::WaitingForAuthentication
                        | JobState::Conflict { .. }
                        | JobState::Reconciling
                );
                // Routine progress of an autosave shows in the tag by the title only.
                if attention || target != CommitTarget::Editor {
                    self.status = pending_status(state);
                }
                if attention && !self.blocked() {
                    self.notify_parent(cx);
                }
                self.operation = Some(PlaylistOperation::Pending {
                    job: job.id,
                    state: state.clone(),
                    target,
                });
            }
        }
        cx.notify();
    }
    /// A write reached Yoto. The editor keeps what the parent typed or moved meanwhile, on
    /// top of the confirmed version, without rebuilding its fields (focus and scroll stay).
    fn complete(&mut self, canonical: Playlist, target: CommitTarget, cx: &mut Context<Self>) {
        self.save_updates.finish();
        self.autosave.finish();
        self.operation = None;
        let sent = self.sent.take().unwrap_or_else(|| self.base.clone());
        let _ = self.gather(cx);
        let local = self.draft.clone();
        self.draft = if self.autosave.has_pending() || !same_content(&local, &sent) {
            reconciliation::merge_local_edits(&sent, &canonical, &local)
        } else {
            canonical.clone()
        };
        let inputs = self.input_updates(&sent, &canonical, &local);
        self.base_revision = canonical.revision;
        self.base = canonical;
        self.external = None;
        self.unsaved = None;
        self.report_failures = false;
        self.status.clear();
        let picker_window = match target {
            CommitTarget::MediaPicker => self.picker_window.take(),
            CommitTarget::Editor | CommitTarget::Delete => None,
        };
        let refresh_icons = self.refresh_icons.take();
        let parent = self.parent.clone();
        let entity = cx.entity().downgrade();
        cx.defer(move |cx| {
            let Some(handle) = picker_window.or_else(|| cx.windows().first().copied()) else {
                return;
            };
            let _ = cx.update_window(handle, move |_, window, cx| {
                if picker_window.is_some() {
                    window.close_dialog(cx);
                }
                let _ = entity.update(cx, |v, cx| v.hydrate(inputs, window, cx));
                if let Some(account) = refresh_icons {
                    let _ = parent.update(cx, |view, cx| {
                        view.send(Command::RefreshIcons(account), cx);
                    });
                }
            });
        });
        self.schedule(cx);
        self.notify_parent(cx);
        cx.notify();
    }
    /// Fields whose value Yoto changed while the parent left them alone, with Yoto's value.
    fn input_updates(
        &self,
        sent: &Playlist,
        canonical: &Playlist,
        local: &Playlist,
    ) -> Vec<(Entity<InputState>, String)> {
        let mut updates = vec![];
        let mut field = |input: &Entity<InputState>, before: String, after: String, now: String| {
            if before != after && now == before {
                updates.push((input.clone(), after));
            }
        };
        let fields: [(&Entity<InputState>, FieldText); 9] = [
            (&self.title, |p| p.title.clone()),
            (&self.description, |p| p.metadata.description.clone()),
            (&self.author, |p| p.metadata.author.clone()),
            (&self.narrator, |p| p.metadata.narrator.clone()),
            (&self.languages, |p| p.metadata.languages.join(", ")),
            (&self.resume, |p| p.playback.resume_seconds.to_string()),
            (&self.min_age, |p| age_text(p.metadata.min_age)),
            (&self.max_age, |p| age_text(p.metadata.max_age)),
            (&self.shuffle, shuffle_text),
        ];
        for (input, text) in fields {
            field(input, text(sent), text(canonical), text(local));
        }
        let chapter = |p: &Playlist, id: ChapterId| {
            p.chapters
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.title.clone())
        };
        for (id, input) in &self.chapter_titles {
            if let (Some(before), Some(after), Some(now)) = (
                chapter(sent, *id),
                chapter(canonical, *id),
                chapter(local, *id),
            ) {
                field(input, before, after, now);
            }
        }
        let track = |p: &Playlist, id: TrackId| {
            p.chapters
                .iter()
                .flat_map(|c| &c.tracks)
                .find(|t| t.id == id)
                .map(|t| t.title.clone())
        };
        for (id, input) in &self.track_titles {
            if let (Some(before), Some(after), Some(now)) =
                (track(sent, *id), track(canonical, *id), track(local, *id))
            {
                field(input, before, after, now);
            }
        }
        updates
    }
    /// Give new chapters and tracks their fields, and show what Yoto changed in the others.
    fn hydrate(
        &mut self,
        inputs: Vec<(Entity<InputState>, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_inputs(window, cx);
        for (input, value) in inputs {
            if input == self.shuffle {
                self.shuffle_projection = value.clone();
            }
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        cx.notify();
    }
    pub fn open_media_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.owning_remote(cx).is_err() {
            self.status = tr!("Connectez le compte propriétaire de cette playlist.").into();
            cx.notify();
            return;
        }
        let Some(parent) = self.parent.upgrade() else {
            return;
        };
        let choices = parent.read(cx).playlist_media_choices.clone();
        let editor = cx.entity();
        let picker = cx.new(|cx| media_picker::MediaPicker::new(editor.clone(), choices, cx));
        self.picker_window = Some(window.window_handle());
        let weak = editor.downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let close = weak.clone();
            let cancel = weak.clone();
            dialog
                .title(tr!("Ajouter des médias"))
                .w(px(720.))
                .child(picker.clone())
                .on_cancel(move |_, _, cx| cancel.update(cx, |v, _| !v.busy()).unwrap_or(true))
                .on_close(move |_, _, cx| {
                    let _ = close.update(cx, |v, cx| {
                        v.picker_window = None;
                        v.replacement = None;
                        cx.notify();
                    });
                })
        });
    }
    /// The 48 px tile showing a track or chapter icon, or an empty slot without one: what
    /// the player will show, never a decoration it will not.
    fn icon_tile(&self, icon: &Option<IconReference>, seed: &str, cx: &App) -> Div {
        match (icon, self.parent.upgrade()) {
            (Some(icon), Some(parent)) => {
                let parent = parent.read(cx);
                // A media's icon arrives with the media: the library has already read it.
                let previews = match icon {
                    IconReference::Local { asset } if !self.icon_previews.contains_key(asset) => {
                        &parent.icon_previews
                    }
                    IconReference::Local { .. } | IconReference::Yoto { .. } => &self.icon_previews,
                };
                theme::icon_frame(seed, 48., 14.)
                    .child(pixelart::icon_image(icon, previews, parent, 32., cx))
            }
            (Some(_), None) | (None, _) => theme::empty_icon_slot(48., 14., true),
        }
    }
    /// Round icon button opening the icon picker for a chapter or one of its tracks.
    fn icon_button(
        &self,
        id: ElementId,
        icon: &Option<IconReference>,
        seed: &str,
        chapter: ChapterId,
        track: Option<TrackId>,
        cx: &mut Context<Self>,
    ) -> Button {
        use gpui_kit::component::button::{ButtonCustomVariant, ButtonRounded};
        let variant = ButtonCustomVariant::new(cx)
            .color(theme::c(theme::SURFACE).opacity(0.))
            .hover(theme::c(theme::CREAM))
            .active(theme::c(theme::SAND));
        Button::new(id)
            .custom(variant)
            .rounded(ButtonRounded::Size(px(14.)))
            .size(px(48.))
            .p(px(0.))
            .accessibility_label(match (track.is_some(), icon.is_some()) {
                (true, true) => tr!("Changer l’icône de la piste"),
                (false, true) => tr!("Changer l’icône du chapitre"),
                (true, false) => tr!("Choisir une icône pour la piste"),
                (false, false) => tr!("Choisir une icône pour le chapitre"),
            })
            .tooltip(if icon.is_some() {
                tr!("Changer l’icône")
            } else {
                tr!("Choisir une icône")
            })
            .child(self.icon_tile(icon, seed, cx))
            .on_click(
                cx.listener(move |v, _, window, cx| v.open_icon_picker(chapter, track, window, cx)),
            )
    }
    fn render_chapter(
        &mut self,
        index: usize,
        snapshot: &Snapshot,
        state: SyncState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use theme::{Height, Look};
        let Some(chapter) = self.draft.chapters.get(index) else {
            return div().into_any_element();
        };
        let id = chapter.id;
        let single = chapter.tracks.len() == 1;
        let count = self.draft.chapters.len();
        let mut group = div().w_full().flex().flex_col().gap(px(8.));
        if !single {
            let owner = cx.entity().downgrade();
            group = group.child(
                div()
                    .w_full()
                    .flex()
                    .gap(px(12.))
                    .items_center()
                    .px(px(10.))
                    .child(
                        div()
                            .w(px(22.))
                            .text_center()
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .text_size(px(14.))
                            .text_color(theme::c(theme::NUMBER))
                            .child(format!("{}", index + 1)),
                    )
                    .child(self.icon_button(
                        element_id("chapter-icon", id),
                        &chapter.icon,
                        &id.to_string(),
                        id,
                        None,
                        cx,
                    ))
                    .children(
                        self.chapter_titles
                            .get(&id)
                            .map(|input| title_field(Input::new(input), 17.).flex_1().min_w_0()),
                    )
                    .child(
                        theme::round(
                            Button::new(element_id("chapter-menu", id))
                                .icon(IconName::EllipsisVertical)
                                .accessibility_label(tr!("Options du chapitre"))
                                .tooltip(tr!("Options du chapitre")),
                            Look::Ghost,
                            36.,
                            false,
                            cx,
                        )
                        .dropdown_menu(move |menu, _, _| {
                            menu.item(editor_menu_item(
                                &owner,
                                tr!("Changer l’icône du chapitre"),
                                move |v, window, cx| v.open_icon_picker(id, None, window, cx),
                            ))
                            .item(
                                editor_menu_item(
                                    &owner,
                                    tr!("Remonter le chapitre"),
                                    move |v, _, cx| {
                                        if let Err(error) = v.draft.move_chapter(id, -1) {
                                            v.status = failure_text(&error);
                                            return;
                                        }
                                        v.changed(Change::Discrete, cx);
                                    },
                                )
                                .disabled(index == 0),
                            )
                            .item(
                                editor_menu_item(
                                    &owner,
                                    tr!("Descendre le chapitre"),
                                    move |v, _, cx| {
                                        if let Err(error) = v.draft.move_chapter(id, 1) {
                                            v.status = failure_text(&error);
                                            return;
                                        }
                                        v.changed(Change::Discrete, cx);
                                    },
                                )
                                .disabled(index + 1 == count),
                            )
                            .separator()
                            .item(editor_menu_item(
                                &owner,
                                tr!("Séparer les pistes en chapitres"),
                                move |v, window, cx| {
                                    if let Err(error) = v.draft.ungroup(id) {
                                        v.status = failure_text(&error);
                                        return;
                                    }
                                    v.refresh_inputs(window, cx);
                                    v.changed(Change::Discrete, cx);
                                },
                            ))
                        }),
                    ),
            );
        }
        for (track_index, track) in chapter.tracks.iter().enumerate() {
            let tid = track.id;
            let track_count = chapter.tracks.len();
            let owner = cx.entity().downgrade();
            // The tile shows the chapter's icon when the track has none, so « Retirer
            // l’icône » removes whichever icon is visible.
            let removal = if track.icon.is_some() {
                Some(Some(tid))
            } else if chapter.icon.is_some() {
                Some(None)
            } else {
                None
            };
            let (meta, playable) = match &track.media {
                TrackMedia::Local { recipe, media } => (
                    snapshot
                        .recipes
                        .iter()
                        .find(|r| r.id == *recipe)
                        .map(|r| {
                            duration_text(
                                r.info
                                    .as_ref()
                                    .map_or(r.trim.duration(), |info| info.duration)
                                    .seconds(),
                            )
                        })
                        .unwrap_or_else(|| tr!("Audio de la médiathèque").into()),
                    Some((*recipe, *media)),
                ),
                TrackMedia::RemoteAudio { info, .. } => {
                    (duration_text(info.duration.seconds()), None)
                }
                TrackMedia::Stream { .. } => (tr!("Streaming").into(), None),
            };
            let menu = theme::round(
                Button::new(element_id("track-menu", tid))
                    .icon(IconName::EllipsisVertical)
                    .accessibility_label(tr!("Options de la piste"))
                    .tooltip(tr!("Options de la piste")),
                Look::Ghost,
                36.,
                false,
                cx,
            )
            .dropdown_menu(move |menu, _, _| {
                menu.item(editor_menu_item(
                    &owner,
                    tr!("Changer l’icône"),
                    move |v, window, cx| v.open_icon_picker(id, Some(tid), window, cx),
                ))
                .item(
                    editor_menu_item(
                        &owner,
                        if removal == Some(None) {
                            tr!("Retirer l’icône du chapitre")
                        } else {
                            tr!("Retirer l’icône")
                        },
                        move |v, _, cx| {
                            if let Some(target) = removal {
                                v.assign_icon(id, target, None);
                                v.changed(Change::Discrete, cx);
                            }
                        },
                    )
                    .disabled(removal.is_none()),
                )
                .separator()
                .item(editor_menu_item(
                    &owner,
                    tr!("Remplacer l’audio…"),
                    move |v, window, cx| {
                        v.replacement = Some(tid);
                        v.open_media_picker(window, cx);
                    },
                ))
                .item(
                    editor_menu_item(
                        &owner,
                        tr!("Déplacer / regrouper…"),
                        move |v, window, cx| v.open_move_picker(id, tid, window, cx),
                    )
                    .disabled(count < 2),
                )
                .item(
                    editor_menu_item(&owner, tr!("Remonter"), move |v, _, cx| {
                        let result = if single {
                            v.draft.move_chapter(id, -1)
                        } else {
                            v.draft.move_track(tid, id, track_index - 1)
                        };
                        if let Err(error) = result {
                            v.status = failure_text(&error);
                            return;
                        }
                        v.changed(Change::Discrete, cx);
                    })
                    .disabled(if single {
                        index == 0
                    } else {
                        track_index == 0
                    }),
                )
                .item(
                    editor_menu_item(&owner, tr!("Descendre"), move |v, _, cx| {
                        let result = if single {
                            v.draft.move_chapter(id, 1)
                        } else {
                            v.draft.move_track(tid, id, track_index + 1)
                        };
                        if let Err(error) = result {
                            v.status = failure_text(&error);
                            return;
                        }
                        v.changed(Change::Discrete, cx);
                    })
                    .disabled(if single {
                        index + 1 == count
                    } else {
                        track_index + 1 == track_count
                    }),
                )
                .separator()
                .item(editor_menu_item(
                    &owner,
                    tr!("Réglages avancés"),
                    move |v, _, cx| {
                        if !v.expanded_tracks.remove(&tid) {
                            v.expanded_tracks.insert(tid);
                        }
                        cx.notify();
                    },
                ))
                .item(editor_menu_item(
                    &owner,
                    tr!("Retirer de la playlist"),
                    move |v, _, cx| {
                        if let Err(error) = v.draft.remove_track(tid) {
                            v.status = failure_text(&error);
                            return;
                        }
                        v.changed(Change::Discrete, cx);
                    },
                ))
            });
            let icon = if track.icon.is_some() {
                &track.icon
            } else {
                &chapter.icon
            };
            group = group.child(
                div().w_full().when(!single, |row| row.pl(px(34.))).child(
                    div()
                        .w_full()
                        .flex()
                        .gap(px(12.))
                        .items_center()
                        .py(px(8.))
                        .px(px(10.))
                        .rounded(px(18.))
                        .bg(theme::c(theme::SURFACE))
                        .border_2()
                        .border_color(theme::c(theme::SURFACE))
                        .shadow(theme::step(theme::BORDER, 2.))
                        .child(
                            div()
                                .w(px(22.))
                                .flex_shrink_0()
                                .text_center()
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .text_size(px(14.))
                                .text_color(theme::c(theme::NUMBER))
                                .child(if single {
                                    format!("{}", index + 1)
                                } else {
                                    format!("{}.{}", index + 1, track_index + 1)
                                }),
                        )
                        .child(self.icon_button(
                            element_id("track-icon", tid),
                            icon,
                            &tid.to_string(),
                            id,
                            Some(tid),
                            cx,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .children(
                                    self.track_titles
                                        .get(&tid)
                                        .map(|input| title_field(Input::new(input), 17.).w_full()),
                                )
                                .child(theme::meta(meta).pl(px(8.))),
                        )
                        .child({
                            let state = self.track_sync(&track.media, snapshot, state);
                            theme::tag(state.track_label(), state.family())
                        })
                        .children(playable.map(|(recipe, media)| {
                            let playing = self.previewing == Some(recipe);
                            theme::round(
                                Button::new(element_id("preview-track-basic", tid))
                                    .icon(if playing {
                                        IconName::Pause
                                    } else {
                                        IconName::Play
                                    })
                                    .accessibility_label(if playing {
                                        tr!("Arrêter l’écoute de {title}", title = track.title)
                                    } else {
                                        tr!("Écouter {title}", title = track.title)
                                    })
                                    .tooltip(if playing {
                                        tr!("Arrêter l’écoute")
                                    } else {
                                        tr!("Écouter sur cet ordinateur")
                                    })
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        v.toggle_preview(recipe, media, cx)
                                    })),
                                Look::Soft(&theme::PEACH),
                                36.,
                                false,
                                cx,
                            )
                        }))
                        .child(menu),
                ),
            );
            if self.expanded_tracks.contains(&tid) {
                let mut advanced = theme::inset().ml(px(34.));
                if single {
                    let owner = cx.entity().downgrade();
                    advanced = advanced
                        .child(theme::note(tr!(
                            "Titre du chapitre (facultatif, distinct du titre de la piste)"
                        )))
                        .children(
                            self.chapter_titles
                                .get(&id)
                                .map(|input| theme::small_field(Input::new(input))),
                        )
                        .child(
                            theme::pill(
                                Button::new(element_id("advanced-chapter-menu", id))
                                    .label(tr!("Icône du chapitre"))
                                    .dropdown_caret(true),
                                Look::Outline,
                                Height::Small,
                                false,
                                cx,
                            )
                            .dropdown_menu(move |menu, _, _| {
                                menu.item(editor_menu_item(
                                    &owner,
                                    tr!("Changer l’icône du chapitre"),
                                    move |v, window, cx| v.open_icon_picker(id, None, window, cx),
                                ))
                                .item(editor_menu_item(
                                    &owner,
                                    tr!("Retirer l’icône du chapitre"),
                                    move |v, _, cx| {
                                        v.assign_icon(id, None, None);
                                        v.changed(Change::Discrete, cx);
                                    },
                                ))
                            }),
                        );
                }
                if self.draft.playback.kind == PlaybackType::Interactive {
                    for trigger in EventTrigger::ALL {
                        let trigger = *trigger;
                        let selected = track
                            .events
                            .as_ref()
                            .and_then(|events| event(events, trigger).as_ref())
                            .map(|event| event.cmd);
                        let owner = cx.entity().downgrade();
                        advanced = advanced.child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .items_center()
                                .child(
                                    div()
                                        .font_weight(FontWeight::BOLD)
                                        .child(event_trigger_label(trigger)),
                                )
                                .child(
                                    theme::pill(
                                        Button::new(element_id(
                                            "event-command",
                                            format!("{tid}-{}", trigger.wire()),
                                        ))
                                        .label(
                                            selected
                                                .map(event_command_label)
                                                .unwrap_or(tr!("Aucun")),
                                        )
                                        .dropdown_caret(true),
                                        Look::Outline,
                                        Height::Small,
                                        false,
                                        cx,
                                    )
                                    .dropdown_menu(
                                        move |mut menu, _, _| {
                                            menu = menu.item(
                                                editor_menu_item(
                                                    &owner,
                                                    tr!("Aucun"),
                                                    move |v, _, cx| {
                                                        v.change_event(tid, trigger, None);
                                                        v.changed(Change::Discrete, cx);
                                                    },
                                                )
                                                .checked(selected.is_none()),
                                            );
                                            for command in EventCommand::ALL {
                                                let command = *command;
                                                menu = menu.item(
                                                    editor_menu_item(
                                                        &owner,
                                                        event_command_label(command),
                                                        move |v, _, cx| {
                                                            v.change_event(
                                                                tid,
                                                                trigger,
                                                                Some(command),
                                                            );
                                                            v.changed(Change::Discrete, cx);
                                                        },
                                                    )
                                                    .checked(selected == Some(command)),
                                                );
                                            }
                                            menu
                                        },
                                    ),
                                ),
                        );
                        if selected.is_some() && self.advanced {
                            advanced = advanced.children(
                                self.event_params
                                    .get(&(tid, trigger))
                                    .map(|input| theme::small_field(Input::new(input))),
                            );
                        }
                    }
                } else if !single && self.advanced {
                    advanced = advanced.child(theme::note(
                        tr!("Les événements de piste s’activent avec la lecture interactive, dans les réglages avancés de la playlist."),
                    ));
                }
                group = group.child(advanced);
            }
        }
        group.into_any_element()
    }
    fn open_icon_picker(
        &mut self,
        chapter: ChapterId,
        track: Option<TrackId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.blocked() {
            return;
        }
        let (account, _) = match self.owning_remote(cx) {
            Ok(remote) => remote,
            Err(error) => {
                self.status = failure_text(&error);
                cx.notify();
                return;
            }
        };
        let Some(parent) = self.parent.upgrade() else {
            return;
        };
        let editor = cx.entity();
        let picker = cx.new(|cx| {
            pixelart::IconPicker::new(
                pixelart::IconTarget::Playlist {
                    editor: editor.downgrade(),
                    chapter,
                    track,
                },
                parent,
                Some(account.clone()),
                cx,
            )
        });
        self.picker_window = Some(window.window_handle());
        self.icon_error = None;
        let weak = editor.downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let cancel = picker.downgrade();
            let close = weak.clone();
            dialog
                .title(tr!("Choisir une icône"))
                .w(px(440.))
                .child(picker.clone())
                .on_cancel(move |_, _, cx| {
                    cancel
                        .upgrade()
                        .is_none_or(|picker| picker.read(cx).can_cancel(cx))
                })
                .on_close(move |_, _, cx| {
                    let _ = close.update(cx, |v, cx| {
                        v.picker_window = None;
                        v.icon_error = None;
                        cx.notify();
                    });
                })
        });
        self.send(Command::RefreshIcons(account), cx);
    }
    pub(super) fn choose_icon(
        &mut self,
        account: &AccountId,
        chapter: ChapterId,
        track: Option<TrackId>,
        icon: IconReference,
        cx: &mut Context<Self>,
    ) {
        if self.blocked()
            || self.picker_window.is_none()
            || !self
                .owning_remote(cx)
                .is_ok_and(|(owner, _)| &owner == account)
        {
            let refused: String =
                tr!("Le compte ou la playlist a changé. Rouvrez le sélecteur d’icônes.").into();
            // With the picker open, the parent reads why there rather than behind it.
            if self.picker_window.is_some() {
                self.icon_error = Some(refused);
            } else {
                self.status = refused;
            }
            if let IconReference::Local { asset } = icon {
                self.send(Command::ReleaseAsset(asset), cx);
            }
            cx.notify();
            return;
        }
        if let IconReference::Local { asset } = &icon {
            self.cache_icon(*asset, cx);
        }
        self.assign_icon(chapter, track, Some(icon));
        self.icon_error = None;
        self.refresh_icons = Some(account.clone());
        self.close_picker(cx);
        self.changed(Change::Discrete, cx);
    }
    fn open_move_picker(
        &mut self,
        chapter: ChapterId,
        track: TrackId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.blocked() || self.owning_remote(cx).is_err() {
            return;
        }
        // The chapters as the editor shows them, with moves not written yet.
        let _ = self.gather(cx);
        let choices = Arc::new(
            self.draft
                .chapters
                .iter()
                .map(|chapter| media_picker::ChapterChoice {
                    id: chapter.id,
                    title: chapter.title.clone(),
                    tracks: chapter.tracks.len(),
                })
                .collect::<Vec<_>>(),
        );
        let editor = cx.entity();
        let picker =
            cx.new(|cx| move_picker::MovePicker::new(editor.clone(), choices, chapter, track, cx));
        self.picker_window = Some(window.window_handle());
        let weak = editor.downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let cancel = weak.clone();
            let close = weak.clone();
            dialog
                .title(tr!("Déplacer dans un chapitre"))
                .w(px(600.))
                .child(picker.clone())
                .on_cancel(move |_, _, cx| {
                    cancel.upgrade().is_none_or(|owner| !owner.read(cx).busy())
                })
                .on_close(move |_, _, cx| {
                    let _ = close.update(cx, |v, cx| {
                        v.picker_window = None;
                        cx.notify();
                    });
                })
        });
    }
    /// Media chosen in the picker or the library: a replacement is a local edit written
    /// with the next group; new media are added on Yoto once pending changes are written.
    pub(super) fn add_media_batch(
        &mut self,
        media: Vec<(MediaId, RecipeId)>,
        cx: &mut Context<Self>,
    ) {
        if media.is_empty() || self.blocked() {
            return;
        }
        if let Err(e) = self.owning_remote(cx) {
            self.status = failure_text(&e);
            cx.notify();
            return;
        }
        if let Some(track) = self.replacement.take() {
            if media.len() != 1 {
                self.status = tr!("Sélectionnez un seul média pour remplacer cette piste.").into();
                cx.notify();
                return;
            }
            let (media, recipe) = media[0];
            if let Some(t) = self
                .draft
                .chapters
                .iter_mut()
                .flat_map(|c| &mut c.tracks)
                .find(|t| t.id == track)
            {
                t.media = TrackMedia::Local { media, recipe };
                self.close_picker(cx);
                self.changed(Change::Discrete, cx);
            } else {
                self.status = tr!(
                    "Cette piste n’existe plus dans la playlist ; l’audio n’a pas été remplacé."
                )
                .into();
                cx.notify();
            }
        } else {
            self.queued_media.get_or_insert_with(Vec::new).extend(media);
            self.status = text::writing().into();
            self.schedule(cx);
            cx.notify();
        }
    }
    pub fn delete_remote(&mut self, cx: &mut Context<Self>) {
        if self.blocked() || self.autosave.in_flight() {
            return;
        }
        let (account, card) = match self.owning_remote(cx) {
            Ok(remote) => remote,
            Err(e) => {
                self.status = failure_text(&e);
                cx.notify();
                return;
            }
        };
        // Deleting wins over changes not written yet.
        self.autosave.discard();
        self.queued_media = None;
        self.autosave.begin();
        self.deleting = true;
        self.operation = Some(PlaylistOperation::Submitting {
            target: CommitTarget::Delete,
        });
        self.save_updates.begin();
        self.status = tr!("Suppression sur Yoto…").into();
        self.notify_parent(cx);
        cx.notify();
        let service = self.service.clone();
        let rt = self.runtime.clone();
        let id = self.base.id;
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move {
                    service
                        .execute(Command::DeleteRemote { account, card })
                        .await
                })
                .await;
            let _ = weak.update(cx, |v, cx| {
                match joined(result).and_then(Reply::into_unit) {
                    Ok(()) => {
                        v.save_updates.finish();
                        v.autosave.finish();
                        v.operation = None;
                        let _ = v.parent.update(cx, |parent, cx| {
                            if parent.selected_playlist == Some(id) {
                                parent.selected_playlist = None;
                            }
                            parent.editors.remove(&id);
                            parent.status = tr!("Playlist supprimée sur Yoto.").into();
                            cx.notify();
                        });
                    }
                    Err(e) => v.fail_operation(e, cx),
                }
                let _ = v.parent.update(cx, |_, cx| cx.notify());
                cx.notify();
            });
        }));
    }
    fn subscribe_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut inputs = vec![
            self.title.clone(),
            self.description.clone(),
            self.author.clone(),
            self.narrator.clone(),
            self.languages.clone(),
            self.resume.clone(),
            self.min_age.clone(),
            self.max_age.clone(),
            self.shuffle.clone(),
        ];
        inputs.extend(self.chapter_titles.values().cloned());
        inputs.extend(self.track_titles.values().cloned());
        inputs.extend(self.event_params.values().cloned());
        for input in inputs {
            self._subscriptions
                .push(
                    cx.subscribe_in(&input, window, |v, _, event, _, cx| match event {
                        gpui_kit::component::input::InputEvent::Blur
                        | gpui_kit::component::input::InputEvent::PressEnter { .. } => v.flush(cx),
                        gpui_kit::component::input::InputEvent::Change => {
                            v.changed(Change::Text, cx)
                        }
                        gpui_kit::component::input::InputEvent::Focus => {}
                    }),
                );
        }
    }
    pub(super) fn cache_icon(&mut self, asset: AssetId, cx: &mut Context<Self>) {
        if self.icon_previews.contains_key(&asset) {
            return;
        }
        let service = self.service.clone();
        let rt = self.runtime.clone();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move { service.execute(Command::ReadPixelArt(asset)).await })
                .await;
            let _ = weak.update(cx, |v, cx| {
                match joined(result).and_then(Reply::into_pixel_art) {
                    Ok(art) => {
                        v.icon_previews.insert(asset, art);
                    }
                    Err(e) => v.status = failure_text(&e),
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn assign_icon(
        &mut self,
        chapter: ChapterId,
        track: Option<TrackId>,
        icon: Option<IconReference>,
    ) {
        if let Some(c) = self.draft.chapters.iter_mut().find(|c| c.id == chapter) {
            match track {
                Some(id) => {
                    if let Some(t) = c.tracks.iter_mut().find(|t| t.id == id) {
                        t.icon = icon;
                    }
                }
                None => c.icon = icon,
            }
        }
    }
    fn change_event(&mut self, track: TrackId, trigger: EventTrigger, cmd: Option<EventCommand>) {
        for c in &mut self.draft.chapters {
            for t in &mut c.tracks {
                if t.id == track {
                    let events = t.events.get_or_insert_with(TrackEvents::new);
                    let target = event_mut(events, trigger);
                    *target = cmd.map(|cmd| {
                        let mut next = target.take().map(|e| *e).unwrap_or_else(|| Event::new(cmd));
                        next.cmd = cmd;
                        Box::new(next)
                    });
                }
            }
        }
    }
    /// The playlist's real cover, or an empty slot inviting the parent to choose one;
    /// either way a click opens the cover choice.
    fn cover_button(&self, blocked: bool, cx: &mut Context<Self>) -> Button {
        use gpui_kit::component::button::{ButtonCustomVariant, ButtonRounded};
        let asset = self.draft.metadata.cover;
        let url = pixelart::cover_url(self.draft.metadata.extra.get("cover"));
        let label = if asset.is_some() || url.is_some() {
            tr!("Changer la couverture")
        } else {
            tr!("Choisir une couverture")
        };
        let picture = match self.parent.upgrade() {
            Some(parent) => {
                if let Some(asset) = asset
                    && !parent.read(cx).cover_previews.contains_key(&asset)
                {
                    let reader = parent.downgrade();
                    cx.defer(move |cx| {
                        let _ =
                            reader.update(cx, |parent, cx| parent.load_cover_preview(asset, cx));
                    });
                }
                pixelart::playlist_cover(asset, url, parent.read(cx), 120., 26., true)
            }
            None => theme::empty_icon_slot(120., 26., true).into_any_element(),
        };
        Button::new("cover")
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(theme::c(theme::SURFACE).opacity(0.))
                    .hover(theme::c(theme::CREAM))
                    .active(theme::c(theme::SAND)),
            )
            .rounded(ButtonRounded::Size(px(26.)))
            .size(px(120.))
            .p(px(0.))
            .accessibility_label(label)
            .tooltip(label)
            .disabled(blocked)
            .child(picture)
            .on_click(cx.listener(|v, _, _, cx| v.cover(cx)))
    }
    fn cover(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let rt = self.runtime.clone();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            if let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter(tr!("Image"), &["png", "jpg", "jpeg", "gif"])
                .pick_file()
                .await
            {
                let r = rt
                    .spawn(async move {
                        service
                            .execute(Command::StoreImage {
                                path: file.path().to_owned(),
                                pixel_art: false,
                            })
                            .await
                    })
                    .await;
                let _ = weak.update(cx, |v, cx| {
                    match joined(r).and_then(Reply::into_asset) {
                        Ok(id) => {
                            v.draft.metadata.cover = Some(id);
                            v.changed(Change::Discrete, cx);
                        }
                        Err(e) => v.status = failure_text(&e),
                    }
                    cx.notify();
                });
            }
        }));
    }
}

/// How a playlist value reads in its text field.
type FieldText = fn(&Playlist) -> String;
/// Whether two versions of a playlist hold the same content, whatever their revisions.
fn same_content(a: &Playlist, b: &Playlist) -> bool {
    let value = |p: &Playlist| {
        serde_json::to_value(p).ok().map(|mut v| {
            if let Some(map) = v.as_object_mut() {
                map.remove("revision");
            }
            v
        })
    };
    value(a) == value(b)
}
/// An age as its field shows it.
fn age_text(age: Option<u8>) -> String {
    age.map(|v| v.to_string()).unwrap_or_default()
}
/// The shuffle ranges as their field shows them, chapters counted from 1.
fn shuffle_text(playlist: &Playlist) -> String {
    playlist
        .playback
        .shuffle
        .iter()
        .map(|g| format!("{}-{}-{}", g.start + 1, g.end + 1, g.limit))
        .collect::<Vec<_>>()
        .join(", ")
}
/// An inline-editable title: reads as heading text, shows its frame only when focused.
fn title_field(input: Input, size: f32) -> Input {
    input
        .bg(theme::c(theme::SURFACE).opacity(0.))
        .border_color(theme::c(theme::SURFACE).opacity(0.))
        .font_family(theme::HEADING_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(size))
        .h(px((size * 1.75).round()))
        .px(px(8.))
}
impl PlaylistEditor {
    /// The state in the tag by the title: changes waiting for their write count as sending.
    fn sync_state(&self) -> SyncState {
        let waiting = (self.autosave.has_pending() || self.queued_media.is_some()) && !self.held();
        let state = SyncState::of(
            self.write_running() || waiting,
            self.blocked() || self.external.is_some() || self.deleted,
            self.unsaved.is_some(),
        );
        if matches!(state, SyncState::OnYoto | SyncState::Unsaved) && self.send_failure().is_some()
        {
            SyncState::Failed
        } else {
            state
        }
    }
    /// Why the last write to Yoto failed, while nothing newer was written. Uncertain or
    /// conflicting writes have their own panel, and a cancelled one is no failure.
    fn send_failure(&self) -> Option<&Failure> {
        match &self.operation {
            Some(PlaylistOperation::Failed(failure)) => match failure {
                Failure::Ambiguous | Failure::Conflict | Failure::Cancelled => None,
                Failure::Validation(_)
                | Failure::Storage(_)
                | Failure::Media(_)
                | Failure::Connection
                | Failure::Authentication
                | Failure::Permission(_)
                | Failure::RateLimit { .. }
                | Failure::Compatibility(_)
                | Failure::Tool(_)
                | Failure::Transcode { .. } => Some(failure),
            },
            Some(PlaylistOperation::Submitting { .. } | PlaylistOperation::Pending { .. })
            | None => None,
        }
    }
    /// On opening: show the failure of this playlist's latest publication, if it failed,
    /// so it does not only appear in « Imports ».
    pub fn restore_failure(&mut self, snapshot: &Snapshot) {
        let DraftOrigin::Existing { base } = &self.base.origin else {
            return;
        };
        let latest = snapshot
            .jobs
            .iter()
            .filter(|job| match &job.request {
                JobRequest::Publish {
                    account,
                    snapshot: draft,
                } => account == &base.account && draft.id == self.base.id,
                JobRequest::Import { .. } | JobRequest::Encode { .. } => false,
            })
            .max_by_key(|job| publication_job_key(job));
        if let Some(job) = latest
            && let JobState::Failed { failure } = &job.state
            && !matches!(
                failure,
                Failure::Ambiguous | Failure::Conflict | Failure::Cancelled
            )
        {
            self.operation = Some(PlaylistOperation::Failed(failure.clone()));
            self.failed_job = Some((job.id, CommitTarget::Editor));
        }
    }
    /// Send the failed publication again, as it was prepared.
    fn retry_failed(&mut self, cx: &mut Context<Self>) {
        let Some((job, target)) = self.failed_job.take() else {
            return;
        };
        self.operation = Some(PlaylistOperation::Submitting { target });
        self.status = text::writing().into();
        self.notify_parent(cx);
        cx.notify();
        let service = self.service.clone();
        let rt = self.runtime.clone();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move { service.execute(Command::Retry(job)).await })
                .await;
            let _ = weak.update(cx, |v, cx| {
                match joined(result) {
                    Ok(_) => {
                        v.operation = Some(PlaylistOperation::Pending {
                            job,
                            state: JobState::Queued,
                            target,
                        });
                        v.status = text::writing_and_checking().into();
                    }
                    Err(e) => v.fail_operation(e, cx),
                }
                if let Some(parent) = v.parent.upgrade() {
                    let snapshot = parent.read(cx).snapshot.clone();
                    v.apply_snapshot(&snapshot, cx);
                }
                cx.notify();
            });
        }));
    }
    /// Under the header when the last write failed: why, and what to do next.
    fn failure_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        use theme::{Height, Look};
        let failure = self.send_failure()?;
        let parent = self.parent.clone();
        let mut actions = div().flex().gap(px(8.)).items_center();
        if self.failed_job.is_some() {
            actions = actions.child(theme::pill(
                Button::new("retry-failed-publication")
                    .label(tr!("Réessayer"))
                    .on_click(cx.listener(|v, _, _, cx| v.retry_failed(cx))),
                Look::Primary,
                Height::Small,
                false,
                cx,
            ));
        }
        actions = actions.child(theme::pill(
            Button::new("failed-publication-details")
                .label(tr!("Voir les détails"))
                .on_click(move |_, _, cx| {
                    let _ = parent.update(cx, |p, cx| p.navigate(Route::Imports, cx));
                }),
            Look::Link(&theme::ROSE),
            Height::Small,
            false,
            cx,
        ));
        Some(
            theme::inset()
                .bg(theme::c(theme::ROSE.soft))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .text_color(theme::c(theme::ROSE.ink))
                        .font_weight(FontWeight::BOLD)
                        .child(text::send_failed()),
                )
                .child(div().child(failure_text(failure)))
                .child(actions),
        )
    }
    /// The state on every track: only an actual write to Yoto shows there, not typing.
    fn track_state(&self) -> SyncState {
        SyncState::of(
            self.write_running(),
            self.blocked() || self.external.is_some() || self.deleted,
            false,
        )
    }
    /// One track's tag. A write sends only the audio Yoto does not hold yet: a track
    /// already on Yoto, or whose audio was sent before to this account, stays « on Yoto »
    /// while the playlist itself is being written.
    fn track_sync(&self, media: &TrackMedia, snapshot: &Snapshot, state: SyncState) -> SyncState {
        if state != SyncState::Sending {
            return state;
        }
        let account = match &self.base.origin {
            DraftOrigin::Existing { base } => Some(&base.account),
            DraftOrigin::New => None,
        };
        let held = match media {
            TrackMedia::RemoteAudio { .. } | TrackMedia::Stream { .. } => true,
            TrackMedia::Local { recipe, .. } => snapshot
                .mappings
                .iter()
                .any(|m| m.recipe == *recipe && Some(&m.account) == account),
        };
        if held { SyncState::OnYoto } else { state }
    }
    fn header(&self, snapshot: &Snapshot, cx: &mut Context<Self>) -> Div {
        use theme::{Height, Look};
        let tracks = self
            .draft
            .chapters
            .iter()
            .flat_map(|c| &c.tracks)
            .collect::<Vec<_>>();
        let seconds: f64 = tracks
            .iter()
            .filter_map(|t| match &t.media {
                TrackMedia::Local { recipe, .. } => snapshot
                    .recipes
                    .iter()
                    .find(|r| r.id == *recipe)
                    .map(|r| r.info.as_ref().map_or(r.trim.duration(), |i| i.duration)),
                TrackMedia::RemoteAudio { info, .. } => Some(info.duration),
                TrackMedia::Stream { .. } => None,
            })
            .map(|d| d.seconds())
            .sum();
        let meta = match tracks.len() {
            0 => tr!("Vide").to_owned(),
            n => trn!(
                "{count} piste · {duration}",
                "{count} pistes · {duration}",
                n,
                duration = duration_text(seconds)
            ),
        };
        let blocked = self.blocked();
        // Duplicating copies what Yoto holds and deleting must not race a write.
        let writing = self.autosave.in_flight() || self.autosave.has_pending();
        let state = self.sync_state();
        let creating = self
            .parent
            .upgrade()
            .is_some_and(|p| p.read(cx).creating_playlist);
        let owner = cx.entity().downgrade();
        let duplicate = owner.clone();
        let remove = owner.clone();
        let cover = self.cover_button(blocked, cx);
        let mut tags = div().flex().gap(px(6.));
        if self.draft.metadata.category != Category::None {
            tags = tags.child(theme::tag(
                category_label(self.draft.metadata.category),
                &theme::LILAC,
            ));
        }
        tags = tags.child(theme::tag(state.playlist_label(), state.family()));
        // On a narrow window the actions move under the title rather than squeezing it.
        div()
            .flex()
            .flex_wrap()
            .gap_x(px(20.))
            .gap_y(px(12.))
            .items_end()
            .child(cover)
            .child(
                div()
                    .flex_1()
                    .min_w(px(220.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(tags)
                    .child(if blocked {
                        theme::heading(self.base.title.clone(), 32.).into_any_element()
                    } else {
                        title_field(Input::new(&self.title), 32.)
                            .w_full()
                            .ml(px(-8.))
                            .into_any_element()
                    })
                    .child(theme::meta(meta).text_size(px(14.))),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        theme::round(
                            Button::new("playlist-actions")
                                .icon(IconName::Ellipsis)
                                .accessibility_label(tr!("Plus d’actions"))
                                .tooltip(tr!("Plus d’actions")),
                            Look::Outline,
                            44.,
                            false,
                            cx,
                        )
                        .dropdown_menu(move |menu, _, _| {
                            let duplicate = duplicate.clone();
                            let remove = remove.clone();
                            menu.item(
                                PopupMenuItem::new(tr!("Dupliquer la playlist"))
                                    .disabled(blocked || creating || writing)
                                    .on_click(move |_, _, cx| {
                                        let _ = duplicate.update(cx, |e, cx| e.duplicate(cx));
                                    }),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new(tr!("Supprimer la playlist de mon compte Yoto"))
                                    .disabled(blocked || writing)
                                    .on_click(move |_, window, cx| {
                                        let Some(title) =
                                            remove.upgrade().map(|e| e.read(cx).draft.title.clone())
                                        else {
                                            return;
                                        };
                                        let remove = remove.clone();
                                        confirm_destructive(
                                            window,
                                            cx,
                                            tr!("Supprimer « {title} » de votre compte Yoto ?", title = title),
                                            tr!("La playlist disparaîtra de votre compte Yoto et de vos lecteurs. Vos fichiers audio restent dans la médiathèque. Cette action est irréversible."),
                                            tr!("Supprimer"),
                                            move |_, cx| {
                                                let _ = remove.update(cx, |e, cx| e.delete_remote(cx));
                                            },
                                        );
                                    }),
                            )
                        }),
                    )
                    .child(theme::pill(
                        Button::new("playlist-add-media")
                            .icon(IconName::Plus)
                            .label(tr!("Ajouter des médias"))
                            .on_click(cx.listener(|_, _, window, cx| {
                                let handle = window.window_handle();
                                let editor = cx.entity();
                                cx.defer(move |cx| {
                                    let _ = cx.update_window(handle, move |_, window, cx| {
                                        editor.update(cx, |e, cx| e.open_media_picker(window, cx));
                                    });
                                });
                            })),
                        Look::Primary,
                        Height::Large,
                        blocked,
                        cx,
                    )),
            )
    }
    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let id = self.base.id;
        let _ = self.parent.update(cx, |v, cx| {
            if let Some(binding) = v.snapshot.bindings.iter().find(|b| {
                b.playlist == id && Some(&b.account) == v.snapshot.active_account.as_ref()
            }) {
                v.send(
                    Command::DuplicateYotoPlaylist {
                        account: binding.account.clone(),
                        card: binding.card.clone(),
                    },
                    cx,
                );
            }
        });
    }
    fn options_panel(&self, cx: &mut Context<Self>) -> Div {
        use theme::{Height, Look};
        let owner = cx.entity().downgrade();
        let category = self.draft.metadata.category;
        let advance = self.draft.playback.autoadvance;
        let advance_owner = owner.clone();
        let labelled = |label: &'static str, child: AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_color(theme::c(theme::MUTED))
                        .child(label),
                )
                .child(child)
        };
        theme::card()
            .child(labelled(
                tr!("Description"),
                theme::small_field(Input::new(&self.description)).into_any_element(),
            ))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(labelled(
                        tr!("Auteur"),
                        theme::small_field(Input::new(&self.author)).into_any_element(),
                    ))
                    .child(labelled(
                        tr!("Narrateur"),
                        theme::small_field(Input::new(&self.narrator)).into_any_element(),
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(labelled(
                        tr!("Langues"),
                        theme::small_field(Input::new(&self.languages)).into_any_element(),
                    ))
                    .child(labelled(
                        tr!("Âge minimum"),
                        theme::small_field(Input::new(&self.min_age)).into_any_element(),
                    ))
                    .child(labelled(
                        tr!("Âge maximum"),
                        theme::small_field(Input::new(&self.max_age)).into_any_element(),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(10.))
                    .items_end()
                    .child(labelled(
                        tr!("Catégorie"),
                        theme::pill(
                            Button::new("playlist-category")
                                .label(category_label(category))
                                .dropdown_caret(true),
                            Look::Outline,
                            Height::Medium,
                            false,
                            cx,
                        )
                        .dropdown_menu(move |mut menu, _, _| {
                            for choice in Category::ALL {
                                let choice = *choice;
                                menu = menu.item(
                                    editor_menu_item(
                                        &owner,
                                        category_label(choice),
                                        move |v, _, cx| {
                                            v.draft.metadata.category = choice;
                                            v.changed(Change::Discrete, cx);
                                        },
                                    )
                                    .checked(category == choice),
                                );
                            }
                            menu
                        })
                        .into_any_element(),
                    ))
                    .child(labelled(
                        tr!("Après une piste"),
                        theme::pill(
                            Button::new("autoadvance-menu")
                                .label(advance_label(advance))
                                .dropdown_caret(true),
                            Look::Outline,
                            Height::Medium,
                            false,
                            cx,
                        )
                        .dropdown_menu(move |mut menu, _, _| {
                            for choice in Autoadvance::ALL {
                                let choice = *choice;
                                menu = menu.item(
                                    editor_menu_item(
                                        &advance_owner,
                                        advance_label(choice),
                                        move |v, _, cx| {
                                            v.draft.playback.autoadvance = choice;
                                            v.changed(Change::Discrete, cx);
                                        },
                                    )
                                    .checked(advance == choice),
                                );
                            }
                            menu
                        })
                        .into_any_element(),
                    )),
            )
            .child(theme::note(text::automatic()))
    }
    fn advanced_panel(&self, cx: &mut Context<Self>) -> Div {
        use theme::{Height, Look};
        let owner = cx.entity().downgrade();
        let kind = self.draft.playback.kind;
        theme::card()
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .items_center()
                    .child(
                        div()
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .child(tr!("Lecture")),
                    )
                    .child(
                        theme::pill(
                            Button::new("playback-menu")
                                .label(playback_label(kind))
                                .dropdown_caret(true),
                            Look::Outline,
                            Height::Medium,
                            false,
                            cx,
                        )
                        .dropdown_menu(move |mut menu, _, _| {
                            for choice in PlaybackType::ALL {
                                let choice = *choice;
                                menu = menu.item(
                                    editor_menu_item(
                                        &owner,
                                        playback_label(choice),
                                        move |v, _, cx| {
                                            v.draft.playback.kind = choice;
                                            v.changed(Change::Discrete, cx);
                                        },
                                    )
                                    .checked(kind == choice),
                                );
                            }
                            menu
                        }),
                    ),
            )
            .child(
                gpui_kit::component::checkbox::Checkbox::new("online-only")
                    .label(tr!("Connexion requise sur le lecteur"))
                    .checked(self.draft.playback.online_only)
                    .on_click(cx.listener(|v, checked, _, cx| {
                        v.draft.playback.online_only = *checked;
                        v.changed(Change::Discrete, cx);
                    })),
            )
            .child(theme::note(tr!(
                "Lecture aléatoire : début-fin-limite (chapitres dès 1)"
            )))
            .child(theme::small_field(Input::new(&self.shuffle)))
            .child(theme::note(tr!("Durée de reprise (secondes)")))
            .child(theme::small_field(Input::new(&self.resume)))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(theme::small_field(Input::new(&self.stream)).flex_1())
                    .child(theme::pill(
                        Button::new("add-stream")
                            .label(tr!("Ajouter un stream"))
                            .on_click(cx.listener(|v, _, window, cx| v.add_stream(window, cx))),
                        Look::Outline,
                        Height::Medium,
                        false,
                        cx,
                    )),
            )
            .child(
                div().child(theme::pill(
                    Button::new("abandon-draft-images")
                        .label(tr!("Abandonner les images non enregistrées"))
                        .on_click(cx.listener(|v, _, _, cx| v.abandon_images(cx))),
                    Look::Ghost,
                    Height::Small,
                    self.draft
                        .image_assets()
                        .difference(&self.base.image_assets())
                        .next()
                        .is_none(),
                    cx,
                )),
            )
            .child(theme::note(tr!(
                "Les événements de chaque piste se règlent depuis son menu, « Réglages avancés »."
            )))
    }
    fn add_stream(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.stream.read(cx).value().to_string();
        if let Ok(mut p) = Playlist::new(tr!("Streaming").into()) {
            let track = Track {
                events: None,
                id: TrackId::new(),
                key: TrackKey::fresh(),
                title: tr!("Streaming").into(),
                icon: None,
                media: TrackMedia::Stream {
                    url,
                    format: AudioFormat::Mp3,
                },
                extra: Extra::new(),
            };
            p.chapters.push(Chapter {
                id: ChapterId::new(),
                key: ChapterKey::fresh(),
                title: tr!("Streaming").into(),
                icon: None,
                tracks: vec![track],
                extra: Extra::new(),
            });
            self.draft.chapters.extend(p.chapters);
            self.refresh_inputs(window, cx);
            self.changed(Change::Discrete, cx);
        }
    }
}

impl PlaylistEditor {
    /// « Recharger la version Yoto »: drop local changes and show what Yoto holds. Not while
    /// a write is on its way, whose outcome would be forgotten.
    fn reload_button(&self, cx: &mut Context<Self>) -> Button {
        use theme::{Height, Look};
        theme::pill(
            Button::new("reload-playlist")
                .label(tr!("Recharger la version Yoto"))
                .on_click(cx.listener(|v, _, window, cx| {
                    if !v.write_running()
                        && let Some(p) = v.external.take()
                    {
                        v.adopt(p, window, cx);
                    }
                })),
            Look::Primary,
            Height::Medium,
            self.write_running(),
            cx,
        )
    }
}
impl Render for PlaylistEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use theme::{Height, Look};
        let snapshot = self
            .parent
            .upgrade()
            .map(|p| p.read(cx).snapshot.clone())
            .unwrap_or_default();
        let mut body = div()
            .id("editor-scroll")
            .size_full()
            .overflow_y_scroll()
            .py(px(24.))
            .px(px(28.))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(self.header(&snapshot, cx));
        let banner = self.failure_banner(cx);
        let banner_shown = banner.is_some();
        body = body.children(banner);
        if self.blocked() {
            let mut pending = theme::inset().child(
                div()
                    .flex()
                    .gap(px(10.))
                    .items_center()
                    .children(self.write_running().then(|| {
                        Spinner::new()
                            .color(theme::c(theme::BUTTER.accent))
                            .into_any_element()
                    }))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::BOLD)
                            .child(self.status.clone()),
                    ),
            );
            let parent = self.parent.clone();
            pending = pending.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .items_center()
                    .children(self.external.is_some().then(|| self.reload_button(cx)))
                    .child(theme::pill(
                        Button::new("remote-operation-details")
                            .label(tr!("Voir le suivi"))
                            .on_click(move |_, _, cx| {
                                let _ = parent.update(cx, |p, cx| p.navigate(Route::Imports, cx));
                            }),
                        Look::Link(&theme::PEACH),
                        Height::Small,
                        false,
                        cx,
                    )),
            );
            body = body.child(pending);
            let state = self.track_state();
            for (index, chapter) in self.base.chapters.iter().enumerate() {
                for track in &chapter.tracks {
                    body = body.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .py(px(10.))
                            .px(px(14.))
                            .rounded(px(18.))
                            .bg(theme::c(theme::SURFACE))
                            .shadow(theme::step(theme::BORDER, 2.))
                            .child(
                                div()
                                    .w(px(22.))
                                    .text_center()
                                    .font_weight(FontWeight::EXTRA_BOLD)
                                    .text_color(theme::c(theme::NUMBER))
                                    .child(format!("{}", index + 1)),
                            )
                            .child(theme::heading(track.title.clone(), 17.).flex_1().truncate())
                            .child({
                                let state = self.track_sync(&track.media, &snapshot, state);
                                theme::tag(state.track_label(), state.family())
                            }),
                    );
                }
            }
            return body.into_any_element();
        }
        if self.draft.chapters.is_empty() {
            body = body.child(
                theme::inset()
                    .p(px(20.))
                    .child(theme::heading(tr!("Cette playlist est vide"), 19.))
                    .child(theme::note(
                        tr!("Cliquez sur « Ajouter des médias » pour y mettre des audios de votre médiathèque."),
                    )),
            );
        }
        let state = self.track_state();
        let mut tracks = div().flex().flex_col().gap(px(8.));
        for index in 0..self.draft.chapters.len() {
            tracks = tracks.child(self.render_chapter(index, &snapshot, state, cx));
        }
        body = body.child(tracks);
        if self.external.is_some() || self.deleted {
            body = body.child(
                theme::inset()
                    .bg(theme::c(theme::ROSE.soft))
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .child(self.status.clone()),
                    )
                    .children(
                        self.external
                            .is_some()
                            .then(|| div().flex().gap(px(8.)).child(self.reload_button(cx))),
                    ),
            );
        }
        body = body
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        theme::disclosure(
                            "playlist-options",
                            tr!("Options de la playlist"),
                            tr!("Description, auteur, catégorie, langue, lecture"),
                            self.options_open,
                            cx,
                        )
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.options_open = !v.options_open;
                            cx.notify();
                        })),
                    )
                    .children(self.options_open.then(|| self.options_panel(cx)))
                    .children(self.advanced.then(|| {
                        theme::disclosure(
                            "playlist-advanced",
                            tr!("Réglages avancés"),
                            tr!("Lecture interactive, événements"),
                            self.advanced_open,
                            cx,
                        )
                        .text_color(theme::c(theme::MUTED))
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.advanced_open = !v.advanced_open;
                            cx.notify();
                        }))
                    }))
                    .children(
                        (self.advanced && self.advanced_open).then(|| self.advanced_panel(cx)),
                    ),
            )
            .children(
                (!self.status.is_empty()
                    && self.external.is_none()
                    && !self.deleted
                    && !banner_shown)
                    .then(|| theme::note(self.status.clone())),
            );
        body.into_any_element()
    }
}
impl Drop for PlaylistEditor {
    fn drop(&mut self) {
        let assets: Vec<_> = self
            .draft
            .image_assets()
            .difference(&self.base.image_assets())
            .copied()
            .collect();
        if !assets.is_empty() {
            let service = self.service.clone();
            self.runtime.spawn(async move {
                for asset in assets {
                    let _ = service.execute(Command::ReleaseAsset(asset)).await;
                }
            });
        }
    }
}
impl PlaylistEditor {
    fn refresh_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for c in &self.draft.chapters {
            self.chapter_titles.entry(c.id).or_insert_with(|| {
                cx.new(|cx| InputState::new(window, cx).default_value(c.title.clone()))
            });
            for t in &c.tracks {
                for trigger in EventTrigger::ALL {
                    self.event_params
                        .entry((t.id, *trigger))
                        .or_insert_with(|| {
                            cx.new(|cx| InputState::new(window, cx).default_value("{}"))
                        });
                }
                self.track_titles.entry(t.id).or_insert_with(|| {
                    cx.new(|cx| InputState::new(window, cx).default_value(t.title.clone()))
                });
            }
        }
    }
}
#[derive(Clone)]
pub struct PlaylistTarget {
    pub account: AccountId,
    pub card: CardId,
}
#[derive(Clone)]
pub struct PlaylistRow {
    pub title: String,
    pub target: PlaylistTarget,
    pub state: PlaylistSyncState,
    /// Track count read from the remote card, when its document has one.
    pub tracks: Option<usize>,
    /// The cover chosen on this computer for the playlist, if any.
    pub cover_asset: Option<AssetId>,
    /// The cover Yoto shows for the card, if any.
    pub cover: Option<SharedString>,
}
/// Build the catalog once on the snapshot worker, never while rendering/scrolling.
fn publication_job_key(job: &Job) -> (u64, Revision, u8) {
    let revision = match &job.request {
        JobRequest::Publish { snapshot, .. } => snapshot.revision,
        JobRequest::Import { .. } | JobRequest::Encode { .. } => Revision(0),
    };
    let priority = match &job.state {
        JobState::Queued
        | JobState::Running { .. }
        | JobState::Retry { .. }
        | JobState::WaitingForAuthentication
        | JobState::Conflict { .. }
        | JobState::Reconciling => 3,
        JobState::Completed { .. } => 2,
        JobState::Failed { .. } | JobState::Cancelled => 1,
    };
    (job.created, revision, priority)
}
pub fn playlist_rows(snapshot: &Snapshot) -> Vec<PlaylistRow> {
    let Some(account) = &snapshot.active_account else {
        return vec![];
    };
    let bindings: BTreeMap<_, _> = snapshot
        .bindings
        .iter()
        .filter(|binding| &binding.account == account)
        .map(|b| (b.card.clone(), b.playlist))
        .collect();
    let mut rows = snapshot
        .remote
        .iter()
        .map(|card| {
            let playlist = bindings.get(&card.card);
            let latest = snapshot
                .jobs
                .iter()
                .filter(|job| match &job.request {
                    JobRequest::Publish {
                        account: owner,
                        snapshot: draft,
                    } => owner == account && playlist == Some(&draft.id),
                    JobRequest::Import { .. } | JobRequest::Encode { .. } => false,
                })
                .max_by_key(|job| publication_job_key(job));
            let state = match latest.map(|j| &j.state) {
                Some(JobState::Queued | JobState::Running { .. } | JobState::Retry { .. }) => {
                    PlaylistSyncState::Publishing
                }
                Some(JobState::WaitingForAuthentication) => {
                    PlaylistSyncState::AuthenticationRequired
                }
                Some(JobState::Conflict { .. }) => PlaylistSyncState::Conflict,
                Some(JobState::Reconciling) => PlaylistSyncState::Uncertain,
                Some(JobState::Failed { failure }) => PlaylistSyncState::Failed {
                    failure: failure.clone(),
                },
                Some(JobState::Completed { .. } | JobState::Cancelled) | None => {
                    PlaylistSyncState::Synced
                }
            };
            PlaylistRow {
                title: card.title.clone(),
                target: PlaylistTarget {
                    account: account.clone(),
                    card: card.card.clone(),
                },
                state,
                tracks: card
                    .document
                    .pointer("/content/chapters")
                    .and_then(serde_json::Value::as_array)
                    .map(|chapters| {
                        chapters
                            .iter()
                            .filter_map(|c| c.get("tracks").and_then(serde_json::Value::as_array))
                            .map(Vec::len)
                            .sum()
                    }),
                cover_asset: playlist.and_then(|id| {
                    snapshot
                        .playlists
                        .iter()
                        .find(|p| p.id == *id)
                        .and_then(|p| p.metadata.cover)
                }),
                cover: pixelart::cover_url(card.document.pointer("/metadata/cover")),
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.title.to_lowercase());
    rows
}
pub fn open_remote(
    view: &mut AppView,
    account: AccountId,
    card: CardId,
    cx: &mut Context<AppView>,
) {
    if view.opening_playlist.is_some() {
        return;
    }
    view.flush_playlists(cx);
    view.send(Command::LoadRemote { account, card }, cx);
}
pub fn render(view: &mut AppView, _window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    use theme::{Height, Look};
    let syncing = remote_syncing(&view.snapshot);
    let mut header = div()
        .flex()
        .items_center()
        .justify_between()
        .px(px(4.))
        .child(theme::heading(tr!("Playlists"), 26.));
    if let Some(account) = view.snapshot.active_account.clone() {
        header = header.child(theme::round(
            Button::new("playlists-sync")
                .icon(IconName::RefreshCw)
                .accessibility_label(tr!("Actualiser depuis Yoto"))
                .tooltip(tr!("Actualiser depuis Yoto"))
                .loading(syncing)
                .on_click(
                    cx.listener(move |v, _, _, cx| v.send(Command::Sync(account.clone()), cx)),
                ),
            Look::Soft(&theme::LIGHT_SAND),
            36.,
            syncing,
            cx,
        ));
    }
    let can_create = view.snapshot.active_account.is_some()
        && !view.creating_playlist
        && Rules::title(view.new_title.read(cx).value().as_ref()).is_ok();
    let mut list = div()
        .id("playlists-sidebar")
        .w(px(268.))
        .flex_shrink_0()
        .h_full()
        .py(px(24.))
        .px(px(16.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .border_r_2()
        .border_color(theme::c(theme::DIVIDER))
        .child(header);
    if view.snapshot.active_account.is_some() {
        list = list.child(
            div()
                .flex()
                .gap(px(6.))
                .child(
                    theme::small_field(Input::new(&view.new_title))
                        .flex_1()
                        .min_w_0(),
                )
                .child(theme::round(
                    Button::new("new-playlist")
                        .icon(if view.creating_playlist {
                            IconName::LoaderCircle
                        } else {
                            IconName::Plus
                        })
                        .accessibility_label(tr!("Créer une playlist"))
                        .tooltip(tr!("Créer une playlist avec ce titre"))
                        .on_click(cx.listener(|v, _, _, cx| {
                            let title = v.new_title.read(cx).value().trim().to_owned();
                            if let Some(account) = v.snapshot.active_account.clone() {
                                v.send(Command::CreateYotoPlaylist { account, title }, cx);
                            }
                        })),
                    Look::Primary,
                    40.,
                    !can_create,
                    cx,
                )),
        );
        if !matches!(
            view.snapshot.remote_sync.get(&RemoteCollection::Playlists),
            Some(RemoteSyncState::Ready { .. })
        ) {
            list = list.child(theme::note(remote_sync_message(
                &view.snapshot,
                RemoteCollection::Playlists,
            )));
        }
    } else {
        list = list
            .child(theme::note(tr!(
                "Connectez votre compte Yoto pour retrouver et modifier vos playlists."
            )))
            .child(theme::pill(
                Button::new("playlists-account")
                    .label(tr!("Connecter Yoto"))
                    .on_click(cx.listener(|v, _, _, cx| v.navigate(Route::Account, cx))),
                Look::Primary,
                Height::Medium,
                false,
                cx,
            ));
    }
    // During the first sync the list is empty because it is still loading, not because
    // there is no playlist.
    if view.snapshot.active_account.is_some()
        && view.playlist_rows.is_empty()
        && matches!(
            view.snapshot.remote_sync.get(&RemoteCollection::Playlists),
            Some(RemoteSyncState::Ready { .. })
        )
    {
        list = list.child(theme::note(tr!(
            "Aucune playlist Yoto. Créez votre première playlist ou actualisez la liste."
        )));
    }
    list = list.child(
        gpui_kit::uniform_list(
            "playlist-list",
            view.playlist_rows.len(),
            cx.processor(
                |view: &mut AppView,
                 range: std::ops::Range<usize>,
                 _,
                 cx: &mut Context<AppView>| {
                    use gpui_kit::component::button::{ButtonCustomVariant, ButtonRounded};
                    range
                        .map(|index| {
                            if let Some(asset) = view.playlist_rows[index].cover_asset {
                                view.load_cover_preview(asset, cx);
                            }
                            let row = &view.playlist_rows[index];
                            let target = row.target.clone();
                            let key = format!("{}:{}", target.account, target.card);
                            let selected = view.snapshot.bindings.iter().any(|b| {
                                b.account == target.account
                                    && b.card == target.card
                                    && view.selected_playlist == Some(b.playlist)
                            });
                            let loading = view.opening_playlist.as_ref()
                                == Some(&(target.account.clone(), target.card.clone()));
                            let meta = if loading {
                                tr!("Ouverture depuis Yoto…").to_owned()
                            } else {
                                match (&row.state, row.tracks) {
                                    (PlaylistSyncState::Synced, Some(0)) => tr!("Vide").into(),
                                    (PlaylistSyncState::Synced, Some(n)) => {
                                        trn!("{count} piste", "{count} pistes", n)
                                    }
                                    (state, _) => wording::labels::playlist_sync(state).into(),
                                }
                            };
                            let surface = if selected {
                                theme::c(theme::SURFACE)
                            } else {
                                theme::c(theme::CREAM)
                            };
                            let item = Button::new(element_id("playlist", &key))
                                .custom(
                                    ButtonCustomVariant::new(cx)
                                        .color(surface)
                                        .foreground(theme::c(theme::INK))
                                        .hover(theme::c(theme::SURFACE))
                                        .active(theme::c(theme::SAND)),
                                )
                                .accessibility_label(format!("{} · {meta}", row.title))
                                .toggled(selected)
                                .bg(surface)
                                .rounded(ButtonRounded::Size(px(16.)))
                                .h(px(60.))
                                .px(px(8.))
                                .w_full()
                                .disabled(view.opening_playlist.is_some() && !loading)
                                .child(
                                    div()
                                        .w_full()
                                        .flex()
                                        .items_center()
                                        .gap(px(12.))
                                        .child(pixelart::playlist_cover(
                                            row.cover_asset,
                                            row.cover.clone(),
                                            view,
                                            44.,
                                            12.,
                                            false,
                                        ))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .text_size(px(15.))
                                                        .font_weight(FontWeight::EXTRA_BOLD)
                                                        .truncate()
                                                        .child(row.title.clone()),
                                                )
                                                .child(theme::meta(meta).text_size(px(12.))),
                                        ),
                                )
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    open_remote(v, target.account.clone(), target.card.clone(), cx)
                                }));
                            div().h(px(64.)).pb(px(4.)).child(if selected {
                                item.shadow(theme::lift())
                            } else {
                                item
                            })
                        })
                        .collect()
                },
            ),
        )
        .flex_1()
        .min_h(px(180.)),
    );
    if let Some(error) = &view.error {
        list = list.child(
            div()
                .text_size(px(13.))
                .text_color(theme::c(theme::ROSE.ink))
                .child(failure_text(error)),
        );
    }
    let mut main = div().flex_1().min_w_0().h_full().flex().flex_col();
    if let Some(id) = view.selected_playlist {
        if let Some(editor) = view.editors.get(&id) {
            main = main.child(div().flex_1().min_h_0().child(editor.clone()));
        } else {
            let job = view
                .snapshot
                .jobs
                .iter()
                .filter(|job| match &job.request {
                    JobRequest::Publish {
                        snapshot: draft,
                        account,
                    } => draft.id == id && Some(account) == view.snapshot.active_account.as_ref(),
                    JobRequest::Import { .. } | JobRequest::Encode { .. } => false,
                })
                .max_by_key(|job| publication_job_key(job));
            let message = job
                .map(|job| wording::labels::job_state(&job.state))
                .unwrap_or(tr!("Création et vérification sur Yoto…"));
            main = main.child(
                div()
                    .p(px(28.))
                    .flex()
                    .gap(px(10.))
                    .items_center()
                    .font_weight(FontWeight::BOLD)
                    .child(Spinner::new().color(theme::c(theme::LILAC.accent)))
                    .child(message),
            );
        }
    } else {
        main = main.child(
            div()
                .p(px(28.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(theme::thumbnail("playlists", 96., 26.))
                .child(theme::heading(tr!("Vos playlists Yoto"), 32.))
                .child(div().text_color(theme::c(theme::INK_SOFT)).child(
                    tr!("Choisissez une playlist à gauche pour la modifier et y ajouter des audios de votre médiathèque."),
                ))
                .child(theme::note(
                    tr!("Les modifications sont enregistrées directement sur votre compte Yoto."),
                )),
        );
    }
    div()
        .size_full()
        .flex()
        .child(list)
        .child(main)
        .into_any_element()
}

fn parse_shuffle(s: &str) -> yoto_domain::Result<Vec<ShuffleGroup>> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let v: Vec<_> = s.split('-').map(|v| v.trim().parse::<usize>()).collect();
            if v.len() != 3 {
                return Err(Failure::Validation(
                    tr!("Aléatoire attendu début-fin-limite").into(),
                ));
            }
            let start = v[0]
                .clone()
                .map_err(|_| Failure::Validation(tr!("Début de chapitre attendu").into()))?
                .checked_sub(1)
                .ok_or_else(|| Failure::Validation(tr!("Les chapitres commencent à 1").into()))?;
            let end = v[1]
                .clone()
                .map_err(|_| Failure::Validation(tr!("Fin de chapitre attendue").into()))?
                .checked_sub(1)
                .ok_or_else(|| Failure::Validation(tr!("Les chapitres commencent à 1").into()))?;
            let limit = v[2]
                .clone()
                .map_err(|_| Failure::Validation(tr!("Limite entière attendue").into()))?;
            Ok(ShuffleGroup { start, end, limit })
        })
        .collect()
}

fn event(events: &TrackEvents, trigger: EventTrigger) -> &Option<Box<Event>> {
    match trigger {
        EventTrigger::Onend => &events.on_end,
        EventTrigger::Onlhb => &events.on_lhb,
        EventTrigger::Onrhb => &events.on_rhb,
    }
}
fn event_mut(events: &mut TrackEvents, trigger: EventTrigger) -> &mut Option<Box<Event>> {
    match trigger {
        EventTrigger::Onend => &mut events.on_end,
        EventTrigger::Onlhb => &mut events.on_lhb,
        EventTrigger::Onrhb => &mut events.on_rhb,
    }
}

#[cfg(test)]
mod playlist_catalogue_tests {
    use super::playlist_rows;
    use yoto_domain::*;
    use yoto_infra::{Snapshot, api::RemoteCard};
    #[test]
    fn remote_catalogue_hides_legacy_local_drafts_and_uses_api_titles() {
        let account = AccountId::new("family").unwrap();
        let local = Playlist::new("Legacy local".into()).unwrap();
        let card = CardId::new("remote").unwrap();
        let snapshot = Snapshot {
            active_account: Some(account.clone()),
            playlists: vec![local],
            remote: vec![RemoteCard {
                card: card.clone(),
                title: "Titre API".into(),
                document: serde_json::Value::Null,
                synced: 1,
            }],
            ..Default::default()
        };
        let rows = playlist_rows(&snapshot);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Titre API");
        assert_eq!(rows[0].target.account, account);
        assert_eq!(rows[0].target.card, card);
        assert!(
            playlist_rows(&Snapshot {
                active_account: None,
                ..snapshot
            })
            .is_empty()
        );
    }
}

fn coalesce_single_track_title(chapter: &mut Chapter, base: Option<&Chapter>) {
    if chapter.tracks.len() == 1
        && let Some(base) = base
        && base.tracks.len() == 1
        && base.title == base.tracks[0].title
        && chapter.title == base.title
    {
        chapter.title = chapter.tracks[0].title.clone();
    }
}
fn editor_menu_item(
    owner: &WeakEntity<PlaylistEditor>,
    label: &str,
    callback: impl Fn(&mut PlaylistEditor, &mut Window, &mut Context<PlaylistEditor>) + 'static,
) -> PopupMenuItem {
    let owner = owner.clone();
    PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| {
        let _ = owner.update(cx, |editor, cx| {
            if !editor.blocked() {
                callback(editor, window, cx);
            }
        });
    })
}
fn category_label(category: Category) -> &'static str {
    match category {
        Category::None => tr!("Aucune"),
        Category::Stories => tr!("Histoires"),
        Category::Music => tr!("Musique"),
        Category::Radio => tr!("Radio"),
        Category::Podcast => tr!("Podcasts"),
        Category::Sfx => tr!("Sons"),
        Category::Activities => trc!("catégorie", "Activités"),
        Category::Alarms => tr!("Alarmes"),
    }
}
fn advance_label(advance: Autoadvance) -> &'static str {
    match advance {
        Autoadvance::Next => tr!("Lire la piste suivante"),
        Autoadvance::Repeat => tr!("Répéter la piste"),
        Autoadvance::None => tr!("Arrêter la lecture"),
    }
}
fn playback_label(kind: PlaybackType) -> &'static str {
    match kind {
        PlaybackType::Linear => tr!("Lecture continue"),
        PlaybackType::Interactive => tr!("Lecture interactive"),
    }
}
fn event_command_label(command: EventCommand) -> &'static str {
    match command {
        EventCommand::Stop => tr!("Arrêter"),
        EventCommand::Repeat => tr!("Répéter"),
        EventCommand::Goto => tr!("Aller à une piste"),
    }
}
fn event_trigger_label(trigger: EventTrigger) -> &'static str {
    match trigger {
        EventTrigger::Onend => tr!("À la fin de la piste"),
        EventTrigger::Onlhb => tr!("Bouton gauche"),
        EventTrigger::Onrhb => tr!("Bouton droit"),
    }
}

#[cfg(test)]
mod title_edit_tests {
    use super::coalesce_single_track_title;
    use yoto_domain::*;
    fn chapter(title: &str, track_title: &str) -> Chapter {
        Chapter {
            id: ChapterId::new(),
            key: ChapterKey::new("chapter-key").unwrap(),
            title: title.into(),
            icon: None,
            extra: Extra::new(),
            tracks: vec![Track {
                id: TrackId::new(),
                key: TrackKey::new("track-key").unwrap(),
                title: track_title.into(),
                icon: None,
                events: None,
                extra: Extra::new(),
                media: TrackMedia::Stream {
                    url: "https://example.test/audio.mp3".into(),
                    format: AudioFormat::Mp3,
                },
            }],
        }
    }
    #[test]
    fn single_title_updates_matching_chapter_without_changing_ids_or_audio() {
        let base = chapter("Comptine", "Comptine");
        let mut edited = base.clone();
        edited.tracks[0].title = "Nouvelle comptine".into();
        coalesce_single_track_title(&mut edited, Some(&base));
        assert_eq!(edited.title, "Nouvelle comptine");
        assert_eq!(edited.tracks[0].title, edited.title);
        assert_eq!(edited.id, base.id);
        assert_eq!(edited.tracks[0].id, base.tracks[0].id);
        assert_eq!(edited.tracks[0].key, base.tracks[0].key);
        assert_eq!(
            serde_json::to_value(&edited.tracks[0].media).unwrap(),
            serde_json::to_value(&base.tracks[0].media).unwrap()
        );
    }
    #[test]
    fn distinct_or_explicit_advanced_chapter_titles_are_preserved() {
        let base = chapter("Chansons du soir", "Comptine");
        let mut edited = base.clone();
        edited.tracks[0].title = "Nouvelle comptine".into();
        coalesce_single_track_title(&mut edited, Some(&base));
        assert_eq!(edited.title, "Chansons du soir");
        let base = chapter("Comptine", "Comptine");
        let mut edited = base.clone();
        edited.title = "Titre avancé".into();
        edited.tracks[0].title = "Nouvelle comptine".into();
        coalesce_single_track_title(&mut edited, Some(&base));
        assert_eq!(edited.title, "Titre avancé");
    }
    #[test]
    fn multi_track_chapter_title_remains_independent() {
        let mut base = chapter("Histoires", "Histoires");
        let mut second = base.tracks[0].clone();
        second.id = TrackId::new();
        second.key = TrackKey::new("second-key").unwrap();
        second.title = "Deuxième histoire".into();
        base.tracks.push(second);
        let mut edited = base.clone();
        edited.tracks[0].title = "Première histoire".into();
        coalesce_single_track_title(&mut edited, Some(&base));
        assert_eq!(edited.title, "Histoires");
        assert_eq!(edited.tracks.len(), 2);
        assert_eq!(edited.tracks[1].id, base.tracks[1].id);
    }
}

#[cfg(test)]
mod parent_input_tests {
    use super::{SyncState, parse_shuffle};
    #[test]
    fn shuffle_ranges_are_typed_from_chapter_one() {
        let groups = parse_shuffle(" 1-3-2 , 4-6-1 ,").unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!((groups[0].start, groups[0].end, groups[0].limit), (0, 2, 2));
        assert_eq!((groups[1].start, groups[1].end, groups[1].limit), (3, 5, 1));
        assert!(parse_shuffle("").unwrap().is_empty());
        assert!(parse_shuffle("0-2-1").is_err());
        assert!(parse_shuffle("1-2").is_err());
        assert!(parse_shuffle("1-x-1").is_err());
    }
    #[test]
    fn a_running_save_outranks_anything_to_check() {
        assert_eq!(SyncState::of(true, true, true), SyncState::Sending);
        assert_eq!(SyncState::of(true, false, false), SyncState::Sending);
        assert_eq!(SyncState::of(false, true, true), SyncState::ToCheck);
        assert_eq!(SyncState::of(false, false, true), SyncState::Unsaved);
        assert_eq!(SyncState::of(false, false, false), SyncState::OnYoto);
        assert_eq!(SyncState::Sending.playlist_label(), "Enregistrement…");
        assert_eq!(SyncState::OnYoto.playlist_label(), "Enregistré sur Yoto");
        assert_eq!(SyncState::OnYoto.track_label(), "Sur Yoto");
    }
}
