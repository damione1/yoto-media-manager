mod account;
mod device;
mod editor;
mod group;
mod imports;
mod library;
mod pixelart;
mod players;
mod reconciliation;
mod theme;
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
pub use wording::failure_text;
use wording::{confirm_destructive, duration_text};
use yoto_domain::*;
use yoto_infra::{Command, Reply, Session as Service, Snapshot};
pub struct AppView {
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
    pub recovery_key: Entity<InputState>,
    recovery_visible: bool,
    pub account_sync_details_open: bool,
    pub account_backup_open: bool,
    pub restored_profile: Option<std::path::PathBuf>,
    pub library_rows: Vec<usize>,
    playlist_media_choices: Arc<editor::MediaChoices>,
    playlist_icon_choices: Arc<pixelart::IconChoices>,
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
}
impl AppView {
    pub fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        let entering_playlists = self.route != Route::Playlists && route == Route::Playlists;
        self.route = route;
        self.error = None;
        self.status = "Prêt.".into();
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
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Rechercher dans vos médias…"));
        let new_title =
            cx.new(|cx| InputState::new(window, cx).placeholder("Titre de la playlist"));
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
        subscriptions.push(cx.observe(&updater, |_, _, cx| cx.notify()));
        let mut view = Self {
            updater,
            service,
            runtime,
            snapshot,
            revisions,
            search_index,
            job_page: None,
            job_offset: 0,
            receipt_card: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("ID de la carte Yoto à associer après déconnexion")
            }),
            route: Route::Library,
            selected_media: None,
            selected_media_set: Default::default(),
            sort: CatalogueSort::Title,
            search_generation: 0,
            recovery_visible: false,
            account_sync_details_open: false,
            account_backup_open: false,
            restored_profile: None,
            recovery_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder("Clé de récupération : 64 caractères hexadécimaux")
            }),
            library_rows,
            playlist_media_choices,
            playlist_icon_choices,
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
            status: "Votre collection personnelle, même hors ligne.".into(),
            connecting: false,
            creating_playlist: false,
            error: None,
            _tasks: vec![task],
            _subscriptions: subscriptions,
            group_editors: BTreeMap::new(),
            selected_group: None,
        };
        view.refresh_playlists(cx);
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
                    match service
                        .execute(Command::JobsPage { offset, limit: 200 })
                        .await?
                    {
                        Reply::Data(value) => serde_json::from_value::<Vec<Job>>(value)
                            .map_err(|e| Failure::Storage(e.to_string())),
                        Reply::AppUpdate(_)
                        | Reply::Updated
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
                        | Reply::Player(_) => {
                            Err(Failure::Compatibility("Unexpected job page reply".into()))
                        }
                    }
                })
                .await;
            let _ = weak.update(cx, |view, cx| {
                match result {
                    Ok(Ok(jobs)) => {
                        view.job_page = Some(Arc::new(jobs));
                        view.job_offset = offset;
                    }
                    Ok(Err(e)) => view.report_error(e, cx),
                    Err(e) => view.report_error(Failure::Storage(e.to_string()), cx),
                };
                cx.notify();
            });
        })
        .detach();
    }
    /// Show a failure where a parent will see it: the status line and a toast.
    pub fn report_error(&mut self, failure: Failure, cx: &mut Context<Self>) {
        let text = failure_text(&failure);
        self.status = text.clone();
        self.error = Some(failure);
        if let Some(window) = cx.windows().first().copied() {
            cx.defer(move |cx| {
                let _ = cx.update_window(window, |_, window, cx| {
                    use gpui_kit::component::{WindowExt, notification::Notification};
                    window.push_notification(Notification::error(text).autohide(false), cx);
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
                            Err(Failure::Authentication) if connecting => {
                                // The browser round trip timed out or was abandoned.
                                view.report_error(
                                    Failure::Validation(
                                        "La connexion Yoto n’a pas abouti. Cliquez de nouveau sur « Connecter mon compte Yoto », terminez la connexion dans votre navigateur, puis revenez ici.".into(),
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
                            | Reply::Updated
                            | Reply::Saved(_)
                            | Reply::Prepared(_)
                            | Reply::Asset(_)
                            | Reply::PixelArt(_)
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
                                view.status = "Playlist Yoto ouverte.".into();
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
                                view.status = "Enregistrement sur Yoto en cours…".into();
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
                                        "Commande envoyée ; effet non confirmé.".into()
                                    }
                                    yoto_infra::players::CommandResult::Accepted => {
                                        "Commande acceptée par le lecteur.".into()
                                    }
                                    yoto_infra::players::CommandResult::Observed { .. } => {
                                        "État du lecteur reçu.".into()
                                    }
                                };
                            }
                            Reply::Data(value) => {
                                if let Some(profile) = value
                                    .get("restored_profile")
                                    .and_then(serde_json::Value::as_str)
                                {
                                    view.restored_profile = Some(profile.into());
                                    view.status = format!("Profil restauré : {profile}");
                                } else if let Some(backup) =
                                    value.get("backup").and_then(serde_json::Value::as_str)
                                {
                                    view.status = format!("Sauvegarde créée : {backup}");
                                } else {
                                    view.status = "Opération terminée.".into();
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
        cx.spawn(async move|weak,cx|{
            let result=runtime.spawn_blocking(move|| {let profile=yoto_infra::canonical_profile(&path)?;if !profile.join("catalogue.db").is_file(){return Err(Failure::Validation("Choisissez un profil Yoto existant contenant catalogue.db".into()));}let mut command=std::process::Command::new(std::env::current_exe().map_err(|e|Failure::Storage(e.to_string()))?);command.env("YOTO_PROFILE",profile);if cfg!(debug_assertions)&&std::env::args().any(|a|a=="--isolated"){command.arg("--isolated");}command.spawn().map_err(|e|Failure::Storage(e.to_string()))?;Ok(())}).await;
            let _=weak.update(cx,|v,cx|{match result {Ok(Ok(()))=>v.status="La médiathèque choisie s’ouvre dans une autre fenêtre ; celle-ci reste disponible.".into(),Ok(Err(e))=>v.report_error(e,cx),Err(e)=>v.report_error(Failure::Storage(e.to_string()),cx)}cx.notify();});
        }).detach();
    }
    pub fn choose_profile(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |weak, cx| {
            if let Some(folder) = rfd::AsyncFileDialog::new()
                .set_title("Ouvrir une autre médiathèque Yoto")
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
        let (audio, ignored): (Vec<_>, Vec<_>) = paths.iter().cloned().partition(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        });
        if audio.is_empty() {
            self.report_error(
                Failure::Validation(
                    "Déposez des fichiers audio (MP3, M4A, WAV, FLAC, OGG…). Les autres fichiers sont ignorés.".into(),
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
            let text = format!(
                "{} fichier(s) ignoré(s) : seuls les fichiers audio (MP3, M4A, WAV, FLAC, OGG…) sont importés.",
                ignored.len()
            );
            if let Some(window) = cx.windows().first().copied() {
                cx.defer(move |cx| {
                    let _ = cx.update_window(window, |_, window, cx| {
                        use gpui_kit::component::{WindowExt, notification::Notification};
                        window.push_notification(Notification::warning(text).autohide(false), cx);
                    });
                });
            }
        }
    }
    pub fn add_files(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |weak, cx| {
            let files = rfd::AsyncFileDialog::new()
                .set_title("Ajouter vos fichiers audio")
                .add_filter("Audio", AUDIO_EXTENSIONS)
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
    pub fn select_media(&mut self, id: MediaId, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_media != Some(id) {
            self.send(Command::StopPreview, cx);
        }
        self.selected_media = Some(id);
        if !self.trims.contains_key(&id)
            && let Some(media) = self.snapshot.media.iter().find(|m| m.id == id)
        {
            let panel = cx.new(|cx| {
                trim::TrimPanel::new(
                    media.clone(),
                    self.service.clone(),
                    self.runtime.clone(),
                    window,
                    cx,
                )
            });
            panel.update(cx, |p, cx| p.apply_snapshot(&self.snapshot, cx));
            self.trims.insert(id, panel);
        }
        self.send(Command::Waveform(id), cx);
    }
    pub fn select_playlist(&mut self, id: PlaylistId, window: &mut Window, cx: &mut Context<Self>) {
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
            self.editors.insert(id, editor);
        }
        cx.notify();
    }
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
                    .px(px(20.))
                    .py(px(6.))
                    .border_t_2()
                    .border_color(theme::c(theme::DIVIDER))
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
    }
}
impl AppView {
    fn recovery(&mut self, restore: bool, cx: &mut Context<Self>) {
        let key = self.recovery_key.read(cx).value().to_string();
        cx.spawn(async move |weak, cx| {
            let source = if restore {
                rfd::AsyncFileDialog::new()
                    .set_title("Dossier de sauvegarde chiffrée")
                    .pick_folder()
                    .await
                    .map(|f| f.path().to_owned())
            } else {
                None
            };
            if restore && source.is_none() {
                return;
            }
            let parent = rfd::AsyncFileDialog::new()
                .set_title("Dossier parent (un nouveau dossier Yoto sera créé)")
                .pick_folder()
                .await;
            if let Some(parent) = parent {
                let destination = parent.path().join(format!(
                    "Yoto-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                ));
                let command = match source {
                    Some(source) => Command::Restore {
                        source,
                        destination,
                        recovery_key: key,
                    },
                    None => Command::Backup {
                        destination,
                        recovery_key: key,
                    },
                };
                let _ = weak.update(cx, |v, cx| v.send(command, cx));
            }
        })
        .detach();
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
                .child(theme::heading("Yoto Atelier", 20.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::c(theme::MUTED_WARM))
                        .child("Votre audio. Vos histoires."),
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
        let item = Button::new(route.label())
            .custom(variant)
            .accessibility_label(if count > 0 {
                format!("{} ({count})", route.label())
            } else {
                route.label().to_owned()
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
                    .child(div().flex_1().child(route.label()))
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
            "Connecté à Yoto",
            true,
        ),
        None => ("?".into(), "Compte Yoto".into(), "Non connecté", false),
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
        .child(theme::pill(
            Button::new("updates-sidebar")
                .label(view.updater.read(cx).sidebar_label())
                .on_click(cx.listener(|view, _, window, cx| {
                    let updater = view.updater.clone();
                    // Opening a Kit dialog mutates the window root; defer outside the AppView borrow.
                    window.defer(cx, move |window, cx| {
                        update::UpdatePanel::open(updater, window, cx)
                    });
                })),
            theme::Look::Ghost,
            theme::Height::Small,
            false,
            cx,
        ))
        .child(account_card)
}

fn element_id(label: &str, id: impl std::fmt::Display) -> ElementId {
    SharedString::from(format!("{label}:{id}")).into()
}
/// Status line while a command runs and once it succeeds. `None` keeps the line as it is:
/// background lookups (waveform, preview) must not claim anything was saved.
fn command_status(command: &Command) -> Option<(&'static str, &'static str)> {
    Some(match command {
        Command::Connect => (
            "Connexion Yoto en cours dans votre navigateur…",
            "Compte connecté. Chargement de vos playlists Yoto…",
        ),
        Command::Disconnect(_) => ("Déconnexion du compte…", "Compte déconnecté."),
        Command::SelectAccount(_) => ("Changement de compte…", "Compte sélectionné."),
        Command::Sync(_) | Command::RefreshPlaylists(_) | Command::RefreshIcons(_) => {
            ("Chargement depuis Yoto…", "Données Yoto actualisées.")
        }
        Command::Import(_) => (
            "Ajout à la file d’import…",
            "Import lancé. Suivez sa progression dans « Imports ».",
        ),
        Command::CreateYotoPlaylist { .. } => (
            "Création de la playlist sur Yoto…",
            "Playlist créée sur Yoto.",
        ),
        Command::DuplicateYotoPlaylist { .. } => {
            ("Copie de la playlist sur Yoto…", "Copie créée sur Yoto.")
        }
        Command::LoadRemote { .. } => ("Ouverture de la playlist…", "Playlist Yoto ouverte."),
        Command::DeleteRemote { .. } => (
            "Suppression sur Yoto…",
            "Playlist supprimée de votre compte Yoto.",
        ),
        Command::DeleteGroup { .. } => ("Suppression du groupe…", "Groupe supprimé."),
        Command::DeleteMedia { .. } | Command::DeleteMediaBatch { .. } => {
            ("Suppression…", "Média supprimé de votre médiathèque.")
        }
        Command::CollectGarbage => ("Nettoyage…", "Fichiers inutilisés supprimés."),
        Command::Retry(_) => ("Relance de la tâche…", "Tâche relancée."),
        Command::Cancel(_) => ("Annulation…", "Tâche annulée."),
        Command::Resolve { .. }
        | Command::Reconcile(_)
        | Command::AttachPublicationReceipt { .. } => {
            ("Vérification auprès de Yoto…", "Vérification lancée.")
        }
        Command::Backup { .. } => ("Sauvegarde en cours…", "Sauvegarde terminée."),
        Command::Restore { .. } => ("Restauration en cours…", "Restauration terminée."),
        Command::Waveform(_)
        | Command::StopPreview
        | Command::PreviewStatus
        | Command::Preview(_)
        | Command::PreviewTrim { .. }
        | Command::ReleaseAsset(_)
        | Command::ReadPixelArt(_)
        | Command::Thumbnail(_)
        | Command::AppUpdate(_)
        | Command::JobsPage { .. } => return None,
        Command::InspectYoutube(_)
        | Command::NewPlaylist(_)
        | Command::SavePlaylist { .. }
        | Command::Duplicate(_)
        | Command::SaveCopy(_)
        | Command::DeletePlaylist(_)
        | Command::AddMedia { .. }
        | Command::DeleteRecipe { .. }
        | Command::UpdateMedia { .. }
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
        | Command::SetCover { .. } => ("Traitement en cours…", "Terminé."),
    })
}
fn account_summary(snapshot: &Snapshot) -> String {
    match snapshot
        .active_account
        .as_ref()
        .and_then(|id| snapshot.accounts.iter().find(|a| &a.id == id))
    {
        Some(account) => format!("Connecté à Yoto · {}", account.label),
        None => "Aucun compte Yoto actif. Connectez ou sélectionnez un compte.".into(),
    }
}
fn remote_syncing(snapshot: &Snapshot) -> bool {
    snapshot
        .remote_sync
        .values()
        .any(|state| matches!(state, RemoteSyncState::Syncing))
}
fn remote_sync_message(snapshot: &Snapshot, collection: RemoteCollection) -> String {
    let label = collection.label();
    match snapshot.remote_sync.get(&collection) {
        None => format!("{label} · Synchroniser pour actualiser les données Yoto."),
        Some(RemoteSyncState::Syncing) => format!("{label} · Synchronisation en cours…"),
        Some(RemoteSyncState::Ready { .. }) => format!("{label} · À jour"),
        Some(RemoteSyncState::Failed { failure }) => {
            format!(
                "{label} · Actualisation impossible : {}",
                failure_text(failure)
            )
        }
    }
}
