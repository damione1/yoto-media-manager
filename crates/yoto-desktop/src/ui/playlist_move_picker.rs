use super::*;
use gpui_kit::component::{dialog::DialogClose, spinner::Spinner};
pub(super) struct MovePicker {
    owner: WeakEntity<PlaylistEditor>,
    rows: Arc<Vec<media_picker::ChapterChoice>>,
    source: ChapterId,
    track: TrackId,
    selected: Option<(ChapterId, usize)>,
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
            _observation: cx.observe(&owner, |_, _, cx| cx.notify()),
        }
    }
}
impl MovePicker {
    /// Move the track at the end of the chosen chapter and save.
    fn confirm(&mut self, cx: &mut Context<Self>) {
        let Some((chapter, end)) = self.selected else {
            return;
        };
        let track = self.track;
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.blocked() || owner.owning_remote(cx).is_err() {
                return;
            }
            if let Err(error) = owner.draft.move_track(track, chapter, end) {
                owner.status = failure_text(&error);
                cx.notify();
                return;
            }
            owner.persist(CommitTarget::MovePicker, cx);
        });
    }
    fn chapter_row(&self, chapter: &media_picker::ChapterChoice, cx: &mut Context<Self>) -> Div {
        let destination = chapter.id;
        let end = chapter.tracks;
        let blocked = self
            .owner
            .upgrade()
            .is_none_or(|owner| owner.read(cx).blocked());
        div().h(px(48.)).child(
            Button::new(element_id("move-chapter", destination))
                .label(chapter.title.clone())
                .w_full()
                .disabled(destination == self.source || blocked)
                .selected(self.selected.is_some_and(|(id, _)| id == destination))
                .on_click(cx.listener(move |v, _, _, cx| {
                    v.selected = Some((destination, end));
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
            self.rows.len(),
            cx.processor(
                |v: &mut Self, range: std::ops::Range<usize>, _, cx: &mut Context<Self>| {
                    let rows = v.rows.clone();
                    range
                        .filter_map(|index| rows.get(index))
                        .map(|chapter| v.chapter_row(chapter, cx).into_any_element())
                        .collect()
                },
            ),
        )
        .h(px(300.));
        let actions = div()
            .flex()
            .gap_2()
            .items_center()
            .child(div().flex_1())
            .child(DialogClose::new().child(theme::pill(
                Button::new("cancel-move").label("Annuler"),
                Look::Ghost,
                Height::Medium,
                busy,
                cx,
            )))
            .child(theme::pill(
                Button::new("confirm-move")
                    .label("Déplacer")
                    .on_click(cx.listener(|v, _, _, cx| v.confirm(cx))),
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
                "Choisissez le chapitre dans lequel ajouter cette piste. Son titre et ses références sont conservés.",
            ))
            .child(targets)
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
