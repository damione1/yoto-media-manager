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
    /// Images the parent already turned into icons, on media or playlists: kept on this
    /// computer, so they can be chosen again without a Yoto account.
    images: Vec<AssetId>,
}
pub(super) fn prepare_icon_choices(snapshot: &Snapshot) -> Arc<IconChoices> {
    let mut choices = IconChoices {
        public: vec![],
        personal: vec![],
        urls: BTreeMap::new(),
        images: vec![],
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
    let used =
        snapshot
            .media
            .iter()
            .map(|media| &media.icon)
            .chain(snapshot.playlists.iter().flat_map(|playlist| {
                playlist.chapters.iter().flat_map(|chapter| {
                    std::iter::once(&chapter.icon).chain(chapter.tracks.iter().map(|t| &t.icon))
                })
            }));
    let mut seen = std::collections::BTreeSet::new();
    for icon in used {
        match icon {
            Some(IconReference::Local { asset }) => {
                if seen.insert(*asset) {
                    choices.images.push(*asset);
                }
            }
            Some(IconReference::Yoto { .. }) | None => {}
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
    fn len(&self, shelf: Shelf) -> usize {
        match shelf {
            Shelf::Yoto(collection) => self.rows(collection).len(),
            Shelf::Images => self.images.len(),
        }
    }
}
/// Where the picker looks: a Yoto collection of the account, or the parent's own images.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shelf {
    Yoto(IconCollection),
    Images,
}
/// The icon selected in the picker, not chosen yet.
#[derive(Clone, PartialEq, Eq)]
enum Selection {
    Yoto(YotoMediaId),
    Image(AssetId),
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
    /// The account whose Yoto icons are offered; without one, the parent's own images are.
    account: Option<AccountId>,
    parent: WeakEntity<AppView>,
    choices: Arc<IconChoices>,
    shelf: Shelf,
    selected: Option<Selection>,
    uploading: bool,
    sync: Option<RemoteSyncState>,
    status: String,
    /// Why the last image could not be prepared; the picker stays open to try again.
    error: Option<String>,
    tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}
/// What the picker reads from the owner of the icon.
struct OwnerState {
    /// Saving the chosen icon.
    busy: bool,
    /// The owner refuses changes for now.
    blocked: bool,
    status: String,
    /// Why the chosen icon was not kept, shown in the picker rather than behind it.
    error: Option<String>,
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
        // The parent's images are drawn from their pixels, read once for the whole app.
        let images = choices.images.clone();
        let reader = parent.downgrade();
        cx.defer(move |cx| {
            let _ = reader.update(cx, |parent, cx| {
                for asset in images {
                    parent.load_icon_preview(asset, cx);
                }
            });
        });
        Self {
            sync: account.as_ref().map(|_| RemoteSyncState::Syncing),
            shelf: if account.is_some() {
                Shelf::Yoto(IconCollection::Public)
            } else {
                Shelf::Images
            },
            target,
            account,
            parent: parent.downgrade(),
            choices,
            selected: None,
            uploading: false,
            status: String::new(),
            error: None,
            tasks: vec![],
            _subscriptions: subscriptions,
        }
    }
    pub(super) fn can_cancel(&self, cx: &App) -> bool {
        !self.uploading && !self.owner_state(cx).busy
    }
    fn owner_state(&self, cx: &App) -> OwnerState {
        match &self.target {
            IconTarget::Playlist { editor, .. } => editor.upgrade().map(|owner| {
                let owner = owner.read(cx);
                OwnerState {
                    busy: owner.busy(),
                    blocked: owner.blocked(),
                    status: owner.status.clone(),
                    error: owner.icon_error().map(str::to_owned),
                }
            }),
            IconTarget::Media(panel) => panel.upgrade().map(|panel| {
                let panel = panel.read(cx);
                OwnerState {
                    busy: panel.saving_icon(),
                    blocked: false,
                    status: panel.status().to_owned(),
                    error: panel.icon_error().map(str::to_owned),
                }
            }),
        }
        .unwrap_or_else(|| OwnerState {
            busy: false,
            blocked: true,
            status: String::new(),
            error: Some(gone().into()),
        })
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
    fn choose(&mut self, icon: IconReference, cx: &mut Context<Self>) -> bool {
        self.error = None;
        let taken = match &self.target {
            IconTarget::Playlist {
                editor,
                chapter,
                track,
            } => match self.account.clone() {
                Some(account) => {
                    let (chapter, track) = (*chapter, *track);
                    editor
                        .update(cx, |owner, cx| {
                            owner.choose_icon(&account, chapter, track, icon, cx)
                        })
                        .is_ok()
                }
                None => false,
            },
            IconTarget::Media(panel) => panel
                .update(cx, |panel, cx| panel.choose_icon(icon, cx))
                .is_ok(),
        };
        if !taken {
            self.error = Some(gone().into());
        }
        cx.notify();
        taken
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
        let blocked = self.owner_state(cx).blocked;
        let Some((service, runtime)) = self.backend(cx) else {
            return;
        };
        if blocked {
            return;
        }
        self.uploading = true;
        self.error = None;
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
                    v.status.clear();
                    let accepted = match staged {
                        Ok(asset) => v.choose(IconReference::Local { asset }, cx),
                        Err(error) => {
                            v.error = Some(error);
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
    /// Keep the selected icon: a Yoto icon of the account, or one of the parent's images.
    fn apply(&mut self, cx: &mut Context<Self>) {
        let icon = match (&self.selected, &self.account) {
            (Some(Selection::Yoto(media)), Some(account)) => IconReference::Yoto {
                account: account.clone(),
                media: media.clone(),
            },
            (Some(Selection::Image(asset)), _) => IconReference::Local { asset: *asset },
            (Some(Selection::Yoto(_)), None) | (None, _) => return,
        };
        self.choose(icon, cx);
    }
    /// One cell of the grid: the icon drawn on a cream tile, lilac when selected.
    fn cell(
        &self,
        id: ElementId,
        choice: Selection,
        title: SharedString,
        picture: AnyElement,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let selected = self.selected.as_ref() == Some(&choice);
        let surface = theme::c(if selected {
            theme::LILAC.soft
        } else {
            theme::CREAM
        });
        let mut cell = Button::new(id)
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
            .accessibility_label(title.clone())
            .tooltip(title)
            .disabled(disabled)
            .child(picture)
            .on_click(cx.listener(move |v, _, _, cx| {
                v.selected = Some(choice.clone());
                cx.notify();
            }));
        if selected {
            cell = cell.border_2().border_color(theme::c(theme::LILAC.accent));
        }
        cell
    }
}
/// The playlist or media the picker was opened for has been closed.
fn gone() -> &'static str {
    tr!("Cet élément n’est plus ouvert. Fermez cette fenêtre et rouvrez-le.")
}
impl Render for IconPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = self.owner_state(cx);
        let (busy, blocked) = (owner.busy, owner.blocked);
        let disabled = blocked || self.uploading;
        const COLUMNS: usize = 6;
        let rows = gpui_kit::uniform_list(
            "icon-catalogue",
            self.choices.len(self.shelf).div_ceil(COLUMNS),
            cx.processor(
                |v: &mut Self, range: std::ops::Range<usize>, _, cx: &mut Context<Self>| {
                    let disabled = v.uploading || v.owner_state(cx).blocked;
                    let choices = v.choices.clone();
                    range
                        .map(|row| {
                            let mut cells = div().h(px(64.)).flex().gap(px(8.)).items_center();
                            match v.shelf {
                                Shelf::Yoto(collection) => {
                                    for icon in choices
                                        .rows(collection)
                                        .iter()
                                        .skip(row * COLUMNS)
                                        .take(COLUMNS)
                                    {
                                        let picture = match &icon.url {
                                            Some(url) => {
                                                crisp_icon(url.clone(), 32.).into_any_element()
                                            }
                                            // Yoto gave no image: say so, never invent one.
                                            None => div().text_sm().child("◇").into_any_element(),
                                        };
                                        cells = cells.child(v.cell(
                                            element_id("choose-icon", icon.media.to_string()),
                                            Selection::Yoto(icon.media.clone()),
                                            icon.title.clone().into(),
                                            picture,
                                            disabled,
                                            cx,
                                        ));
                                    }
                                }
                                Shelf::Images => {
                                    for asset in
                                        choices.images.iter().skip(row * COLUMNS).take(COLUMNS)
                                    {
                                        let picture = v
                                            .parent
                                            .upgrade()
                                            .and_then(|parent| {
                                                parent
                                                    .read(cx)
                                                    .icon_previews
                                                    .get(asset)
                                                    .map(|art| thumbnail(art, 2., cx))
                                            })
                                            .unwrap_or_else(|| {
                                                div().size(px(32.)).into_any_element()
                                            });
                                        cells = cells.child(v.cell(
                                            element_id("choose-image", asset.to_string()),
                                            Selection::Image(*asset),
                                            tr!("Une de vos images").into(),
                                            picture,
                                            disabled,
                                            cx,
                                        ));
                                    }
                                }
                            }
                            cells
                        })
                        .collect()
                },
            ),
        )
        .h(px(272.));
        let yoto = matches!(self.shelf, Shelf::Yoto(_));
        let loading = yoto && matches!(self.sync, Some(RemoteSyncState::Syncing));
        let failure = match &self.sync {
            Some(RemoteSyncState::Failed { failure }) if yoto => Some(failure_text(failure)),
            Some(
                RemoteSyncState::Failed { .. }
                | RemoteSyncState::Syncing
                | RemoteSyncState::Ready { .. },
            )
            | None => None,
        };
        let mut body = div().flex().flex_col().gap(px(14.));
        if self.account.is_some() {
            let shelves = [
                (Shelf::Yoto(IconCollection::Public), "Yoto"),
                (Shelf::Yoto(IconCollection::User), tr!("Mes icônes")),
                (Shelf::Images, tr!("Mes images")),
            ]
            .map(|(shelf, label)| {
                theme::segment(
                    Button::new(element_id("icon-collection", label))
                        .label(label)
                        .on_click(cx.listener(move |v, _, _, cx| {
                            v.shelf = shelf;
                            v.selected = None;
                            cx.notify();
                        })),
                    self.shelf == shelf,
                    Height::Tiny,
                    cx,
                )
            });
            body = body.child(div().child(theme::segmented().children(shelves)));
        } else {
            body = body.child(theme::note(tr!(
                "Connectez votre compte Yoto pour choisir parmi ses icônes. Vous pouvez déjà reprendre une de vos images ou en importer une."
            )));
        }
        body = body.child(rows);
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
        if !loading && failure.is_none() && self.choices.len(self.shelf) == 0 {
            body = body.child(div().text_sm().child(if yoto {
                tr!("Aucune icône dans cette collection. Vous pouvez importer une image.")
            } else {
                tr!("Aucune image pour l’instant. Importez-en une : vous pourrez la reprendre pour d’autres médias.")
            }));
        }
        body = body.child(theme::note(
            tr!("Choisissez une icône Yoto ou ajoutez votre image : elle sera adaptée en 16 × 16 pour l’écran du lecteur."),
        ));
        // A refused icon is explained here, in the dialog the parent is looking at.
        if !busy
            && !self.uploading
            && let Some(error) = self.error.clone().or(owner.error)
        {
            body = body.child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::c(theme::ROSE.ink))
                    .child(error),
            );
        }
        body = body.child(
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
                        disabled || busy,
                        cx,
                    )
                    .border_dashed()
                    .border_color(theme::c(theme::BOX_BORDER)),
                )
                .child(div().flex_1())
                .child(theme::dialog_close(
                    "cancel-icon-footer",
                    theme::pill(
                        Button::new("cancel-icon").label(tr!("Annuler")),
                        Look::Ghost,
                        Height::Medium,
                        busy || self.uploading,
                        cx,
                    ),
                ))
                .child(theme::pill(
                    Button::new("apply-icon")
                        .label(tr!("Utiliser cette icône"))
                        .on_click(cx.listener(|v, _, _, cx| v.apply(cx))),
                    Look::Primary,
                    Height::Medium,
                    disabled || busy || self.selected.is_none(),
                    cx,
                )),
        );
        if busy || self.uploading {
            body = body.child(div().flex().gap_2().child(Spinner::new().small()).child(
                if self.uploading {
                    self.status.clone()
                } else {
                    owner.status.clone()
                },
            ));
        }
        if !busy && blocked {
            body = body.child(div().text_sm().child(owner.status));
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
/// Where Yoto keeps the cover it shows for a card, read from a card document or from
/// the card fields a playlist carries unchanged.
pub(super) fn cover_url(cover: Option<&serde_json::Value>) -> Option<SharedString> {
    cover
        .and_then(|cover| cover.get("imageL"))
        .and_then(serde_json::Value::as_str)
        .filter(|url| url.starts_with("https://"))
        .map(|url| SharedString::from(url.to_owned()))
}
/// A playlist's cover as this computer can draw it, `size` wide: the image chosen here,
/// or the one Yoto shows for the card. Without either it is an honest empty slot, never a
/// decorative drawing a parent could take for the cover. `plus` marks a slot that opens
/// the cover choice.
pub(super) fn playlist_cover(
    asset: Option<AssetId>,
    url: Option<SharedString>,
    parent: &AppView,
    size: f32,
    radius: f32,
    plus: bool,
) -> AnyElement {
    match asset.map(|asset| parent.cover_previews.get(&asset)) {
        Some(Some(Some(image))) => img(image.clone())
            .size(px(size))
            .flex_shrink_0()
            .rounded(px(radius))
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        // Still being read: a plain tile, not a drawing that could pass for the cover.
        Some(None) => div()
            .size(px(size))
            .flex_shrink_0()
            .rounded(px(radius))
            .bg(theme::c(theme::CREAM))
            .into_any_element(),
        Some(Some(None)) | None => remote_cover(url, size, radius, plus),
    }
}
/// The cover Yoto shows for a card, or the empty slot when it has none or it cannot load.
pub(super) fn remote_cover(
    url: Option<SharedString>,
    size: f32,
    radius: f32,
    plus: bool,
) -> AnyElement {
    let empty = move || theme::empty_icon_slot(size, radius, plus).into_any_element();
    match url {
        Some(url) => img(url)
            .size(px(size))
            .flex_shrink_0()
            .rounded(px(radius))
            .object_fit(ObjectFit::Cover)
            .with_fallback(empty)
            .into_any_element(),
        None => empty(),
    }
}
impl AppView {
    /// Read a local playlist cover once; a cover that cannot be read is not asked again.
    pub(super) fn load_cover_preview(&mut self, asset: AssetId, cx: &mut Context<Self>) {
        if self.cover_previews.contains_key(&asset) || !self.cover_requests.insert(asset) {
            return;
        }
        let service = self.service.clone();
        let runtime = self.runtime.clone();
        self._tasks.push(cx.spawn(async move |weak, cx| {
            let result = runtime
                .spawn(async move { service.execute(Command::CoverPreview(asset)).await })
                .await;
            let image = joined(result)
                .and_then(Reply::into_picture)
                .ok()
                .map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
            let _ = weak.update(cx, |v, cx| {
                v.cover_previews.insert(asset, image);
                cx.notify();
            });
        }));
    }
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
    use super::{cover_url, prepare_icon_choices, upscale, upscale_factor};
    #[test]
    fn only_a_secure_yoto_cover_address_is_drawn() {
        let card = serde_json::json!({
            "metadata": {"cover": {"imageL": "https://example.test/cover.png"}},
            "content": {"cover": {"imageL": "http://example.test/plain.png"}},
        });
        assert_eq!(
            cover_url(card.pointer("/metadata/cover")).as_deref(),
            Some("https://example.test/cover.png")
        );
        assert_eq!(cover_url(card.pointer("/content/cover")), None);
        assert_eq!(cover_url(card.pointer("/metadata/missing")), None);
        assert_eq!(cover_url(Some(&serde_json::json!({"imageL": null}))), None);
    }
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
