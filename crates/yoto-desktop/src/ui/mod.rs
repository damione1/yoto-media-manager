mod account;
mod autosave;
mod chapters;
mod device;
mod editor;
mod group;
mod imports;
mod library;
mod pixelart;
mod players;
mod reconciliation;
mod theme;
mod transfer;
mod trim;
mod update;
mod wording;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Disableable, Selectable, Sizable,
    button::{Button, ButtonVariants},
    input::{Input, InputState},
};
use gpui_kit::*;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
pub use theme::install as install_theme;
pub use wording::{PRODUCT_NAME, failure_text, phase_text};
use wording::{confirm_destructive, duration_text};
use yoto_domain::*;
use yoto_infra::{Command, Reply, Session as Service, Snapshot};
pub struct AppView {
    /// The window itself, focused when a click lands outside the text field being edited.
    focus_handle: FocusHandle,
    updater: Entity<update::UpdatePanel>,
    pub service: Arc<Service>,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub snapshot: Arc<Snapshot>,
    revisions: Vec<u64>,
    search_index: Arc<Vec<SearchRow>>,
    pub job_page: Option<Arc<Vec<Job>>>,
    pub job_offset: usize,
    pub receipt_card: Entity<InputState>,
    pub route: Route,
    pub selected_media: Option<MediaId>,
    pub selected_media_set: std::collections::BTreeSet<MediaId>,
    pub sort: CatalogueSort,
    search_generation: u64,
    pub account_sync_details_open: bool,
    /// The last export or import dialog, kept while it runs so its result is not lost.
    transfer: Option<Entity<transfer::LibraryTransfer>>,
    pub library_rows: Vec<usize>,
    playlist_media_choices: Arc<editor::MediaChoices>,
    playlist_icon_choices: Arc<pixelart::IconChoices>,
    /// Pixels of local media icons, read once and shared by every view of the library.
    pub icon_previews: BTreeMap<AssetId, PixelArt>,
    icon_requests: std::collections::BTreeSet<AssetId>,
    /// Playlist covers kept on this computer, decoded once for every view that shows them.
    /// `None` marks a cover that could not be read, so it is not asked again.
    pub cover_previews: BTreeMap<AssetId, Option<Arc<Image>>>,
    cover_requests: std::collections::BTreeSet<AssetId>,
    pub playlist_rows: Arc<Vec<editor::PlaylistRow>>,
    pub selected_playlist: Option<PlaylistId>,
    pub opening_playlist: Option<(AccountId, CardId)>,
    playlist_open_generation: u64,
    pub selected_device: Option<DeviceId>,
    pub device_panels: BTreeMap<(AccountId, DeviceId), Entity<device::DevicePanel>>,
    pub editors: BTreeMap<PlaylistId, Entity<editor::PlaylistEditor>>,
    pub trims: BTreeMap<MediaId, Entity<trim::TrimPanel>>,
    pub search: Entity<InputState>,
    pub new_title: Entity<InputState>,
    pub youtube: Entity<InputState>,
    pub youtube_entries: Vec<SourceEntry>,
    /// Titles learned while inspecting YouTube links, keyed by the imported URL, so a
    /// running import shows the video title instead of a generic label.
    pub youtube_titles: BTreeMap<String, String>,
    pub selected_entries: std::collections::BTreeSet<String>,
    pub youtube_import_pending: bool,
    pub youtube_inspection_generation: u64,
    pub youtube_selection_url: Option<String>,
    pub status: String,
    connecting: bool,
    pub creating_playlist: bool,
    pub error: Option<Failure>,
    pub _tasks: Vec<Task<()>>,
    pub _subscriptions: Vec<Subscription>,
    pub group_editors: BTreeMap<(AccountId, Option<GroupId>), Entity<group::GroupEditor>>,
    pub selected_group: Option<Option<GroupId>>,
    /// « Mode avancé » in Compte: shows the technical settings parents rarely need.
    /// Saved in [`Self::settings`]; it starts off.
    pub advanced: bool,
    /// Interface settings saved in the profile: the language and the advanced mode.
    settings: yoto_infra::settings::Settings,
    /// The YouTube tool imports use, read when « Mode avancé » is turned on.
    pub youtube_tool: Option<yoto_infra::youtube_tool::YoutubeToolStatus>,
    /// Quitting waits for playlist edits to reach Yoto.
    quitting: bool,
}
impl AppView {
    pub fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        if self.route == Route::Playlists && route != Route::Playlists {
            self.flush_playlists(cx);
        }
        let entering_playlists = self.route != Route::Playlists && route == Route::Playlists;
        self.route = route;
        self.error = None;
        self.status = tr!("Prêt.").into();
        if entering_playlists {
            self.refresh_playlists(cx);
        }
        cx.notify();
    }
    pub fn refresh_playlists(&mut self, cx: &mut Context<Self>) {
        if let Some(account) = self.snapshot.active_account.clone()
            && !matches!(
                self.snapshot.remote_sync.get(&RemoteCollection::Playlists),
                Some(RemoteSyncState::Syncing)
            )
        {
            self.send(Command::RefreshPlaylists(account), cx);
        }
    }
    pub fn new(
        service: Arc<Service>,
        runtime: Arc<tokio::runtime::Runtime>,
        prepared: PreparedSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx
            .new(|cx| InputState::new(window, cx).placeholder(tr!("Rechercher dans vos médias…")));
        let new_title =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr!("Titre de la playlist")));
        let youtube = cx
            .new(|cx| InputState::new(window, cx).placeholder("https://www.youtube.com/watch?v=…"));
        let mut subscriptions = vec![cx.observe(&search, |view, _, cx| {
            view.filter_library(cx);
            cx.notify();
        })];
        subscriptions.push(cx.observe(&new_title, |_, _, cx| cx.notify()));
        subscriptions.push(cx.observe(&youtube, |_, _, cx| cx.notify()));
        let PreparedSnapshot {
            snapshot,
            revisions,
            playlist_media_choices,
            playlist_icon_choices,
            playlist_rows,
            media_ids: _,
            index: search_index,
        } = prepared;
        let service_clone = service.clone();
        let rt = runtime.clone();
        let task = cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let Ok((base, previous)) = weak.update(cx, |view, _| {
                    (view.snapshot.clone(), view.revisions.clone())
                }) else {
                    break;
                };
                let service = service_clone.clone();
                let base_for_worker = base.clone();
                let result = rt
                    .spawn(async move {
                        let delta = service.interface_changes(previous).await?;
                        if delta.parts.is_empty() {
                            return Ok::<_, Failure>(None);
                        }
                        tokio::task::spawn_blocking(move || {
                            let mut snapshot = (*base_for_worker).clone();
                            let revisions = delta.apply(&mut snapshot);
                            Some(PreparedSnapshot::new(snapshot, revisions))
                        })
                        .await
                        .map_err(|e| Failure::Storage(e.to_string()))
                    })
                    .await;
                if let Ok(Ok(Some(prepared))) = result
                    && weak
                        .update(cx, |view, cx| {
                            if Arc::ptr_eq(&view.snapshot, &base) {
                                view.adopt(prepared, cx);
                            }
                        })
                        .is_err()
                {
                    break;
                }
            }
        });
        let library_rows = (0..snapshot.media.len()).collect();
        let updater = cx.new(|cx| update::UpdatePanel::new(runtime.clone(), service.clone(), cx));
        let settings = yoto_infra::settings::Settings::load(service.profile());
        subscriptions.push(cx.observe(&updater, |_, _, cx| cx.notify()));
        let mut view = Self {
            focus_handle: cx.focus_handle(),
            updater,
            service,
            runtime,
            snapshot,
            revisions,
            search_index,
            job_page: None,
            job_offset: 0,
            receipt_card: cx
                .new(|cx| InputState::new(window, cx).placeholder(tr!("Identifiant de la carte"))),
            route: Route::Library,
            selected_media: None,
            selected_media_set: Default::default(),
            sort: CatalogueSort::Title,
            search_generation: 0,
            account_sync_details_open: false,
            transfer: None,
            library_rows,
            playlist_media_choices,
            playlist_icon_choices,
            icon_previews: BTreeMap::new(),
            icon_requests: Default::default(),
            cover_previews: BTreeMap::new(),
            cover_requests: Default::default(),
            playlist_rows,
            selected_playlist: None,
            opening_playlist: None,
            playlist_open_generation: 0,
            selected_device: None,
            device_panels: BTreeMap::new(),
            editors: BTreeMap::new(),
            trims: BTreeMap::new(),
            search,
            new_title,
            youtube,
            youtube_entries: vec![],
            youtube_titles: BTreeMap::new(),
            selected_entries: Default::default(),
            youtube_import_pending: false,
            youtube_inspection_generation: 0,
            youtube_selection_url: None,
            status: tr!("Votre collection personnelle, même hors ligne.").into(),
            connecting: false,
            creating_playlist: false,
            error: None,
            _tasks: vec![task],
            _subscriptions: subscriptions,
            group_editors: BTreeMap::new(),
            selected_group: None,
            advanced: settings.advanced_mode,
            settings,
            youtube_tool: None,
            quitting: false,
        };
        view.refresh_playlists(cx);
        view.load_media_icons(cx);
        if view.advanced {
            view.youtube_tool(yoto_infra::youtube_tool::YoutubeToolAction::State, cx);
        }
        view
    }
    fn adopt(&mut self, prepared: PreparedSnapshot, cx: &mut Context<Self>) {
        let PreparedSnapshot {
            snapshot,
            revisions,
            playlist_media_choices,
            playlist_icon_choices,
            playlist_rows,
            media_ids,
            index,
        } = prepared;
        let account_changed = self.snapshot.active_account != snapshot.active_account;
        if account_changed {
            self.selected_device = None;
            self.selected_group = None;
            self.selected_playlist = None;
            self.opening_playlist = None;
            self.playlist_open_generation = self.playlist_open_generation.wrapping_add(1);
            self.editors.clear();
        }
        if self
            .selected_media
            .is_some_and(|id| !media_ids.contains(&id))
        {
            self.selected_media = None;
            for panel in self.trims.values() {
                panel.update(cx, |p, cx| p.set_active(false, cx));
            }
        }
        self.selected_media_set.retain(|id| media_ids.contains(id));
        self.trims.retain(|id, _| media_ids.contains(id));
        for panel in self.trims.values() {
            panel.update(cx, |p, cx| p.apply_snapshot(&snapshot, cx));
        }
        for (id, editor) in &self.editors {
            editor.update(cx, |e, cx| {
                e.apply_snapshot(&snapshot, cx);
                e.reconcile(snapshot.playlists.iter().find(|p| p.id == *id).cloned(), cx);
            });
        }
        self.playlist_media_choices = playlist_media_choices;
        self.playlist_icon_choices = playlist_icon_choices;
        self.playlist_rows = playlist_rows;
        self.snapshot = snapshot;
        self.revisions = revisions;
        self.search_index = index;
        self.load_media_icons(cx);
        if let Some(id) = self.selected_playlist
            && !self.editors.contains_key(&id)
            && self
                .snapshot
                .playlists
                .iter()
                .any(|playlist| playlist.id == id)
            && let Some(window) = cx.windows().first().copied()
        {
            let weak = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = cx.update_window(window, |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        if view.selected_playlist == Some(id) && !view.editors.contains_key(&id) {
                            view.select_playlist(id, window, cx);
                        }
                    });
                });
            });
        }
        self.filter_library(cx);
        if account_changed {
            self.refresh_playlists(cx);
        }
        cx.notify();
    }
    fn filter_library(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().to_lowercase();
        let sort = self.sort;
        self.search_generation = self.search_generation.wrapping_add(1);
        let generation = self.search_generation;
        let index = self.search_index.clone();
        cx.spawn(async move |weak, cx| {
            let indices = cx
                .background_executor()
                .spawn(async move {
                    let mut rows = index
                        .iter()
                        .filter(|row| row.search.contains(&query))
                        .collect::<Vec<_>>();
                    rows.sort_by(|a, b| match sort {
                        CatalogueSort::Title => a.title.cmp(&b.title),
                        CatalogueSort::Author => a.author.cmp(&b.author),
                        CatalogueSort::Duration => a.duration.cmp(&b.duration),
                        CatalogueSort::RecentlyImported => b.index.cmp(&a.index),
                    });
                    rows.into_iter().map(|row| row.index).collect::<Vec<_>>()
                })
                .await;
            let _ = weak.update(cx, |view, cx| {
                if view.search_generation == generation {
                    view.library_rows = indices;
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub fn load_job_page(&mut self, offset: usize, cx: &mut Context<Self>) {
        if offset == 0 {
            self.job_page = None;
            self.job_offset = 0;
            cx.notify();
            return;
        }
        let service = self.service.clone();
        let rt = self.runtime.clone();
        cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move {
                    let page = service
                        .execute(Command::JobsPage { offset, limit: 200 })
                        .await?
                        .into_data()?;
                    serde_json::from_value::<Vec<Job>>(page)
                        .map_err(|e| Failure::Storage(e.to_string()))
                })
                .await;
            let _ = weak.update(cx, |view, cx| {
                match joined(result) {
                    Ok(jobs) => {
                        view.job_page = Some(Arc::new(jobs));
                        view.job_offset = offset;
                    }
                    Err(e) => view.report_error(e, cx),
                };
                cx.notify();
            });
        })
        .detach();
    }
    /// An error toast that stays until the parent dismisses it.
    pub fn alert(&self, text: String, cx: &mut Context<Self>) {
        if let Some(window) = cx.windows().first().copied() {
            cx.defer(move |cx| {
                let _ = cx.update_window(window, |_, window, cx| {
                    use gpui_kit::component::{WindowExt, notification::Notification};
                    window.push_notification(Notification::error(text).autohide(false), cx);
                });
            });
        }
    }
    /// Show a failure where a parent will see it: the status line and a toast.
    pub fn report_error(&mut self, failure: Failure, cx: &mut Context<Self>) {
        let text = failure_text(&failure);
        // A hint about what was typed fades by itself; real failures stay until dismissed.
        let persistent = !matches!(failure, Failure::Validation(_));
        self.status = text.clone();
        self.error = Some(failure);
        if let Some(window) = cx.windows().first().copied() {
            cx.defer(move |cx| {
                let _ = cx.update_window(window, |_, window, cx| {
                    use gpui_kit::component::{WindowExt, notification::Notification};
                    window.push_notification(Notification::error(text).autohide(!persistent), cx);
                });
            });
        }
        cx.notify();
    }
    pub fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        let creating_playlist = matches!(
            &command,
            Command::CreateYotoPlaylist { .. } | Command::DuplicateYotoPlaylist { .. }
        );
        if creating_playlist && self.creating_playlist {
            return;
        }
        let clear_new_title = if let Command::CreateYotoPlaylist { title, .. } = &command {
            Some(title.clone())
        } else {
            None
        };
        if creating_playlist {
            self.creating_playlist = true;
        }
        self.error = None;
        cx.notify();
        let service = self.service.clone();
        let rt = self.runtime.clone();
        let window = cx.windows().first().copied();
        let connecting = matches!(&command, Command::Connect);
        let reconciling = matches!(&command, Command::Reconcile(_));
        if connecting {
            self.connecting = true;
        }
        let progress = command_status(&command);
        if matches!(
            &command,
            Command::NewPlaylist(_)
                | Command::Duplicate(_)
                | Command::SaveCopy(_)
                | Command::LoadRemote { .. }
                | Command::CreateYotoPlaylist { .. }
                | Command::DuplicateYotoPlaylist { .. }
        ) {
            self.playlist_open_generation = self.playlist_open_generation.wrapping_add(1);
        }
        let playlist_open_generation = self.playlist_open_generation;
        if matches!(
            &command,
            Command::NewPlaylist(_)
                | Command::Duplicate(_)
                | Command::SaveCopy(_)
                | Command::CreateYotoPlaylist { .. }
                | Command::DuplicateYotoPlaylist { .. }
        ) {
            self.opening_playlist = None;
        }
        let requested_route = self.route;
        let opening_account = if let Command::LoadRemote { account, .. } = &command {
            Some(account.clone())
        } else if let Command::CreateYotoPlaylist { account, .. }
        | Command::DuplicateYotoPlaylist { account, .. } = &command
        {
            Some(account.clone())
        } else {
            None
        };
        let opening_remote = if let Command::LoadRemote { account, card } = &command {
            Some((account.clone(), card.clone()))
        } else {
            None
        };
        if let Some(request) = &opening_remote {
            self.opening_playlist = Some(request.clone());
        }
        if let Some((pending, _)) = progress {
            self.status = pending.into();
        }
        let selection = self.selected_media;
        let base = self.snapshot.clone();
        let worker_base = base.clone();
        let previous = self.revisions.clone();
        cx.spawn(async move |weak, cx| {
            let result = rt
                .spawn(async move {
                    let reply = service.execute(command).await;
                    let delta = service.interface_changes(previous).await?;
                    let prepared = tokio::task::spawn_blocking(move || {
                        let mut snapshot = (*worker_base).clone();
                        let revisions = delta.apply(&mut snapshot);
                        PreparedSnapshot::new(snapshot, revisions)
                    })
                    .await
                    .map_err(|e| Failure::Storage(e.to_string()))?;
                    Ok::<_, Failure>((reply, prepared))
                })
                .await;
            let opened = match &result {
                Ok(Ok((Ok(Reply::OpenedPlaylist(id)), _))) => Some(*id),
                Ok(Ok((Ok(Reply::PlaylistOperation { playlist, .. }), _))) if creating_playlist => {
                    Some(*playlist)
                }
                _ => None,
            };
            let _ = weak.update(cx, |view, cx| {
                if creating_playlist {
                    view.creating_playlist = false;
                }
                if opening_remote.is_some() && view.opening_playlist == opening_remote {
                    view.opening_playlist = None;
                }
                if connecting {
                    view.connecting = false;
                }
                match result {
                    Ok(Ok((reply, prepared))) => {
                        if Arc::ptr_eq(&view.snapshot, &base)
                            || (view.revisions.len() == prepared.revisions.len()
                                && view
                                    .revisions
                                    .iter()
                                    .zip(&prepared.revisions)
                                    .all(|(current, next)| next >= current))
                        {
                            view.adopt(prepared, cx);
                        }
                        if let Some((_, done)) = progress {
                            view.status = done.into();
                        }
                        let reply = match reply {
                            Ok(reply) => reply,
                            Err(Failure::Cancelled) if connecting => {
                                // The parent gave up on the browser sign-in: nothing failed.
                                view.status = wording::connect::cancelled().into();
                                cx.notify();
                                return;
                            }
                            Err(Failure::Authentication) if connecting => {
                                // The browser round trip timed out or was abandoned.
                                view.report_error(
                                    Failure::Validation(
                                        tr!("La connexion Yoto n’a pas abouti. Cliquez de nouveau sur « Connecter mon compte Yoto », terminez la connexion dans votre navigateur, puis revenez ici.").into(),
                                    ),
                                    cx,
                                );
                                return;
                            }
                            Err(Failure::Ambiguous) if reconciling => {
                                // Already on the activity list: say what Yoto shows instead
                                // of pointing back to where the parent already is.
                                view.report_error(
                                    Failure::Validation(
                                        tr!("Yoto ne montre pas exactement cet envoi. Si la playlist vous convient dans l’application Yoto, choisissez « Garder la version de Yoto » dans « Vérifier la publication ».").into(),
                                    ),
                                    cx,
                                );
                                return;
                            }
                            Err(error) => {
                                view.report_error(error, cx);
                                return;
                            }
                        };
                        match reply {
                            Reply::AppUpdate(_)
                            | Reply::YoutubeTool(_)
                            | Reply::Updated
                            | Reply::Saved(_)
                            | Reply::Prepared(_)
                            | Reply::Asset(_)
                            | Reply::PixelArt(_)
                            | Reply::Picture(_)
                            | Reply::Image(_)
                            | Reply::Images(_) => {}
                            Reply::OpenedPlaylist(id) => {
                                if view.playlist_open_generation == playlist_open_generation
                                    && view.route == requested_route
                                    && opening_account.as_ref().is_none_or(|account| {
                                        view.snapshot.active_account.as_ref() == Some(account)
                                    })
                                {
                                    view.route = Route::Playlists;
                                    view.selected_playlist = Some(id);
                                }
                                view.status = tr!("Playlist Yoto ouverte.").into();
                            }
                            Reply::PlaylistOperation { playlist, .. } => {
                                if clear_new_title.as_ref().is_some_and(|title| {
                                    view.new_title.read(cx).value().trim() == title.trim()
                                }) && opening_account.as_ref()
                                    == view.snapshot.active_account.as_ref()
                                    && let Some(window_handle) = window
                                {
                                    let input = view.new_title.clone();
                                    cx.defer(move |cx| {
                                        let _ = cx.update_window(window_handle, |_, window, cx| {
                                            input.update(cx, |input, cx| {
                                                input.set_value("", window, cx)
                                            });
                                        });
                                    });
                                }
                                if creating_playlist
                                    && view.playlist_open_generation == playlist_open_generation
                                    && view.route == requested_route
                                    && opening_account.as_ref().is_none_or(|account| {
                                        view.snapshot.active_account.as_ref() == Some(account)
                                    })
                                {
                                    view.route = Route::Playlists;
                                    view.selected_playlist = Some(playlist);
                                }
                                view.status = tr!("Enregistrement sur Yoto en cours…").into();
                            }
                            Reply::Entries(entries) => {
                                view.youtube_entries = entries.entries;
                                view.selected_entries.clear();
                            }
                            Reply::Waveform(w) => {
                                if view.selected_media == selection
                                    && let Some(id) = selection
                                    && let Some(panel) = view.trims.get(&id)
                                {
                                    panel.update(cx, |p, cx| {
                                        p.waveform = Some(w);
                                        cx.notify();
                                    });
                                }
                            }
                            Reply::Player(result) => {
                                view.status = match result {
                                    yoto_infra::players::CommandResult::Sent => {
                                        tr!("Commande envoyée ; effet non confirmé.").into()
                                    }
                                    yoto_infra::players::CommandResult::Accepted => {
                                        tr!("Commande acceptée par le lecteur.").into()
                                    }
                                    yoto_infra::players::CommandResult::Observed { .. } => {
                                        tr!("État du lecteur reçu.").into()
                                    }
                                };
                            }
                            Reply::Data(value) => {
                                if let (Some(deleted), Some(kept)) = (
                                    value.get("deleted").and_then(serde_json::Value::as_u64),
                                    value.get("kept").and_then(serde_json::Value::as_u64),
                                ) {
                                    view.status = wording::batch_deletion_text(deleted, kept);
                                    if kept > 0 {
                                        view.warn(view.status.clone(), cx);
                                    }
                                } else {
                                    view.status = tr!("Opération terminée.").into();
                                }
                            }
                        }
                    }
                    Ok(Err(e)) => view.report_error(e, cx),
                    Err(e) => view.report_error(Failure::Storage(e.to_string()), cx),
                }
                cx.notify();
            });
            if let (Some(id), Some(window)) = (opened, window) {
                let _ = cx.update_window(window, |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        if view.route == Route::Playlists && view.selected_playlist == Some(id) {
                            view.select_playlist(id, window, cx);
                        }
                    });
                });
            }
        })
        .detach();
    }
    pub fn open_profile(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let runtime = self.runtime.clone();
        cx.spawn(async move |weak, cx| {
            let result = runtime.spawn_blocking(move || launch_profile(&path)).await;
            let _ = weak.update(cx, |v, cx| {
                match joined(result) {
                    Ok(()) => {
                        v.status = tr!("La médiathèque choisie s’ouvre dans une autre fenêtre ; celle-ci reste disponible.").into()
                    }
                    Err(e) => v.report_error(e, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub fn choose_profile(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |weak, cx| {
            if let Some(folder) = rfd::AsyncFileDialog::new()
                .set_title(wording::other_library::picker())
                .pick_folder()
                .await
            {
                let _ = weak.update(cx, |v, cx| v.open_profile(folder.path().to_owned(), cx));
            }
        })
        .detach();
    }
    /// Files dropped anywhere on the window: keep the audio, explain the rest, and show
    /// the import queue so the parent sees something happen.
    pub fn import_dropped(&mut self, paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        let (audio, ignored) = partition_audio(paths);
        if audio.is_empty() {
            self.report_error(
                Failure::Validation(
                    tr!("Déposez des fichiers audio (MP3, M4A, WAV, FLAC, OGG…). Les autres fichiers sont ignorés.").into(),
                ),
                cx,
            );
            return;
        }
        self.navigate(Route::Imports, cx);
        self.send(
            Command::Import(
                audio
                    .into_iter()
                    .map(|path| ImportSelection::LocalFile { path })
                    .collect(),
            ),
            cx,
        );
        if !ignored.is_empty() {
            // The import's own completion text replaces the status line, so the skipped
            // files are reported in a toast that stays until dismissed.
            self.warn(
                trn!(
                    "{count} fichier ignoré : seuls les fichiers audio (MP3, M4A, WAV, FLAC, OGG…) sont importés.",
                    "{count} fichiers ignorés : seuls les fichiers audio (MP3, M4A, WAV, FLAC, OGG…) sont importés.",
                    ignored.len()
                ),
                cx,
            );
        }
    }
    /// A warning toast that stays until the parent dismisses it.
    pub fn warn(&self, text: String, cx: &mut Context<Self>) {
        if let Some(window) = cx.windows().first().copied() {
            cx.defer(move |cx| {
                let _ = cx.update_window(window, |_, window, cx| {
                    use gpui_kit::component::{WindowExt, notification::Notification};
                    window.push_notification(Notification::warning(text).autohide(false), cx);
                });
            });
        }
    }
    pub fn add_files(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |weak, cx| {
            let files = rfd::AsyncFileDialog::new()
                .set_title(tr!("Ajouter vos fichiers audio"))
                .add_filter(tr!("Audio"), AUDIO_EXTENSIONS)
                .pick_files()
                .await;
            if let Some(files) = files {
                let selections = files
                    .into_iter()
                    .map(|f| ImportSelection::LocalFile {
                        path: f.path().to_owned(),
                    })
                    .collect();
                let _ = weak.update(cx, |view, cx| {
                    view.navigate(Route::Imports, cx);
                    view.send(Command::Import(selections), cx);
                });
            }
        })
        .detach();
    }
    /// A click outside the text field being edited ends the editing, as on a web page:
    /// the window takes the focus, so the field's blur runs (and with it the playlist
    /// autosave). This runs after the clicked element: a click in another field has
    /// already focused it, and a click in the edited field itself keeps it.
    fn blur_input_outside(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui_kit::component::{WindowExt, input::AnyInputState};
        let Some(input) = window.focused_input(cx) else {
            return;
        };
        let bounds = match &input {
            AnyInputState::Input(state) => Some(state.read(cx).input_bounds()),
            AnyInputState::Textarea(state) => Some(state.read(cx).input_bounds()),
            AnyInputState::Editor(state) => Some(state.read(cx).input_bounds()),
            AnyInputState::Otp(_) => None,
        };
        if bounds.is_some_and(|bounds| !bounds.contains(&event.position)) {
            window.focus(&self.focus_handle, cx);
        }
    }
    pub fn select_media(&mut self, id: MediaId, window: &mut Window, cx: &mut Context<Self>) {
        self.open_media(id, false, window, cx);
    }
    /// Open a media in the side panel; with `play`, start listening to it at once (a new
    /// playback replaces the previous one, so nothing is stopped first).
    pub fn open_media(
        &mut self,
        id: MediaId,
        play: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_media != Some(id) && !play {
            self.send(Command::StopPreview, cx);
        }
        self.selected_media = Some(id);
        for (media, panel) in &self.trims {
            let active = *media == id;
            panel.update(cx, |p, cx| p.set_active(active, cx));
        }
        if !self.trims.contains_key(&id)
            && let Some(media) = self.snapshot.media.iter().find(|m| m.id == id)
        {
            let parent = cx.entity().downgrade();
            let panel = cx.new(|cx| {
                trim::TrimPanel::new(
                    media.clone(),
                    parent.clone(),
                    self.service.clone(),
                    self.runtime.clone(),
                    window,
                    cx,
                )
            });
            panel.update(cx, |p, cx| p.apply_snapshot(&self.snapshot, cx));
            self.trims.insert(id, panel);
        }
        if play && let Some(panel) = self.trims.get(&id).cloned() {
            panel.update(cx, |p, cx| p.toggle_play(cx));
        }
        self.send(Command::Waveform(id), cx);
    }
    pub fn select_playlist(&mut self, id: PlaylistId, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_playlist != Some(id) {
            self.flush_playlists(cx);
        }
        self.playlist_open_generation = self.playlist_open_generation.wrapping_add(1);
        self.opening_playlist = None;
        self.selected_playlist = Some(id);
        if !self.editors.contains_key(&id)
            && let Some(p) = self.snapshot.playlists.iter().find(|p| p.id == id)
        {
            let parent = cx.entity().downgrade();
            let editor = cx.new(|cx| {
                editor::PlaylistEditor::new(
                    p.clone(),
                    parent,
                    self.service.clone(),
                    self.runtime.clone(),
                    window,
                    cx,
                )
            });
            let advanced = self.advanced;
            let snapshot = &self.snapshot;
            editor.update(cx, |e, cx| {
                e.advanced = advanced;
                e.restore_failure(snapshot);
                e.verify_uncertain(cx);
            });
            self.editors.insert(id, editor);
            if let Some(account) = self.icons_missing(p) {
                self.send(Command::RefreshIcons(account), cx);
            }
        }
        cx.notify();
    }
    /// The account whose Yoto icon list must be read again to draw this playlist: one of
    /// its icons is not in the list the app holds, and no reading of it is under way.
    fn icons_missing(&self, playlist: &Playlist) -> Option<AccountId> {
        let active = self.snapshot.active_account.as_ref()?;
        let missing = playlist
            .chapters
            .iter()
            .flat_map(|c| std::iter::once(&c.icon).chain(c.tracks.iter().map(|t| &t.icon)))
            .any(|icon| match icon {
                Some(IconReference::Yoto { account, media }) => {
                    account == active && self.playlist_icon_choices.url(media).is_none()
                }
                Some(IconReference::Local { .. }) | None => false,
            });
        let reading = matches!(
            self.snapshot.remote_sync.get(&RemoteCollection::Icons),
            Some(RemoteSyncState::Syncing)
        );
        (missing && !reading).then(|| active.clone())
    }
    /// Write the playlists' pending changes now: the parent is leaving them. A write that
    /// fails is reported in a toast naming the playlist.
    pub fn flush_playlists(&mut self, cx: &mut Context<Self>) {
        for editor in self.editors.values() {
            editor.update(cx, |e, cx| e.flush_before_leaving(cx));
        }
    }
    /// The parent asked to quit. Pending playlist edits are written first; the app then
    /// quits, or asks what to do when some could not be written. Returns whether it may
    /// quit at once.
    pub fn request_quit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.quitting {
            return false;
        }
        self.flush_playlists(cx);
        let (waiting, unsaved) = self.unwritten_playlists(cx);
        if waiting.is_empty() && unsaved.is_empty() {
            return true;
        }
        self.quitting = true;
        if let Some(window) = cx.windows().first().copied() {
            cx.defer(move |cx| {
                let _ = cx.update_window(window, |_, window, cx| {
                    use gpui_kit::component::{WindowExt, notification::Notification};
                    window.push_notification(
                        Notification::info(wording::autosave::quit_saving()),
                        cx,
                    );
                });
            });
        }
        cx.spawn(async move |weak, cx| {
            let started = std::time::Instant::now();
            loop {
                cx.background_executor().timer(autosave::QUIT_POLL).await;
                let Ok((waiting, mut unsaved)) = weak.update(cx, |v, cx| v.unwritten_playlists(cx))
                else {
                    return;
                };
                if !waiting.is_empty() && started.elapsed() < autosave::QUIT_WAIT {
                    continue;
                }
                if waiting.is_empty() && unsaved.is_empty() {
                    cx.update(|cx| cx.quit());
                    return;
                }
                unsaved.extend(
                    waiting
                        .into_iter()
                        .map(|title| (title, wording::autosave::quit_timed_out().to_owned())),
                );
                let _ = weak.update(cx, |v, cx| {
                    v.quitting = false;
                    v.confirm_quit(unsaved, cx);
                });
                return;
            }
        })
        .detach();
        false
    }
    /// Playlists still writing (by title), and those whose edits could not be written.
    fn unwritten_playlists(&self, cx: &App) -> (Vec<String>, Vec<(String, String)>) {
        let (mut waiting, mut unsaved) = (vec![], vec![]);
        for editor in self.editors.values() {
            let editor = editor.read(cx);
            match editor.quit_state() {
                editor::QuitState::Settled => {}
                editor::QuitState::Waiting => waiting.push(editor.title().to_owned()),
                editor::QuitState::Unsaved(reason) => {
                    unsaved.push((editor.title().to_owned(), reason))
                }
            }
        }
        (waiting, unsaved)
    }
    /// Some edits could not be written: let the parent stay, or quit and lose them.
    fn confirm_quit(&self, unsaved: Vec<(String, String)>, cx: &mut Context<Self>) {
        let Some(window) = cx.windows().first().copied() else {
            cx.quit();
            return;
        };
        cx.defer(move |cx| {
            let _ = cx.update_window(window, |_, window, cx| {
                confirm_destructive(
                    window,
                    cx,
                    wording::autosave::quit_title(),
                    wording::autosave::quit_description(&unsaved),
                    wording::autosave::quit_anyway(),
                    |_, cx| cx.quit(),
                );
            });
        });
    }
    /// Turn « Mode avancé » on or off everywhere it applies.
    pub fn set_advanced(&mut self, advanced: bool, cx: &mut Context<Self>) {
        self.advanced = advanced;
        self.settings.advanced_mode = advanced;
        self.save_settings();
        if advanced {
            self.youtube_tool(yoto_infra::youtube_tool::YoutubeToolAction::State, cx);
        }
        for editor in self.editors.values() {
            editor.update(cx, |e, cx| {
                e.advanced = advanced;
                cx.notify();
            });
        }
        cx.notify();
    }
    /// Switch the interface language now and remember it. Every view renders again in the
    /// new language; texts kept in inputs (their placeholders) are set again too.
    pub fn set_language(
        &mut self,
        language: crate::i18n::Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::i18n::set_language(language);
        // The status line keeps its text until the next action; start it afresh.
        self.status = tr!("Prêt.").into();
        self.settings.language = Some(language.code().into());
        self.save_settings();
        self.relabel(window, cx);
        cx.refresh_windows();
    }
    /// Placeholders of the inputs, in the interface language.
    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholders = [
            (&self.search, tr!("Rechercher dans vos médias…")),
            (&self.new_title, tr!("Titre de la playlist")),
            (&self.receipt_card, tr!("Identifiant de la carte")),
        ];
        for (input, placeholder) in placeholders {
            input.update(cx, |s, cx| s.set_placeholder(placeholder, window, cx));
        }
        for editor in self.group_editors.values() {
            editor.update(cx, |e, cx| e.relabel(window, cx));
        }
        for panel in self.trims.values() {
            panel.update(cx, |p, cx| p.relabel(window, cx));
        }
        for panel in self.device_panels.values() {
            panel.update(cx, |p, cx| p.relabel(window, cx));
        }
        for editor in self.editors.values() {
            editor.update(cx, |e, cx| e.relabel(window, cx));
        }
    }
    /// Keep the interface settings for the next launch. Failing to write them only costs
    /// the choice at the next launch, so it is reported without interrupting.
    fn save_settings(&mut self) {
        if let Err(e) = self.settings.save(self.service.profile()) {
            self.status = failure_text(&e);
        }
    }
}
impl AppView {
    /// Ask about the YouTube tool or start its update, then follow a running update
    /// until it ends.
    pub fn youtube_tool(
        &mut self,
        action: yoto_infra::youtube_tool::YoutubeToolAction,
        cx: &mut Context<Self>,
    ) {
        use yoto_infra::youtube_tool::{ToolActivity, YoutubeToolAction};
        let service = self.service.clone();
        let rt = self.runtime.clone();
        cx.spawn(async move |weak, cx| {
            let mut action = action;
            loop {
                let service = service.clone();
                let reply = rt
                    .spawn(async move {
                        service
                            .execute(Command::YoutubeTool(action))
                            .await?
                            .into_youtube_tool()
                    })
                    .await;
                let Ok(status) = joined(reply) else { break };
                let running = status.activity == ToolActivity::Checking;
                let shown = weak.update(cx, |view, cx| {
                    view.youtube_tool = Some(status);
                    cx.notify();
                });
                if shown.is_err() || !running {
                    break;
                }
                action = YoutubeToolAction::State;
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
            }
        })
        .detach();
    }
}
/// Start another copy of the app on an existing library folder.
fn launch_profile(path: &std::path::Path) -> yoto_domain::Result<()> {
    let profile = yoto_infra::canonical_profile(path)?;
    if !profile.join("catalogue.db").is_file() {
        return Err(Failure::Validation(
            tr!("Choisissez un profil Yoto existant contenant catalogue.db").into(),
        ));
    }
    let executable = std::env::current_exe().map_err(|e| Failure::Storage(e.to_string()))?;
    let mut command = std::process::Command::new(executable);
    command.env("YOTO_PROFILE", profile);
    if cfg!(debug_assertions) && std::env::args().any(|a| a == "--isolated") {
        command.arg("--isolated");
    }
    command
        .spawn()
        .map_err(|e| Failure::Storage(e.to_string()))?;
    Ok(())
}
/// A service call run on the Tokio runtime, with a failed runtime task reported like any
/// other local failure.
fn joined<T>(
    result: std::result::Result<yoto_domain::Result<T>, tokio::task::JoinError>,
) -> yoto_domain::Result<T> {
    result.unwrap_or_else(|error| Err(Failure::Storage(error.to_string())))
}
/// Dropped files split into audio to import and the rest, by extension, ignoring case.
fn partition_audio(
    paths: &[std::path::PathBuf],
) -> (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) {
    paths.iter().cloned().partition(|path| {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
    })
}
/// Audio formats accepted by the file picker and by drag-and-drop.
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "aac", "flac", "wav", "ogg", "opus", "aiff", "aif", "wma",
];
struct SearchRow {
    index: usize,
    title: String,
    author: String,
    search: String,
    duration: MediaTime,
}
pub struct PreparedSnapshot {
    snapshot: Arc<Snapshot>,
    revisions: Vec<u64>,
    playlist_media_choices: Arc<editor::MediaChoices>,
    playlist_icon_choices: Arc<pixelart::IconChoices>,
    playlist_rows: Arc<Vec<editor::PlaylistRow>>,
    media_ids: std::collections::BTreeSet<MediaId>,
    index: Arc<Vec<SearchRow>>,
}
impl PreparedSnapshot {
    pub fn new(snapshot: Snapshot, revisions: Vec<u64>) -> Self {
        let index = Arc::new(
            snapshot
                .media
                .iter()
                .enumerate()
                .map(|(index, m)| SearchRow {
                    index,
                    title: m.title.to_lowercase(),
                    author: m.author.to_lowercase(),
                    search: format!("{} {} {}", m.title, m.author, m.tags.join(" ")).to_lowercase(),
                    duration: m.info.duration,
                })
                .collect(),
        );
        let playlist_media_choices = editor::prepare_media_choices(&snapshot);
        let playlist_icon_choices = pixelart::prepare_icon_choices(&snapshot);
        let playlist_rows = Arc::new(editor::playlist_rows(&snapshot));
        let media_ids = snapshot.media.iter().map(|media| media.id).collect();
        Self {
            snapshot: Arc::new(snapshot),
            revisions,
            playlist_media_choices,
            playlist_icon_choices,
            playlist_rows,
            media_ids,
            index,
        }
    }
}
impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.route {
            Route::Library => library::render(self, window, cx),
            Route::Playlists => editor::render(self, window, cx),
            Route::Imports => imports::render(self, window, cx),
            Route::Players => players::render(self, window, cx),
            Route::Account => account::render(self, cx),
        };
        let nav = sidebar(self, cx);
        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::blur_input_outside))
            .on_drop(cx.listener(|view, files: &ExternalPaths, _, cx| {
                view.import_dropped(files.paths(), cx);
            }))
            .flex()
            .flex_col()
            .bg(theme::c(theme::CREAM))
            .text_color(theme::c(theme::INK))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(nav)
                    .child(div().flex_1().min_w_0().h_full().child(body)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .pl(px(20.))
                    .pr(px(12.))
                    .py(px(3.))
                    .border_t_2()
                    .border_color(theme::c(theme::DIVIDER))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::c(if self.error.is_some() {
                                theme::ROSE.ink
                            } else {
                                theme::MUTED
                            }))
                            .truncate()
                            .child(self.status.clone()),
                    )
                    .child(updates_control(self, cx)),
            )
    }
}

/// Each section's pastel family and icon in the sidebar.
fn route_style(route: Route) -> (&'static theme::Family, gpui_kit::assets::IconName) {
    use gpui_kit::assets::IconName;
    match route {
        Route::Library => (&theme::PEACH, IconName::Music),
        Route::Playlists => (&theme::LILAC, IconName::ListMusic),
        Route::Imports => (&theme::BUTTER, IconName::Download),
        Route::Players => (&theme::SKY, IconName::Speaker),
        Route::Account => (&theme::MINT, IconName::User),
    }
}
/// Jobs a parent is still waiting on, shown as the Imports counter.
fn active_jobs(snapshot: &Snapshot) -> usize {
    snapshot
        .jobs
        .iter()
        .filter(|job| match job.state {
            JobState::Queued
            | JobState::Running { .. }
            | JobState::Retry { .. }
            | JobState::WaitingForAuthentication
            | JobState::Conflict { .. }
            | JobState::Reconciling => true,
            JobState::Completed { .. } | JobState::Failed { .. } | JobState::Cancelled => false,
        })
        .count()
}
fn sidebar(view: &AppView, cx: &mut Context<AppView>) -> Div {
    use gpui_kit::component::{
        Icon,
        button::{ButtonCustomVariant, ButtonRounded},
    };
    let brand = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(6.))
        .pt(px(16.))
        .pb(px(18.))
        .child(theme::logo(40.))
        .child(
            div()
                .flex()
                .flex_col()
                .child(theme::heading(PRODUCT_NAME, 18.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::c(theme::MUTED_WARM))
                        .child(tr!("Votre audio. Vos histoires.")),
                ),
        );
    let items = Route::ALL.iter().copied().map(|route| {
        let (family, icon) = route_style(route);
        let active = view.route == route;
        let count = match route {
            Route::Library => view.snapshot.media.len(),
            Route::Playlists => view.playlist_rows.len(),
            Route::Imports => active_jobs(&view.snapshot),
            Route::Players | Route::Account => 0,
        };
        let surface = if active {
            theme::c(theme::SURFACE)
        } else {
            theme::c(theme::SAND)
        };
        let variant = ButtonCustomVariant::new(cx)
            .color(surface)
            .foreground(theme::c(theme::INK))
            .hover(if active {
                surface
            } else {
                theme::c(theme::CREAM)
            })
            .active(theme::c(theme::SURFACE));
        let item = Button::new(wording::labels::route(route))
            .custom(variant)
            .accessibility_label(if count > 0 {
                format!("{} ({count})", wording::labels::route(route))
            } else {
                wording::labels::route(route).to_owned()
            })
            .toggled(active)
            .bg(surface)
            .rounded(ButtonRounded::Size(px(14.)))
            .h(px(48.))
            .px(px(10.))
            .w_full()
            .text_size(px(15.))
            .font_weight(if active {
                FontWeight::EXTRA_BOLD
            } else {
                FontWeight::BOLD
            })
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(32.))
                            .flex_shrink_0()
                            .rounded(px(10.))
                            .bg(theme::c(family.soft))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(icon)
                                    .size(px(17.))
                                    .text_color(theme::c(family.icon)),
                            ),
                    )
                    .child(div().flex_1().child(wording::labels::route(route)))
                    .children((count > 0).then(|| theme::badge(count, family))),
            )
            .on_click(cx.listener(move |view, _, _, cx| {
                view.navigate(route, cx);
            }));
        if active {
            item.shadow(theme::lift())
        } else {
            item
        }
    });
    let account = view
        .snapshot
        .active_account
        .as_ref()
        .and_then(|id| view.snapshot.accounts.iter().find(|a| &a.id == id));
    let (initial, name, state, connected) = match account {
        Some(account) => (
            account
                .label
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "Y".into()),
            account.label.clone(),
            tr!("Connecté à Yoto"),
            true,
        ),
        None => (
            "?".into(),
            tr!("Compte Yoto").into(),
            tr!("Non connecté"),
            false,
        ),
    };
    let account_card = Button::new("sidebar-account")
        .custom(
            ButtonCustomVariant::new(cx)
                .color(theme::c(theme::SURFACE))
                .foreground(theme::c(theme::INK))
                .hover(theme::c(theme::CREAM))
                .active(theme::c(theme::SAND)),
        )
        .accessibility_label(format!("{name} · {state}"))
        .bg(theme::c(theme::SURFACE))
        .rounded(ButtonRounded::Size(px(16.)))
        .h(px(58.))
        .px(px(12.))
        .w_full()
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .size(px(34.))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(theme::c(if connected {
                            theme::MINT.soft
                        } else {
                            theme::SAND
                        }))
                        .text_color(theme::c(if connected {
                            theme::MINT.ink
                        } else {
                            theme::MUTED_WARM
                        }))
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(initial),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .truncate()
                                .child(name),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_size(px(12.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::c(if connected {
                                    theme::MINT.ink
                                } else {
                                    theme::MUTED_WARM
                                }))
                                .child(theme::dot(if connected {
                                    theme::MINT.accent
                                } else {
                                    theme::OFFLINE
                                }))
                                .child(state),
                        ),
                ),
        )
        .on_click(cx.listener(|view, _, _, cx| view.navigate(Route::Account, cx)));
    div()
        .w(px(244.))
        .flex_shrink_0()
        .h_full()
        .bg(theme::c(theme::SAND))
        .px(px(16.))
        .pt(px(8.))
        .pb(px(18.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(brand)
        .children(items)
        .child(div().flex_1())
        .child(account_card)
}

/// Right end of the status bar: opens the update dialog, with a dot when a new
/// version is waiting.
fn updates_control(view: &AppView, cx: &mut Context<AppView>) -> Div {
    let updater = view.updater.read(cx);
    let (available, label) = (updater.update_available(), updater.status_label());
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.))
        .children(available.then(|| theme::dot(theme::PEACH.accent)))
        .child(
            theme::pill(
                Button::new("updates-status")
                    .label(label)
                    .on_click(cx.listener(|view, _, window, cx| {
                        let updater = view.updater.clone();
                        // Opening a Kit dialog mutates the window root; defer outside the AppView borrow.
                        window.defer(cx, move |window, cx| {
                            update::UpdatePanel::open(updater, window, cx)
                        });
                    })),
                if available {
                    theme::Look::Link(&theme::PEACH)
                } else {
                    theme::Look::Ghost
                },
                theme::Height::Tiny,
                false,
                cx,
            )
            .h(px(24.))
            .px(px(8.))
            .text_size(px(13.)),
        )
}

/// « Annuler » beside a connect button while the browser sign-in runs: a sign-in that
/// failed on Yoto's side never comes back, and the parent must be able to start over.
fn cancel_connect(id: &'static str, height: theme::Height, cx: &mut Context<AppView>) -> Button {
    theme::pill(
        Button::new(id)
            .label(wording::connect::cancel())
            .tooltip(wording::connect::cancel_hint())
            .on_click(cx.listener(|v, _, _, cx| v.send(Command::CancelConnect, cx))),
        theme::Look::Ghost,
        height,
        false,
        cx,
    )
}
fn element_id(label: &str, id: impl std::fmt::Display) -> ElementId {
    SharedString::from(format!("{label}:{id}")).into()
}
/// Status line while a command runs and once it succeeds. `None` keeps the line as it is:
/// background lookups (waveform, preview) must not claim anything was saved.
fn command_status(command: &Command) -> Option<(&'static str, &'static str)> {
    Some(match command {
        Command::Connect => (
            tr!("Connexion Yoto en cours dans votre navigateur…"),
            tr!("Compte connecté. Chargement de vos playlists Yoto…"),
        ),
        Command::Disconnect(_) => (tr!("Déconnexion du compte…"), tr!("Compte déconnecté.")),
        Command::SelectAccount(_) => (tr!("Changement de compte…"), tr!("Compte sélectionné.")),
        Command::Sync(_) | Command::RefreshPlaylists(_) | Command::RefreshIcons(_) => (
            tr!("Chargement depuis Yoto…"),
            tr!("Données Yoto actualisées."),
        ),
        Command::Import(_) => (
            tr!("Ajout à la file d’import…"),
            tr!("Import lancé. Suivez sa progression dans « Imports »."),
        ),
        Command::CreateYotoPlaylist { .. } => (
            tr!("Création de la playlist sur Yoto…"),
            tr!("Playlist créée sur Yoto."),
        ),
        Command::DuplicateYotoPlaylist { .. } => (
            tr!("Copie de la playlist sur Yoto…"),
            tr!("Copie créée sur Yoto."),
        ),
        Command::LoadRemote { .. } => (
            tr!("Ouverture de la playlist…"),
            tr!("Playlist Yoto ouverte."),
        ),
        Command::DeleteRemote { .. } => (
            tr!("Suppression sur Yoto…"),
            tr!("Playlist supprimée de votre compte Yoto."),
        ),
        Command::DeleteGroup { .. } => (tr!("Suppression du groupe…"), tr!("Groupe supprimé.")),
        Command::DeleteMedia { .. } | Command::DeleteMediaBatch { .. } => (
            tr!("Suppression…"),
            tr!("Média supprimé de votre médiathèque."),
        ),
        Command::CollectGarbage => (tr!("Nettoyage…"), tr!("Fichiers inutilisés supprimés.")),
        Command::Retry(_) => (tr!("Relance de la tâche…"), tr!("Tâche relancée.")),
        Command::Cancel(_) => (tr!("Annulation…"), tr!("Tâche annulée.")),
        Command::KeepYotoVersion(_) => (
            tr!("Lecture de la playlist sur Yoto…"),
            tr!("Version de Yoto gardée : la playlist est à jour."),
        ),
        Command::Reconcile(_) => (
            tr!("Vérification auprès de Yoto…"),
            tr!("Yoto confirme l’envoi : la playlist est à jour."),
        ),
        Command::Resolve { .. } | Command::AttachPublicationReceipt { .. } => (
            tr!("Vérification auprès de Yoto…"),
            tr!("Vérification lancée."),
        ),
        Command::Backup { .. } => (
            tr!("Export de la médiathèque…"),
            tr!("Médiathèque exportée."),
        ),
        Command::Restore { .. } => (
            tr!("Import de la médiathèque…"),
            tr!("Médiathèque importée."),
        ),
        Command::Waveform(_)
        | Command::StopPreview
        | Command::PreviewStatus
        | Command::Preview(_)
        | Command::PreviewTrim { .. }
        | Command::ReleaseAsset(_)
        | Command::ReadPixelArt(_)
        | Command::Thumbnail(_)
        | Command::CoverPreview(_)
        | Command::AppUpdate(_)
        | Command::YoutubeTool(_)
        | Command::CancelConnect
        | Command::JobsPage { .. }
        | Command::DismissJobs(_) => return None,
        Command::InspectYoutube(_)
        | Command::NewPlaylist(_)
        | Command::SavePlaylist { .. }
        | Command::Duplicate(_)
        | Command::SaveCopy(_)
        | Command::DeletePlaylist(_)
        | Command::AddMedia { .. }
        | Command::DeleteRecipe { .. }
        | Command::UpdateMedia { .. }
        | Command::SetMediaIcon { .. }
        | Command::SetMediaChapters { .. }
        | Command::Prepare { .. }
        | Command::Export { .. }
        | Command::UpdateYotoPlaylist { .. }
        | Command::AddYotoMedia { .. }
        | Command::DeviceConfig { .. }
        | Command::DeviceStatus { .. }
        | Command::SaveDeviceConfig { .. }
        | Command::SaveShortcuts { .. }
        | Command::Publish { .. }
        | Command::Player { .. }
        | Command::RenameDevice { .. }
        | Command::SaveGroup { .. }
        | Command::LoadGroup { .. }
        | Command::FamilyImages(_)
        | Command::FamilyImage { .. }
        | Command::UploadFamilyImage { .. }
        | Command::SavePixelArt(_)
        | Command::StoreImage { .. }
        | Command::ImportIcon { .. }
        | Command::SetCover { .. } => (tr!("Traitement en cours…"), tr!("Terminé.")),
    })
}
fn remote_syncing(snapshot: &Snapshot) -> bool {
    snapshot
        .remote_sync
        .values()
        .any(|state| matches!(state, RemoteSyncState::Syncing))
}
fn remote_sync_message(snapshot: &Snapshot, collection: RemoteCollection) -> String {
    let label = wording::labels::remote_collection(collection);
    match snapshot.remote_sync.get(&collection) {
        None => tr!(
            "{label} · Synchroniser pour actualiser les données Yoto.",
            label = label
        ),
        Some(RemoteSyncState::Syncing) => tr!("{label} · Synchronisation en cours…", label = label),
        Some(RemoteSyncState::Ready { .. }) => tr!("{label} · À jour", label = label),
        Some(RemoteSyncState::Failed { failure }) => {
            tr!(
                "{label} · Actualisation impossible : {reason}",
                label = label,
                reason = failure_text(failure)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn dropped_files_keep_audio_whatever_the_case() {
        let paths: Vec<std::path::PathBuf> = [
            "a/conte.MP3",
            "b/chanson.m4a",
            "c/photo.jpg",
            "d/sans-extension",
            "e/Berceuse.FlAc",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        let (audio, ignored) = partition_audio(&paths);
        assert_eq!(
            audio,
            ["a/conte.MP3", "b/chanson.m4a", "e/Berceuse.FlAc"]
                .map(std::path::PathBuf::from)
                .to_vec()
        );
        assert_eq!(
            ignored,
            ["c/photo.jpg", "d/sans-extension"]
                .map(std::path::PathBuf::from)
                .to_vec()
        );
    }
}
