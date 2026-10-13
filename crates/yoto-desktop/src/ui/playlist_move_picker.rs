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
impl Render for MovePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (busy, blocked, status) = self
            .owner
            .upgrade()
            .map(|owner| {
                let owner = owner.read(cx);
                (owner.busy(), owner.blocked(), owner.status.clone())
            })
            .unwrap_or((false, true, String::new()));
        div().flex().flex_col().gap_4()
            .child("Choisissez le chapitre dans lequel ajouter cette piste. Son titre et ses références sont conservés.")
            .child(gpui_kit::uniform_list("move-chapter-targets", self.rows.len(), cx.processor(|v: &mut Self, range: std::ops::Range<usize>, _, cx: &mut Context<Self>| {
                range.filter_map(|index| v.rows.get(index)).map(|chapter| { let destination = chapter.id; let end = chapter.tracks;
                    div().h(px(48.)).child(Button::new(element_id("move-chapter", destination)).label(chapter.title.clone()).w_full().disabled(destination == v.source || v.owner.upgrade().is_none_or(|owner| owner.read(cx).blocked())).selected(v.selected.is_some_and(|(id, _)| id == destination)).on_click(cx.listener(move |v, _, _, cx| { v.selected = Some((destination, end)); cx.notify(); })))
                }).collect()
            })).h(px(300.)))
            .child(div().flex().gap_2().items_center().child(div().flex_1())
                .child(DialogClose::new().child(Button::new("cancel-move").label("Annuler").disabled(busy)))
                .child(Button::new("confirm-move").label("Déplacer").primary().disabled(blocked || self.selected.is_none()).on_click(cx.listener(|v, _, _, cx| {
                    if let Some((chapter, end)) = v.selected { let track = v.track; let _ = v.owner.update(cx, |owner, cx| {
                        if owner.blocked() || owner.owning_remote(cx).is_err() { return }
                        if let Err(error) = owner.draft.move_track(track, chapter, end) { owner.status = failure_text(&error); cx.notify(); return }
                        owner.persist(CommitTarget::MovePicker, cx);
                    }); }
                }))))
            .children(busy.then(|| div().flex().gap_2().child(Spinner::new().small()).child(status.clone())))
            .children((!busy && blocked).then(|| div().text_sm().child(status)))
    }
}
