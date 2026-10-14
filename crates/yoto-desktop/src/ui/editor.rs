use super::*;
use gpui_kit::prelude::FluentBuilder;
#[path = "playlist_media_picker.rs"]
mod media_picker;
#[path = "playlist_move_picker.rs"]
mod move_picker;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{WindowExt, spinner::Spinner};
pub(super) use media_picker::{MediaChoices, prepare_media_choices};
#[derive(Clone, Copy)]
enum CommitTarget {
    Editor,
    MediaPicker,
    IconPicker,
    MovePicker,
}
enum PlaylistOperation {
    Submitting,
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
    /// A save is on its way to Yoto.
    Sending,
    /// Something needs the parent: a blocked upload, a newer or deleted Yoto version.
    ToCheck,
    /// What is shown is what Yoto has.
    OnYoto,
}
impl SyncState {
    /// A running save wins over anything to check, which wins over « on Yoto ».
    fn of(busy: bool, needs_attention: bool) -> Self {
        if busy {
            Self::Sending
        } else if needs_attention {
            Self::ToCheck
        } else {
            Self::OnYoto
        }
    }
    fn track_label(self) -> &'static str {
        match self {
            Self::Sending => "Envoi sur Yoto…",
            Self::ToCheck => "À vérifier",
            Self::OnYoto => "Sur Yoto",
        }
    }
    fn playlist_label(self) -> &'static str {
        match self {
            Self::Sending | Self::ToCheck => self.track_label(),
            Self::OnYoto => "Enregistré sur Yoto",
        }
    }
    fn family(self) -> &'static theme::Family {
        match self {
            Self::Sending => &theme::BUTTER,
            Self::ToCheck => &theme::ROSE,
            Self::OnYoto => &theme::MINT,
        }
    }
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
    picker_window: Option<AnyWindowHandle>,
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
                .placeholder("Description")
                .default_value(draft.metadata.description.clone())
        });
        let author = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Auteur")
                .default_value(draft.metadata.author.clone())
        });
        let narrator = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Narrateur")
                .default_value(draft.metadata.narrator.clone())
        });
        let languages = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Langues : fr, en…")
                .default_value(draft.metadata.languages.join(", "))
        });
        let resume = cx.new(|cx| {
            InputState::new(window, cx).default_value(draft.playback.resume_seconds.to_string())
        });
        let min_age = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Âge minimum")
                .default_value(
                    draft
                        .metadata
                        .min_age
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                )
        });
        let max_age = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Âge maximum")
                .default_value(
                    draft
                        .metadata
                        .max_age
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                )
        });
        let shuffle = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Aléatoire : début-fin-limite (chapitres dès 1)")
                .default_value(
                    draft
                        .playback
                        .shuffle
                        .iter()
                        .map(|g| format!("{}-{}-{}", g.start + 1, g.end + 1, g.limit))
                        .collect::<Vec<_>>()
                        .join(", "),
                )
        });
        let stream =
            cx.new(|cx| InputState::new(window, cx).placeholder("URL audio HTTPS de streaming"));
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
                                .placeholder("Paramètres de l’événement")
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
            picker_window: None,
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
        let valid = self.gather(cx).is_ok();
        self.draft.revision = self.base_revision;
        let clean = valid
            && serde_json::to_value(&self.draft).ok() == serde_json::to_value(&self.base).ok();
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
                            if canonical.revision <= v.base_revision || v.external.as_ref().is_some_and(|p|p.revision>canonical.revision) { return }
                            let valid=v.gather(cx).is_ok();
                            v.draft.revision=v.base_revision;
                            if valid && serde_json::to_value(&v.draft).ok()==serde_json::to_value(&v.base).ok() {
                                v.adopt(canonical, window, cx);
                            } else {
                                v.external=Some(canonical);
                                v.status="La playlist a changé sur Yoto. Choisissez « Recharger la version Yoto » avant de continuer.".into();
                                cx.notify();
                            }
                        })
                    });
                }
            });
        } else {
            self.status=if self.deleted { "Cette playlist a été supprimée sur Yoto. Choisissez une autre playlist dans la liste." } else { "La playlist a changé sur Yoto. Choisissez « Recharger la version Yoto » avant de continuer." }.into();
        }
        cx.notify();
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
        self.status = "Playlist actualisée depuis Yoto.".into();
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
                    "La durée de reprise doit être un nombre entier de secondes.".into(),
                )
            })?;
        let age = |input: &Entity<InputState>| {
            let s = input.read(cx).value().to_string();
            if s.trim().is_empty() {
                Ok(None)
            } else {
                s.parse::<u8>()
                    .map(Some)
                    .map_err(|_| Failure::Validation("Âge entier attendu".into()))
            }
        };
        self.draft.metadata.min_age = age(&self.min_age)?;
        self.draft.metadata.max_age = age(&self.max_age)?;
        if let (Some(a), Some(b)) = (self.draft.metadata.min_age, self.draft.metadata.max_age)
            && a > b
        {
            return Err(Failure::Validation(
                "Âge minimum supérieur au maximum".into(),
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
                                        "Les paramètres doivent être un objet JSON".into(),
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
                    Ok(_) => "Opération terminée.".into(),
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
    pub fn save(&mut self, cx: &mut Context<Self>) {
        self.persist(CommitTarget::Editor, cx);
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
            if matches!(&chapter.icon,Some(IconReference::Local {asset}) if abandoned.contains(asset))
            {
                chapter.icon = original.and_then(|c| c.icon.clone());
            }
            for track in &mut chapter.tracks {
                if matches!(&track.icon,Some(IconReference::Local {asset}) if abandoned.contains(asset))
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
            "Images non enregistrées abandonnées. Les autres saisies sont conservées.".into();
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
                    "Ouvrez une playlist de votre compte Yoto.".into(),
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
    pub(super) fn blocked(&self) -> bool {
        if self.save_updates.is_saving() {
            return true;
        }
        match &self.operation {
            Some(PlaylistOperation::Submitting | PlaylistOperation::Pending { .. }) => true,
            Some(PlaylistOperation::Failed(failure)) => {
                matches!(failure, Failure::Ambiguous | Failure::Conflict)
            }
            None => false,
        }
    }
    pub(super) fn busy(&self) -> bool {
        match &self.operation {
            Some(PlaylistOperation::Submitting) => true,
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
    fn persist(&mut self, target: CommitTarget, cx: &mut Context<Self>) {
        if self.blocked() {
            return;
        }
        let account = match self.owning_remote(cx) {
            Ok((account, _)) => account,
            Err(e) => {
                self.status = failure_text(&e);
                cx.notify();
                return;
            }
        };
        if let Err(e) = self.gather(cx) {
            self.status = failure_text(&e);
            cx.notify();
            return;
        }
        self.draft.revision = self.base_revision;
        if serde_json::to_value(&self.draft).ok() == serde_json::to_value(&self.base).ok() {
            let picker = match target {
                CommitTarget::Editor => None,
                CommitTarget::MediaPicker | CommitTarget::IconPicker | CommitTarget::MovePicker => {
                    self.picker_window.take()
                }
            };
            if let Some(handle) = picker {
                self.replacement = None;
                cx.defer(move |cx| {
                    let _ = cx.update_window(handle, |_, window, cx| window.close_dialog(cx));
                });
            }
            return;
        }
        if let Err(e) = self.draft.touch() {
            self.status = failure_text(&e);
            cx.notify();
            return;
        }
        self.start_operation(
            Command::UpdateYotoPlaylist {
                account,
                draft: Box::new(self.draft.clone()),
                expected: self.base_revision,
            },
            target,
            cx,
        );
    }
    fn start_operation(&mut self, command: Command, target: CommitTarget, cx: &mut Context<Self>) {
        if self.blocked() {
            return;
        }
        self.operation = Some(PlaylistOperation::Submitting);
        self.save_updates.begin();
        self.status = "Enregistrement sur Yoto…".into();
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
                        v.status = "Envoi et vérification sur Yoto…".into();
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
        self.status = failure_text(&failure);
        let needs_verification = matches!(failure, Failure::Ambiguous | Failure::Conflict);
        self.operation = Some(PlaylistOperation::Failed(failure));
        self.draft = self.base.clone();
        let queued = self.save_updates.finish();
        if let Some(update) = queued {
            if needs_verification {
                self.external = update.into_playlist();
            } else {
                self.reconcile(update.into_playlist(), cx);
            }
        }
        self.notify_parent(cx);
        cx.notify();
    }
    pub fn apply_snapshot(&mut self, snapshot: &Snapshot, cx: &mut Context<Self>) {
        let (job, target) = match &self.operation {
            Some(PlaylistOperation::Pending { job, target, .. }) => (*job, *target),
            Some(PlaylistOperation::Submitting | PlaylistOperation::Failed(_)) | None => return,
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
                self.save_updates.finish();
                self.operation = None;
                self.base_revision = canonical.revision;
                self.base = canonical.clone();
                self.draft = canonical.clone();
                self.external = None;
                let picker_window = match target {
                    CommitTarget::MediaPicker
                    | CommitTarget::IconPicker
                    | CommitTarget::MovePicker => self.picker_window.take(),
                    CommitTarget::Editor => None,
                };
                let retained_picker = self.picker_window;
                let refresh_icons = if matches!(target, CommitTarget::IconPicker)
                    && let DraftOrigin::Existing { base } = &canonical.origin
                {
                    Some(base.account.clone())
                } else {
                    None
                };
                let parent = self.parent.clone();
                let entity = cx.entity().downgrade();
                cx.defer(move |cx| {
                    if let Some(handle) = picker_window.or_else(|| cx.windows().first().copied()) {
                        let _ = cx.update_window(handle, move |_, window, cx| {
                            if picker_window.is_some() {
                                window.close_dialog(cx);
                            }
                            let _ = entity.update(cx, |v, cx| {
                                v.adopt(canonical, window, cx);
                                v.picker_window = retained_picker;
                                v.status = "Enregistré sur Yoto.".into();
                            });
                            if let Some(account) = refresh_icons {
                                let _ = parent.update(cx, |view, cx| {
                                    view.send(Command::RefreshIcons(account), cx);
                                });
                            }
                        });
                    }
                });
            }
            JobState::Completed {
                output: JobOutput::Imported { .. } | JobOutput::Encoded { .. },
            } => self.fail_operation(
                Failure::Compatibility("Résultat de playlist inattendu.".into()),
                cx,
            ),
            JobState::Failed { failure } => self.fail_operation(failure.clone(), cx),
            JobState::Cancelled => self.fail_operation(Failure::Cancelled, cx),
            state @ (JobState::Queued
            | JobState::Running { .. }
            | JobState::Retry { .. }
            | JobState::WaitingForAuthentication
            | JobState::Conflict { .. }
            | JobState::Reconciling) => {
                self.status = match state {
                    JobState::Queued => "Envoi sur Yoto en attente…".into(),
                    JobState::Running { phase } => format!("{}…", phase.label()),
                    JobState::Retry { failure, .. } => format!("Nouvel essai automatique : {}", failure_text(failure)),
                    JobState::WaitingForAuthentication => "Reconnectez le compte Yoto pour terminer l’envoi.".into(),
                    JobState::Conflict { .. } => "La playlist a changé sur Yoto. Vérifiez la version distante avant de continuer.".into(),
                    JobState::Reconciling => "Yoto a peut-être reçu la modification. Vérification requise avant un nouvel envoi.".into(),
                    JobState::Completed { .. } | JobState::Failed { .. } | JobState::Cancelled => unreachable!("terminal states handled above"),
                };
                self.operation = Some(PlaylistOperation::Pending {
                    job: job.id,
                    state: state.clone(),
                    target,
                });
            }
        }
        cx.notify();
    }
    pub fn open_media_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.owning_remote(cx).is_err() {
            self.status = "Connectez le compte propriétaire de cette playlist.".into();
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
                .title("Ajouter des médias")
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
    /// The 48 px tile showing a track or chapter icon, or a stable pastel glyph without one.
    fn icon_tile(&self, icon: &Option<IconReference>, seed: &str, cx: &App) -> Div {
        let family = theme::family_for(seed);
        let tile = div()
            .size(px(48.))
            .flex_shrink_0()
            .rounded(px(14.))
            .bg(theme::c(family.soft))
            .flex()
            .items_center()
            .justify_center();
        match self.preview_icon(icon, cx) {
            Some(icon) => tile.child(icon),
            None => tile.child(theme::pixel_glyph(
                theme::Glyph::for_seed(seed),
                family.accent,
                3.,
            )),
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
            .accessibility_label(if track.is_some() {
                "Changer l’icône de la piste"
            } else {
                "Changer l’icône du chapitre"
            })
            .tooltip("Changer l’icône")
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
        let Some(chapter) = self.base.chapters.get(index) else {
            return div().into_any_element();
        };
        let id = chapter.id;
        let single = chapter.tracks.len() == 1;
        let count = self.base.chapters.len();
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
                                .accessibility_label("Options du chapitre")
                                .tooltip("Options du chapitre"),
                            Look::Ghost,
                            36.,
                            false,
                            cx,
                        )
                        .dropdown_menu(move |menu, _, _| {
                            menu.item(editor_menu_item(
                                &owner,
                                "Changer l’icône du chapitre",
                                move |v, window, cx| v.open_icon_picker(id, None, window, cx),
                            ))
                            .item(
                                editor_menu_item(
                                    &owner,
                                    "Remonter le chapitre",
                                    move |v, _, cx| {
                                        if let Err(error) = v.draft.move_chapter(id, -1) {
                                            v.status = failure_text(&error);
                                            return;
                                        }
                                        v.save(cx);
                                    },
                                )
                                .disabled(index == 0),
                            )
                            .item(
                                editor_menu_item(
                                    &owner,
                                    "Descendre le chapitre",
                                    move |v, _, cx| {
                                        if let Err(error) = v.draft.move_chapter(id, 1) {
                                            v.status = failure_text(&error);
                                            return;
                                        }
                                        v.save(cx);
                                    },
                                )
                                .disabled(index + 1 == count),
                            )
                            .separator()
                            .item(editor_menu_item(
                                &owner,
                                "Séparer les pistes en chapitres",
                                move |v, window, cx| {
                                    if let Err(error) = v.draft.ungroup(id) {
                                        v.status = failure_text(&error);
                                        return;
                                    }
                                    v.refresh_inputs(window, cx);
                                    v.save(cx);
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
                        .unwrap_or_else(|| "Audio de la médiathèque".into()),
                    Some((*recipe, *media)),
                ),
                TrackMedia::RemoteAudio { info, .. } => {
                    (duration_text(info.duration.seconds()), None)
                }
                TrackMedia::Stream { .. } => ("Streaming".into(), None),
            };
            let menu = theme::round(
                Button::new(element_id("track-menu", tid))
                    .icon(IconName::EllipsisVertical)
                    .accessibility_label("Options de la piste")
                    .tooltip("Options de la piste"),
                Look::Ghost,
                36.,
                false,
                cx,
            )
            .dropdown_menu(move |menu, _, _| {
                menu.item(editor_menu_item(
                    &owner,
                    "Changer l’icône",
                    move |v, window, cx| v.open_icon_picker(id, Some(tid), window, cx),
                ))
                .item(
                    editor_menu_item(
                        &owner,
                        if removal == Some(None) {
                            "Retirer l’icône du chapitre"
                        } else {
                            "Retirer l’icône"
                        },
                        move |v, _, cx| {
                            if let Some(target) = removal {
                                v.assign_icon(id, target, None);
                                v.save(cx);
                            }
                        },
                    )
                    .disabled(removal.is_none()),
                )
                .separator()
                .item(editor_menu_item(
                    &owner,
                    "Remplacer l’audio…",
                    move |v, window, cx| {
                        v.replacement = Some(tid);
                        v.open_media_picker(window, cx);
                    },
                ))
                .item(
                    editor_menu_item(&owner, "Déplacer / regrouper…", move |v, window, cx| {
                        v.open_move_picker(id, tid, window, cx)
                    })
                    .disabled(count < 2),
                )
                .item(
                    editor_menu_item(&owner, "Remonter", move |v, _, cx| {
                        let result = if single {
                            v.draft.move_chapter(id, -1)
                        } else {
                            v.draft.move_track(tid, id, track_index - 1)
                        };
                        if let Err(error) = result {
                            v.status = failure_text(&error);
                            return;
                        }
                        v.save(cx);
                    })
                    .disabled(if single {
                        index == 0
                    } else {
                        track_index == 0
                    }),
                )
                .item(
                    editor_menu_item(&owner, "Descendre", move |v, _, cx| {
                        let result = if single {
                            v.draft.move_chapter(id, 1)
                        } else {
                            v.draft.move_track(tid, id, track_index + 1)
                        };
                        if let Err(error) = result {
                            v.status = failure_text(&error);
                            return;
                        }
                        v.save(cx);
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
                    "Réglages avancés",
                    move |v, _, cx| {
                        if !v.expanded_tracks.remove(&tid) {
                            v.expanded_tracks.insert(tid);
                        }
                        cx.notify();
                    },
                ))
                .item(editor_menu_item(
                    &owner,
                    "Retirer de la playlist",
                    move |v, _, cx| {
                        if let Err(error) = v.draft.remove_track(tid) {
                            v.status = failure_text(&error);
                            return;
                        }
                        v.save(cx);
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
                        .child(theme::tag(state.track_label(), state.family()))
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
                                        format!("Arrêter l’écoute de {}", track.title)
                                    } else {
                                        format!("Écouter {}", track.title)
                                    })
                                    .tooltip(if playing {
                                        "Arrêter l’écoute"
                                    } else {
                                        "Écouter sur cet ordinateur"
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
                        .child(theme::note(
                            "Titre du chapitre (facultatif, distinct du titre de la piste)",
                        ))
                        .children(
                            self.chapter_titles
                                .get(&id)
                                .map(|input| theme::small_field(Input::new(input))),
                        )
                        .child(
                            theme::pill(
                                Button::new(element_id("advanced-chapter-menu", id))
                                    .label("Icône du chapitre")
                                    .dropdown_caret(true),
                                Look::Outline,
                                Height::Small,
                                false,
                                cx,
                            )
                            .dropdown_menu(move |menu, _, _| {
                                menu.item(editor_menu_item(
                                    &owner,
                                    "Changer l’icône du chapitre",
                                    move |v, window, cx| v.open_icon_picker(id, None, window, cx),
                                ))
                                .item(editor_menu_item(
                                    &owner,
                                    "Retirer l’icône du chapitre",
                                    move |v, _, cx| {
                                        v.assign_icon(id, None, None);
                                        v.save(cx);
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
                                        .label(selected.map(event_command_label).unwrap_or("Aucun"))
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
                                                    "Aucun",
                                                    move |v, _, cx| {
                                                        v.change_event(tid, trigger, None);
                                                        v.save(cx);
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
                                                            v.save(cx);
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
                        "Les événements de piste s’activent avec la lecture interactive, dans les réglages avancés de la playlist.",
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
            pixelart::IconPicker::new(editor.clone(), parent, account.clone(), chapter, track, cx)
        });
        self.picker_window = Some(window.window_handle());
        let weak = editor.downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let cancel = picker.downgrade();
            let close = weak.clone();
            dialog
                .title("Choisir une icône")
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
            self.status =
                "Le compte ou la playlist a changé. Rouvrez le sélecteur d’icônes.".into();
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
        self.persist(CommitTarget::IconPicker, cx);
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
        let Some(parent) = self.parent.upgrade() else {
            return;
        };
        let Some(choices) = parent
            .read(cx)
            .playlist_media_choices
            .chapters
            .get(&self.base.id)
            .cloned()
        else {
            return;
        };
        let editor = cx.entity();
        let picker =
            cx.new(|cx| move_picker::MovePicker::new(editor.clone(), choices, chapter, track, cx));
        self.picker_window = Some(window.window_handle());
        let weak = editor.downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let cancel = weak.clone();
            let close = weak.clone();
            dialog
                .title("Déplacer dans un chapitre")
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
    pub(super) fn add_media_batch(
        &mut self,
        media: Vec<(MediaId, RecipeId)>,
        cx: &mut Context<Self>,
    ) {
        if media.is_empty() || self.blocked() {
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
        if let Some(track) = self.replacement.take() {
            if media.len() != 1 {
                self.status = "Sélectionnez un seul média pour remplacer cette piste.".into();
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
                self.persist(CommitTarget::MediaPicker, cx);
            } else {
                self.status =
                    "Cette piste n’existe plus dans la playlist ; l’audio n’a pas été remplacé."
                        .into();
                cx.notify();
            }
        } else {
            self.start_operation(
                Command::AddYotoMedia {
                    account,
                    card,
                    media,
                },
                CommitTarget::MediaPicker,
                cx,
            );
        }
    }
    pub fn delete_remote(&mut self, cx: &mut Context<Self>) {
        if self.blocked() {
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
        self.operation = Some(PlaylistOperation::Submitting);
        self.save_updates.begin();
        self.status = "Suppression sur Yoto…".into();
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
                        v.operation = None;
                        let _ = v.parent.update(cx, |parent, cx| {
                            if parent.selected_playlist == Some(id) {
                                parent.selected_playlist = None;
                            }
                            parent.editors.remove(&id);
                            parent.status = "Playlist supprimée sur Yoto.".into();
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
                        | gpui_kit::component::input::InputEvent::PressEnter { .. } => v.save(cx),
                        gpui_kit::component::input::InputEvent::Change
                        | gpui_kit::component::input::InputEvent::Focus => {}
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
    fn preview_icon(&self, icon: &Option<IconReference>, cx: &App) -> Option<AnyElement> {
        match icon {
            Some(IconReference::Local { asset }) => self
                .icon_previews
                .get(asset)
                .map(|art| pixelart::thumbnail(art, 2., cx)),
            Some(IconReference::Yoto { account, media }) => {
                self.parent.upgrade().and_then(|parent| {
                    let parent = parent.read(cx);
                    if parent.snapshot.active_account.as_ref() != Some(account) {
                        return None;
                    }
                    parent
                        .playlist_icon_choices
                        .url(media)
                        .map(|url| {
                            img(url.clone())
                                .w(px(32.))
                                .h(px(32.))
                                .object_fit(ObjectFit::Contain)
                                .into_any_element()
                        })
                        .or_else(|| {
                            Some(
                                div()
                                    .w(px(32.))
                                    .h(px(32.))
                                    .text_sm()
                                    .child("◇")
                                    .into_any_element(),
                            )
                        })
                })
            }
            None => None,
        }
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
    fn cover(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let rt = self.runtime.clone();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            if let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("Image", &["png", "jpg", "jpeg", "gif"])
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
                            v.save(cx);
                        }
                        Err(e) => v.status = failure_text(&e),
                    }
                    cx.notify();
                });
            }
        }));
    }
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
    /// The sending state shown on every track and in the header.
    fn sync_state(&self) -> SyncState {
        SyncState::of(
            self.busy(),
            self.blocked() || self.external.is_some() || self.deleted,
        )
    }
    fn header(&self, snapshot: &Snapshot, cx: &mut Context<Self>) -> Div {
        use gpui_kit::component::button::{ButtonCustomVariant, ButtonRounded};
        use theme::{Height, Look};
        let tracks = self
            .base
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
            0 => "Vide".to_owned(),
            1 => format!("1 piste · {}", duration_text(seconds)),
            n => format!("{n} pistes · {}", duration_text(seconds)),
        };
        let blocked = self.blocked();
        let state = self.sync_state();
        let seed = self.base.id.to_string();
        let cover_family = theme::family_for(&seed);
        let creating = self
            .parent
            .upgrade()
            .is_some_and(|p| p.read(cx).creating_playlist);
        let owner = cx.entity().downgrade();
        let duplicate = owner.clone();
        let remove = owner.clone();
        let cover = Button::new("cover")
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(theme::c(cover_family.soft))
                    .hover(theme::c(cover_family.soft))
                    .active(theme::c(cover_family.step)),
            )
            .bg(theme::c(cover_family.soft))
            .rounded(ButtonRounded::Size(px(26.)))
            .size(px(120.))
            .shadow(theme::step(cover_family.step, 3.))
            .accessibility_label("Choisir une couverture")
            .tooltip(if self.base.metadata.cover.is_some() {
                "Changer la couverture"
            } else {
                "Choisir une couverture"
            })
            .disabled(blocked)
            .child(theme::pixel_glyph(
                theme::Glyph::for_seed(&seed),
                cover_family.accent,
                7.,
            ))
            .on_click(cx.listener(|v, _, _, cx| v.cover(cx)));
        let mut tags = div().flex().gap(px(6.));
        if self.base.metadata.category != Category::None {
            tags = tags.child(theme::tag(
                category_label(self.base.metadata.category),
                &theme::LILAC,
            ));
        }
        tags = tags.child(theme::tag(state.playlist_label(), state.family()));
        div()
            .flex()
            .gap(px(20.))
            .items_end()
            .child(cover)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
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
                                .accessibility_label("Plus d’actions")
                                .tooltip("Plus d’actions"),
                            Look::Outline,
                            44.,
                            false,
                            cx,
                        )
                        .dropdown_menu(move |menu, _, _| {
                            let duplicate = duplicate.clone();
                            let remove = remove.clone();
                            menu.item(
                                PopupMenuItem::new("Dupliquer la playlist")
                                    .disabled(blocked || creating)
                                    .on_click(move |_, _, cx| {
                                        let _ = duplicate.update(cx, |e, cx| e.duplicate(cx));
                                    }),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new("Supprimer la playlist de mon compte Yoto")
                                    .disabled(blocked)
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
                                            format!("Supprimer « {title} » de votre compte Yoto ?"),
                                            "La playlist disparaîtra de votre compte Yoto et de vos lecteurs. Vos fichiers audio restent dans la médiathèque. Cette action est irréversible.",
                                            "Supprimer",
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
                            .label("Ajouter des médias")
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
                "Description",
                theme::small_field(Input::new(&self.description)).into_any_element(),
            ))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(labelled(
                        "Auteur",
                        theme::small_field(Input::new(&self.author)).into_any_element(),
                    ))
                    .child(labelled(
                        "Narrateur",
                        theme::small_field(Input::new(&self.narrator)).into_any_element(),
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(labelled(
                        "Langues",
                        theme::small_field(Input::new(&self.languages)).into_any_element(),
                    ))
                    .child(labelled(
                        "Âge minimum",
                        theme::small_field(Input::new(&self.min_age)).into_any_element(),
                    ))
                    .child(labelled(
                        "Âge maximum",
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
                        "Catégorie",
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
                                            v.save(cx);
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
                        "Après une piste",
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
                                            v.save(cx);
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
            .child(theme::note(
                "Les modifications sont enregistrées sur Yoto quand vous quittez un champ ou appuyez sur Entrée.",
            ))
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
                    .child(div().font_weight(FontWeight::EXTRA_BOLD).child("Lecture"))
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
                                            v.save(cx);
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
                    .label("Connexion requise sur le lecteur")
                    .checked(self.draft.playback.online_only)
                    .on_click(cx.listener(|v, checked, _, cx| {
                        v.draft.playback.online_only = *checked;
                        v.save(cx);
                    })),
            )
            .child(theme::note(
                "Lecture aléatoire : début-fin-limite (chapitres dès 1)",
            ))
            .child(theme::small_field(Input::new(&self.shuffle)))
            .child(theme::note("Durée de reprise (secondes)"))
            .child(theme::small_field(Input::new(&self.resume)))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(theme::small_field(Input::new(&self.stream)).flex_1())
                    .child(theme::pill(
                        Button::new("add-stream")
                            .label("Ajouter un stream")
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
                        .label("Abandonner les images non enregistrées")
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
            .child(theme::note(
                "Les événements de chaque piste se règlent depuis son menu, « Réglages avancés ».",
            ))
    }
    fn add_stream(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.stream.read(cx).value().to_string();
        if let Ok(mut p) = Playlist::new("Streaming".into()) {
            let track = Track {
                events: None,
                id: TrackId::new(),
                key: TrackKey::new(uuid_string()).expect("uuid"),
                title: "Streaming".into(),
                icon: None,
                media: TrackMedia::Stream {
                    url,
                    format: AudioFormat::Mp3,
                },
                extra: Extra::new(),
            };
            p.chapters.push(Chapter {
                id: ChapterId::new(),
                key: ChapterKey::new(uuid_string()).expect("uuid"),
                title: "Streaming".into(),
                icon: None,
                tracks: vec![track],
                extra: Extra::new(),
            });
            self.draft.chapters.extend(p.chapters);
            self.refresh_inputs(window, cx);
            self.save(cx);
        }
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
        if self.blocked() {
            let mut pending = theme::inset().child(
                div()
                    .flex()
                    .gap(px(10.))
                    .items_center()
                    .children(self.busy().then(|| {
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
                div().child(theme::pill(
                    Button::new("remote-operation-details")
                        .label("Voir le suivi")
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
            let state = self.sync_state();
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
                            .child(theme::tag(state.track_label(), state.family())),
                    );
                }
            }
            return body.into_any_element();
        }
        if self.draft.chapters.is_empty() {
            body = body.child(
                theme::inset()
                    .p(px(20.))
                    .child(theme::heading("Cette playlist est vide", 19.))
                    .child(theme::note(
                        "Cliquez sur « Ajouter des médias » pour y mettre des audios de votre médiathèque.",
                    )),
            );
        }
        let state = self.sync_state();
        let mut tracks = div().flex().flex_col().gap(px(8.));
        for index in 0..self.base.chapters.len() {
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
                    .children(self.external.is_some().then(|| {
                        div().flex().gap(px(8.)).child(theme::pill(
                            Button::new("reload-playlist")
                                .label("Recharger la version Yoto")
                                .on_click(cx.listener(|v, _, window, cx| {
                                    if let Some(p) = v.external.take() {
                                        v.adopt(p, window, cx);
                                    }
                                })),
                            Look::Primary,
                            Height::Medium,
                            false,
                            cx,
                        ))
                    })),
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
                            "Options de la playlist",
                            "Description, auteur, catégorie, langue, lecture",
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
                            "Réglages avancés",
                            "Lecture interactive, événements",
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
                (!self.status.is_empty() && self.external.is_none() && !self.deleted)
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
fn uuid_string() -> String {
    TrackId::new().to_string().replace('-', "")
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
        .child(theme::heading("Playlists", 26.));
    if let Some(account) = view.snapshot.active_account.clone() {
        header = header.child(theme::round(
            Button::new("playlists-sync")
                .icon(IconName::RefreshCw)
                .accessibility_label("Actualiser depuis Yoto")
                .tooltip("Actualiser depuis Yoto")
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
                        .accessibility_label("Créer une playlist")
                        .tooltip("Créer une playlist avec ce titre")
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
            .child(theme::note(
                "Connectez votre compte Yoto pour retrouver et modifier vos playlists.",
            ))
            .child(theme::pill(
                Button::new("playlists-account")
                    .label("Connecter Yoto")
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
        list = list.child(theme::note(
            "Aucune playlist Yoto. Créez votre première playlist ou actualisez la liste.",
        ));
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
                                "Ouverture depuis Yoto…".to_owned()
                            } else {
                                match (&row.state, row.tracks) {
                                    (PlaylistSyncState::Synced, Some(0)) => "Vide".into(),
                                    (PlaylistSyncState::Synced, Some(1)) => "1 piste".into(),
                                    (PlaylistSyncState::Synced, Some(n)) => format!("{n} pistes"),
                                    (state, _) => state.label().into(),
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
                                        .child(theme::thumbnail(&key, 44., 12.))
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
                .map(|job| job.state.label())
                .unwrap_or("Création et vérification sur Yoto…");
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
                .child(theme::heading("Vos playlists Yoto", 32.))
                .child(div().text_color(theme::c(theme::INK_SOFT)).child(
                    "Choisissez une playlist à gauche pour la modifier et y ajouter des audios de votre médiathèque.",
                ))
                .child(theme::note(
                    "Les modifications sont enregistrées directement sur votre compte Yoto.",
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
                    "Aléatoire attendu début-fin-limite".into(),
                ));
            }
            let start = v[0]
                .clone()
                .map_err(|_| Failure::Validation("Début de chapitre attendu".into()))?
                .checked_sub(1)
                .ok_or_else(|| Failure::Validation("Les chapitres commencent à 1".into()))?;
            let end = v[1]
                .clone()
                .map_err(|_| Failure::Validation("Fin de chapitre attendue".into()))?
                .checked_sub(1)
                .ok_or_else(|| Failure::Validation("Les chapitres commencent à 1".into()))?;
            let limit = v[2]
                .clone()
                .map_err(|_| Failure::Validation("Limite entière attendue".into()))?;
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
        Category::None => "Aucune",
        Category::Stories => "Histoires",
        Category::Music => "Musique",
        Category::Radio => "Radio",
        Category::Podcast => "Podcasts",
        Category::Sfx => "Sons",
        Category::Activities => "Activités",
        Category::Alarms => "Alarmes",
    }
}
fn advance_label(advance: Autoadvance) -> &'static str {
    match advance {
        Autoadvance::Next => "Lire la piste suivante",
        Autoadvance::Repeat => "Répéter la piste",
        Autoadvance::None => "Arrêter la lecture",
    }
}
fn playback_label(kind: PlaybackType) -> &'static str {
    match kind {
        PlaybackType::Linear => "Lecture continue",
        PlaybackType::Interactive => "Lecture interactive",
    }
}
fn event_command_label(command: EventCommand) -> &'static str {
    match command {
        EventCommand::Stop => "Arrêter",
        EventCommand::Repeat => "Répéter",
        EventCommand::Goto => "Aller à une piste",
    }
}
fn event_trigger_label(trigger: EventTrigger) -> &'static str {
    match trigger {
        EventTrigger::Onend => "À la fin de la piste",
        EventTrigger::Onlhb => "Bouton gauche",
        EventTrigger::Onrhb => "Bouton droit",
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
