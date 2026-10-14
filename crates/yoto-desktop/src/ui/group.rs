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
    /// Pixel previews of the family images, loaded once each.
    previews: BTreeMap<FamilyImageId, PixelArt>,
    service: Arc<Service>,
    runtime: Arc<tokio::runtime::Runtime>,
    tasks: Vec<Task<()>>,
    status: String,
    saving: bool,
}
/// What the group editor tells the screen that holds it.
pub enum GroupEditorEvent {
    /// A new group was created on Yoto; the editor now edits that group.
    Created(GroupId),
    /// The parent closed the editor.
    Close,
}
impl EventEmitter<GroupEditorEvent> for GroupEditor {}
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
            previews: BTreeMap::new(),
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
                let mut created = None;
                let result = result.map(|r| {
                    r.map(|(reply, refresh_error)| {
                        if writing
                            && let Reply::Data(data) = &reply
                            && let Some(id) = data
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .and_then(|id| GroupId::new(id).ok())
                        {
                            if v.id.is_none() {
                                created = Some(id.clone());
                            }
                            v.id = Some(id);
                        }
                        refresh_failure = refresh_error;
                        reply
                    })
                });
                match result {
                    Ok(Ok(Reply::Images(images))) => {
                        for id in images.iter().take(MAX_PREVIEWS) {
                            v.load_preview(id.clone(), cx);
                        }
                        v.status = if images.is_empty() {
                            "Aucune image pour l’instant : ajoutez-en une depuis votre ordinateur."
                                .into()
                        } else {
                            "Choisissez l’image du groupe.".into()
                        };
                        v.images = images;
                    }
                    Ok(Ok(Reply::Image(id))) => {
                        if !v.images.contains(&id) {
                            v.images.insert(0, id.clone());
                        }
                        v.select_image(id, cx);
                        v.status = "Image ajoutée à votre famille.".into();
                    }
                    Ok(Ok(
                        Reply::Data(_)
                        | Reply::AppUpdate(_)
                        | Reply::Updated
                        | Reply::Saved(_)
                        | Reply::OpenedPlaylist(_)
                        | Reply::PlaylistOperation { .. },
                    )) => v.status = "Groupe enregistré sur Yoto.".into(),
                    Ok(Ok(
                        Reply::Entries(_)
                        | Reply::Waveform(_)
                        | Reply::Prepared(_)
                        | Reply::Asset(_)
                        | Reply::PixelArt(_)
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
                if let Some(id) = created {
                    cx.emit(GroupEditorEvent::Created(id));
                }
                cx.notify();
            });
        }));
    }
    /// Shows the current image of a group being edited.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.image.clone() {
            self.load_preview(id, cx);
        }
    }
    fn select_image(&mut self, id: FamilyImageId, cx: &mut Context<Self>) {
        self.image = Some(id.clone());
        self.load_preview(id, cx);
        cx.notify();
    }
    fn load_preview(&mut self, id: FamilyImageId, cx: &mut Context<Self>) {
        if self.previews.contains_key(&id) {
            return;
        }
        let image = id.clone();
        let account = self.account.clone();
        let service = self.service.clone();
        let rt = self.runtime.clone();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move {
                    let asset = service
                        .execute(Command::FamilyImage {
                            account,
                            image,
                            size: FamilyImageSize::Square,
                        })
                        .await?
                        .into_asset()?;
                    service
                        .execute(Command::Thumbnail(asset))
                        .await?
                        .into_pixel_art()
                })
                .await;
            let _ = weak.update(cx, |v, cx| {
                // A missing preview only shows the image's number; it is not worth an error.
                if let Ok(art) = joined(result) {
                    v.previews.insert(id, art);
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
        use theme::{Height, Look};
        let section = |title: &'static str| {
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::EXTRA_BOLD)
                .child(title)
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(theme::icon_tile(IconName::Users, &theme::PEACH, 36., 11.))
            .child(div().flex_1().child(theme::heading(
                if self.id.is_some() {
                    "Modifier le groupe"
                } else {
                    "Nouveau groupe"
                },
                18.,
            )))
            .child(theme::round(
                Button::new("close-group-editor")
                    .icon(IconName::X)
                    .accessibility_label("Fermer l’éditeur de groupe")
                    .tooltip("Fermer")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(GroupEditorEvent::Close))),
                Look::Soft(&theme::LIGHT_SAND),
                36.,
                false,
                cx,
            ));
        let available = gpui_kit::uniform_list(
            "group-available",
            self.available.len(),
            cx.processor(
                |v: &mut Self, range: std::ops::Range<usize>, _, cx: &mut Context<Self>| {
                    range
                        .map(|index| {
                            let card = &v.available[index];
                            let id = card.card.clone();
                            div()
                                .h(px(36.))
                                .flex()
                                .items_center()
                                .child(
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
                                        })),
                                )
                                .into_any_element()
                        })
                        .collect()
                },
            ),
        )
        .h(px(180.));
        let mut order = div().flex().flex_col().gap(px(6.));
        for (index, id) in self.cards.iter().enumerate() {
            let title = self
                .available
                .iter()
                .find(|card| &card.card == id)
                .map_or_else(|| id.to_string(), |card| card.title.clone());
            let last = index + 1 >= self.cards.len();
            order = order.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .h(px(46.))
                    .rounded(px(14.))
                    .bg(theme::c(theme::SURFACE))
                    .child(
                        div()
                            .w(px(22.))
                            .text_center()
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .text_color(theme::c(theme::NUMBER))
                            .child((index + 1).to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::BOLD)
                            .child(title.clone()),
                    )
                    .child(theme::round(
                        Button::new(element_id("group-up", index))
                            .icon(IconName::ArrowUp)
                            .accessibility_label(format!("Monter « {title} »"))
                            .tooltip("Monter")
                            .on_click(cx.listener(move |v, _, _, cx| {
                                v.cards.swap(index, index - 1);
                                cx.notify();
                            })),
                        Look::Ghost,
                        32.,
                        index == 0,
                        cx,
                    ))
                    .child(theme::round(
                        Button::new(element_id("group-down", index))
                            .icon(IconName::ArrowDown)
                            .accessibility_label(format!("Descendre « {title} »"))
                            .tooltip("Descendre")
                            .on_click(cx.listener(move |v, _, _, cx| {
                                v.cards.swap(index, index + 1);
                                cx.notify();
                            })),
                        Look::Ghost,
                        32.,
                        last,
                        cx,
                    )),
            );
        }
        if self.cards.is_empty() {
            order = order.child(theme::note(
                "Cochez les playlists à mettre dans le groupe ; vous pourrez ensuite les ordonner ici.",
            ));
        }
        let mut images = div().flex().flex_wrap().gap(px(8.));
        for (index, id) in self.images.iter().enumerate() {
            let selected = self.image.as_ref() == Some(id);
            let choose = id.clone();
            let mut tile = div()
                .id(element_id("family-image", id.to_string()))
                .size(px(72.))
                .rounded(px(14.))
                .overflow_hidden()
                .bg(theme::c(theme::SAND))
                .border_3()
                .border_color(theme::c(if selected {
                    theme::PEACH.accent
                } else {
                    theme::SURFACE
                }))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_click(cx.listener(move |v, _, _, cx| v.select_image(choose.clone(), cx)));
            tile = match self.previews.get(id) {
                Some(art) => tile.child(pixelart::thumbnail(art, 4., cx)),
                None => tile.child(theme::meta(format!("Image {}", index + 1))),
            };
            images = images.child(tile);
        }
        let selected_preview = self.image.as_ref().and_then(|id| self.previews.get(id));
        let mut image_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(theme::pill(
                Button::new("family-image-upload")
                    .icon(IconName::Upload)
                    .label("Ajouter une image")
                    .on_click(cx.listener(|v, _, _, cx| v.upload(cx))),
                Look::Soft(&theme::PEACH),
                Height::Small,
                false,
                cx,
            ))
            .child(theme::pill(
                Button::new("family-image-list")
                    .label("Choisir parmi vos images")
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.send(Command::FamilyImages(v.account.clone()), cx)
                    })),
                Look::Outline,
                Height::Small,
                false,
                cx,
            ));
        // Yoto cannot remove the image of an existing group, so the action only exists
        // while creating one.
        if self.id.is_none() && self.image.is_some() {
            image_row = image_row.child(theme::pill(
                Button::new("group-clear-image")
                    .label("Retirer l’image")
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.image = None;
                        cx.notify();
                    })),
                Look::Link(&theme::ROSE),
                Height::Small,
                false,
                cx,
            ));
        }
        theme::inset()
            .gap(px(14.))
            .child(header)
            .child(section("Nom du groupe"))
            .child(theme::small_field(Input::new(&self.name)))
            .child(section("Playlists du groupe"))
            .child(available)
            .child(section("Ordre dans le groupe"))
            .child(order)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().flex_1().child(section("Image du groupe")))
                    .children(selected_preview.map(|art| pixelart::thumbnail(art, 3., cx))),
            )
            .child(image_row)
            .children((!self.images.is_empty()).then_some(images))
            .children((!self.status.is_empty()).then(|| theme::note(self.status.clone())))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(theme::pill(
                        Button::new("cancel-group-draft")
                            .label("Fermer")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(GroupEditorEvent::Close))),
                        Look::Outline,
                        Height::Medium,
                        false,
                        cx,
                    ))
                    .child(theme::pill(
                        Button::new("save-group-draft")
                            .label(if self.saving {
                                "Enregistrement…"
                            } else {
                                "Enregistrer le groupe"
                            })
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
                        Look::Primary,
                        Height::Medium,
                        self.saving,
                        cx,
                    )),
            )
    }
}

/// Family images previewed at once when the list opens; the rest show their number.
const MAX_PREVIEWS: usize = 24;
