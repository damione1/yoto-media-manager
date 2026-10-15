use super::*;
use gpui_kit::component::spinner::Spinner;
/// Where a moved track goes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Destination {
    Chapter(ChapterId),
    /// A chapter of its own, last in the playlist.
    NewChapter,
}
/// Whether the track comes first or last in the chosen chapter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Place {
    Start,
    End,
}
/// The position a track takes in a chapter of `len` tracks.
fn index_in(place: Place, len: usize) -> usize {
    match place {
        Place::Start => 0,
        Place::End => len,
    }
}
pub(super) struct MovePicker {
    owner: WeakEntity<PlaylistEditor>,
    rows: Arc<Vec<media_picker::ChapterChoice>>,
    source: ChapterId,
    track: TrackId,
    selected: Option<Destination>,
    place: Place,
    _observation: Subscription,
}
impl MovePicker {
    pub(super) fn new(
        owner: Entity<PlaylistEditor>,
        rows: Arc<Vec<media_picker::ChapterChoice>>,
        source: ChapterId,
        track: TrackId,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            owner: owner.downgrade(),
            rows,
            source,
            track,
            selected: None,
            place: Place::End,
            _observation: cx.observe(&owner, |_, _, cx| cx.notify()),
        }
    }
}
impl MovePicker {
    /// Move the track where it was chosen; it is written with the next group.
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(destination) = self.selected else {
            return;
        };
        let (track, place) = (self.track, self.place);
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.blocked() || owner.owning_remote(cx).is_err() {
                return;
            }
            let moved = owner.restructure(
                move |draft| match destination {
                    Destination::Chapter(chapter) => {
                        let len = draft
                            .chapters
                            .iter()
                            .find(|c| c.id == chapter)
                            .map_or(0, |c| c.tracks.len());
                        draft.move_track(track, chapter, index_in(place, len))
                    }
                    Destination::NewChapter => {
                        let end = draft.chapters.len();
                        place_track_as_chapter(draft, track, end)
                    }
                },
                window,
                cx,
            );
            if moved {
                owner.close_picker(cx);
            }
        });
    }
    /// One choice: a chapter by its number, title and size, or a new chapter after the last.
    fn destination_row(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let blocked = self
            .owner
            .upgrade()
            .is_none_or(|owner| owner.read(cx).blocked());
        let (button, destination) = match self.rows.get(index) {
            Some(chapter) => (
                Button::new(element_id("move-chapter", chapter.id))
                    .label(trn!(
                        "Chapitre {n} · {title} · {count} piste",
                        "Chapitre {n} · {title} · {count} pistes",
                        chapter.tracks,
                        n = index + 1,
                        title = chapter.title.clone()
                    ))
                    .disabled(chapter.id == self.source || blocked),
                Destination::Chapter(chapter.id),
            ),
            None => (
                Button::new("move-new-chapter")
                    .icon(IconName::Plus)
                    .label(tr!("Nouveau chapitre"))
                    .disabled(blocked),
                Destination::NewChapter,
            ),
        };
        div().h(px(48.)).child(
            button
                .w_full()
                .selected(self.selected == Some(destination))
                .on_click(cx.listener(move |v, _, _, cx| {
                    v.selected = Some(destination);
                    cx.notify();
                })),
        )
    }
}
impl Render for MovePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use theme::{Height, Look};
        let (busy, blocked, status) = self
            .owner
            .upgrade()
            .map(|owner| {
                let owner = owner.read(cx);
                (owner.busy(), owner.blocked(), owner.status.clone())
            })
            .unwrap_or((false, true, String::new()));
        let targets = gpui_kit::uniform_list(
            "move-chapter-targets",
            self.rows.len() + 1,
            cx.processor(
                |v: &mut Self, range: std::ops::Range<usize>, _, cx: &mut Context<Self>| {
                    range
                        .map(|index| v.destination_row(index, cx).into_any_element())
                        .collect()
                },
            ),
        )
        .h(px(300.));
        let in_chapter = matches!(self.selected, Some(Destination::Chapter(_)));
        let places = [
            (Place::Start, tr!("Au début")),
            (Place::End, tr!("À la fin")),
        ]
        .map(|(place, label)| {
            theme::segment(
                Button::new(element_id("move-place", label))
                    .label(label)
                    .disabled(!in_chapter)
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.place = place;
                        cx.notify();
                    })),
                self.place == place,
                Height::Small,
                cx,
            )
        });
        let actions = div()
            .flex()
            .gap_2()
            .items_center()
            .child(div().flex_1())
            .child(theme::dialog_close(
                "cancel-move-footer",
                theme::pill(
                    Button::new("cancel-move").label(tr!("Annuler")),
                    Look::Ghost,
                    Height::Medium,
                    busy,
                    cx,
                ),
            ))
            .child(theme::pill(
                Button::new("confirm-move")
                    .label(tr!("Déplacer"))
                    .on_click(cx.listener(|v, _, window, cx| v.confirm(window, cx))),
                Look::Primary,
                Height::Medium,
                blocked || self.selected.is_none(),
                cx,
            ));
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(theme::note(
                tr!("Choisissez le chapitre dans lequel placer cette piste, ou créez un nouveau chapitre. Son titre et ses références sont conservés."),
            ))
            .child(targets)
            .child(
                div()
                    .flex()
                    .gap_3()
                    .items_center()
                    .child(theme::meta(tr!("Place dans le chapitre")))
                    .child(theme::segmented().children(places)),
            )
            .child(actions)
            .children(busy.then(|| {
                div()
                    .flex()
                    .gap_2()
                    .child(Spinner::new().small())
                    .child(status.clone())
            }))
            .children((!busy && blocked).then(|| theme::note(status)))
    }
}
#[cfg(test)]
mod tests {
    use super::{Place, index_in};
    #[test]
    fn a_track_goes_first_or_last_in_the_chosen_chapter() {
        assert_eq!(index_in(Place::Start, 4), 0);
        assert_eq!(index_in(Place::End, 4), 4);
        assert_eq!(index_in(Place::Start, 0), 0);
        assert_eq!(index_in(Place::End, 0), 0);
    }
}
