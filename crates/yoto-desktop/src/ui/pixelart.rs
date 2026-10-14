use super::*;
use gpui_kit::component::{
    button::{ButtonCustomVariant, ButtonRounded},
    spinner::Spinner,
};
use theme::{Height, Look};
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
/// What a chosen icon is for: a chapter or track of an open playlist, or a media of the
/// library, which passes it on to the chapters it becomes.
pub(super) enum IconTarget {
    Playlist {
        editor: WeakEntity<editor::PlaylistEditor>,
        chapter: ChapterId,
        track: Option<TrackId>,
    },
    Media(WeakEntity<trim::TrimPanel>),
}
pub(super) struct IconPicker {
    target: IconTarget,
    /// The account whose Yoto icons are offered; without one, only an image can be imported.
    account: Option<AccountId>,
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
        target: IconTarget,
        parent: Entity<AppView>,
        account: Option<AccountId>,
        cx: &mut Context<Self>,
    ) -> Self {
        let choices = parent.read(cx).playlist_icon_choices.clone();
        let mut subscriptions = vec![cx.observe(&parent, |v, parent, cx| {
            let parent = parent.read(cx);
            v.choices = parent.playlist_icon_choices.clone();
            if v.account.is_some() {
                v.sync = parent
                    .snapshot
                    .remote_sync
                    .get(&RemoteCollection::Icons)
                    .cloned();
            }
            cx.notify();
        })];
        match &target {
            IconTarget::Playlist { editor, .. } => {
                if let Some(editor) = editor.upgrade() {
                    subscriptions.push(cx.observe(&editor, |_, _, cx| cx.notify()));
                }
            }
            IconTarget::Media(panel) => {
                if let Some(panel) = panel.upgrade() {
                    subscriptions.push(cx.observe(&panel, |_, _, cx| cx.notify()));
                }
            }
        }
        Self {
            sync: account.as_ref().map(|_| RemoteSyncState::Syncing),
            target,
            account,
            choices,
            collection: IconCollection::Public,
            selected: None,
            uploading: false,
            status: String::new(),
            tasks: vec![],
            _subscriptions: subscriptions,
        }
    }
    pub(super) fn can_cancel(&self, cx: &App) -> bool {
        !self.uploading && !self.owner_state(cx).0
    }
    /// Whether the owner is busy, whether it refuses changes, and its status line.
    fn owner_state(&self, cx: &App) -> (bool, bool, String) {
        match &self.target {
            IconTarget::Playlist { editor, .. } => editor.upgrade().map(|owner| {
                let owner = owner.read(cx);
                (owner.busy(), owner.blocked(), owner.status.clone())
            }),
            IconTarget::Media(panel) => panel.upgrade().map(|panel| {
                let panel = panel.read(cx);
                (panel.saving_icon(), false, panel.status().to_owned())
            }),
        }
        .unwrap_or((false, true, String::new()))
    }
    fn backend(&self, cx: &App) -> Option<(Arc<Service>, Arc<tokio::runtime::Runtime>)> {
        match &self.target {
            IconTarget::Playlist { editor, .. } => editor.upgrade().map(|owner| {
                let owner = owner.read(cx);
                (owner.service.clone(), owner.runtime.clone())
            }),
            IconTarget::Media(panel) => panel.upgrade().map(|panel| panel.read(cx).backend()),
        }
    }
    /// Hand the icon to its owner; false when the owner is gone and did not take it.
    fn choose(&self, icon: IconReference, cx: &mut Context<Self>) -> bool {
        match &self.target {
            IconTarget::Playlist {
                editor,
                chapter,
                track,
            } => {
                let Some(account) = self.account.clone() else {
                    return false;
                };
                let (chapter, track) = (*chapter, *track);
                editor
                    .update(cx, |owner, cx| {
                        owner.choose_icon(&account, chapter, track, icon, cx)
                    })
                    .is_ok()
            }
            IconTarget::Media(panel) => panel
                .update(cx, |panel, cx| panel.choose_icon(icon, cx))
                .is_ok(),
        }
    }
    fn refresh(&self, cx: &mut Context<Self>) {
        let Some(account) = self.account.clone() else {
            return;
        };
        match &self.target {
            IconTarget::Playlist { editor, .. } => {
                let _ = editor.update(cx, |owner, cx| {
                    if owner
                        .owning_remote(cx)
                        .is_ok_and(|(active, _)| active == account)
                    {
                        owner.send(Command::RefreshIcons(account), cx);
                    }
                });
            }
            IconTarget::Media(_) => {
                if let Some((service, runtime)) = self.backend(cx) {
                    runtime.spawn(async move {
                        let _ = service.execute(Command::RefreshIcons(account)).await;
                    });
                }
            }
        }
    }
    fn import_image(&mut self, cx: &mut Context<Self>) {
        if self.uploading {
            return;
        }
        let (_, blocked, _) = self.owner_state(cx);
        let Some((service, runtime)) = self.backend(cx) else {
            return;
        };
        if blocked {
            return;
        }
        self.uploading = true;
        self.status = tr!("Choisissez une image…").into();
        cx.notify();
        self.tasks.push(cx.spawn(async move |weak, cx| {
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter(tr!("Image"), &["png", "jpg", "jpeg", "gif"])
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
                v.status = tr!("Préparation de l’image…").into();
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
            let staged = joined(result)
                .and_then(Reply::into_asset)
                .map_err(|error| failure_text(&error));
            let asset = staged.as_ref().ok().copied();
            let accepted = weak
                .update(cx, |v, cx| {
                    v.uploading = false;
                    let accepted = match staged {
                        Ok(asset) => v.choose(IconReference::Local { asset }, cx),
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
        let (busy, blocked, status) = self.owner_state(cx);
        let disabled = blocked || self.uploading;
        const COLUMNS: usize = 6;
        let rows = gpui_kit::uniform_list(
            "icon-catalogue",
            self.choices.rows(self.collection).len().div_ceil(COLUMNS),
            cx.processor(
                |v: &mut Self, range: std::ops::Range<usize>, _, cx: &mut Context<Self>| {
                    range
                        .map(|row| {
                            let disabled = v.uploading || v.owner_state(cx).1;
                            let mut cells = div().h(px(64.)).flex().gap(px(8.)).items_center();
                            for icon in v
                                .choices
                                .rows(v.collection)
                                .iter()
                                .skip(row * COLUMNS)
                                .take(COLUMNS)
                            {
                                let media = icon.media.clone();
                                let selected = v.selected.as_ref() == Some(&media);
                                let surface = theme::c(if selected {
                                    theme::LILAC.soft
                                } else {
                                    theme::CREAM
                                });
                                let mut cell =
                                    Button::new(element_id("choose-icon", media.to_string()))
                                        .custom(
                                            ButtonCustomVariant::new(cx)
                                                .color(surface)
                                                .hover(theme::c(theme::LILAC.soft))
                                                .active(theme::c(theme::LILAC.soft)),
                                        )
                                        .bg(surface)
                                        .rounded(ButtonRounded::Size(px(14.)))
                                        .size(px(56.))
                                        .toggled(selected)
                                        .accessibility_label(icon.title.clone())
                                        .tooltip(icon.title.clone())
                                        .disabled(disabled)
                                        .children(
                                            icon.url
                                                .as_ref()
                                                .map(|url| crisp_icon(url.clone(), 32.)),
                                        )
                                        .children(icon.url.is_none().then(|| {
                                            // Yoto gave no image: say so, never invent one.
                                            div().text_sm().child("◇")
                                        }))
                                        .on_click(cx.listener(move |v, _, _, cx| {
                                            v.selected = Some(media.clone());
                                            cx.notify();
                                        }));
                                if selected {
                                    cell =
                                        cell.border_2().border_color(theme::c(theme::LILAC.accent));
                                }
                                cells = cells.child(cell);
                            }
                            cells
                        })
                        .collect()
                },
            ),
        )
        .h(px(272.));
        let loading = matches!(self.sync, Some(RemoteSyncState::Syncing));
        let failure = match &self.sync {
            Some(RemoteSyncState::Failed { failure }) => Some(failure_text(failure)),
            Some(RemoteSyncState::Syncing | RemoteSyncState::Ready { .. }) | None => None,
        };
        let collections = [
            (IconCollection::Public, "Yoto"),
            (IconCollection::User, tr!("Mes icônes")),
        ]
        .map(|(collection, label)| {
            theme::segment(
                Button::new(element_id("icon-collection", label))
                    .label(label)
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.collection = collection;
                        v.selected = None;
                        cx.notify();
                    })),
                self.collection == collection,
                Height::Tiny,
                cx,
            )
        });
        let mut body = div().flex().flex_col().gap(px(14.));
        body = if self.account.is_some() {
            body.child(div().child(theme::segmented().children(collections)))
                .child(rows)
        } else {
            body.child(theme::note(tr!(
                "Connectez votre compte Yoto pour choisir parmi ses icônes. Vous pouvez aussi importer une image."
            )))
        };
        if loading {
            body = body.child(
                div()
                    .flex()
                    .gap_2()
                    .child(Spinner::new().small())
                    .child(tr!("Chargement des icônes…")),
            );
        }
        if let Some(failure) = &failure {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(tr!("Chargement impossible : {failure}", failure = failure))
                    .child(
                        Button::new("retry-icons")
                            .label(tr!("Réessayer"))
                            .disabled(disabled)
                            .outline()
                            .on_click(cx.listener(|v, _, _, cx| v.refresh(cx))),
                    ),
            );
        }
        if self.account.is_some()
            && !loading
            && failure.is_none()
            && self.choices.rows(self.collection).is_empty()
        {
            body = body.child(div().text_sm().child(tr!(
                "Aucune icône dans cette collection. Vous pouvez importer une image."
            )));
        }
        body = body
            .child(theme::note(
                tr!("Choisissez une icône Yoto ou ajoutez votre image : elle sera adaptée en 16 × 16 pour l’écran du lecteur."),
            ))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        theme::pill(
                            Button::new("import-icon-image")
                                .label(tr!("Importer une image"))
                                .on_click(cx.listener(|v, _, _, cx| v.import_image(cx))),
                            Look::Outline,
                            Height::Medium,
                            disabled,
                            cx,
                        )
                        .border_dashed()
                        .border_color(theme::c(theme::BOX_BORDER)),
                    )
                    .child(div().flex_1())
                    .child(theme::dialog_close(theme::pill(
                        Button::new("cancel-icon").label(tr!("Annuler")),
                        Look::Ghost,
                        Height::Medium,
                        busy || self.uploading,
                        cx,
                    )))
                    .child(theme::pill(
                        Button::new("apply-icon")
                            .label(tr!("Utiliser cette icône"))
                            .on_click(cx.listener(|v, _, _, cx| {
                                if let (Some(media), Some(account)) = (&v.selected, &v.account) {
                                    let icon = IconReference::Yoto {
                                        account: account.clone(),
                                        media: media.clone(),
                                    };
                                    v.choose(icon, cx);
                                }
                            })),
                        Look::Primary,
                        Height::Medium,
                        disabled || self.selected.is_none(),
                        cx,
                    )),
            );
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

/// Most a Yoto icon is enlarged before display: 16 pixels become at most 256.
const UPSCALE_MAX: u32 = 16;
/// Enlarged icons, by address and factor, kept so each is sent to the GPU only once.
type CrispIcons = std::collections::HashMap<(SharedString, u32), Arc<RenderImage>>;
static CRISP: std::sync::LazyLock<std::sync::Mutex<CrispIcons>> =
    std::sync::LazyLock::new(Default::default);

/// How many screen pixels each icon pixel needs to cover `size` points at `scale`.
fn upscale_factor(size: f32, scale: f32) -> u32 {
    let wanted = (size * scale / 16.).ceil();
    if wanted.is_finite() && wanted >= 1. {
        (wanted as u32).min(UPSCALE_MAX)
    } else {
        1
    }
}

/// A `width` × `height` image enlarged by whole pixels: every source pixel becomes a
/// `factor` × `factor` square of the same color. The channel order is kept as GPUI decoded it.
fn upscale(bytes: &[u8], width: u32, height: u32, factor: u32) -> Option<image::RgbaImage> {
    if bytes.len() != (width * height * 4) as usize {
        return None;
    }
    Some(image::RgbaImage::from_fn(
        width * factor,
        height * factor,
        |x, y| {
            let at = (((y / factor) * width + x / factor) * 4) as usize;
            image::Rgba([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        },
    ))
}

/// A Yoto icon, a 16 × 16 pixel drawing, shown `size` points wide with hard pixel edges.
/// GPUI samples images with linear smoothing, which turns a tiny bitmap stretched over many
/// screen pixels into a blur. The bitmap is first enlarged by whole pixels to about its size
/// on screen, so the smoothing only ever blends a pixel with copies of itself.
pub fn crisp_icon(url: SharedString, size: f32) -> Img {
    img(ImageSource::Custom(Arc::new(move |window, cx| {
        let factor = upscale_factor(size, window.scale_factor());
        let key = (url.clone(), factor);
        if let Some(ready) = CRISP.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
            return Some(Ok(ready));
        }
        let source = match window
            .use_asset::<ImgResourceLoader>(&Resource::Uri(SharedUri::from(url.clone())), cx)?
        {
            Ok(source) => source,
            Err(error) => return Some(Err(error)),
        };
        let dimensions = source.size(0);
        let (Ok(width), Ok(height)) = (
            u32::try_from(dimensions.width.0),
            u32::try_from(dimensions.height.0),
        ) else {
            return Some(Ok(source));
        };
        // Only pixel icons are enlarged; a larger picture is drawn as it is.
        if factor == 1 || width > 32 || height > 32 || source.frame_count() != 1 {
            return Some(Ok(source));
        }
        let Some(enlarged) = source
            .as_bytes(0)
            .and_then(|bytes| upscale(bytes, width, height, factor))
        else {
            return Some(Ok(source));
        };
        let ready = Arc::new(RenderImage::new(vec![image::Frame::new(enlarged)]));
        if let Ok(mut cache) = CRISP.lock() {
            cache.insert(key, ready.clone());
        }
        Some(Ok(ready))
    })))
    .size(px(size))
    .flex_shrink_0()
}

/// A chosen icon as this computer can draw it, `size` pixels wide: a local image once its
/// pixels are read, or a Yoto icon of the connected account. A Yoto icon this computer
/// has no image for (another account's, say) shows a small diamond: it is there, unseen.
pub(super) fn icon_image(
    icon: &IconReference,
    previews: &BTreeMap<AssetId, PixelArt>,
    parent: &AppView,
    size: f32,
    cx: &App,
) -> AnyElement {
    match icon {
        IconReference::Local { asset } => previews
            .get(asset)
            .map(|art| thumbnail(art, size / 16., cx))
            .unwrap_or_else(|| div().size(px(size)).into_any_element()),
        IconReference::Yoto { account, media } => {
            match parent
                .playlist_icon_choices
                .url(media)
                .filter(|_| parent.snapshot.active_account.as_ref() == Some(account))
            {
                Some(url) => crisp_icon(url.clone(), size).into_any_element(),
                None => div()
                    .size(px(size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .child("◇")
                    .into_any_element(),
            }
        }
    }
}
impl AppView {
    /// The icon of a media in a tile of `size`: the icon itself, or an empty slot.
    pub(super) fn media_icon_tile(
        &self,
        id: MediaId,
        icon: Option<&IconReference>,
        size: f32,
        radius: f32,
        cx: &App,
    ) -> Div {
        match icon {
            Some(icon) => theme::icon_frame(&id.to_string(), size, radius).child(icon_image(
                icon,
                &self.icon_previews,
                self,
                (size * 0.66).round(),
                cx,
            )),
            None => theme::empty_icon_slot(size, radius, false),
        }
    }
    /// Read the pixels of every local media icon not drawn yet.
    pub(super) fn load_media_icons(&mut self, cx: &mut Context<Self>) {
        let missing: Vec<AssetId> = self
            .snapshot
            .media
            .iter()
            .filter_map(|media| match &media.icon {
                Some(IconReference::Local { asset }) => Some(*asset),
                Some(IconReference::Yoto { .. }) | None => None,
            })
            .collect();
        for asset in missing {
            self.load_icon_preview(asset, cx);
        }
    }
    /// Read one local icon's pixels once; an icon that cannot be read is not asked again.
    pub(super) fn load_icon_preview(&mut self, asset: AssetId, cx: &mut Context<Self>) {
        if self.icon_previews.contains_key(&asset) || !self.icon_requests.insert(asset) {
            return;
        }
        let service = self.service.clone();
        let runtime = self.runtime.clone();
        self._tasks.push(cx.spawn(async move |weak, cx| {
            let result = runtime
                .spawn(async move { service.execute(Command::ReadPixelArt(asset)).await })
                .await;
            if let Ok(art) = joined(result).and_then(Reply::into_pixel_art) {
                let _ = weak.update(cx, |v, cx| {
                    v.icon_previews.insert(asset, art);
                    cx.notify();
                });
            }
        }));
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
    use super::{prepare_icon_choices, upscale, upscale_factor};
    #[test]
    fn icons_grow_by_whole_pixels_to_their_screen_size() {
        assert_eq!(upscale_factor(32., 2.), 4);
        assert_eq!(upscale_factor(32., 1.), 2);
        assert_eq!(upscale_factor(30., 2.), 4);
        assert_eq!(upscale_factor(4096., 2.), 16);
        assert_eq!(upscale_factor(0., 2.), 1);
        // Two pixels side by side keep hard edges: each becomes a block of its own color.
        let bytes = [1, 2, 3, 255, 9, 8, 7, 0];
        let big = upscale(&bytes, 2, 1, 3).unwrap();
        assert_eq!(big.dimensions(), (6, 3));
        for y in 0..3 {
            for x in 0..3 {
                assert_eq!(big.get_pixel(x, y).0, [1, 2, 3, 255]);
                assert_eq!(big.get_pixel(x + 3, y).0, [9, 8, 7, 0]);
            }
        }
        assert!(upscale(&bytes, 3, 1, 2).is_none());
    }
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
