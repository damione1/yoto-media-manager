use super::*;
use gpui_kit::component::{WindowExt, dialog::DialogClose, spinner::Spinner};
use yoto_updater::{CURRENT_VERSION, UpdateAction, UpdateState};
pub(super) struct UpdatePanel {
    state: Arc<UpdateState>,
    notice: Option<Failure>,
    runtime: Arc<tokio::runtime::Runtime>,
    service: Arc<Service>,
    _observer: Task<()>,
}
impl UpdatePanel {
    pub fn new(
        runtime: Arc<tokio::runtime::Runtime>,
        service: Arc<Service>,
        cx: &mut Context<Self>,
    ) -> Self {
        let worker_service = service.clone();
        let rt = runtime.clone();
        let observer = cx.spawn(async move |weak, cx| {
            let mut previous = Arc::new(UpdateState::Idle);
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(300))
                    .await;
                let service = worker_service.clone();
                let before = previous.clone();
                let response = rt
                    .spawn(async move {
                        match service
                            .execute(Command::AppUpdate(UpdateAction::State))
                            .await?
                        {
                            Reply::AppUpdate(state) => Ok::<_, Failure>((
                                state.clone(),
                                if Arc::ptr_eq(&state, &before) || state == before {
                                    None
                                } else {
                                    Some(state)
                                },
                            )),
                            Reply::Updated
                            | Reply::Saved(_)
                            | Reply::OpenedPlaylist(_)
                            | Reply::PlaylistOperation { .. }
                            | Reply::Entries(_)
                            | Reply::Waveform(_)
                            | Reply::Prepared(_)
                            | Reply::Asset(_)
                            | Reply::PixelArt(_)
                            | Reply::Image(_)
                            | Reply::Images(_)
                            | Reply::Player(_)
                            | Reply::Data(_) => {
                                Err(Failure::Validation("Unexpected updater response".into()))
                            }
                        }
                    })
                    .await;
                match response {
                    Ok(Ok((next, changed))) => {
                        previous = next;
                        if let Some(state) = changed
                            && weak
                                .update(cx, |view, cx| {
                                    view.state = state;
                                    cx.notify();
                                })
                                .is_err()
                        {
                            break;
                        }
                    }
                    Ok(Err(_)) | Err(_) => {
                        if weak.upgrade().is_none() {
                            break;
                        }
                    }
                }
            }
        });
        if !cfg!(debug_assertions) {
            let service = service.clone();
            runtime.spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let _ = service
                    .execute(Command::AppUpdate(UpdateAction::Check))
                    .await;
            });
        }
        Self {
            notice: None,
            state: Arc::new(UpdateState::Idle),
            runtime,
            service,
            _observer: observer,
        }
    }
    pub fn open(entity: Entity<Self>, window: &mut Window, cx: &mut App) {
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Mises à jour")
                .w(px(520.))
                .child(entity.clone())
        });
    }
    pub fn sidebar_label(&self) -> &'static str {
        match self.state.as_ref() {
            UpdateState::Available(_)
            | UpdateState::Downloading { .. }
            | UpdateState::Verifying(_)
            | UpdateState::Ready { .. } => "Mise à jour disponible",
            UpdateState::Idle
            | UpdateState::Checking
            | UpdateState::Current
            | UpdateState::Installing
            | UpdateState::Failed(_) => "Mises à jour",
        }
    }
    fn request(&self, action: UpdateAction) {
        let service = self.service.clone();
        self.runtime.spawn(async move {
            let _ = service.execute(Command::AppUpdate(action)).await;
        });
    }
    fn install(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        let service = self.service.clone();
        let owner = service.is_owner();
        let rt = self.runtime.clone();
        cx.spawn(async move |weak, cx| {
            let outcome = rt
                .spawn(async move { service.install_update().await })
                .await;
            match outcome {
                Ok(Ok(())) => {
                    cx.update(|cx| cx.quit());
                }
                Ok(Err(error)) => {
                    let _ = weak.update(cx, |view, cx| {
                        if !owner {
                            view.notice = Some(error);
                        }
                        cx.notify();
                    });
                }
                Err(error) => {
                    let _ = weak.update(cx, |view, cx| {
                        if !owner {
                            view.notice = Some(Failure::Storage(error.to_string()));
                        }
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }
}
impl Render for UpdatePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use theme::{Height, Look};
        let busy = |text: String| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::c(theme::INK_SOFT))
                .child(Spinner::new().color(theme::c(theme::MINT.accent)))
                .child(text)
        };
        let problem = |text: String| {
            theme::inset().bg(theme::c(theme::ROSE.soft)).child(
                div()
                    .text_color(theme::c(theme::ROSE.ink))
                    .font_weight(FontWeight::BOLD)
                    .child(text),
            )
        };
        let mut body = div().flex().flex_col().gap(px(14.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(theme::icon_tile(IconName::Download, &theme::MINT, 44., 13.))
                .child(
                    div()
                        .child(theme::heading(PRODUCT_NAME, 19.))
                        .child(theme::meta(format!(
                            "Version installée : {CURRENT_VERSION}"
                        ))),
                ),
        );
        let mut action = None;
        match self.state.as_ref() {
            UpdateState::Idle => {
                body = body.child(theme::note(format!(
                    "Recevez les nouvelles versions de {PRODUCT_NAME} directement dans l’application."
                )));
                action = Some(theme::pill(
                    Button::new("check-update")
                        .label("Rechercher une mise à jour")
                        .on_click(cx.listener(|view, _, _, _| view.request(UpdateAction::Check))),
                    Look::Primary,
                    Height::Medium,
                    false,
                    cx,
                ));
            }
            UpdateState::Checking => {
                body = body.child(busy("Recherche de mises à jour…".into()));
            }
            UpdateState::Current => {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(theme::done_mark(22.))
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .child("Vous utilisez la dernière version disponible."),
                        ),
                );
                action = Some(theme::pill(
                    Button::new("recheck-update")
                        .label("Vérifier à nouveau")
                        .on_click(cx.listener(|view, _, _, _| view.request(UpdateAction::Check))),
                    Look::Outline,
                    Height::Medium,
                    false,
                    cx,
                ));
            }
            UpdateState::Available(release) => {
                body = body
                    .child(
                        div()
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .child(format!("La version {} est disponible.", release.version)),
                    )
                    .child(release_notes(&release.notes));
                action = Some(theme::pill(
                    Button::new("download-update")
                        .label("Télécharger la mise à jour")
                        .on_click(
                            cx.listener(|view, _, _, _| view.request(UpdateAction::Download)),
                        ),
                    Look::Primary,
                    Height::Medium,
                    false,
                    cx,
                ));
            }
            UpdateState::Downloading {
                received, total, ..
            } => {
                let percent = if *total == 0 {
                    0
                } else {
                    received.saturating_mul(100) / total
                };
                body = body.child(busy(format!(
                    "Téléchargement : {percent} % ({:.1} / {:.1} Mo)",
                    *received as f64 / 1_000_000.,
                    *total as f64 / 1_000_000.
                )));
                action = Some(theme::pill(
                    Button::new("cancel-update")
                        .label("Annuler le téléchargement")
                        .on_click(cx.listener(|view, _, _, _| view.request(UpdateAction::Cancel))),
                    Look::Outline,
                    Height::Medium,
                    false,
                    cx,
                ));
            }
            UpdateState::Verifying(_) => {
                body = body.child(busy(
                    "Vérification et préparation de la mise à jour…".into(),
                ));
            }
            UpdateState::Ready { release, issue } => {
                body = body
                    .children(
                        issue
                            .clone()
                            .map(|message| problem(failure_text(&Failure::Validation(message)))),
                    )
                    .child(div().font_weight(FontWeight::EXTRA_BOLD).child(format!(
                        "La version {} est prête à être installée.",
                        release.version
                    )))
                    .child(theme::note(
                        "Votre médiathèque est conservée. Terminez vos imports et envois avant de redémarrer.",
                    ));
                action = Some(theme::pill(
                    Button::new("install-update")
                        .label("Installer et redémarrer")
                        .on_click(cx.listener(|view, _, _, cx| view.install(cx))),
                    Look::Primary,
                    Height::Medium,
                    false,
                    cx,
                ));
            }
            UpdateState::Installing => {
                body = body.child(busy("Redémarrage de l’application…".into()));
            }
            UpdateState::Failed(error) => {
                body = body.child(problem(wording::update_failure_text(error)));
                action = Some(theme::pill(
                    Button::new("retry-update")
                        .label("Réessayer")
                        .on_click(cx.listener(|view, _, _, _| view.request(UpdateAction::Check))),
                    Look::Primary,
                    Height::Medium,
                    false,
                    cx,
                ));
            }
        }
        body = body.children(
            self.notice
                .as_ref()
                .map(|error| problem(failure_text(error))),
        );
        let installing = matches!(self.state.as_ref(), UpdateState::Installing);
        body.child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(DialogClose::new().child(theme::pill(
                    Button::new("close-update").label("Fermer"),
                    Look::Outline,
                    Height::Medium,
                    installing,
                    cx,
                )))
                .children(action),
        )
    }
}

/// Release notes as short paragraphs and bullets, in a scrolling inset. Markdown markers
/// (`#`, `-`, `*`) are read as structure instead of being shown.
fn release_notes(notes: &str) -> Stateful<Div> {
    let mut list = div()
        .id("release-notes")
        .max_h(px(220.))
        .overflow_y_scroll()
        .bg(theme::c(theme::CREAM))
        .rounded(px(16.))
        .p(px(14.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .text_size(px(13.))
        .line_height(relative(1.45))
        .text_color(theme::c(theme::INK_SOFT));
    for line in notes.lines().map(str::trim).filter(|line| !line.is_empty()) {
        list = list.child(if let Some(title) = line.strip_prefix('#') {
            div()
                .font_weight(FontWeight::EXTRA_BOLD)
                .text_color(theme::c(theme::INK))
                .child(title.trim_start_matches('#').trim().to_owned())
        } else if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            div()
                .flex()
                .gap(px(8.))
                .child("•")
                .child(div().flex_1().child(item.to_owned()))
        } else {
            div().child(line.to_owned())
        });
    }
    list
}
