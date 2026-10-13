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
        let mut body = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(format!("Version installée : {CURRENT_VERSION}"));
        match self.state.as_ref() {
            UpdateState::Idle => {
                body = body.child("Recevez les nouvelles versions de YotoMediaManager directement dans l’application.").child(Button::new("check-update").label("Rechercher une mise à jour").primary().on_click(cx.listener(|view, _, _, _| view.request(UpdateAction::Check))));
            }
            UpdateState::Checking => {
                body = body.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child("Recherche de mises à jour…"),
                );
            }
            UpdateState::Current => {
                body = body
                    .child("Vous utilisez la dernière version disponible.")
                    .child(
                        Button::new("recheck-update")
                            .label("Vérifier à nouveau")
                            .on_click(
                                cx.listener(|view, _, _, _| view.request(UpdateAction::Check)),
                            ),
                    );
            }
            UpdateState::Available(release) => {
                let release = release.clone();
                body = body
                    .child(format!("La version {} est disponible.", release.version))
                    .child(release.notes.clone())
                    .child(
                        Button::new("download-update")
                            .label("Télécharger la mise à jour")
                            .primary()
                            .on_click(
                                cx.listener(|view, _, _, _| view.request(UpdateAction::Download)),
                            ),
                    );
            }
            UpdateState::Downloading {
                received, total, ..
            } => {
                let percent = if *total == 0 {
                    0
                } else {
                    received.saturating_mul(100) / total
                };
                body = body
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(Spinner::new().small())
                            .child(format!(
                                "Téléchargement : {percent} % ({:.1} / {:.1} Mo)",
                                *received as f64 / 1_000_000.,
                                *total as f64 / 1_000_000.
                            )),
                    )
                    .child(
                        Button::new("cancel-update")
                            .label("Annuler le téléchargement")
                            .on_click(
                                cx.listener(|view, _, _, _| view.request(UpdateAction::Cancel)),
                            ),
                    );
            }
            UpdateState::Verifying(_) => {
                body = body.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child("Vérification et préparation de la mise à jour…"),
                );
            }
            UpdateState::Ready { release, issue } => {
                body = body.children(
                    issue
                        .clone()
                        .map(|message| div().text_color(cx.theme().danger).child(message)),
                );
                body = body.child(format!("La version {} est prête à être installée.", release.version)).child("Votre médiathèque est conservée. Terminez vos imports et envois avant de redémarrer.").child(Button::new("install-update").label("Installer et redémarrer").primary().on_click(cx.listener(|view, _, _, cx| view.install(cx))));
            }
            UpdateState::Installing => {
                body = body.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child("Redémarrage de l’application…"),
                );
            }
            UpdateState::Failed(error) => {
                body =
                    body.child(div().text_color(cx.theme().danger).child(error.to_string()))
                        .child(Button::new("retry-update").label("Réessayer").on_click(
                            cx.listener(|view, _, _, _| view.request(UpdateAction::Check)),
                        ));
            }
        }
        body = body.children(
            self.notice
                .as_ref()
                .map(|error| div().text_color(cx.theme().danger).child(error.to_string())),
        );
        body.child(
            div().flex().justify_end().child(
                DialogClose::new().child(
                    Button::new("close-update")
                        .label("Fermer")
                        .disabled(matches!(self.state.as_ref(), UpdateState::Installing)),
                ),
            ),
        )
    }
}
