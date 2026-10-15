use super::trim::{HANDLE_REACH, TrimPanel, WaveDrag, WaveGrab, wave_handle, wave_time};
use super::*;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::{WindowExt, spinner::Spinner};

/// The least a chapter may last, in seconds: marks closer than this are refused.
const MIN_CHAPTER_SECONDS: f64 = 1.;
/// How far the two small buttons move the playhead, in seconds, to settle on the exact
/// spot before marking it.
const NUDGE_FINE_SECONDS: f64 = 1.;

/// The chapter marks of a media while their dialog is open.
pub(super) struct ChapterDraft {
    /// Where each chapter starts, in seconds of the original; the first is always zero.
    starts: Vec<f64>,
    /// The title typed for each chapter, aligned with `starts`; empty means the default.
    titles: Vec<Entity<InputState>>,
    /// The mark held by the pointer, as an index of `starts`.
    dragging: Option<usize>,
    /// The marks are being saved.
    saving: bool,
    /// What a successful save leaves on the media, shown without waiting for the snapshot.
    pending: Vec<MediaChapter>,
}
impl ChapterDraft {
    pub(super) fn saving(&self) -> bool {
        self.saving
    }
}

/// The notes under a media saying what it becomes in a playlist: how many chapters when it
/// has marks, and that a chapter too long for one Yoto track is cut into parts as well.
pub(super) fn playlist_notes(planned: &[PlannedChapter]) -> Vec<String> {
    let mut notes = vec![];
    if planned.len() >= 2 {
        notes.push(wording::chapters_note(planned.len()));
    }
    let long = planned.iter().filter(|c| c.ranges.len() > 1).count();
    if long > 0 {
        notes.push(if planned.len() >= 2 {
            wording::chapters_split_note(long)
        } else {
            wording::split_note(planned.iter().map(|c| c.ranges.len()).sum())
        });
    }
    notes
}

impl TrimPanel {
    /// Open the marks of this media for editing: its saved chapters, or one chapter holding
    /// the whole audio.
    pub(super) fn begin_chapters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = self.media.chapters.len() >= 2;
        let starts: Vec<f64> = if saved {
            self.media
                .chapters
                .iter()
                .map(|c| c.start.seconds())
                .collect()
        } else {
            vec![0.]
        };
        let titles = (0..starts.len())
            .map(|index| {
                let title = if saved {
                    self.media.chapters[index].title.clone()
                } else {
                    String::new()
                };
                cx.new(|cx| InputState::new(window, cx).default_value(title))
            })
            .collect();
        self.chapters = Some(ChapterDraft {
            starts,
            titles,
            dragging: None,
            saving: false,
            pending: vec![],
        });
        self.cursor_seconds = 0.;
        self.label_chapters(window, cx);
    }
    /// The dialog is gone: the player outside starts again from the beginning.
    pub(super) fn end_chapters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.wave_grab = None;
        self.cursor_seconds = self.playback_range().start().seconds();
        self.cursor.update(cx, |s, cx| s.set_value(0., window, cx));
        cx.notify();
    }
    /// Each title field shows the name its chapter gets when left empty.
    fn label_chapters(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = &self.chapters else {
            return;
        };
        let count = draft.titles.len();
        for (index, input) in draft.titles.iter().enumerate() {
            let title =
                yoto_domain::mark_title_with(wording::mark_title_template(), index + 1, count);
            input.update(cx, |s, cx| s.set_placeholder(title, window, cx));
        }
    }
    /// Mark a chapter start at the playhead.
    fn add_mark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = (self.cursor_seconds * 10.).round() / 10.;
        let duration = self.media.info.duration.seconds();
        let Some(draft) = self.chapters.as_mut().filter(|d| !d.saving) else {
            return;
        };
        if !can_add_mark(&draft.starts, at, duration) {
            self.status = tr!(
                "Placez la lecture à un autre endroit : deux chapitres doivent être séparés d’au moins une seconde."
            )
            .into();
            cx.notify();
            return;
        }
        let index = insert_mark(&mut draft.starts, at);
        draft
            .titles
            .insert(index, cx.new(|cx| InputState::new(window, cx)));
        self.status.clear();
        self.label_chapters(window, cx);
        cx.notify();
    }
    fn remove_mark(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.chapters.as_mut().filter(|d| !d.saving) else {
            return;
        };
        if index == 0 || index >= draft.starts.len() {
            return;
        }
        draft.starts.remove(index);
        draft.titles.remove(index);
        draft.dragging = None;
        self.label_chapters(window, cx);
        cx.notify();
    }
    /// Play from `seconds` right away, whether something is playing, paused or not: one
    /// playback replaces the other in a single step.
    fn listen_from(&mut self, seconds: f64, cx: &mut Context<Self>) {
        let range = self.playback_range();
        self.cursor_seconds = seconds.clamp(range.start().seconds(), range.end().seconds());
        self.play_from_cursor(cx);
    }
    /// The pointer pressed the waveform: a mark within reach is grabbed, anywhere else
    /// moves the playhead.
    fn chapters_press(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.chapters.as_ref().is_none_or(|d| d.saving) {
            return;
        }
        window.focus(&self.wave_focus, cx);
        let (x, width) = self.wave_pointer(position);
        let duration = self.media.info.duration.seconds();
        let held = self
            .chapters
            .as_ref()
            .and_then(|d| grab_mark(x, width, &d.starts, duration));
        if let Some(draft) = &mut self.chapters {
            draft.dragging = held;
        }
        if held.is_none() {
            self.wave_grab = Some(WaveGrab::Playhead);
        }
        self.chapters_move(position, window, cx);
    }
    fn chapters_move(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let duration = self.media.info.duration;
        let Some(index) = self.chapters.as_ref().and_then(|d| d.dragging) else {
            self.wave_move(position, window, cx);
            return;
        };
        let (x, width) = self.wave_pointer(position);
        let at = wave_time(x, width, duration);
        if let Some(draft) = &mut self.chapters {
            draft.starts = moved_mark(&draft.starts, index, at, duration.seconds());
        }
        cx.notify();
    }
    fn chapters_release(&mut self, cx: &mut Context<Self>) {
        match self.chapters.as_mut().and_then(|d| d.dragging.take()) {
            Some(_) => cx.notify(),
            None => self.wave_release(cx),
        }
    }
    fn save_chapters(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.chapters.as_ref().filter(|d| !d.saving) else {
            return;
        };
        let titles: Vec<String> = draft
            .titles
            .iter()
            .map(|input| input.read(cx).value().to_string())
            .collect();
        let built = build_chapters(&draft.starts, &titles).and_then(|chapters| {
            validate_chapters(&chapters, self.media.info.duration).map(|()| chapters)
        });
        match built {
            Ok(chapters) => self.send_chapters(chapters, cx),
            Err(failure) => {
                self.status = failure_text(&failure);
                cx.notify();
            }
        }
    }
    fn send_chapters(&mut self, chapters: Vec<MediaChapter>, cx: &mut Context<Self>) {
        if let Some(draft) = &mut self.chapters {
            draft.saving = true;
            draft.pending = chapters.clone();
        }
        self.status.clear();
        self.stop(cx);
        self.send(
            Command::SetMediaChapters {
                media: self.media.id,
                chapters,
            },
            cx,
        );
    }
    /// The service answered the save: close the dialog on success, otherwise stay on it
    /// with the reason in the status.
    pub(super) fn chapters_saved(&mut self, saved: bool, cx: &mut Context<Self>) {
        let Some(draft) = &mut self.chapters else {
            return;
        };
        draft.saving = false;
        if !saved {
            return;
        }
        let pending = std::mem::take(&mut draft.pending);
        self.status = if pending.is_empty() {
            tr!("Chapitres retirés.").into()
        } else {
            tr!("Chapitres enregistrés.").into()
        };
        self.media.chapters = if pending.len() < 2 { vec![] } else { pending };
        if let Some(handle) = self.editor_window.take() {
            let _ = cx.update_window(handle, |_, window, cx| window.close_dialog(cx));
        }
    }
    pub(super) fn chapters_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use theme::{Height, Look};
        let Some(draft) = &self.chapters else {
            return div().into_any_element();
        };
        let saving = draft.saving;
        let starts = draft.starts.clone();
        let duration = self.media.info.duration.seconds().max(0.001);
        let fraction = |seconds: f64| (seconds / duration) as f32;
        let version = self
            .selected_version
            .map(|v| (fraction(v.start().seconds()), fraction(v.end().seconds())));
        let (kept_start, kept_end) = version.unwrap_or((0., 1.));
        let cursor = fraction(self.cursor_seconds).clamp(0., 1.);
        let bars = self.wave_bars(|position| {
            if position < kept_start || position > kept_end {
                theme::WAVE_IDLE
            } else if position <= cursor {
                theme::SKY.accent
            } else {
                theme::WAVE_SELECTED
            }
        });
        let panel = cx.entity();
        let mut overlay = div()
            .id("chapters-wave")
            .track_focus(&self.wave_focus)
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(14.))
            .right(px(14.))
            .cursor(if saving {
                CursorStyle::Arrow
            } else {
                CursorStyle::PointingHand
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|v, event: &MouseDownEvent, window, cx| {
                    v.chapters_press(event.position, window, cx)
                }),
            )
            .on_drag(WaveDrag, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_drag_move(
                cx.listener(|v, event: &DragMoveEvent<WaveDrag>, window, cx| {
                    v.chapters_move(event.event.position, window, cx)
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|v, _: &MouseUpEvent, _, cx| v.chapters_release(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|v, _: &MouseUpEvent, _, cx| v.chapters_release(cx)),
            )
            .on_key_down(cx.listener(|v, event: &KeyDownEvent, _, cx| v.wave_key(event, cx)))
            .on_prepaint(move |bounds, _, cx| {
                panel.update(cx, |v, cx| v.wave_painted(bounds, cx));
            });
        if version.is_some() {
            overlay = overlay.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(kept_start))
                    .w(relative((kept_end - kept_start).max(0.)))
                    .border_t_3()
                    .border_b_3()
                    .border_color(theme::c(theme::SKY.accent))
                    .bg(theme::c(theme::SKY.accent).opacity(0.06)),
            );
        }
        for start in starts.iter().skip(1) {
            overlay = overlay.child(wave_handle(fraction(*start), theme::LILAC.accent));
        }
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
        let wave = div()
            .relative()
            .h(px(150.))
            .rounded(px(20.))
            .bg(theme::c(theme::CREAM))
            .px(px(14.))
            .overflow_hidden()
            .child(bars)
            .children(self.waveform.is_some().then_some(overlay));
        let ends: Vec<f64> = starts
            .iter()
            .skip(1)
            .copied()
            .chain([self.media.info.duration.seconds()])
            .collect();
        let rows = starts
            .iter()
            .zip(&ends)
            .zip(&draft.titles)
            .enumerate()
            .map(|(index, ((start, end), title))| {
                let (start, end) = (*start, *end);
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .w(px(26.))
                            .flex_none()
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .text_color(theme::c(theme::LILAC.ink))
                            .child((index + 1).to_string()),
                    )
                    .child(
                        theme::meta(format!(
                            "{} → {} · {}",
                            wording::position_text(start),
                            wording::position_text(end),
                            duration_text(end - start)
                        ))
                        .w(px(190.))
                        .flex_none(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(theme::small_field(Input::new(title).disabled(saving))),
                    )
                    .child(theme::round(
                        Button::new(element_id("listen-chapter", index))
                            .icon(IconName::Play)
                            .accessibility_label(tr!("Écouter"))
                            .on_click(cx.listener(move |v, _, _, cx| v.listen_from(start, cx))),
                        Look::Solid(&theme::MINT),
                        36.,
                        saving,
                        cx,
                    ))
                    .child(if index == 0 {
                        div().size(px(36.)).flex_none().into_any_element()
                    } else {
                        theme::round(
                            Button::new(element_id("remove-chapter", index))
                                .icon(IconName::X)
                                .accessibility_label(tr!("Retirer"))
                                .on_click(cx.listener(move |v, _, window, cx| {
                                    v.remove_mark(index, window, cx)
                                })),
                            Look::Soft(&theme::ROSE),
                            36.,
                            saving,
                            cx,
                        )
                        .into_any_element()
                    })
            })
            .collect::<Vec<_>>();
        let untitled = vec![String::new(); starts.len()];
        let marks = build_chapters(&starts, &untitled).unwrap_or_default();
        let notes = playlist_notes(&plan_chapters(self.playlist_range(), &marks));
        let close = theme::dialog_close(
            "close-chapters-corner",
            theme::round(
                Button::new("close-chapters")
                    .icon(IconName::X)
                    .accessibility_label(tr!("Fermer")),
                Look::Soft(&theme::LIGHT_SAND),
                40.,
                saving,
                cx,
            ),
        );
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(14.))
                    .child(theme::icon_tile(
                        IconName::BookmarkPlus,
                        &theme::LILAC,
                        52.,
                        16.,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(theme::heading(
                                tr!("Chapitres de « {title} »", title = self.media.title),
                                26.,
                            ))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .text_color(theme::c(theme::MUTED))
                                    .child(div().child(tr!("Écoutez, mettez en pause là où une nouvelle partie commence, puis ajoutez un chapitre à cet endroit.")))
                                    .child(div().child(tr!("Ajouté à une playlist, cet audio arrivera en un seul chapitre, avec une piste par repère : les boutons du lecteur passent de l’une à l’autre."))),
                            ),
                    )
                    .child(close),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(wave)
                    .children(version.is_some().then(|| {
                        theme::meta(tr!(
                            "Les repères en dehors de la version découpée ne seront pas utilisés."
                        ))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(10.))
                    .child(theme::pill(
                        Button::new("add-chapter")
                            .icon(IconName::BookmarkPlus)
                            .label(tr!("Ajouter un chapitre ici"))
                            .on_click(cx.listener(|v, _, window, cx| v.add_mark(window, cx))),
                        Look::Soft(&theme::LILAC),
                        Height::Large,
                        saving,
                        cx,
                    ))
                    .child(theme::pill(
                        Button::new("preview-chapters")
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
                                tr!("Écouter")
                            })
                            .on_click(cx.listener(|v, _, _, cx| {
                                if v.playing {
                                    v.pause(cx);
                                } else {
                                    v.play_from_cursor(cx);
                                }
                            })),
                        Look::Solid(&theme::MINT),
                        Height::Large,
                        saving || self.preview_pending.is_some(),
                        cx,
                    ))
                    .child(theme::round(
                        Button::new("nudge-back")
                            .icon(IconName::ChevronLeft)
                            .accessibility_label(tr!("Reculer d’une seconde"))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.seek(v.cursor_seconds - NUDGE_FINE_SECONDS, cx)
                            })),
                        Look::Soft(&theme::LIGHT_SAND),
                        40.,
                        saving,
                        cx,
                    ))
                    .child(theme::round(
                        Button::new("nudge-forward")
                            .icon(IconName::ChevronRight)
                            .accessibility_label(tr!("Avancer d’une seconde"))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.seek(v.cursor_seconds + NUDGE_FINE_SECONDS, cx)
                            })),
                        Look::Soft(&theme::LIGHT_SAND),
                        40.,
                        saving,
                        cx,
                    ))
                    .child(theme::meta(format!(
                        "{} / {}",
                        wording::position_text(self.cursor_seconds),
                        wording::position_text(duration)
                    ))),
            )
            .child(
                div()
                    .id("chapter-rows")
                    .max_h(px(240.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(rows),
            )
            .children(notes.into_iter().map(theme::note))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(10.))
                    .children(saving.then(|| Spinner::new().color(theme::c(theme::LILAC.accent))))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(200.))
                            .text_size(px(14.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::c(theme::INK_SOFT))
                            .child(self.status.clone()),
                    )
                    .children((self.media.chapters.len() >= 2).then(|| {
                        theme::pill(
                            Button::new("remove-chapters")
                                .label(tr!("Retirer tous les chapitres"))
                                .on_click(
                                    cx.listener(|v, _, _, cx| v.send_chapters(vec![], cx)),
                                ),
                            Look::Link(&theme::ROSE),
                            Height::Large,
                            saving,
                            cx,
                        )
                    }))
                    .child(theme::dialog_close(
                        "cancel-chapters-footer",
                        theme::pill(
                            Button::new("cancel-chapters").label(tr!("Annuler")),
                        Look::Outline,
                        Height::Large,
                        saving,
                        cx,
                    )))
                    .child(theme::pill(
                        Button::new("save-chapters")
                            .label(if saving {
                                tr!("Enregistrement…")
                            } else {
                                tr!("Enregistrer")
                            })
                            .loading(saving)
                            .on_click(cx.listener(|v, _, _, cx| v.save_chapters(cx))),
                        Look::Primary,
                        Height::Large,
                        saving,
                        cx,
                    )),
            )
            .into_any_element()
    }
}

/// The mark a press `x` pixels into a waveform `width` pixels wide grabs: the nearest one
/// within reach, else none. The start of the first chapter is not a mark and cannot be held.
fn grab_mark(x: f32, width: f32, starts: &[f64], duration: f64) -> Option<usize> {
    if duration <= 0. {
        return None;
    }
    starts
        .iter()
        .enumerate()
        .skip(1)
        .map(|(index, start)| (index, (x - (*start / duration) as f32 * width).abs()))
        .filter(|(_, distance)| *distance <= HANDLE_REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}
/// Whether a chapter may start at `at` seconds: inside the audio, not at its very start
/// or end, and not within a second of another mark.
fn can_add_mark(starts: &[f64], at: f64, duration: f64) -> bool {
    at >= MIN_CHAPTER_SECONDS
        && at <= duration - MIN_CHAPTER_SECONDS
        && starts
            .iter()
            .all(|start| (start - at).abs() >= MIN_CHAPTER_SECONDS)
}
/// Put a mark at `at` among the sorted `starts`; returns where it landed.
fn insert_mark(starts: &mut Vec<f64>, at: f64) -> usize {
    let index = starts.partition_point(|start| *start < at);
    starts.insert(index, at);
    index
}
/// The marks after moving the one at `index` to `to`: it stops a second short of its
/// neighbours (and of the end of the audio), so the order never changes. The first
/// chapter's start stays at zero.
fn moved_mark(starts: &[f64], index: usize, to: f64, duration: f64) -> Vec<f64> {
    let mut moved = starts.to_vec();
    if index == 0 || index >= moved.len() {
        return moved;
    }
    let low = moved[index - 1] + MIN_CHAPTER_SECONDS;
    let high = moved.get(index + 1).copied().unwrap_or(duration) - MIN_CHAPTER_SECONDS;
    if low <= high {
        moved[index] = to.clamp(low, high);
    }
    moved
}
/// The chapters to save from the marks and the titles typed for them: the first starts at
/// zero, titles are trimmed and may stay empty (the default wording). Fewer than two
/// chapters are no marks at all.
fn build_chapters(starts: &[f64], titles: &[String]) -> yoto_domain::Result<Vec<MediaChapter>> {
    if starts.len() < 2 {
        return Ok(vec![]);
    }
    starts
        .iter()
        .zip(titles)
        .enumerate()
        .map(|(index, (start, title))| {
            Ok(MediaChapter {
                start: if index == 0 {
                    MediaTime::ZERO
                } else {
                    MediaTime::from_seconds(*start)?
                },
                title: title.trim().to_owned(),
            })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn a_press_grabs_the_nearest_mark_within_reach() {
        // 600 px for 120 s: marks at 100 px (20 s) and 300 px (60 s).
        let starts = [0., 20., 60.];
        assert_eq!(grab_mark(104., 600., &starts, 120.), Some(1));
        assert_eq!(grab_mark(290., 600., &starts, 120.), Some(2));
        assert_eq!(grab_mark(200., 600., &starts, 120.), None);
        // The first chapter's start is not a mark.
        assert_eq!(grab_mark(2., 600., &starts, 120.), None);
        // Marks drawn almost on top of each other: the nearer one wins.
        let close = [0., 20., 21.];
        assert_eq!(grab_mark(100., 600., &close, 120.), Some(1));
        assert_eq!(grab_mark(106., 600., &close, 120.), Some(2));
        assert_eq!(grab_mark(10., 600., &starts, 0.), None);
    }
    #[core::prelude::v1::test]
    fn a_mark_needs_room_inside_the_audio_and_away_from_the_others() {
        let starts = [0., 30.];
        assert!(can_add_mark(&starts, 10., 120.));
        assert!(!can_add_mark(&starts, 0., 120.));
        assert!(!can_add_mark(&starts, 0.4, 120.));
        assert!(!can_add_mark(&starts, 30., 120.));
        assert!(!can_add_mark(&starts, 30.5, 120.));
        assert!(can_add_mark(&starts, 31., 120.));
        assert!(!can_add_mark(&starts, 120., 120.));
        assert!(!can_add_mark(&starts, 119.5, 120.));
        assert!(can_add_mark(&starts, 119., 120.));
    }
    #[core::prelude::v1::test]
    fn marks_stay_in_order_when_added_or_moved() {
        let mut starts = vec![0., 30., 90.];
        assert_eq!(insert_mark(&mut starts, 60.), 2);
        assert_eq!(starts, [0., 30., 60., 90.]);
        assert_eq!(moved_mark(&starts, 2, 70., 120.), [0., 30., 70., 90.]);
        // Never past a neighbour, never onto the start or the end.
        assert_eq!(moved_mark(&starts, 2, 200., 120.), [0., 30., 89., 90.]);
        assert_eq!(moved_mark(&starts, 2, 5., 120.), [0., 30., 31., 90.]);
        assert_eq!(moved_mark(&starts, 1, -10., 120.), [0., 1., 60., 90.]);
        assert_eq!(moved_mark(&starts, 3, 500., 120.), [0., 30., 60., 119.]);
        assert_eq!(moved_mark(&starts, 0, 10., 120.), starts);
        // Neighbours too close to leave room: the mark stays.
        assert_eq!(moved_mark(&[0., 5., 6.], 1, 5.5, 120.), [0., 5., 6.]);
    }
    #[core::prelude::v1::test]
    fn chapters_are_built_from_marks_and_typed_titles() {
        let titles = ["  Prologue ".to_owned(), String::new(), "   ".to_owned()];
        let chapters = build_chapters(&[0., 12.5, 60.], &titles).unwrap();
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].start, MediaTime::ZERO);
        assert_eq!(chapters[0].title, "Prologue");
        assert_eq!(chapters[1].start, MediaTime::from_seconds(12.5).unwrap());
        assert_eq!(chapters[1].title, "");
        assert_eq!(chapters[2].title, "");
        // A first mark that drifted from zero is still saved at zero.
        let drifted = build_chapters(&[0.3, 10.], &titles).unwrap();
        assert_eq!(drifted[0].start, MediaTime::ZERO);
        // One chapter alone is no marks.
        assert!(build_chapters(&[0.], &titles).unwrap().is_empty());
    }
}
