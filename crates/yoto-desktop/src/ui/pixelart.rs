use super::*;
use gpui_kit::component::{dialog::DialogClose, spinner::Spinner, tab::TabBar};
use yoto_infra::api::IconCollection;
struct IconChoice {
    media: YotoMediaId,
    title: String,
    url: Option<SharedString>,
}
/// Catalogue labels and image URLs projected once by the snapshot worker.
pub(crate) struct IconChoices {
    public: Vec<IconChoice>,
    personal: Vec<IconChoice>,
    urls: BTreeMap<YotoMediaId, SharedString>,
}
pub(super) fn prepare_icon_choices(snapshot: &Snapshot) -> Arc<IconChoices> {
    let mut choices = IconChoices {
        public: vec![],
        personal: vec![],
        urls: BTreeMap::new(),
    };
    for icon in &snapshot.icons {
        if let Some(url) = &icon.url {
            choices.urls.insert(icon.media.clone(), url.clone().into());
        }
        let row = IconChoice {
            media: icon.media.clone(),
            title: icon.title.clone(),
            url: icon.url.clone().map(Into::into),
        };
        match icon.collection {
            IconCollection::Public => choices.public.push(row),
            IconCollection::User => choices.personal.push(row),
        }
    }
    Arc::new(choices)
}
impl IconChoices {
    pub(super) fn url(&self, media: &YotoMediaId) -> Option<&SharedString> {
        self.urls.get(media)
    }
    fn rows(&self, collection: IconCollection) -> &[IconChoice] {
        match collection {
            IconCollection::Public => &self.public,
            IconCollection::User => &self.personal,
        }
    }
}
pub(super) struct IconPicker {
    owner: WeakEntity<editor::PlaylistEditor>,
    account: AccountId,
    chapter: ChapterId,
    track: Option<TrackId>,
    choices: Arc<IconChoices>,
    collection: IconCollection,
    selected: Option<YotoMediaId>,
    uploading: bool,
    sync: Option<RemoteSyncState>,
    status: String,
    tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}
impl IconPicker {
    pub(super) fn new(
        owner: Entity<editor::PlaylistEditor>,
        parent: Entity<AppView>,
        account: AccountId,
        chapter: ChapterId,
        track: Option<TrackId>,
        cx: &mut Context<Self>,
    ) -> Self {
        let choices = parent.read(cx).playlist_icon_choices.clone();
        Self {
            owner: owner.downgrade(),
            account,
            chapter,
            track,
            choices,
            collection: IconCollection::Public,
            selected: None,
            uploading: false,
            sync: Some(RemoteSyncState::Syncing),
            status: String::new(),
            tasks: vec![],
            _subscriptions: vec![
                cx.observe(&owner, |_, _, cx| cx.notify()),
                cx.observe(&parent, |v, parent, cx| {
                    let parent = parent.read(cx);
                    v.choices = parent.playlist_icon_choices.clone();
                    v.sync = parent
                        .snapshot
                        .remote_sync
                        .get(&RemoteCollection::Icons)
                        .cloned();
                    cx.notify();
                }),
            ],
        }
    }
    pub(super) fn can_cancel(&self, cx: &App) -> bool {
        !self.uploading && self.owner.upgrade().is_none_or(|v| !v.read(cx).busy())
    }
    fn import_image(&mut self, cx: &mut Context<Self>) {
        if self.uploading {
            return;
        }
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        if owner.read(cx).blocked() {
            return;
        }
        let service = owner.read(cx).service.clone();
        let runtime = owner.read(cx).runtime.clone();
        self.uploading = true;
        self.status = "Choisissez une image…".into();
        cx.notify();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("Image", &["png", "jpg", "jpeg", "gif"])
                .pick_file()
                .await
            else {
                let _ = weak.update(cx, |v, cx| {
                    v.uploading = false;
                    v.status.clear();
                    cx.notify();
                });
                return;
            };
            let _ = weak.update(cx, |v, cx| {
                v.status = "Préparation de l’image…".into();
                cx.notify();
            });
            let cleanup_service = service.clone();
            let result = runtime
                .spawn(async move {
                    service
                        .execute(Command::StoreImage {
                            path: file.path().to_owned(),
                            pixel_art: true,
                        })
                        .await
                })
                .await;
            let staged = match result {
                Ok(Ok(Reply::Asset(asset))) => Ok(asset),
                Ok(Ok(
                    Reply::Updated
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
                    | Reply::Data(_),
                )) => Err("Résultat d’image inattendu.".into()),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let asset = staged.as_ref().ok().copied();
            let accepted = weak
                .update(cx, |v, cx| {
                    v.uploading = false;
                    let accepted = match staged {
                        Ok(asset) => {
                            let account = v.account.clone();
                            let chapter = v.chapter;
                            let track = v.track;
                            v.owner
                                .update(cx, |owner, cx| {
                                    owner.choose_icon(
                                        &account,
                                        chapter,
                                        track,
                                        IconReference::Local { asset },
                                        cx,
                                    )
                                })
                                .is_ok()
                        }
                        Err(error) => {
                            v.status = error;
                            false
                        }
                    };
                    cx.notify();
                    accepted
                })
                .unwrap_or(false);
            // A closed picker or replaced account must not leave an unattached image pinned.
            if !accepted && let Some(asset) = asset {
                runtime.spawn(async move {
                    let _ = cleanup_service.execute(Command::ReleaseAsset(asset)).await;
                });
            }
        }));
    }
}
impl Render for IconPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (busy, blocked, status) = self
            .owner
            .upgrade()
            .map(|owner| {
                let owner = owner.read(cx);
                (owner.busy(), owner.blocked(), owner.status.clone())
            })
            .unwrap_or((false, true, String::new()));
        let disabled = blocked || self.uploading;
        const COLUMNS: usize = 4;
        let rows = gpui_kit::uniform_list(
            "icon-catalogue",
            self.choices.rows(self.collection).len().div_ceil(COLUMNS),
            cx.processor(
                |v: &mut Self, range: std::ops::Range<usize>, _, cx: &mut Context<Self>| {
                    range
                        .map(|row| {
                            let disabled = v.uploading
                                || v.owner
                                    .upgrade()
                                    .is_none_or(|owner| owner.read(cx).blocked());
                            let mut cells = div().h(px(112.)).flex().gap_2().items_center();
                            for icon in v
                                .choices
                                .rows(v.collection)
                                .iter()
                                .skip(row * COLUMNS)
                                .take(COLUMNS)
                            {
                                let media = icon.media.clone();
                                cells = cells.child(
                                    Button::new(element_id("choose-icon", media.to_string()))
                                        .w(px(128.))
                                        .h(px(104.))
                                        .selected(v.selected.as_ref() == Some(&media))
                                        .disabled(disabled)
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap_2()
                                                .items_center()
                                                .children(icon.url.as_ref().map(|url| {
                                                    img(url.clone())
                                                        .w(px(48.))
                                                        .h(px(48.))
                                                        .object_fit(ObjectFit::Contain)
                                                }))
                                                .children(
                                                    icon.url
                                                        .is_none()
                                                        .then(|| div().h(px(48.)).child("◇")),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(108.))
                                                        .text_sm()
                                                        .truncate()
                                                        .child(icon.title.clone()),
                                                ),
                                        )
                                        .on_click(cx.listener(move |v, _, _, cx| {
                                            v.selected = Some(media.clone());
                                            cx.notify();
                                        })),
                                );
                            }
                            cells
                        })
                        .collect()
                },
            ),
        )
        .h(px(336.));
        let loading = matches!(self.sync, Some(RemoteSyncState::Syncing));
        let failure = match &self.sync {
            Some(RemoteSyncState::Failed { failure }) => Some(failure.to_string()),
            Some(RemoteSyncState::Syncing | RemoteSyncState::Ready { .. }) | None => None,
        };
        let mut body = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                TabBar::new("icon-collections")
                    .segmented()
                    .child("Yoto")
                    .child("Mes icônes")
                    .selected_index(match self.collection {
                        IconCollection::Public => 0,
                        IconCollection::User => 1,
                    })
                    .on_click(cx.listener(|v, index, _, cx| {
                        v.collection = match index {
                            0 => IconCollection::Public,
                            1 => IconCollection::User,
                            _ => unreachable!("Kit emits a valid tab index"),
                        };
                        v.selected = None;
                        cx.notify();
                    })),
            )
            .child(rows);
        if loading {
            body = body.child(
                div()
                    .flex()
                    .gap_2()
                    .child(Spinner::new().small())
                    .child("Chargement des icônes…"),
            );
        }
        if let Some(failure) = &failure {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(format!("Chargement impossible : {failure}"))
                    .child(
                        Button::new("retry-icons")
                            .label("Réessayer")
                            .disabled(disabled)
                            .on_click(cx.listener(|v, _, _, cx| {
                                let account = v.account.clone();
                                let _ = v.owner.update(cx, |owner, cx| {
                                    if owner
                                        .owning_remote(cx)
                                        .is_ok_and(|(active, _)| active == account)
                                    {
                                        owner.send(Command::RefreshIcons(account), cx);
                                    }
                                });
                            })),
                    ),
            );
        }
        if !loading && failure.is_none() && self.choices.rows(self.collection).is_empty() {
            body = body.child(
                div()
                    .text_sm()
                    .child("Aucune icône dans cette collection. Vous pouvez importer une image."),
            );
        }
        body = body.child(div().text_sm().text_color(cx.theme().muted_foreground)
            .child("Choisissez une icône Yoto ou ajoutez votre image. Elle sera adaptée au lecteur."))
            .child(div().flex().gap_2().items_center()
                .child(Button::new("import-icon-image").label("Importer une image").disabled(disabled)
                    .on_click(cx.listener(|v, _, _, cx| v.import_image(cx))))
                .child(div().flex_1())
                .child(DialogClose::new().child(Button::new("cancel-icon").label("Annuler").disabled(busy || self.uploading)))
                .child(Button::new("apply-icon").label("Utiliser cette icône").primary()
                    .disabled(disabled || self.selected.is_none())
                    .on_click(cx.listener(|v, _, _, cx| {
                        if let Some(media) = &v.selected {
                            let icon = IconReference::Yoto { account: v.account.clone(), media: media.clone() };
                            let account = v.account.clone(); let chapter = v.chapter; let track = v.track;
                            let _ = v.owner.update(cx, |owner, cx| owner.choose_icon(&account, chapter, track, icon, cx));
                        }
                    }))));
        if busy || self.uploading {
            body = body.child(div().flex().gap_2().child(Spinner::new().small()).child(
                if self.uploading {
                    self.status.clone()
                } else {
                    status.clone()
                },
            ));
        }
        if !busy && !self.uploading && !self.status.is_empty() {
            body = body.child(div().text_sm().child(self.status.clone()));
        }
        if !busy && blocked {
            body = body.child(div().text_sm().child(status));
        }
        body
    }
}

pub fn thumbnail(art: &PixelArt, scale: f32, cx: &App) -> AnyElement {
    let mut image = div()
        .flex()
        .flex_col()
        .w(px(16. * scale))
        .h(px(16. * scale));
    for row in &art.pixels {
        let mut line = div().flex();
        for pixel in row {
            let color = if pixel[3] == 0 {
                cx.theme().muted
            } else {
                rgba(
                    (u32::from(pixel[0]) << 24)
                        | (u32::from(pixel[1]) << 16)
                        | (u32::from(pixel[2]) << 8)
                        | u32::from(pixel[3]),
                )
                .into()
            };
            line = line.child(div().w(px(scale)).h(px(scale)).bg(color));
        }
        image = image.child(line);
    }
    image.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::prepare_icon_choices;
    use yoto_domain::YotoMediaId;
    use yoto_infra::{
        Snapshot,
        api::{Icon, IconCollection},
    };
    #[test]
    fn icon_tabs_use_api_collection_not_names_or_overlapping_ids() {
        let media = YotoMediaId::new("identical-id").unwrap();
        let snapshot = Snapshot {
            icons: vec![
                Icon {
                    media: media.clone(),
                    title: "Mes icônes".into(),
                    url: Some("https://example.test/icon.png".into()),
                    collection: IconCollection::Public,
                },
                Icon {
                    media: media.clone(),
                    title: "Yoto".into(),
                    url: Some("https://example.test/icon.png".into()),
                    collection: IconCollection::User,
                },
            ],
            ..Default::default()
        };
        let choices = prepare_icon_choices(&snapshot);
        assert_eq!(choices.rows(IconCollection::Public).len(), 1);
        assert_eq!(choices.rows(IconCollection::Public)[0].title, "Mes icônes");
        assert_eq!(choices.rows(IconCollection::User).len(), 1);
        assert_eq!(choices.rows(IconCollection::User)[0].title, "Yoto");
        assert_eq!(
            choices.url(&media).unwrap().as_ref(),
            "https://example.test/icon.png"
        );
    }
}
