use super::*;
use gpui_kit::component::{
    WindowExt,
    checkbox::Checkbox,
    dialog::DialogClose,
    menu::{DropdownMenu, PopupMenuItem},
    slider::{Slider, SliderState},
};

#[derive(Clone, Copy)]
enum SelectionOrigin {
    Handles,
    NumericInput,
    Canonical,
}
#[derive(Clone, Copy)]
enum AudioEditor {
    Trim,
    Metadata,
}
#[derive(Default)]
enum Preparation {
    #[default]
    Idle,
    Preparing,
    Encoding(RecipeId),
    Ready(RecipeId),
    Failed,
}
impl Preparation {
    fn busy(&self) -> bool {
        match self {
            Self::Preparing | Self::Encoding(_) => true,
            Self::Idle | Self::Ready(_) | Self::Failed => false,
        }
    }
}
struct TrimDraft {
    before: TrimRange,
    normalization: Normalization,
    preparation: Preparation,
}

/// Retained player state. The modal observes this entity instead of duplicating its recipe or inputs.
pub struct TrimPanel {
    media: MediaItem,
    service: Arc<Service>,
    runtime: Arc<tokio::runtime::Runtime>,
    title: Entity<InputState>,
    author: Entity<InputState>,
    tags: Entity<InputState>,
    start: Entity<InputState>,
    end: Entity<InputState>,
    range: Entity<SliderState>,
    cursor: Entity<SliderState>,
    cursor_seconds: f64,
    playing: bool,
    preview_pending: Option<u64>,
    generation: u64,
    normalization: Normalization,
    pub waveform: Option<Waveform>,
    prepared: Option<RecipeId>,
    selected_version: Option<TrimRange>,
    editing: Option<TrimDraft>,
    editor_window: Option<AnyWindowHandle>,
    tasks: Vec<Task<()>>,
    status: String,
    selection: TrimRange,
    _subscriptions: Vec<Subscription>,
}
impl TrimPanel {
    pub fn new(
        media: MediaItem,
        service: Arc<Service>,
        runtime: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = cx.new(|cx| InputState::new(window, cx).default_value(media.title.clone()));
        let author = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Auteur")
                .default_value(media.author.clone())
        });
        let tags = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Étiquettes séparées par des virgules")
                .default_value(media.tags.join(", "))
        });
        let start = cx.new(|cx| InputState::new(window, cx).default_value("0.000"));
        let end = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format!("{:.3}", media.info.duration.seconds()))
        });
        let range = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(media.info.duration.seconds() as f32)
                .step(0.001)
                .default_value((0., media.info.duration.seconds() as f32))
        });
        let cursor = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(1.)
                .step(0.0001)
                .default_value(0.)
        });
        let selection = TrimRange::new(MediaTime::ZERO, media.info.duration, media.info.duration)
            .expect("probed duration");
        let mut subscriptions = vec![cx.subscribe_in(&range, window, |v, _, event, window, cx| {
            let value = match event {
                gpui_kit::component::slider::SliderEvent::Change(value)
                | gpui_kit::component::slider::SliderEvent::Release(value) => *value,
            };
            match bounded_slider_range(value.start(), value.end(), v.media.info.duration) {
                Ok(trim) => v.set_selection(trim, SelectionOrigin::Handles, window, cx),
                Err(_) => {
                    v.status = "La fin doit se trouver après le début.".into();
                    cx.notify();
                }
            }
        })];
        for input in [&start, &end] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |v, _, event, window, cx| match event {
                    gpui_kit::component::input::InputEvent::Change
                    | gpui_kit::component::input::InputEvent::Blur
                    | gpui_kit::component::input::InputEvent::PressEnter { .. } => {
                        match v.range(cx) {
                            Ok(trim) => {
                                v.set_selection(trim, SelectionOrigin::NumericInput, window, cx)
                            }
                            Err(_) => {
                                v.status =
                                    "Choisissez un début et une fin valides, en secondes.".into();
                                cx.notify();
                            }
                        }
                    }
                    gpui_kit::component::input::InputEvent::Focus => {}
                },
            ));
        }
        subscriptions.push(
            cx.subscribe_in(&cursor, window, |v, _, event, _, cx| match event {
                gpui_kit::component::slider::SliderEvent::Release(value) => {
                    v.cursor_seconds = v.playback_range().start().seconds()
                        + value.start() as f64 * v.playback_range().duration().seconds();
                    v.play_from_cursor(cx);
                }
                gpui_kit::component::slider::SliderEvent::Change(value) => {
                    v.cursor_seconds = v.playback_range().start().seconds()
                        + value.start() as f64 * v.playback_range().duration().seconds();
                    cx.notify();
                }
            }),
        );
        let mut view = Self {
            cursor,
            cursor_seconds: 0.,
            playing: false,
            preview_pending: None,
            generation: 0,
            selection,
            _subscriptions: subscriptions,
            media,
            service,
            runtime,
            title,
            author,
            tags,
            start,
            end,
            range,
            normalization: Normalization::Preserve,
            waveform: None,
            prepared: None,
            selected_version: None,
            editing: None,
            editor_window: None,
            tasks: vec![],
            status: String::new(),
        };
        let service = view.service.clone();
        let rt = view.runtime.clone();
        let media = view.media.id;
        view.tasks.push(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let service = service.clone();
                let result = rt
                    .spawn(async move { service.execute(Command::PreviewStatus).await })
                    .await;
                if let Ok(Ok(Reply::Data(data))) = result {
                    if let Ok(status) = serde_json::from_value::<PreviewStatus>(data) {
                        let update = weak.update(cx, |v, cx| {
                            match status {
                                PreviewStatus::Playing {
                                    media: id,
                                    position,
                                } if id == media => {
                                    v.playing = true;
                                    v.cursor_seconds = position.seconds();
                                    let input = v.cursor.clone();
                                    let range = v.playback_range();
                                    let value = ((position.seconds() - range.start().seconds())
                                        / range.duration().seconds())
                                    .clamp(0., 1.)
                                        as f32;
                                    if let Some(window) = cx.windows().first().copied() {
                                        let _ = cx.update_window(window, move |_, window, cx| {
                                            input.update(cx, |s, cx| s.set_value(value, window, cx))
                                        });
                                    }
                                }
                                PreviewStatus::Playing { .. } | PreviewStatus::Stopped => {
                                    v.playing = false
                                }
                            }
                            cx.notify();
                        });
                        if update.is_err() {
                            break;
                        }
                    }
                } else if weak.upgrade().is_none() {
                    break;
                }
            }
        }));
        view
    }
    fn range(&self, cx: &App) -> yoto_domain::Result<TrimRange> {
        let s = self
            .start
            .read(cx)
            .value()
            .parse::<f64>()
            .map_err(|_| Failure::Validation("Début invalide".into()))?;
        let e = self
            .end
            .read(cx)
            .value()
            .parse::<f64>()
            .map_err(|_| Failure::Validation("Fin invalide".into()))?;
        TrimRange::new(
            MediaTime::from_seconds(s)?,
            MediaTime::from_seconds(e)?,
            self.media.info.duration,
        )
    }
    fn set_selection(
        &mut self,
        trim: TrimRange,
        origin: SelectionOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection != trim {
            self.generation = self.generation.wrapping_add(1);
            self.stop(cx);
        }
        self.selection = trim;
        match origin {
            SelectionOrigin::Handles | SelectionOrigin::Canonical => {
                self.start.update(cx, |v, cx| {
                    v.set_value(format!("{:.3}", trim.start().seconds()), window, cx)
                });
                self.end.update(cx, |v, cx| {
                    v.set_value(format!("{:.3}", trim.end().seconds()), window, cx)
                });
            }
            SelectionOrigin::NumericInput => {}
        }
        match origin {
            SelectionOrigin::NumericInput | SelectionOrigin::Canonical => {
                self.range.update(cx, |v, cx| {
                    v.set_value(
                        (trim.start().seconds() as f32, trim.end().seconds() as f32),
                        window,
                        cx,
                    )
                });
            }
            SelectionOrigin::Handles => {}
        }
        self.status.clear();
        cx.notify();
    }
    pub fn apply_snapshot(&mut self, snapshot: &Snapshot, cx: &mut Context<Self>) {
        if let Some(media) = snapshot.media.iter().find(|m| m.id == self.media.id) {
            self.media = media.clone();
        }
        // Recipe order belongs to the media; database iteration order is unrelated to chronology.
        if self.editing.is_none() {
            let latest = self.media.recipes.iter().rev().find_map(|id| {
                snapshot
                    .recipes
                    .iter()
                    .find(|r| r.id == *id && r.result.is_some())
            });
            let prepared = latest.map(|r| r.id);
            if prepared != self.prepared {
                self.prepared = prepared;
                self.selected_version = latest.map(|r| r.trim);
                self.normalization = latest
                    .map(|r| r.normalization)
                    .unwrap_or(Normalization::Preserve);
                self.cursor_seconds = self.playback_range().start().seconds();
                self.stop(cx);
            }
        }
        if let Some(draft) = &mut self.editing
            && let Preparation::Encoding(id) = draft.preparation
        {
            if let Some(recipe) = snapshot
                .recipes
                .iter()
                .find(|r| r.id == id && r.result.is_some())
            {
                draft.preparation = Preparation::Ready(id);
                self.prepared = Some(id);
                self.selected_version = Some(recipe.trim);
                self.status = "Découpe enregistrée. Vous pouvez l’ajouter à une playlist.".into();
            } else if let Some(job) = snapshot.jobs.iter().find(|j| match j.request {
                JobRequest::Encode { recipe } => recipe == id,
                JobRequest::Import { .. } | JobRequest::Publish { .. } => false,
            }) {
                match &job.state {
                    JobState::Failed { failure } => {
                        draft.preparation = Preparation::Failed;
                        self.status = failure_text(failure);
                    }
                    JobState::Cancelled => {
                        draft.preparation = Preparation::Failed;
                        self.status = "Préparation annulée.".into();
                    }
                    JobState::Queued
                    | JobState::Running { .. }
                    | JobState::Retry { .. }
                    | JobState::WaitingForAuthentication
                    | JobState::Conflict { .. }
                    | JobState::Reconciling
                    | JobState::Completed { .. } => {
                        self.status = format!("Préparation : {}…", job.state.label())
                    }
                }
            }
        }
        if self
            .editing
            .as_ref()
            .is_some_and(|d| matches!(d.preparation, Preparation::Ready(_)))
        {
            self.editing = None;
            self.cursor_seconds = self.playback_range().start().seconds();
            if let Some(handle) = self.editor_window.take() {
                let cursor = self.cursor.clone();
                let _ = cx.update_window(handle, move |_, window, cx| {
                    cursor.update(cx, |s, cx| s.set_value(0., window, cx));
                    window.close_dialog(cx);
                });
            }
        }
        cx.notify();
    }
    fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        let rt = self.runtime.clone();
        let service = self.service.clone();
        let generation = self.generation;
        let preview = matches!(&command, Command::Preview(_) | Command::PreviewTrim { .. });
        if preview {
            self.preview_pending = Some(generation);
            cx.notify();
        }
        let metadata = if let Command::UpdateMedia { metadata, .. } = &command {
            Some(metadata.clone())
        } else {
            None
        };
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let r = rt
                .spawn(async move { service.execute(command).await })
                .await;
            let _ = weak.update(cx, |view, cx| {
                if preview && generation != view.generation {
                    return;
                }
                if preview {
                    view.preview_pending = None;
                }
                match r {
                    Ok(Ok(Reply::Prepared(id))) => {
                        if let Some(draft) = &mut view.editing {
                            draft.preparation = Preparation::Encoding(id);
                        }
                        view.status = "Préparation de votre découpe…".into();
                    }
                    Ok(Ok(
                        Reply::AppUpdate(_)
                        | Reply::Updated
                        | Reply::Saved(_)
                        | Reply::OpenedPlaylist(_)
                        | Reply::PlaylistOperation { .. },
                    ))
                    | Ok(Ok(Reply::Entries(_)))
                    | Ok(Ok(Reply::Waveform(_)))
                    | Ok(Ok(Reply::Player(_)))
                    | Ok(Ok(Reply::Data(_)))
                    | Ok(Ok(Reply::Asset(_)))
                    | Ok(Ok(Reply::PixelArt(_) | Reply::Image(_) | Reply::Images(_))) => {
                        if let Some(metadata) = metadata {
                            view.media.title = metadata.title;
                            view.media.author = metadata.author;
                            view.media.tags = metadata.tags;
                            view.status = "Informations enregistrées.".into();
                        }
                    }
                    Ok(Err(e)) => {
                        view.status = failure_text(&e);
                        if let Some(draft) = &mut view.editing
                            && draft.preparation.busy()
                        {
                            draft.preparation = Preparation::Failed;
                        }
                    }
                    Err(e) => {
                        view.status = failure_text(&Failure::Storage(e.to_string()));
                        if let Some(draft) = &mut view.editing
                            && draft.preparation.busy()
                        {
                            draft.preparation = Preparation::Failed;
                        }
                    }
                }
                cx.notify();
            });
        }));
    }
    fn stop(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        self.playing = false;
        self.preview_pending = None;
        self.send(Command::StopPreview, cx);
    }
    pub fn chosen_recipe(&self) -> Option<RecipeId> {
        self.prepared
    }
    fn playback_range(&self) -> TrimRange {
        if self.editing.is_some() {
            self.selection
        } else {
            self.selected_version.unwrap_or_else(|| {
                TrimRange::new(
                    MediaTime::ZERO,
                    self.media.info.duration,
                    self.media.info.duration,
                )
                .expect("probed duration")
            })
        }
    }
    fn play_from_cursor(&mut self, cx: &mut Context<Self>) {
        let range = self.playback_range();
        let start = MediaTime::from_seconds(self.cursor_seconds).unwrap_or(range.start());
        let start = if start < range.start() || start >= range.end() {
            range.start()
        } else {
            start
        };
        let trim = TrimRange::new(start, range.end(), self.media.info.duration)
            .expect("bounded playback cursor");
        self.send(
            Command::PreviewTrim {
                media: self.media.id,
                trim,
            },
            cx,
        );
    }
    fn prepare(&mut self, cx: &mut Context<Self>) {
        if self.editing.as_ref().is_some_and(|d| d.preparation.busy()) {
            return;
        }
        match self.range(cx) {
            Ok(trim) => {
                if let Some(draft) = &mut self.editing {
                    draft.preparation = Preparation::Preparing;
                }
                self.status = "Préparation de votre découpe…".into();
                self.stop(cx);
                self.send(
                    Command::Prepare {
                        media: self.media.id,
                        trim,
                        normalization: self.normalization,
                    },
                    cx,
                );
                cx.notify();
            }
            Err(_) => {
                self.status = "Choisissez un début et une fin valides avant d’appliquer.".into();
                cx.notify();
            }
        }
    }
    fn open_editor(&mut self, kind: AudioEditor, window: &mut Window, cx: &mut Context<Self>) {
        self.stop(cx);
        self.editor_window = Some(window.window_handle());
        self.status.clear();
        match kind {
            AudioEditor::Trim => {
                let initial = self.playback_range();
                self.editing = Some(TrimDraft {
                    before: initial,
                    normalization: self.normalization,
                    preparation: Preparation::Idle,
                });
                self.set_selection(initial, SelectionOrigin::Canonical, window, cx);
            }
            AudioEditor::Metadata => {
                self.title.update(cx, |v, cx| {
                    v.set_value(self.media.title.clone(), window, cx)
                });
                self.author.update(cx, |v, cx| {
                    v.set_value(self.media.author.clone(), window, cx)
                });
                self.tags.update(cx, |v, cx| {
                    v.set_value(self.media.tags.join(", "), window, cx)
                });
            }
        }
        let panel = cx.entity().downgrade();
        let observed = cx.entity();
        let editor = cx.new(|cx| AudioEditorView {
            panel: panel.clone(),
            kind,
            _observation: cx.observe(&observed, |_, _, cx| cx.notify()),
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let close = panel.clone();
            let cancel = panel.clone();
            dialog
                .title(match kind {
                    AudioEditor::Trim => "Découper l’audio",
                    AudioEditor::Metadata => "Modifier les informations",
                })
                .w(px(660.))
                .child(editor.clone())
                .on_cancel(move |_, _, cx| {
                    cancel
                        .update(cx, |v, _| {
                            !v.editing.as_ref().is_some_and(|d| d.preparation.busy())
                        })
                        .unwrap_or(true)
                })
                .on_close(move |_, window, cx| {
                    let _ = close.update(cx, |v, cx| {
                        v.stop(cx);
                        v.editor_window = None;
                        if let Some(draft) = v.editing.take() {
                            match draft.preparation {
                                Preparation::Ready(id) => {
                                    v.prepared = Some(id);
                                }
                                Preparation::Idle
                                | Preparation::Preparing
                                | Preparation::Encoding(_)
                                | Preparation::Failed => {
                                    v.normalization = draft.normalization;
                                    v.set_selection(
                                        draft.before,
                                        SelectionOrigin::Canonical,
                                        window,
                                        cx,
                                    );
                                }
                            }
                        }
                        cx.notify();
                    });
                })
        });
    }
    fn metadata_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child("Titre")
            .child(Input::new(&self.title))
            .child("Auteur")
            .child(Input::new(&self.author))
            .child("Étiquettes")
            .child(Input::new(&self.tags))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        DialogClose::new().child(Button::new("cancel-metadata").label("Annuler")),
                    )
                    .child(
                        Button::new("save-media")
                            .label("Enregistrer")
                            .primary()
                            .on_click(cx.listener(|v, _, _, cx| {
                                let metadata = MediaMetadata {
                                    title: v.title.read(cx).value().to_string(),
                                    author: v.author.read(cx).value().to_string(),
                                    tags: v
                                        .tags
                                        .read(cx)
                                        .value()
                                        .split(',')
                                        .map(str::trim)
                                        .filter(|s| !s.is_empty())
                                        .map(str::to_owned)
                                        .collect(),
                                };
                                v.send(
                                    Command::UpdateMedia {
                                        media: v.media.id,
                                        metadata,
                                    },
                                    cx,
                                );
                            })),
                    ),
            )
            .child(self.status.clone())
            .into_any_element()
    }
    fn trim_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.editing.as_ref().is_some_and(|d| d.preparation.busy());
        let ready = self
            .editing
            .as_ref()
            .is_some_and(|d| matches!(d.preparation, Preparation::Ready(_)));
        let mut wave = div()
            .h(px(110.))
            .flex()
            .items_center()
            .gap_px()
            .overflow_hidden()
            .rounded_md()
            .bg(cx.theme().muted);
        if let Some(w) = &self.waveform {
            if let Some(peaks) = w
                .levels
                .iter()
                .find(|v| v.len() <= 384)
                .or_else(|| w.levels.last())
            {
                for (lo, hi) in peaks {
                    wave = wave.child(
                        div()
                            .flex_1()
                            .min_w(px(1.))
                            .h(px(((hi - lo).abs() * 50.).clamp(1., 100.)))
                            .bg(cx.theme().primary),
                    );
                }
            }
        } else {
            wave = wave.child("Analyse de la forme d’onde…");
        }
        div().flex().flex_col().gap_4().child(div().font_weight(FontWeight::SEMIBOLD).child(self.media.title.clone()))
            .child("Déplacez les poignées pour garder la partie qui vous plaît.")
            .child(wave).child(Slider::new(&self.range).disabled(busy))
            .child(format!("{} → {} · {} conservées", clock(self.selection.start().seconds()), clock(self.selection.end().seconds()), clock(self.selection.duration().seconds())))
            .child(div().flex().gap_3()
                .child(div().flex_1().min_w_0().flex().flex_col().gap_2().child("Début (s)").child(Input::new(&self.start).disabled(busy)))
                .child(div().flex_1().min_w_0().flex().flex_col().gap_2().child("Fin (s)").child(Input::new(&self.end).disabled(busy))))
            .child(Checkbox::new("normalize").label("Égaliser le volume").checked(self.normalization == Normalization::Loudness).disabled(busy).on_click(cx.listener(|v, checked, _, cx| {
                v.normalization = if *checked { Normalization::Loudness } else { Normalization::Preserve }; cx.notify();
            })))
            .child(div().flex().gap_2()
                .child(Button::new("preview-trim").label(if self.preview_pending.is_some() { "Préparation de l’écoute…" } else if self.playing { "■ Arrêter" } else { "▶ Écouter la sélection" }).disabled(busy || self.preview_pending.is_some()).on_click(cx.listener(|v, _, _, cx| {
                    if v.playing { v.stop(cx); } else { v.send(Command::PreviewTrim { media: v.media.id, trim: v.selection }, cx); }
                })))
                .child(div().flex_1())
                .child(DialogClose::new().child(Button::new("cancel-trim").label(if ready { "Fermer" } else { "Annuler" }).disabled(busy)))
                .child(Button::new("apply-handles").label(if busy { "Préparation…" } else { "Appliquer" }).primary().disabled(busy || ready).on_click(cx.listener(|v, _, _, cx| v.prepare(cx)))))
            .child(div().text_sm().text_color(cx.theme().muted_foreground).child(if self.status.is_empty() { "Votre original est conservé. Appliquer crée une version pour vos playlists.".to_owned() } else { self.status.clone() }))
            .into_any_element()
    }
}
struct AudioEditorView {
    panel: WeakEntity<TrimPanel>,
    kind: AudioEditor,
    _observation: Subscription,
}
impl Render for AudioEditorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.panel
            .update(cx, |v, cx| match self.kind {
                AudioEditor::Trim => v.trim_editor(cx),
                AudioEditor::Metadata => v.metadata_editor(cx),
            })
            .unwrap_or_else(|_| {
                div()
                    .child("Ce média n’est plus disponible.")
                    .into_any_element()
            })
    }
}
impl Render for TrimPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.entity().downgrade();
        let metadata = panel.clone();
        let export = panel.clone();
        let remove = panel.clone();
        let range = self.playback_range();
        div()
            .id("audio-player")
            .size_full()
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.media.title.clone()),
                    )
                    .child(Button::new("audio-more").label("⋯").ghost().dropdown_menu(
                        move |menu, _, _| {
                            let metadata = metadata.clone();
                            let export = export.clone();
                            let remove = remove.clone();
                            menu.item(PopupMenuItem::new("Modifier les informations").on_click(
                                move |_, window, cx| {
                                    let _ = metadata.update(cx, |v, cx| {
                                        v.open_editor(AudioEditor::Metadata, window, cx)
                                    });
                                },
                            ))
                            .item(PopupMenuItem::new("Exporter l’original").on_click(
                                move |_, _, cx| {
                                    let _ = export.update(cx, |v, cx| {
                                        let media = v.media.id;
                                        let name = v.media.title.clone();
                                        v.tasks.push(cx.spawn(async move |weak, cx| {
                                            if let Some(file) = rfd::AsyncFileDialog::new()
                                                .set_file_name(name)
                                                .save_file()
                                                .await
                                            {
                                                let destination = file.path().to_owned();
                                                let _ = weak.update(cx, |v, cx| {
                                                    v.send(
                                                        Command::Export { media, destination },
                                                        cx,
                                                    )
                                                });
                                            }
                                        }));
                                    });
                                },
                            ))
                            .separator()
                            .item(
                                PopupMenuItem::new("Supprimer de la médiathèque").on_click(
                                    move |_, window, cx| {
                                        let Some(title) =
                                            remove.upgrade().map(|v| v.read(cx).media.title.clone())
                                        else {
                                            return;
                                        };
                                        let remove = remove.clone();
                                        confirm_destructive(
                                            window,
                                            cx,
                                            format!("Supprimer « {title} » ?"),
                                            "L’audio et ses versions préparées seront effacés de cet ordinateur. Cette action est irréversible. Un média utilisé dans une playlist ne peut pas être supprimé.",
                                            "Supprimer",
                                            move |_, cx| {
                                                let _ = remove.update(cx, |v, cx| {
                                                    v.send(
                                                        Command::DeleteMedia {
                                                            media: v.media.id,
                                                            policy: ReferencePolicy::RejectReferenced,
                                                        },
                                                        cx,
                                                    )
                                                });
                                            },
                                        );
                                    },
                                ),
                            )
                        },
                    )),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.media.author.is_empty() {
                        format!(
                            "{} · {}",
                            clock(range.duration().seconds()),
                            self.media.info.codec
                        )
                    } else {
                        format!(
                            "{} · {}",
                            self.media.author,
                            clock(range.duration().seconds())
                        )
                    }),
            )
            .child(
                div()
                    .p_4()
                    .rounded_lg()
                    .bg(cx.theme().muted)
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        Button::new("play-audio")
                            .label(if self.preview_pending.is_some() {
                                "Préparation de l’écoute…"
                            } else if self.playing {
                                "■ Arrêter"
                            } else {
                                "▶ Écouter"
                            })
                            .disabled(self.preview_pending.is_some())
                            .primary()
                            .on_click(cx.listener(|v, _, _, cx| {
                                if v.playing {
                                    v.stop(cx);
                                } else if let Some(recipe) = v.prepared {
                                    v.send(Command::Preview(recipe), cx);
                                } else {
                                    v.play_from_cursor(cx);
                                }
                            })),
                    )
                    .child(Slider::new(&self.cursor))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_sm()
                            .child(clock(
                                (self.cursor_seconds - range.start().seconds()).max(0.),
                            ))
                            .child(clock(range.duration().seconds())),
                    ),
            )
            .child(
                Button::new("edit-audio")
                    .label("Découper l’audio")
                    .on_click(
                        cx.listener(|v, _, window, cx| {
                            v.open_editor(AudioEditor::Trim, window, cx)
                        }),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.prepared.is_some() {
                        "Version préparée · Original conservé"
                    } else {
                        "Original · Aucune découpe"
                    }),
            )
            .child(self.status.clone())
    }
}
fn clock(seconds: f64) -> String {
    let millis = (seconds.max(0.) * 1000.).round() as u64;
    format!(
        "{}:{:02}.{:03}",
        millis / 60_000,
        (millis / 1000) % 60,
        millis % 1000
    )
}
fn bounded_slider_range(
    start: f32,
    end: f32,
    duration: MediaTime,
) -> yoto_domain::Result<TrimRange> {
    TrimRange::new(
        MediaTime::from_seconds((start as f64).max(0.))?,
        MediaTime::from_seconds((end as f64).min(duration.seconds()))?,
        duration,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn float_slider_end_does_not_exceed_probed_duration() {
        let duration = MediaTime::from_seconds(131.181).unwrap();
        let trim = bounded_slider_range(4.5, duration.seconds() as f32, duration).unwrap();
        assert_eq!(trim.end(), duration);
        assert_eq!(trim.start(), MediaTime::from_seconds(4.5).unwrap());
        assert!(bounded_slider_range(5., 5., duration).is_err());
    }
    #[core::prelude::v1::test]
    fn player_displays_duration_without_losing_subseconds() {
        assert_eq!(clock(131.181), "2:11.181");
        assert_eq!(clock(59.9996), "1:00.000");
        assert_eq!(clock(0.), "0:00.000");
    }
}
