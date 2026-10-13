use super::*;
use gpui_kit::component::checkbox::Checkbox;
use yoto_infra::api::{Group, RemoteCard};

/// Explicit editable draft; the remote catalogue remains owned by AppView's snapshot.
pub struct GroupEditor {
    account: AccountId,
    id: Option<GroupId>,
    name: Entity<InputState>,
    cards: Vec<CardId>,
    available: Vec<RemoteCard>,
    image: Option<FamilyImageId>,
    images: Vec<FamilyImageId>,
    preview: Option<PixelArt>,
    service: Arc<Service>,
    runtime: Arc<tokio::runtime::Runtime>,
    tasks: Vec<Task<()>>,
    status: String,
    saving: bool,
}
impl GroupEditor {
    pub fn new(
        account: AccountId,
        group: Option<Group>,
        available: Vec<RemoteCard>,
        service: Arc<Service>,
        runtime: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Nom du groupe")
                .default_value(group.as_ref().map(|g| g.name.clone()).unwrap_or_default())
        });
        Self {
            account,
            id: group.as_ref().map(|g| g.id.clone()),
            name,
            cards: group.as_ref().map(|g| g.cards.to_vec()).unwrap_or_default(),
            image: group.and_then(|g| g.image),
            available,
            images: vec![],
            preview: None,
            service,
            runtime,
            tasks: vec![],
            status: String::new(),
            saving: false,
        }
    }
    fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        let writing = matches!(&command, Command::SaveGroup { .. });
        if writing && self.saving {
            return;
        }
        if writing {
            self.saving = true;
        }
        let service = self.service.clone();
        let rt = self.runtime.clone();
        let account = self.account.clone();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move {
                    let reply = service.execute(command).await?;
                    // Refresh the account projection after a group write, without optimistic parallel state.
                    let refresh_error = if writing {
                        service.execute(Command::Sync(account)).await.err()
                    } else {
                        None
                    };
                    Ok::<_, Failure>((reply, refresh_error))
                })
                .await;
            let _ = weak.update(cx, |v, cx| {
                v.saving = false;
                let mut refresh_failure = None;
                let result = result.map(|r| {
                    r.map(|(reply, refresh_error)| {
                        if writing && let Reply::Data(data) = &reply {
                            v.id = data
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .and_then(|id| GroupId::new(id).ok());
                        }
                        refresh_failure = refresh_error;
                        reply
                    })
                });
                match result {
                    Ok(Ok(Reply::Images(images))) => {
                        v.images = images;
                        v.status = "Choisissez une image familiale.".into();
                    }
                    Ok(Ok(Reply::Image(id))) => {
                        v.image = Some(id.clone());
                        v.load_image(id, cx);
                        v.status = "Image ajoutée à votre famille.".into();
                    }
                    Ok(Ok(Reply::PixelArt(art))) => v.preview = Some(*art),
                    Ok(Ok(
                        Reply::Data(_)
                        | Reply::AppUpdate(_)
                        | Reply::Updated
                        | Reply::Saved(_)
                        | Reply::OpenedPlaylist(_)
                        | Reply::PlaylistOperation { .. },
                    )) => v.status = "Groupe enregistré. La bibliothèque a été actualisée.".into(),
                    Ok(Ok(
                        Reply::Entries(_)
                        | Reply::Waveform(_)
                        | Reply::Prepared(_)
                        | Reply::Asset(_)
                        | Reply::Player(_),
                    )) => v.status = "Réponse inattendue.".into(),
                    Ok(Err(e)) => {
                        if writing && matches!(e, Failure::Ambiguous) {
                            v.saving = true;
                        }
                        v.status = failure_text(&e);
                    }
                    Err(e) => v.status = failure_text(&Failure::Storage(e.to_string())),
                }
                if let Some(error) = refresh_failure {
                    v.status = format!(
                        "Groupe enregistré. Actualisation impossible : {}",
                        failure_text(&error)
                    );
                }
                cx.notify();
            });
        }));
    }
    fn load_image(&mut self, id: FamilyImageId, cx: &mut Context<Self>) {
        self.image = Some(id.clone());
        let selected = id.clone();
        let account = self.account.clone();
        let service = self.service.clone();
        let rt = self.runtime.clone();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move {
                    let reply = service
                        .execute(Command::FamilyImage {
                            account,
                            image: id,
                            size: FamilyImageSize::Square,
                        })
                        .await?;
                    match reply {
                        Reply::Asset(asset) => service.execute(Command::Thumbnail(asset)).await,
                        Reply::AppUpdate(_)
                        | Reply::Updated
                        | Reply::Saved(_)
                        | Reply::OpenedPlaylist(_)
                        | Reply::PlaylistOperation { .. }
                        | Reply::Entries(_)
                        | Reply::Waveform(_)
                        | Reply::Prepared(_)
                        | Reply::PixelArt(_)
                        | Reply::Image(_)
                        | Reply::Images(_)
                        | Reply::Player(_)
                        | Reply::Data(_) => {
                            Err(Failure::Compatibility("Image reply missing".into()))
                        }
                    }
                })
                .await;
            let _ = weak.update(cx, |v, cx| {
                if v.image.as_ref() == Some(&selected) {
                    match result {
                        Ok(Ok(Reply::PixelArt(art))) => v.preview = Some(*art),
                        Ok(Ok(
                            Reply::AppUpdate(_)
                            | Reply::Updated
                            | Reply::Saved(_)
                            | Reply::OpenedPlaylist(_)
                            | Reply::PlaylistOperation { .. }
                            | Reply::Entries(_)
                            | Reply::Waveform(_)
                            | Reply::Prepared(_)
                            | Reply::Asset(_)
                            | Reply::Image(_)
                            | Reply::Images(_)
                            | Reply::Player(_)
                            | Reply::Data(_),
                        )) => v.status = "Aperçu indisponible.".into(),
                        Ok(Err(e)) => v.status = failure_text(&e),
                        Err(e) => v.status = failure_text(&Failure::Storage(e.to_string())),
                    }
                    cx.notify();
                }
            });
        }));
    }
    fn upload(&mut self, cx: &mut Context<Self>) {
        let account = self.account.clone();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            if let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("Image", &["png", "jpg", "jpeg", "gif"])
                .pick_file()
                .await
            {
                let _ = weak.update(cx, |v, cx| {
                    v.send(
                        Command::UploadFamilyImage {
                            account,
                            path: file.path().to_owned(),
                        },
                        cx,
                    )
                });
            }
        }));
    }
}
impl Render for GroupEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div()
            .p_4()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .flex()
            .flex_col()
            .gap_3()
            .child(Input::new(&self.name));
        body = body.child(
            gpui_kit::uniform_list(
                "group-available",
                self.available.len(),
                cx.processor(
                    |v: &mut Self, range: std::ops::Range<usize>, _, cx: &mut Context<Self>| {
                        range
                            .map(|index| {
                                let card = &v.available[index];
                                let id = card.card.clone();
                                Checkbox::new(element_id("group-card", id.to_string()))
                                    .label(card.title.clone())
                                    .checked(v.cards.contains(&id))
                                    .on_click(cx.listener(move |v, checked, _, cx| {
                                        if *checked {
                                            if !v.cards.contains(&id) {
                                                v.cards.push(id.clone());
                                            }
                                        } else {
                                            v.cards.retain(|card| card != &id);
                                        }
                                        cx.notify();
                                    }))
                                    .into_any_element()
                            })
                            .collect()
                    },
                ),
            )
            .h(px(180.)),
        );
        body = body.child(
            gpui_kit::uniform_list(
                "group-order",
                self.cards.len(),
                cx.processor(
                    |v: &mut Self, range: std::ops::Range<usize>, _, cx: &mut Context<Self>| {
                        range
                            .map(|index| {
                                let id = v.cards[index].clone();
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(format!("{} · {}", index + 1, id))
                                    .child(
                                        Button::new(element_id("group-up", index.to_string()))
                                            .label("↑")
                                            .disabled(index == 0)
                                            .on_click(cx.listener(move |v, _, _, cx| {
                                                v.cards.swap(index, index - 1);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new(element_id("group-down", index.to_string()))
                                            .label("↓")
                                            .disabled(index + 1 >= v.cards.len())
                                            .on_click(cx.listener(move |v, _, _, cx| {
                                                v.cards.swap(index, index + 1);
                                                cx.notify();
                                            })),
                                    )
                                    .into_any_element()
                            })
                            .collect()
                    },
                ),
            )
            .h(px(160.)),
        );
        body = body.child(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("family-image-upload")
                        .label("Ajouter une image au groupe")
                        .on_click(cx.listener(|v, _, _, cx| v.upload(cx))),
                )
                .child(
                    Button::new("family-image-list")
                        .label("Images de la famille")
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.send(Command::FamilyImages(v.account.clone()), cx)
                        })),
                )
                .child(
                    Button::new("group-clear-image")
                        .label("Retirer l’image")
                        .disabled(self.id.is_some())
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.image = None;
                            v.preview = None;
                            cx.notify();
                        })),
                ),
        );
        body =
            body.child(
                gpui_kit::uniform_list(
                    "group-images",
                    self.images.len(),
                    cx.processor(
                        |v: &mut Self, range: std::ops::Range<usize>, _, cx: &mut Context<Self>| {
                            range
                                .map(|index| {
                                    let id = v.images[index].clone();
                                    Button::new(element_id("family-image", id.to_string()))
                                        .label(format!("Image {}", index + 1))
                                        .small()
                                        .selected(v.image.as_ref() == Some(&id))
                                        .on_click(cx.listener(move |v, _, _, cx| {
                                            v.load_image(id.clone(), cx)
                                        }))
                                        .into_any_element()
                                })
                                .collect()
                        },
                    ),
                )
                .h(px(100.)),
            );
        if let Some(art) = &self.preview {
            body = body.child(pixelart::thumbnail(art, 4., cx));
        }
        body.child(
            Button::new("save-group-draft")
                .label("Enregistrer le groupe")
                .disabled(self.saving)
                .primary()
                .on_click(cx.listener(|v, _, _, cx| {
                    v.send(
                        Command::SaveGroup {
                            account: v.account.clone(),
                            id: v.id.clone(),
                            name: v.name.read(cx).value().to_string(),
                            cards: v.cards.to_vec(),
                            image: v.image.clone(),
                        },
                        cx,
                    )
                })),
        )
        .child(self.status.clone())
    }
}
