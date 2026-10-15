use super::*;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::{
    WindowExt,
    slider::{Slider, SliderState},
    spinner::Spinner,
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
    Chapters,
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
/// What a press on the trim waveform holds until it is released.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaveGrab {
    Start,
    End,
    Playhead,
}
/// The waveform dragged with the pointer; it draws nothing of its own.
#[derive(Clone)]
pub(super) struct WaveDrag;
impl Render for WaveDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}
/// How far from a selection handle drawn on the waveform a press still grabs it, in pixels.
pub(super) const HANDLE_REACH: f32 = 12.;
/// How far the arrow keys move the playhead, in seconds.
const NUDGE_SECONDS: f64 = 5.;
/// Pixels of waveform per bar, gap included: fine enough for a pause between two songs
/// to show as a dip on a file of a quarter of an hour.
const WAVE_BAR_PITCH: f32 = 3.;
const WAVE_BAR_GAP: f32 = 1.;
/// The height of the loudest bar, in pixels, and of a bar over silence.
const WAVE_HEIGHT: f32 = 118.;
const WAVE_FLOOR: f32 = 2.;
/// How many bars a waveform gets before it has been painted and measured.
const WAVE_COLUMNS_UNMEASURED: usize = 260;

/// What the player's report means for a panel's playhead (see [`playhead_after`]).
#[derive(Debug, PartialEq)]
pub(super) struct Playhead {
    pub(super) playing: bool,
    pub(super) paused: bool,
    /// Where to put the playhead, when the report moves it.
    pub(super) cursor: Option<f64>,
}
struct TrimDraft {
    before: TrimRange,
    normalization: Normalization,
    preparation: Preparation,
}

/// Retained player state. The modal observes this entity instead of duplicating its recipe or inputs.
pub struct TrimPanel {
    pub(super) media: MediaItem,
    parent: WeakEntity<AppView>,
    service: Arc<Service>,
    runtime: Arc<tokio::runtime::Runtime>,
    title: Entity<InputState>,
    author: Entity<InputState>,
    tags: Entity<InputState>,
    start: Entity<InputState>,
    end: Entity<InputState>,
    range: Entity<SliderState>,
    pub(super) cursor: Entity<SliderState>,
    pub(super) cursor_seconds: f64,
    /// What the pointer holds on the trim waveform, while pressed.
    pub(super) wave_grab: Option<WaveGrab>,
    /// The trim waveform's drawing area as last painted, to read a pointer as a time.
    pub(super) wave_bounds: Bounds<Pixels>,
    pub(super) wave_focus: FocusHandle,
    pub(super) playing: bool,
    /// The sound is held where the playhead stands, to go on from there.
    pub(super) paused: bool,
    pub(super) preview_pending: Option<u64>,
    generation: u64,
    normalization: Normalization,
    pub waveform: Option<Waveform>,
    prepared: Option<RecipeId>,
    /// The finished cut a playlist would receive, when there is one.
    pub(super) selected_version: Option<TrimRange>,
    /// The chapter marks being edited, while their dialog is open.
    pub(super) chapters: Option<chapters::ChapterDraft>,
    editing: Option<TrimDraft>,
    pub(super) editor_window: Option<AnyWindowHandle>,
    /// The icon picker, while it is open.
    icon_window: Option<AnyWindowHandle>,
    /// A new icon for the media is being saved.
    saving_icon: bool,
    /// Why the icon chosen in the open picker was not saved, shown inside the picker.
    icon_error: Option<String>,
    pub(super) tasks: Vec<Task<()>>,
    pub(super) status: String,
    more_open: bool,
    /// Only the open media follows the player; the others stay quiet.
    active: bool,
    selection: TrimRange,
    _subscriptions: Vec<Subscription>,
}
impl TrimPanel {
    pub fn new(
        media: MediaItem,
        parent: WeakEntity<AppView>,
        service: Arc<Service>,
        runtime: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = cx.new(|cx| InputState::new(window, cx).default_value(media.title.clone()));
        let author = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!("Auteur"))
                .default_value(media.author.clone())
        });
        let tags = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr!("Étiquettes séparées par des virgules"))
                .default_value(media.tags.join(", "))
        });
        let start = cx.new(|cx| InputState::new(window, cx).default_value(trim_text(0.)));
        let end = cx.new(|cx| {
            InputState::new(window, cx).default_value(trim_text(media.info.duration.seconds()))
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
                    v.status = tr!("La fin doit se trouver après le début.").into();
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
                                    tr!("Choisissez un début et une fin valides, en minutes et secondes (par exemple 1:05).").into();
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
                    // Releasing the cursor only moves it; playback continues from there
                    // when it was already playing, and « Écouter » starts it otherwise.
                    let range = v.playback_range();
                    v.seek(
                        range.start().seconds() + value.start() as f64 * range.duration().seconds(),
                        cx,
                    );
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
            wave_grab: None,
            wave_bounds: Bounds::default(),
            wave_focus: cx.focus_handle(),
            playing: false,
            paused: false,
            preview_pending: None,
            generation: 0,
            selection,
            _subscriptions: subscriptions,
            media,
            parent,
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
            chapters: None,
            editing: None,
            editor_window: None,
            icon_window: None,
            saving_icon: false,
            icon_error: None,
            tasks: vec![],
            status: String::new(),
            more_open: false,
            active: true,
        };
        let service = view.service.clone();
        let rt = view.runtime.clone();
        let media = view.media.id;
        view.tasks.push(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let generation = match weak.read_with(cx, |v, _| v.active.then_some(v.generation)) {
                    Ok(Some(generation)) => generation,
                    Ok(None) => continue,
                    Err(_) => break,
                };
                let service = service.clone();
                let result = rt
                    .spawn(async move { service.execute(Command::PreviewStatus).await })
                    .await;
                if let Ok(Ok(Reply::Data(data))) = result {
                    if let Ok(status) = serde_json::from_value::<PreviewStatus>(data) {
                        let update = weak.update(cx, |v, cx| {
                            // A report asked for before the player was told to stop, pause
                            // or move describes a sound this panel no longer follows.
                            if v.generation != generation {
                                return;
                            }
                            // The playhead held by the pointer, or moved while the next
                            // playback starts, stays where it was put.
                            let held = v.wave_grab.is_some() || v.preview_pending.is_some();
                            let range = v.playback_range();
                            let playhead = playhead_after(
                                &status,
                                media,
                                v.playing,
                                held,
                                range.start().seconds(),
                            );
                            v.playing = playhead.playing;
                            v.paused = playhead.paused;
                            if let Some(seconds) = playhead.cursor {
                                v.cursor_seconds = seconds;
                                let value = ((seconds - range.start().seconds())
                                    / range.duration().seconds())
                                .clamp(0., 1.) as f32;
                                let input = v.cursor.clone();
                                if let Some(window) = cx.windows().first().copied() {
                                    let _ = cx.update_window(window, move |_, window, cx| {
                                        input.update(cx, |s, cx| s.set_value(value, window, cx))
                                    });
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
    /// The selection typed in the two fields. A field still showing the current bound keeps
    /// its exact value, so displaying whole seconds never moves a handle by itself.
    fn range(&self, cx: &App) -> yoto_domain::Result<TrimRange> {
        let read = |input: &Entity<InputState>, current: MediaTime, invalid: &str| {
            let text = input.read(cx).value().trim().to_owned();
            if text == trim_text(current.seconds()) {
                Ok(current)
            } else {
                parse_time(&text)
                    .ok_or_else(|| Failure::Validation(invalid.to_owned()))
                    .and_then(MediaTime::from_seconds)
            }
        };
        TrimRange::new(
            read(&self.start, self.selection.start(), tr!("Début invalide"))?,
            read(&self.end, self.selection.end(), tr!("Fin invalide"))?,
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
        if self.editing.is_some()
            && !(trim.start().seconds()..=trim.end().seconds()).contains(&self.cursor_seconds)
        {
            self.cursor_seconds = trim.start().seconds();
        }
        match origin {
            SelectionOrigin::Handles | SelectionOrigin::Canonical => {
                self.start.update(cx, |v, cx| {
                    v.set_value(trim_text(trim.start().seconds()), window, cx)
                });
                self.end.update(cx, |v, cx| {
                    v.set_value(trim_text(trim.end().seconds()), window, cx)
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
                self.status =
                    tr!("Découpe enregistrée. Vous pouvez l’ajouter à une playlist.").into();
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
                        self.status = tr!("Préparation annulée.").into();
                    }
                    JobState::Queued
                    | JobState::Running { .. }
                    | JobState::Retry { .. }
                    | JobState::WaitingForAuthentication
                    | JobState::Conflict { .. }
                    | JobState::Reconciling
                    | JobState::Completed { .. } => {
                        self.status = tr!(
                            "Préparation : {state}…",
                            state = wording::labels::job_state(&job.state)
                        )
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
    pub(super) fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        let rt = self.runtime.clone();
        let service = self.service.clone();
        let generation = self.generation;
        let preview = matches!(&command, Command::Preview(_) | Command::PreviewTrim { .. });
        if preview {
            self.preview_pending = Some(generation);
            cx.notify();
        }
        let chapters = matches!(&command, Command::SetMediaChapters { .. });
        let warmup = matches!(&command, Command::PreparePreview(_));
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
                if warmup {
                    // Decoding ahead of the first listen is a convenience: when it fails,
                    // the first listen decodes and reports on its own.
                    match &r {
                        Ok(Err(e)) => eprintln!("Preview warm-up failed: {e:?}"),
                        Err(e) => eprintln!("Preview warm-up failed: {e}"),
                        Ok(Ok(_)) => {}
                    }
                    return;
                }
                let chapters_saved = chapters.then_some(matches!(&r, Ok(Ok(_))));
                match r {
                    Ok(Ok(Reply::Prepared(id))) => {
                        if let Some(draft) = &mut view.editing {
                            draft.preparation = Preparation::Encoding(id);
                        }
                        view.status = tr!("Préparation de votre découpe…").into();
                    }
                    Ok(Ok(
                        Reply::AppUpdate(_)
                        | Reply::YoutubeTool(_)
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
                    | Ok(Ok(
                        Reply::PixelArt(_) | Reply::Picture(_) | Reply::Image(_) | Reply::Images(_),
                    )) => {
                        if let Some(metadata) = metadata {
                            view.media.title = metadata.title;
                            view.media.author = metadata.author;
                            view.media.tags = metadata.tags;
                            view.status = tr!("Informations enregistrées.").into();
                            // Saving is the end of this dialog: close it, as « Appliquer » does.
                            if view.editing.is_none()
                                && let Some(handle) = view.editor_window.take()
                            {
                                let _ = cx
                                    .update_window(handle, |_, window, cx| window.close_dialog(cx));
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        if preview {
                            eprintln!("Media preview failed: {e:?}");
                        }
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
                if let Some(saved) = chapters_saved {
                    view.chapters_saved(saved, cx);
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        self.playing = false;
        self.paused = false;
        self.preview_pending = None;
        self.send(Command::StopPreview, cx);
    }
    /// Hold the sound where it stands: « Écouter » resumes from the playhead, which can be
    /// moved meanwhile.
    pub(super) fn pause(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        self.playing = false;
        self.paused = true;
        self.preview_pending = None;
        self.send(Command::PausePreview, cx);
    }
    /// Called when the open media changes: a panel that is not shown stops asking the
    /// player for its position, and forgets a playback it no longer follows.
    /// Input placeholders, again in the interface language after it changed.
    pub fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.author
            .update(cx, |s, cx| s.set_placeholder(tr!("Auteur"), window, cx));
        self.tags.update(cx, |s, cx| {
            s.set_placeholder(tr!("Étiquettes séparées par des virgules"), window, cx)
        });
    }
    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active != active {
            self.active = active;
            if !active {
                self.playing = false;
                self.paused = false;
                self.preview_pending = None;
            }
            cx.notify();
        }
    }
    pub fn is_playing(&self) -> bool {
        self.playing
    }
    /// Play the chosen version (or the original) from the cursor, or stop.
    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.preview_pending.is_some() {
            return;
        }
        let from_start = self.cursor_seconds <= self.playback_range().start().seconds() + 0.5;
        if self.playing {
            self.stop(cx);
        } else if let Some(recipe) = self.prepared
            && from_start
        {
            self.send(Command::Preview(recipe), cx);
        } else {
            self.play_from_cursor(cx);
        }
    }
    pub fn chosen_recipe(&self) -> Option<RecipeId> {
        self.prepared
    }
    pub(super) fn playback_range(&self) -> TrimRange {
        if self.chapters.is_some() {
            // Marking chapters is done on the original, heard in full.
            self.whole_original()
        } else if self.editing.is_some() {
            self.selection
        } else {
            self.selected_version
                .unwrap_or_else(|| self.whole_original())
        }
    }
    pub(super) fn whole_original(&self) -> TrimRange {
        TrimRange::new(
            MediaTime::ZERO,
            self.media.info.duration,
            self.media.info.duration,
        )
        .expect("probed duration")
    }
    /// What a playlist receives of this media: the latest finished cut, else the original.
    pub(super) fn playlist_range(&self) -> TrimRange {
        self.selected_version
            .unwrap_or_else(|| self.whole_original())
    }
    pub(super) fn play_from_cursor(&mut self, cx: &mut Context<Self>) {
        let range = self.playback_range();
        let trim = range
            .played_from(MediaTime::from_seconds(self.cursor_seconds).unwrap_or(range.start()));
        self.cursor_seconds = trim.start().seconds();
        // Reports of the sound being replaced, paused or not, are not for this playback.
        self.generation = self.generation.wrapping_add(1);
        self.paused = false;
        self.send(
            Command::PreviewTrim {
                media: self.media.id,
                trim,
            },
            cx,
        );
    }
    /// Move the playhead to `seconds`, kept inside what is being listened to. Playback that
    /// is running (or about to) continues from there; otherwise the next listen starts there.
    pub(super) fn seek(&mut self, seconds: f64, cx: &mut Context<Self>) {
        let range = self.playback_range();
        self.cursor_seconds = seconds.clamp(range.start().seconds(), range.end().seconds());
        // Reports asked for before the move still describe the old position.
        self.generation = self.generation.wrapping_add(1);
        if self.playing || self.preview_pending.is_some() {
            self.play_from_cursor(cx);
        } else if self.paused {
            // The pause no longer holds a place: the next listen starts at the playhead.
            self.paused = false;
            self.send(Command::StopPreview, cx);
        }
        cx.notify();
    }
    /// The pointer pressed the trim waveform: a selection handle within reach is grabbed,
    /// anywhere else moves the playhead.
    fn wave_press(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.as_ref().is_some_and(|d| d.preparation.busy()) {
            return;
        }
        window.focus(&self.wave_focus, cx);
        let (x, width) = self.wave_pointer(position);
        let duration = self.media.info.duration.seconds().max(0.001);
        self.wave_grab = Some(wave_grab(
            x,
            width,
            (self.selection.start().seconds() / duration) as f32,
            (self.selection.end().seconds() / duration) as f32,
        ));
        self.wave_move(position, window, cx);
    }
    pub(super) fn wave_move(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(grab) = self.wave_grab else {
            return;
        };
        let (x, width) = self.wave_pointer(position);
        let duration = self.media.info.duration;
        let Ok(time) = MediaTime::from_seconds(wave_time(x, width, duration)) else {
            return;
        };
        let moved = match grab {
            WaveGrab::Playhead => {
                let range = self.playback_range();
                self.cursor_seconds = time
                    .seconds()
                    .clamp(range.start().seconds(), range.end().seconds());
                cx.notify();
                return;
            }
            WaveGrab::Start => TrimRange::new(time, self.selection.end(), duration),
            WaveGrab::End => TrimRange::new(self.selection.start(), time, duration),
        };
        // A handle pushed onto the other one stays where it last was.
        if let Ok(trim) = moved {
            self.set_selection(trim, SelectionOrigin::Canonical, window, cx);
        }
    }
    pub(super) fn wave_release(&mut self, cx: &mut Context<Self>) {
        if self.wave_grab.take() == Some(WaveGrab::Playhead) {
            self.seek(self.cursor_seconds, cx);
        }
    }
    /// The arrow keys move the playhead a few seconds while the waveform has focus.
    pub(super) fn wave_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let step = match event.keystroke.key.as_str() {
            "left" => -NUDGE_SECONDS,
            "right" => NUDGE_SECONDS,
            _ => return,
        };
        cx.stop_propagation();
        self.seek(self.cursor_seconds + step, cx);
    }
    /// The pointer's distance from the waveform's left edge, and the waveform's width.
    pub(super) fn wave_pointer(&self, position: Point<Pixels>) -> (f32, f32) {
        (
            (position.x - self.wave_bounds.left()).as_f32(),
            self.wave_bounds.size.width.as_f32(),
        )
    }
    /// The waveform was laid out at `bounds`: a new width is painted again with the number
    /// of bars that fits it.
    pub(super) fn wave_painted(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        if self.wave_bounds != bounds {
            let repaint = self.wave_bounds.size.width != bounds.size.width;
            self.wave_bounds = bounds;
            if repaint {
                cx.notify();
            }
        }
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
                self.status = tr!("Préparation de votre découpe…").into();
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
                self.status =
                    tr!("Choisissez un début et une fin valides avant d’appliquer.").into();
                cx.notify();
            }
        }
    }
    fn open_editor(&mut self, kind: AudioEditor, window: &mut Window, cx: &mut Context<Self>) {
        self.stop(cx);
        self.editor_window = Some(window.window_handle());
        self.status.clear();
        // Both listening dialogs play the original: decoding it now makes the first
        // « Écouter » start at once.
        if matches!(kind, AudioEditor::Trim | AudioEditor::Chapters) {
            self.send(Command::PreparePreview(self.media.id), cx);
        }
        match kind {
            AudioEditor::Trim => {
                let initial = self.playback_range();
                self.editing = Some(TrimDraft {
                    before: initial,
                    normalization: self.normalization,
                    preparation: Preparation::Idle,
                });
                self.set_selection(initial, SelectionOrigin::Canonical, window, cx);
                self.cursor_seconds = initial.start().seconds();
            }
            AudioEditor::Chapters => self.begin_chapters(window, cx),
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
            let dialog = match kind {
                AudioEditor::Trim | AudioEditor::Chapters => dialog.close_button(false).w(px(820.)),
                AudioEditor::Metadata => dialog.title(tr!("Modifier les informations")).w(px(560.)),
            };
            dialog
                .child(editor.clone())
                .on_cancel(move |_, _, cx| {
                    cancel
                        .update(cx, |v, _| {
                            !v.editing.as_ref().is_some_and(|d| d.preparation.busy())
                                && !v.chapters.as_ref().is_some_and(|d| d.saving())
                        })
                        .unwrap_or(true)
                })
                .on_close(move |_, window, cx| {
                    let _ = close.update(cx, |v, cx| {
                        // Only the trim and chapters dialogs play their own audio.
                        if matches!(kind, AudioEditor::Trim | AudioEditor::Chapters) {
                            v.stop(cx);
                        }
                        v.editor_window = None;
                        if v.chapters.take().is_some() {
                            v.end_chapters(window, cx);
                        }
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
                            // The player outside starts again from the beginning.
                            v.wave_grab = None;
                            v.cursor_seconds = v.playback_range().start().seconds();
                            v.cursor.update(cx, |s, cx| s.set_value(0., window, cx));
                        }
                        cx.notify();
                    });
                })
        });
    }
    fn metadata_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use theme::{Height, Look};
        let field = |label: &'static str, input: &Entity<InputState>| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::BOLD)
                        .child(label),
                )
                .child(theme::field(Input::new(input)))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(field(tr!("Titre"), &self.title))
            .child(field(tr!("Auteur"), &self.author))
            .child(field(tr!("Étiquettes"), &self.tags))
            .children((!self.status.is_empty()).then(|| theme::note(self.status.clone())))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .justify_end()
                    .child(theme::dialog_close(theme::pill(
                        Button::new("cancel-metadata").label(tr!("Annuler")),
                        Look::Outline,
                        Height::Medium,
                        false,
                        cx,
                    )))
                    .child(theme::pill(
                        Button::new("save-media")
                            .label(tr!("Enregistrer"))
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
                        Look::Primary,
                        Height::Medium,
                        false,
                        cx,
                    )),
            )
            .into_any_element()
    }
    /// The waveform of the whole original as bars, one per few pixels of the width it was
    /// last painted at, each coloured by `color_at` from its position (a fraction of the
    /// media); a spinner while it is still being analysed.
    pub(super) fn wave_bars(&self, color_at: impl Fn(f32) -> u32) -> Div {
        let mut bars = div()
            .size_full()
            .flex()
            .items_center()
            .gap(px(WAVE_BAR_GAP));
        if let Some(w) = &self.waveform {
            let finest = w.levels.first().map_or(&[][..], Vec::as_slice);
            let heights = wave_columns(
                finest,
                wave_column_count(self.wave_bounds.size.width.as_f32()),
            );
            let count = heights.len().max(1) as f32;
            for (index, height) in heights.iter().enumerate() {
                let position = (index as f32 + 0.5) / count;
                bars = bars.child(
                    div()
                        .flex_1()
                        .min_w(px(1.))
                        .rounded(px(1.))
                        .h(px((height * WAVE_HEIGHT).max(WAVE_FLOOR)))
                        .bg(theme::c(color_at(position))),
                );
            }
        } else {
            bars = bars.justify_center().child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::c(theme::MUTED))
                    .child(Spinner::new().color(theme::c(theme::SKY.accent)))
                    .child(tr!("Analyse de la forme d’onde…")),
            );
        }
        bars
    }
    fn trim_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use gpui_kit::component::switch::Switch;
        use theme::{Height, Look};
        let busy = self.editing.as_ref().is_some_and(|d| d.preparation.busy());
        let ready = self
            .editing
            .as_ref()
            .is_some_and(|d| matches!(d.preparation, Preparation::Ready(_)));
        let duration = self.media.info.duration.seconds().max(0.001);
        let selection = self.selection;
        let start = (selection.start().seconds() / duration) as f32;
        let end = (selection.end().seconds() / duration) as f32;
        let cursor =
            Some((self.cursor_seconds / duration) as f32).filter(|c| (start..=end).contains(c));
        let bars = self.wave_bars(|position| {
            if position < start || position > end {
                theme::WAVE_IDLE
            } else if cursor.is_some_and(|c| position <= c) {
                theme::SKY.accent
            } else {
                theme::WAVE_SELECTED
            }
        });
        let handle = |left: f32| wave_handle(left, theme::SKY.accent);
        let panel = cx.entity();
        let mut overlay = div()
            .id("trim-wave")
            .track_focus(&self.wave_focus)
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(14.))
            .right(px(14.))
            .cursor(if busy {
                CursorStyle::Arrow
            } else {
                CursorStyle::PointingHand
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|v, event: &MouseDownEvent, window, cx| {
                    v.wave_press(event.position, window, cx)
                }),
            )
            .on_drag(WaveDrag, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_drag_move(
                cx.listener(|v, event: &DragMoveEvent<WaveDrag>, window, cx| {
                    v.wave_move(event.event.position, window, cx)
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|v, _: &MouseUpEvent, _, cx| v.wave_release(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|v, _: &MouseUpEvent, _, cx| v.wave_release(cx)),
            )
            .on_key_down(cx.listener(|v, event: &KeyDownEvent, _, cx| v.wave_key(event, cx)))
            .on_prepaint(move |bounds, _, cx| {
                panel.update(cx, |v, cx| v.wave_painted(bounds, cx));
            })
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(start))
                    .w(relative((end - start).max(0.)))
                    .border_t_3()
                    .border_b_3()
                    .border_color(theme::c(theme::SKY.accent))
                    .bg(theme::c(theme::SKY.accent).opacity(0.06)),
            )
            .child(handle(start))
            .child(handle(end));
        if let Some(cursor) = cursor {
            overlay = overlay.child(
                div()
                    .absolute()
                    .top(px(10.))
                    .bottom(px(10.))
                    .left(relative(cursor))
                    .w(px(3.))
                    .rounded(px(3.))
                    .bg(theme::c(theme::PEACH.accent)),
            );
        }
        let wave = div()
            .relative()
            .h(px(150.))
            .rounded(px(20.))
            .bg(theme::c(theme::CREAM))
            .px(px(14.))
            .overflow_hidden()
            .child(bars)
            .children(self.waveform.is_some().then_some(overlay));
        let label = |text: &'static str| {
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::EXTRA_BOLD)
                .text_color(theme::c(theme::MUTED))
                .child(text)
        };
        let time_field = |input: &Entity<InputState>| {
            Input::new(input)
                .disabled(busy)
                .w(px(104.))
                .h(px(44.))
                .rounded(px(12.))
                .border_2()
                .border_color(theme::c(theme::BORDER))
                .bg(theme::c(theme::SURFACE))
                .text_size(px(17.))
                .font_weight(FontWeight::EXTRA_BOLD)
        };
        let close = theme::dialog_close(theme::round(
            Button::new("close-trim")
                .icon(IconName::X)
                .accessibility_label(tr!("Fermer")),
            Look::Soft(&theme::LIGHT_SAND),
            40.,
            busy,
            cx,
        ));
        div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(14.))
                    .child(theme::icon_tile(IconName::Scissors, &theme::SKY, 52., 16.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(theme::heading(
                                tr!("Découper « {title} »", title = self.media.title),
                                26.,
                            ))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .text_color(theme::c(theme::MUTED))
                                    .child(div().child(tr!("Déplacez les poignées pour garder la partie qui vous plaît.")))
                                    .child(div().child(tr!("Cliquez sur la forme d’onde pour écouter à partir de cet endroit."))),
                            ),
                    )
                    .child(close),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(wave)
                    .child(
                        div().px(px(14.)).child(
                            Slider::new(&self.range)
                                .bg(theme::c(theme::SKY.accent))
                                .disabled(busy),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(label(tr!("Début")))
                            .child(time_field(&self.start)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(label(tr!("Fin")))
                            .child(time_field(&self.end)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(label(tr!("Durée gardée")))
                            .child(
                                theme::heading(duration_text(selection.duration().seconds()), 20.)
                                    .h(px(44.))
                                    .flex()
                                    .items_center()
                                    .text_color(theme::c(theme::SKY.ink)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_end()
                    .gap(px(10.))
                    .child(theme::pill(
                        Button::new("preview-trim")
                            .icon(if self.playing {
                                IconName::Pause
                            } else {
                                IconName::Play
                            })
                            .label(if self.preview_pending.is_some() {
                                tr!("Préparation de l’écoute…")
                            } else if self.playing {
                                tr!("Pause")
                            } else {
                                tr!("Écouter la sélection")
                            })
                            .on_click(cx.listener(|v, _, _, cx| {
                                if v.playing {
                                    v.pause(cx);
                                } else {
                                    // From the playhead, which a click on the waveform moves.
                                    v.play_from_cursor(cx);
                                }
                            })),
                        Look::Solid(&theme::MINT),
                        Height::Hero,
                        busy || self.preview_pending.is_some(),
                        cx,
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .py(px(14.))
                    .px(px(16.))
                    .rounded(px(18.))
                    .bg(theme::c(theme::CREAM))
                    .child(
                        div()
                            .flex_1()
                            .child(
                                div()
                                    .font_weight(FontWeight::EXTRA_BOLD)
                                    .child(tr!("Égaliser le volume")),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::c(theme::MUTED))
                                    .child(tr!("Utile quand une chanson est beaucoup plus forte que les autres.")),
                            ),
                    )
                    .child(
                        Switch::new("normalize")
                            .accessibility_label(tr!("Égaliser le volume"))
                            .checked(self.normalization == Normalization::Loudness)
                            .disabled(busy)
                            .on_click(cx.listener(|v, checked, _, cx| {
                                v.normalization = if *checked {
                                    Normalization::Loudness
                                } else {
                                    Normalization::Preserve
                                };
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(10.))
                    .child(if busy {
                        Spinner::new()
                            .color(theme::c(theme::SKY.accent))
                            .into_any_element()
                    } else {
                        theme::icon_tile(IconName::ShieldCheck, &theme::MINT, 28., 14.)
                            .into_any_element()
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(200.))
                            .text_size(px(14.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::c(theme::INK_SOFT))
                            .child(if self.status.is_empty() {
                                tr!("Votre original est conservé. Appliquer crée une version pour vos playlists.").to_owned()
                            } else {
                                self.status.clone()
                            }),
                    )
                    .child(theme::dialog_close(theme::pill(
                        Button::new("cancel-trim").label(if ready { tr!("Fermer") } else { tr!("Annuler") }),
                        Look::Outline,
                        Height::Large,
                        busy,
                        cx,
                    )))
                    .child(theme::pill(
                        Button::new("apply-handles")
                            .label(if busy { tr!("Préparation…") } else { tr!("Appliquer") })
                            .loading(busy)
                            .on_click(cx.listener(|v, _, _, cx| v.prepare(cx))),
                        Look::Primary,
                        Height::Large,
                        busy || ready,
                        cx,
                    )),
            )
            .into_any_element()
    }
}
/// A grab handle drawn over the waveform at `left` (a fraction of its width), in `color`.
pub(super) fn wave_handle(left: f32, color: u32) -> Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(relative(left))
        .ml(px(-9.))
        .w(px(18.))
        .rounded(px(10.))
        .bg(theme::c(color))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(4.))
                .h(px(26.))
                .rounded(px(4.))
                .bg(theme::c(theme::SURFACE)),
        )
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
                AudioEditor::Chapters => v.chapters_editor(cx),
                AudioEditor::Metadata => v.metadata_editor(cx),
            })
            .unwrap_or_else(|_| {
                div()
                    .child(tr!("Ce média n’est plus disponible."))
                    .into_any_element()
            })
    }
}
impl TrimPanel {
    pub(super) fn saving_icon(&self) -> bool {
        self.saving_icon
    }
    pub(super) fn status(&self) -> &str {
        &self.status
    }
    pub(super) fn icon_error(&self) -> Option<&str> {
        self.icon_error.as_deref()
    }
    pub(super) fn backend(&self) -> (Arc<Service>, Arc<tokio::runtime::Runtime>) {
        (self.service.clone(), self.runtime.clone())
    }
    /// Open the playlist editor's icon picker for this media, with the Yoto icons of the
    /// connected account when there is one.
    fn open_icon_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving_icon {
            return;
        }
        let Some(parent) = self.parent.upgrade() else {
            return;
        };
        let account = parent.read(cx).snapshot.active_account.clone();
        let panel = cx.entity();
        let picker = cx.new(|cx| {
            pixelart::IconPicker::new(
                pixelart::IconTarget::Media(panel.downgrade()),
                parent,
                account.clone(),
                cx,
            )
        });
        self.icon_window = Some(window.window_handle());
        self.icon_error = None;
        self.status.clear();
        let weak = panel.downgrade();
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
                        v.icon_window = None;
                        v.icon_error = None;
                        cx.notify();
                    });
                })
        });
        if let Some(account) = account {
            self.send(Command::RefreshIcons(account), cx);
        }
    }
    /// Keep the icon chosen in the picker; an image nobody keeps is released.
    pub(super) fn choose_icon(&mut self, icon: IconReference, cx: &mut Context<Self>) {
        if self.icon_window.is_none() || self.saving_icon {
            if let IconReference::Local { asset } = icon {
                self.send(Command::ReleaseAsset(asset), cx);
            }
            return;
        }
        self.set_icon(Some(icon), cx);
    }
    /// Save the media's icon (`None` removes it). The chapters it is added to from now on
    /// get this icon; playlists that already hold it keep theirs.
    fn set_icon(&mut self, icon: Option<IconReference>, cx: &mut Context<Self>) {
        self.saving_icon = true;
        self.icon_error = None;
        self.status = tr!("Enregistrement de l’icône…").into();
        cx.notify();
        let service = self.service.clone();
        let runtime = self.runtime.clone();
        let media = self.media.id;
        let saved = icon.clone();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let command = Command::SetMediaIcon {
                media,
                icon: saved.clone(),
            };
            let request = service.clone();
            let result = runtime
                .spawn(async move { request.execute(command).await })
                .await;
            let outcome = joined(result);
            if outcome.is_err()
                && let Some(IconReference::Local { asset }) = saved
            {
                runtime.spawn(async move {
                    let _ = service.execute(Command::ReleaseAsset(asset)).await;
                });
            }
            let _ = weak.update(cx, |v, cx| {
                v.saving_icon = false;
                match outcome {
                    Ok(_) => {
                        if let Some(IconReference::Local { asset }) = &icon {
                            let asset = *asset;
                            let _ = v
                                .parent
                                .update(cx, |parent, cx| parent.load_icon_preview(asset, cx));
                        }
                        v.status = if icon.is_some() {
                            tr!("Icône enregistrée : elle accompagnera ce média dans vos prochaines playlists.")
                                .into()
                        } else {
                            tr!("Icône retirée.").into()
                        };
                        v.media.icon = icon;
                        if let Some(handle) = v.icon_window.take() {
                            let _ = cx
                                .update_window(handle, |_, window, cx| window.close_dialog(cx));
                        }
                    }
                    // With the picker open, the parent reads why there, and can try again.
                    Err(error) if v.icon_window.is_some() => {
                        v.status.clear();
                        v.icon_error = Some(failure_text(&error));
                    }
                    Err(error) => v.status = failure_text(&error),
                }
                cx.notify();
            });
        }));
    }
    /// The media's icon, large: what its chapters will show on the player, or an empty slot
    /// inviting the parent to choose one.
    fn icon_slot(&self, cx: &mut Context<Self>) -> AnyElement {
        use gpui_kit::component::button::{ButtonCustomVariant, ButtonRounded};
        let tile =
            match (&self.media.icon, self.parent.upgrade()) {
                (Some(icon), Some(parent)) => {
                    let parent = parent.read(cx);
                    theme::icon_frame(&self.media.id.to_string(), 96., 22.).child(
                        pixelart::icon_image(icon, &parent.icon_previews, parent, 64., cx),
                    )
                }
                (Some(_), None) | (None, _) => theme::empty_icon_slot(96., 22., true),
            };
        let label = if self.media.icon.is_some() {
            tr!("Changer l’icône")
        } else {
            tr!("Choisir une icône")
        };
        let variant = ButtonCustomVariant::new(cx)
            .color(theme::c(theme::SURFACE).opacity(0.))
            .hover(theme::c(theme::CREAM))
            .active(theme::c(theme::SAND));
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .child(
                Button::new("media-icon")
                    .custom(variant)
                    .rounded(ButtonRounded::Size(px(22.)))
                    .size(px(96.))
                    .p(px(0.))
                    .accessibility_label(label)
                    .tooltip(label)
                    .disabled(self.saving_icon)
                    .child(tile)
                    .on_click(cx.listener(|v, _, window, cx| v.open_icon_picker(window, cx))),
            )
            .child(if self.media.icon.is_some() {
                theme::pill(
                    Button::new("remove-media-icon")
                        .label(tr!("Retirer l’icône"))
                        .on_click(cx.listener(|v, _, _, cx| v.set_icon(None, cx))),
                    theme::Look::Link(&theme::ROSE),
                    theme::Height::Small,
                    self.saving_icon,
                    cx,
                )
                .into_any_element()
            } else {
                theme::meta(label).into_any_element()
            })
            .into_any_element()
    }
    fn export_original(&mut self, cx: &mut Context<Self>) {
        let media = self.media.id;
        let name = export_name(&self.media);
        self.tasks.push(cx.spawn(async move |weak, cx| {
            if let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name(name)
                .save_file()
                .await
            {
                let destination = file.path().to_owned();
                let _ = weak.update(cx, |v, cx| {
                    v.send(Command::Export { media, destination }, cx)
                });
            }
        }));
    }
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.media.title.clone();
        let remove = cx.entity().downgrade();
        confirm_destructive(
            window,
            cx,
            tr!("Supprimer « {title} » ?", title = title),
            tr!(
                "L’audio et ses versions préparées seront effacés de cet ordinateur. Cette action est irréversible. Un média utilisé dans une playlist ne peut pas être supprimé."
            ),
            tr!("Supprimer"),
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
    }
}
impl Render for TrimPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use theme::{Height, Look};
        let range = self.playback_range();
        let elapsed = (self.cursor_seconds - range.start().seconds()).max(0.);
        let mut tags = div().flex().flex_wrap().gap(px(6.));
        tags = if self.prepared.is_some() {
            tags.child(theme::tag(tr!("Version découpée"), &theme::SKY))
                .child(theme::tag_colors(
                    tr!("Original conservé"),
                    theme::SAND,
                    theme::BROWN,
                ))
        } else {
            tags.child(theme::tag_colors(
                tr!("Original"),
                theme::SAND,
                theme::BROWN,
            ))
        };
        if self.media.chapters.len() >= 2 {
            tags = tags.child(theme::tag(
                trn!(
                    "{count} chapitre",
                    "{count} chapitres",
                    self.media.chapters.len()
                ),
                &theme::LILAC,
            ));
        }
        let player = theme::card().rounded(px(20.)).p(px(16.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(theme::round(
                    Button::new("play-audio")
                        .icon(if self.playing {
                            IconName::Pause
                        } else {
                            IconName::Play
                        })
                        .accessibility_label(if self.playing {
                            tr!("Arrêter l’écoute")
                        } else {
                            tr!("Écouter")
                        })
                        .loading(self.preview_pending.is_some())
                        .on_click(cx.listener(|v, _, _, cx| v.toggle_play(cx))),
                    Look::Solid(&theme::MINT),
                    48.,
                    false,
                    cx,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(Slider::new(&self.cursor).bg(theme::c(theme::MINT.accent)))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .text_size(px(12.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::c(theme::MUTED))
                                .child(wording::position_text(elapsed))
                                .child(wording::position_text(range.duration().seconds())),
                        ),
                ),
        );
        let more = if self.more_open {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(theme::pill(
                    Button::new("export-original")
                        .label(tr!("Exporter l’original"))
                        .on_click(cx.listener(|v, _, _, cx| v.export_original(cx))),
                    Look::Ghost,
                    Height::Small,
                    false,
                    cx,
                ))
                .child(theme::pill(
                    Button::new("delete-media")
                        .label(tr!("Supprimer ce média local"))
                        .on_click(cx.listener(|v, _, window, cx| v.confirm_delete(window, cx))),
                    Look::Link(&theme::ROSE),
                    Height::Small,
                    false,
                    cx,
                ))
                .into_any_element()
        } else {
            div().into_any_element()
        };
        div()
            .id("audio-player")
            .size_full()
            .overflow_y_scroll()
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .py(px(12.))
                    .flex_shrink_0()
                    .rounded(px(22.))
                    .bg(theme::c(theme::SURFACE))
                    .shadow(theme::step(theme::BORDER, 2.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(self.icon_slot(cx)),
            )
            .child(
                div()
                    .child(theme::heading(self.media.title.clone(), 22.))
                    .child(
                        theme::meta(if self.media.author.trim().is_empty() {
                            format!(
                                "{} · {}",
                                duration_text(range.duration().seconds()),
                                library::source_label(&self.media)
                            )
                        } else {
                            format!(
                                "{} · {} · {}",
                                duration_text(range.duration().seconds()),
                                self.media.author,
                                library::source_label(&self.media)
                            )
                        })
                        .text_size(px(14.)),
                    ),
            )
            .children(
                chapters::playlist_notes(&plan_chapters(
                    self.playlist_range(),
                    &self.media.chapters,
                ))
                .into_iter()
                .map(theme::note),
            )
            .child(tags)
            .child(player)
            .child(theme::pill(
                Button::new("edit-audio")
                    .icon(IconName::Scissors)
                    .label(tr!("Découper l’audio"))
                    .w_full()
                    .on_click(
                        cx.listener(|v, _, window, cx| {
                            v.open_editor(AudioEditor::Trim, window, cx)
                        }),
                    ),
                Look::Soft(&theme::SKY),
                Height::Hero,
                false,
                cx,
            ))
            .child(theme::pill(
                Button::new("edit-chapters")
                    .icon(IconName::BookmarkPlus)
                    .label(tr!("Marquer des chapitres"))
                    .w_full()
                    .on_click(cx.listener(|v, _, window, cx| {
                        v.open_editor(AudioEditor::Chapters, window, cx)
                    })),
                Look::Soft(&theme::LILAC),
                Height::Hero,
                false,
                cx,
            ))
            .child(theme::pill(
                Button::new("edit-metadata")
                    .label(tr!("Modifier les informations"))
                    .w_full()
                    .on_click(cx.listener(|v, _, window, cx| {
                        v.open_editor(AudioEditor::Metadata, window, cx)
                    })),
                Look::Outline,
                Height::Large,
                false,
                cx,
            ))
            .children((!self.status.is_empty()).then(|| theme::note(self.status.clone())))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(theme::pill(
                        Button::new("audio-more")
                            .label(tr!("Plus d’options"))
                            .icon(if self.more_open {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            })
                            .toggled(self.more_open)
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.more_open = !v.more_open;
                                cx.notify();
                            })),
                        Look::Ghost,
                        Height::Small,
                        false,
                        cx,
                    ))
                    .child(more),
            )
    }
}
/// File name offered when exporting the original: its title, with the original file's
/// extension when known (the service adds it otherwise). Path separators are replaced.
fn export_name(media: &MediaItem) -> String {
    let title: String = media
        .title
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let extension = match &media.provenance {
        Provenance::LocalFile { name } => std::path::Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_owned),
        Provenance::Youtube { .. } => None,
    };
    match extension {
        Some(extension) => format!("{title}.{extension}"),
        None => title,
    }
}
/// A trim bound as a parent reads it: « 1:05 », or « 1:05,4 » when it falls between seconds.
fn trim_text(seconds: f64) -> String {
    let tenths = (seconds.max(0.) * 10.).round() as u64;
    let (whole, tenth) = (tenths / 10, tenths % 10);
    let clock = if whole >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            whole / 3600,
            (whole / 60) % 60,
            whole % 60
        )
    } else {
        format!("{}:{:02}", whole / 60, whole % 60)
    };
    if tenth == 0 {
        clock
    } else {
        // TRANSLATORS: a time with tenths of a second, such as 1:05,4; keep the
        // language's decimal separator.
        tr!("{clock},{tenth}", clock = clock, tenth = tenth)
    }
}
/// Parse « 75 », « 1:15 », « 1:15,5 », « 1:15.5 » or « 1:01:15 » into seconds.
fn parse_time(text: &str) -> Option<f64> {
    let text = text.trim().replace(',', ".");
    if text.is_empty() {
        return None;
    }
    let mut seconds = 0.;
    let parts: Vec<_> = text.split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    for (index, part) in parts.iter().enumerate() {
        let value = part.trim().parse::<f64>().ok().filter(|v| *v >= 0.)?;
        if index > 0 && value >= 60. {
            return None;
        }
        seconds = seconds * 60. + value;
    }
    seconds.is_finite().then_some(seconds)
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
/// What a press `x` pixels into a waveform `width` pixels wide grabs, its selection handles
/// standing at `start` and `end` (fractions of the media). A handle within reach wins, the
/// nearer one first; handles drawn on top of each other go by the side pressed.
fn wave_grab(x: f32, width: f32, start: f32, end: f32) -> WaveGrab {
    let (start, end) = (start * width, end * width);
    let (to_start, to_end) = ((x - start).abs(), (x - end).abs());
    if to_start.min(to_end) > HANDLE_REACH {
        WaveGrab::Playhead
    } else if to_end < to_start || (to_end == to_start && x >= end) {
        WaveGrab::End
    } else {
        WaveGrab::Start
    }
}
/// The media time, in seconds, under a pointer `x` pixels into a waveform `width` pixels
/// wide. A pointer dragged past either edge stays at that edge.
pub(super) fn wave_time(x: f32, width: f32, duration: MediaTime) -> f64 {
    if width <= 0. {
        return 0.;
    }
    f64::from((x / width).clamp(0., 1.)) * duration.seconds()
}
/// The playhead after the player reported `status` to the panel of `media`, which was
/// `playing` or not, and whose playhead is `held` by the pointer or by a playback about to
/// start: it follows a sound that plays, stays where a pause left it, returns to `start`
/// once the sound it followed ran to its end, and is left alone otherwise.
pub(super) fn playhead_after(
    status: &PreviewStatus,
    media: MediaId,
    playing: bool,
    held: bool,
    start: f64,
) -> Playhead {
    match status {
        PreviewStatus::Playing {
            media: id,
            position,
        } if *id == media => Playhead {
            playing: true,
            paused: false,
            cursor: (!held).then(|| position.seconds()),
        },
        PreviewStatus::Paused {
            media: id,
            position,
        } if *id == media => Playhead {
            playing: false,
            paused: true,
            cursor: (!held).then(|| position.seconds()),
        },
        PreviewStatus::Playing { .. } | PreviewStatus::Paused { .. } | PreviewStatus::Stopped => {
            Playhead {
                playing: false,
                paused: false,
                cursor: (playing && !held).then_some(start),
            }
        }
    }
}
/// How many bars a waveform `width` pixels wide gets, before it is measured as well.
fn wave_column_count(width: f32) -> usize {
    if width <= 0. {
        return WAVE_COLUMNS_UNMEASURED;
    }
    ((width / WAVE_BAR_PITCH).floor() as usize).max(1)
}
/// The height of each of `columns` bars, from 0 for silence to 1 for the loudest, out of the
/// finest `peaks` of a waveform (the lowest and highest sample of each stretch). A bar
/// averages the swing of the stretches it covers, so a pause inside it still lowers it
/// instead of hiding behind the loudest moment. The bars are then scaled to the loudest
/// one, so a quiet recording still fills the height; a file with nothing louder than
/// [`WAVE_QUIETEST`] is left flat rather than blown up.
pub(super) fn wave_columns(peaks: &[(f32, f32)], columns: usize) -> Vec<f32> {
    if peaks.is_empty() || columns == 0 {
        return vec![];
    }
    let count = peaks.len();
    let mut heights: Vec<f32> = (0..columns)
        .map(|column| {
            let from = column * count / columns;
            let to = ((column + 1) * count / columns).clamp(from + 1, count);
            let swing: f32 = peaks[from..to]
                .iter()
                .map(|(lo, hi)| (hi - lo).max(0.) / 2.)
                .sum();
            swing / (to - from) as f32
        })
        .collect();
    let loudest = heights.iter().copied().fold(WAVE_QUIETEST, f32::max);
    for height in &mut heights {
        *height /= loudest;
    }
    heights
}
/// The swing (half the distance between the lowest and highest sample, averaged over a
/// bar) under which a whole file counts as silent: about −26 dBFS.
const WAVE_QUIETEST: f32 = 0.05;
#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn a_press_on_the_waveform_grabs_a_handle_within_reach_or_moves_the_playhead() {
        // 500 px wide; handles at 100 px and 400 px.
        assert_eq!(wave_grab(250., 500., 0.2, 0.8), WaveGrab::Playhead);
        assert_eq!(wave_grab(20., 500., 0.2, 0.8), WaveGrab::Playhead);
        assert_eq!(wave_grab(110., 500., 0.2, 0.8), WaveGrab::Start);
        assert_eq!(wave_grab(90., 500., 0.2, 0.8), WaveGrab::Start);
        assert_eq!(wave_grab(395., 500., 0.2, 0.8), WaveGrab::End);
        assert_eq!(wave_grab(413., 500., 0.2, 0.8), WaveGrab::Playhead);
        // Close handles: the nearer one, then the side pressed.
        assert_eq!(wave_grab(102., 500., 0.2, 0.21), WaveGrab::Start);
        assert_eq!(wave_grab(104., 500., 0.2, 0.21), WaveGrab::End);
        assert_eq!(wave_grab(95., 500., 0.5, 0.5), WaveGrab::Playhead);
        assert_eq!(wave_grab(245., 500., 0.5, 0.5), WaveGrab::Start);
        assert_eq!(wave_grab(255., 500., 0.5, 0.5), WaveGrab::End);
    }
    #[core::prelude::v1::test]
    fn the_waveform_reads_a_pointer_as_a_time_inside_the_media() {
        let duration = MediaTime::from_seconds(120.).unwrap();
        assert_eq!(wave_time(0., 600., duration), 0.);
        assert_eq!(wave_time(150., 600., duration), 30.);
        assert_eq!(wave_time(600., 600., duration), 120.);
        assert_eq!(wave_time(-40., 600., duration), 0.);
        assert_eq!(wave_time(900., 600., duration), 120.);
        assert_eq!(wave_time(10., 0., duration), 0.);
    }
    #[core::prelude::v1::test]
    fn silence_between_songs_shows_as_low_bars() {
        // Ten stretches: a loud song, two of silence, a quieter song, a last loud moment.
        let loud = (-0.8, 0.8);
        let quiet = (-0.2, 0.2);
        let silent = (-0.001, 0.001);
        let peaks = [
            loud, loud, loud, silent, silent, quiet, quiet, quiet, quiet, loud,
        ];
        let bars = wave_columns(&peaks, 10);
        assert_eq!(bars.len(), 10);
        assert!((bars[0] - 1.).abs() < 1e-6 && (bars[9] - 1.).abs() < 1e-6);
        assert!(bars[3] < 0.01 && bars[4] < 0.01, "{bars:?}");
        assert!((bars[5] - 0.25).abs() < 1e-6, "{bars:?}");
        // Fewer bars than stretches: a bar over a song and a pause dips, and never
        // hides the pause behind the song's loudest moment.
        let five = wave_columns(&peaks, 5);
        assert_eq!(five.len(), 5);
        assert!((five[0] - 1.).abs() < 1e-6, "{five:?}");
        assert!(five[1] > 0.4 && five[1] < 0.6, "{five:?}");
        assert!(five[2] < 0.3, "{five:?}");
        // More bars than stretches: each stretch is repeated, nothing is skipped.
        let twenty = wave_columns(&peaks, 20);
        assert_eq!(twenty.len(), 20);
        assert!(
            twenty[6] < 0.01 && twenty[7] < 0.01 && twenty[8] < 0.01,
            "{twenty:?}"
        );
        // A file of silence stays flat rather than being blown up to full height.
        assert!(wave_columns(&[silent; 4], 4).iter().all(|h| *h < 0.05));
        assert!(wave_columns(&[(0., 0.); 4], 4).iter().all(|h| *h == 0.));
        assert!(wave_columns(&[], 4).is_empty());
        assert!(wave_columns(&peaks, 0).is_empty());
    }
    #[core::prelude::v1::test]
    fn the_number_of_bars_follows_the_painted_width() {
        assert_eq!(wave_column_count(0.), WAVE_COLUMNS_UNMEASURED);
        assert_eq!(wave_column_count(792.), 264);
        assert_eq!(wave_column_count(2.), 1);
    }
    #[core::prelude::v1::test]
    fn the_playhead_follows_pauses_and_returns_to_the_start_when_the_sound_ends() {
        let media = MediaId::new();
        let other = MediaId::new();
        let at = |seconds: f64| MediaTime::from_seconds(seconds).unwrap();
        let playing = PreviewStatus::Playing {
            media,
            position: at(42.5),
        };
        assert_eq!(
            playhead_after(&playing, media, false, false, 10.),
            Playhead {
                playing: true,
                paused: false,
                cursor: Some(42.5)
            }
        );
        // Held by the pointer: the sound is followed, the playhead is not moved.
        assert_eq!(
            playhead_after(&playing, media, true, true, 10.),
            Playhead {
                playing: true,
                paused: false,
                cursor: None
            }
        );
        // A pause keeps its place, so that a chapter can be marked right there.
        let paused = PreviewStatus::Paused {
            media,
            position: at(42.5),
        };
        assert_eq!(
            playhead_after(&paused, media, true, false, 10.),
            Playhead {
                playing: false,
                paused: true,
                cursor: Some(42.5)
            }
        );
        assert_eq!(
            playhead_after(&paused, media, false, true, 10.),
            Playhead {
                playing: false,
                paused: true,
                cursor: None
            }
        );
        // The sound ran to its end: back to the start, ready for the next listen.
        assert_eq!(
            playhead_after(&PreviewStatus::Stopped, media, true, false, 10.),
            Playhead {
                playing: false,
                paused: false,
                cursor: Some(10.)
            }
        );
        // Nothing was playing here: a stop elsewhere leaves the playhead alone.
        assert_eq!(
            playhead_after(&PreviewStatus::Stopped, media, false, false, 10.),
            Playhead {
                playing: false,
                paused: false,
                cursor: None
            }
        );
        // Another media playing is no concern of this panel.
        let elsewhere = PreviewStatus::Playing {
            media: other,
            position: at(3.),
        };
        assert_eq!(
            playhead_after(&elsewhere, media, false, false, 10.),
            Playhead {
                playing: false,
                paused: false,
                cursor: None
            }
        );
    }
    #[core::prelude::v1::test]
    fn float_slider_end_does_not_exceed_probed_duration() {
        let duration = MediaTime::from_seconds(131.181).unwrap();
        let trim = bounded_slider_range(4.5, duration.seconds() as f32, duration).unwrap();
        assert_eq!(trim.end(), duration);
        assert_eq!(trim.start(), MediaTime::from_seconds(4.5).unwrap());
        assert!(bounded_slider_range(5., 5., duration).is_err());
    }
    #[core::prelude::v1::test]
    fn export_keeps_the_original_extension() {
        let mut media = MediaItem {
            id: MediaId::new(),
            title: "Contes / Soir".into(),
            author: String::new(),
            tags: vec![],
            original: AssetId::new(),
            info: AudioInfo {
                duration: MediaTime::from_seconds(1.).unwrap(),
                bytes: 1,
                codec: "mp3".into(),
                sample_rate: 44_100,
                channels: 2,
            },
            provenance: Provenance::LocalFile {
                name: "conte.M4A".into(),
            },
            recipes: vec![],
            icon: None,
            chapters: vec![],
        };
        assert_eq!(export_name(&media), "Contes - Soir.M4A");
        media.provenance = Provenance::LocalFile {
            name: "sans-extension".into(),
        };
        assert_eq!(export_name(&media), "Contes - Soir");
    }
    #[core::prelude::v1::test]
    fn trim_bounds_read_as_minutes_and_seconds() {
        assert_eq!(trim_text(131.181), "2:11,2");
        crate::i18n::tests::with_language(crate::i18n::Language::English, || {
            assert_eq!(trim_text(131.181), "2:11.2");
            assert_eq!(parse_time(&trim_text(131.181)), Some(131.2));
        });
        assert_eq!(trim_text(59.9996), "1:00");
        assert_eq!(trim_text(0.), "0:00");
        assert_eq!(trim_text(3725.), "1:02:05");
        assert!((parse_time("2:11,2").unwrap() - 131.2).abs() < 1e-9);
        assert_eq!(parse_time("1:05"), Some(65.));
        assert!((parse_time("75.5").unwrap() - 75.5).abs() < 1e-9);
        assert_eq!(parse_time("1:02:05"), Some(3725.));
        assert_eq!(parse_time("1:75"), None);
        assert_eq!(parse_time("abc"), None);
        assert_eq!(parse_time(""), None);
    }
}
