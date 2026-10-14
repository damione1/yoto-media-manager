use crate::{
    api::{Group, Icon, RemoteCard, YotoGateway},
    assets::{AssetStore, Lease},
    auth::TokenBroker,
    db::{Database, Table},
    media::Tools,
    now, storage,
};
use fs2::FileExt;
use std::{
    collections::BTreeMap,
    fs::File,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;
use yoto_domain::*;
#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub active_account: Option<AccountId>,
    pub media: Vec<MediaItem>,
    pub recipes: Vec<MediaRecipe>,
    pub playlists: Vec<Playlist>,
    pub jobs: Vec<Job>,
    pub accounts: Vec<Account>,
    pub remote: Vec<RemoteCard>,
    pub devices: Vec<Device>,
    pub groups: Vec<Group>,
    pub icons: Vec<Icon>,
    #[serde(default)]
    pub remote_sync: BTreeMap<RemoteCollection, RemoteSyncState>,
    pub mappings: Vec<PublicationMapping>,
    pub bindings: Vec<PublicationBinding>,
    #[serde(default)]
    pub sync_states: BTreeMap<RecipeId, MediaSyncState>,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub enum Command {
    AppUpdate(yoto_updater::UpdateAction),
    /// Report or update the YouTube tool used for imports.
    YoutubeTool(crate::youtube_tool::YoutubeToolAction),
    Import(Vec<ImportSelection>),
    InspectYoutube(String),
    NewPlaylist(String),
    SavePlaylist {
        draft: Box<Playlist>,
        expected: Revision,
    },
    Duplicate(PlaylistId),
    SaveCopy(Box<Playlist>),
    DeletePlaylist(PlaylistId),
    AddMedia {
        playlist: PlaylistId,
        media: Vec<(MediaId, RecipeId)>,
        /// As in [`Command::AddYotoMedia`].
        #[serde(default)]
        part_title: Option<String>,
    },
    DeleteMedia {
        media: MediaId,
        policy: ReferencePolicy,
    },
    DeleteRecipe {
        recipe: RecipeId,
        policy: ReferencePolicy,
    },
    DeleteMediaBatch {
        media: Vec<MediaId>,
        policy: ReferencePolicy,
    },
    CollectGarbage,
    /// Release an unattached image lease. Referenced assets remain rooted; GC reclaims only unused assets.
    ReleaseAsset(AssetId),
    /// Historical jobs, newest first. Page size must be 1..=200.
    JobsPage {
        offset: usize,
        limit: u16,
    },
    /// Portable encrypted directory backup; recovery_key is 64 hex characters of random key material, never returned in outputs.
    Backup {
        destination: PathBuf,
        recovery_key: String,
    },
    /// Restore into a new profile. Existing/active profiles are never overwritten.
    Restore {
        source: PathBuf,
        destination: PathBuf,
        recovery_key: String,
    },
    UpdateMedia {
        media: MediaId,
        metadata: MediaMetadata,
    },
    Prepare {
        media: MediaId,
        trim: TrimRange,
        normalization: Normalization,
    },
    Waveform(MediaId),
    Preview(RecipeId),
    PreviewTrim {
        media: MediaId,
        trim: TrimRange,
    },
    StopPreview,
    PreviewStatus,
    Export {
        media: MediaId,
        destination: PathBuf,
    },
    Connect,
    /// Stop waiting for the browser sign-in started by `Connect`.
    CancelConnect,
    Disconnect(AccountId),
    SelectAccount(AccountId),
    Sync(AccountId),
    RefreshPlaylists(AccountId),
    RefreshIcons(AccountId),
    CreateYotoPlaylist {
        account: AccountId,
        title: String,
    },
    UpdateYotoPlaylist {
        account: AccountId,
        draft: Box<Playlist>,
        expected: Revision,
    },
    AddYotoMedia {
        account: AccountId,
        card: CardId,
        media: Vec<(MediaId, RecipeId)>,
        /// Title of each part of a media too long for one track, in the client's language;
        /// `{title}`, `{part}` and `{parts}` are replaced. `None` uses [`PART_TITLE`].
        #[serde(default)]
        part_title: Option<String>,
    },
    DuplicateYotoPlaylist {
        account: AccountId,
        card: CardId,
    },
    DeviceConfig {
        account: AccountId,
        device: DeviceId,
    },
    DeviceStatus {
        account: AccountId,
        device: DeviceId,
    },
    SaveDeviceConfig {
        account: AccountId,
        device: DeviceId,
        patch: Box<DeviceConfiguration>,
    },
    SaveShortcuts {
        account: AccountId,
        device: DeviceId,
        /// None preserves the existing mode; Some([]) explicitly clears it.
        day: Option<Vec<Shortcut>>,
        night: Option<Vec<Shortcut>>,
    },
    LoadRemote {
        account: AccountId,
        card: CardId,
    },
    DeleteRemote {
        account: AccountId,
        card: CardId,
    },
    Publish {
        account: AccountId,
        playlist: PlaylistId,
    },
    Retry(JobId),
    Reconcile(JobId),
    /// After logout purged a private checkpoint, explicitly select its remote receipt; never re-POST.
    AttachPublicationReceipt {
        job: JobId,
        card: CardId,
    },
    Cancel(JobId),
    Player {
        account: AccountId,
        device: DeviceId,
        command: PlayerCommand,
    },
    RenameDevice {
        account: AccountId,
        device: DeviceId,
        name: String,
    },
    SaveGroup {
        account: AccountId,
        id: Option<GroupId>,
        name: String,
        cards: Vec<CardId>,
        image: Option<FamilyImageId>,
    },
    DeleteGroup {
        account: AccountId,
        id: GroupId,
    },
    FamilyImages(AccountId),
    FamilyImage {
        account: AccountId,
        image: FamilyImageId,
        size: FamilyImageSize,
    },
    LoadGroup {
        account: AccountId,
        id: GroupId,
    },
    UploadFamilyImage {
        account: AccountId,
        path: PathBuf,
    },
    SavePixelArt(Box<PixelArt>),
    ReadPixelArt(AssetId),
    Thumbnail(AssetId),
    StoreImage {
        path: PathBuf,
        pixel_art: bool,
    },
    ImportIcon {
        path: PathBuf,
        playlist: PlaylistId,
        chapter: ChapterId,
        track: Option<TrackId>,
    },
    SetCover {
        path: PathBuf,
        playlist: PlaylistId,
    },
    Resolve {
        job: JobId,
        choice: ConflictChoice,
    },
}
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub enum ConflictChoice {
    KeepLocal,
    UseRemote,
    CreateCopy,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub enum Reply {
    AppUpdate(std::sync::Arc<yoto_updater::UpdateState>),
    YoutubeTool(crate::youtube_tool::YoutubeToolStatus),
    Updated,
    Saved(Revision),
    OpenedPlaylist(PlaylistId),
    PlaylistOperation { playlist: PlaylistId, job: JobId },
    Entries(SourceInspection),
    Waveform(Waveform),
    Prepared(RecipeId),
    Asset(AssetId),
    PixelArt(Box<PixelArt>),
    Image(FamilyImageId),
    Images(Vec<FamilyImageId>),
    Player(crate::players::CommandResult),
    Data(serde_json::Value),
}
/// Typed access to the reply a command is documented to return. Any other reply is a
/// compatibility failure, so callers handle one success shape and the usual failures.
impl Reply {
    fn unexpected() -> Failure {
        Failure::Compatibility(messages::UNEXPECTED_REPLY.into())
    }
    pub fn into_unit(self) -> Result<()> {
        if let Self::Updated = self {
            Ok(())
        } else {
            Err(Self::unexpected())
        }
    }
    pub fn into_data(self) -> Result<serde_json::Value> {
        if let Self::Data(value) = self {
            Ok(value)
        } else {
            Err(Self::unexpected())
        }
    }
    pub fn into_asset(self) -> Result<AssetId> {
        if let Self::Asset(asset) = self {
            Ok(asset)
        } else {
            Err(Self::unexpected())
        }
    }
    pub fn into_pixel_art(self) -> Result<PixelArt> {
        if let Self::PixelArt(art) = self {
            Ok(*art)
        } else {
            Err(Self::unexpected())
        }
    }
    pub fn into_entries(self) -> Result<SourceInspection> {
        if let Self::Entries(entries) = self {
            Ok(entries)
        } else {
            Err(Self::unexpected())
        }
    }
    pub fn into_app_update(self) -> Result<Arc<yoto_updater::UpdateState>> {
        if let Self::AppUpdate(state) = self {
            Ok(state)
        } else {
            Err(Self::unexpected())
        }
    }
    pub fn into_youtube_tool(self) -> Result<crate::youtube_tool::YoutubeToolStatus> {
        if let Self::YoutubeTool(status) = self {
            Ok(status)
        } else {
            Err(Self::unexpected())
        }
    }
    pub fn into_operation(self) -> Result<(PlaylistId, JobId)> {
        if let Self::PlaylistOperation { playlist, job } = self {
            Ok((playlist, job))
        } else {
            Err(Self::unexpected())
        }
    }
}
type InterfaceBytes = (String, Vec<u64>, Arc<Vec<u8>>);
type PlayerOwners =
    tokio::sync::Mutex<BTreeMap<(AccountId, DeviceId), Arc<tokio::sync::Mutex<()>>>>;
pub struct Service {
    pub db: Database,
    pub store: AssetStore,
    pub tools: Tools,
    pub auth: TokenBroker,
    pub api: YotoGateway,
    snapshot_bytes: Mutex<Option<(String, Arc<Vec<u8>>)>>,
    pub(crate) interface_bytes: Mutex<Option<InterfaceBytes>>,
    notify: Notify,
    _lock: File,
    preview: std::sync::mpsc::Sender<PreviewCommand>,
    playback: Arc<Mutex<PreviewStatus>>,
    preview_cancel: Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
    pub(crate) remote: Mutex<AccountCaches>,
    asset_mutation: Mutex<()>,
    cloud_mutation: tokio::sync::Mutex<()>,
    jobs: Mutex<BTreeMap<JobId, Arc<std::sync::atomic::AtomicBool>>>,
    login_gate: tokio::sync::RwLock<()>,
    login_session: Mutex<Arc<AccountSession>>,
    sessions: Mutex<BTreeMap<AccountId, Arc<AccountSession>>>,
    pub(crate) updater: Arc<yoto_updater::Updater>,
    /// The YouTube tool imports run: the bundled copy or a newer verified download.
    pub youtube: Arc<crate::youtube_tool::YoutubeTool>,
    players: PlayerOwners,
    update_gate: tokio::sync::RwLock<()>,
    update_shutdown: std::sync::atomic::AtomicBool,
}
/// A publication job shared by the stages of its upload pipeline.
type JobCell = Arc<Mutex<Job>>;
/// A prepared track on its way to Yoto, with the mapping of an earlier upload if any.
struct Outgoing {
    recipe: RecipeId,
    title: String,
    asset: Asset,
    key: String,
    known: Option<PublicationMapping>,
}
/// A track after its upload stage.
enum Sent {
    /// Yoto already had this exact audio.
    Known(PublicationMapping),
    Uploaded {
        track: Outgoing,
        upload: UploadId,
    },
}
/// How waiting for Yoto's conversion of one upload ended.
enum TranscodeEnd {
    Ready(Box<crate::api::Ingested>),
    Stalled,
    OutOfTime,
    Failed,
}
struct AccountSession {
    cancelled: std::sync::atomic::AtomicBool,
    notify: Notify,
    gate: Arc<tokio::sync::RwLock<()>>,
}
impl AccountSession {
    fn new() -> Self {
        Self {
            cancelled: false.into(),
            notify: Notify::new(),
            gate: Arc::new(tokio::sync::RwLock::new(())),
        }
    }
    async fn cancelled(&self) {
        let wait = self.notify.notified();
        if !self.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            wait.await;
        }
    }
    fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_waiters();
    }
}
#[derive(Default)]
pub(crate) struct AccountCaches {
    pub(crate) active: Option<AccountId>,
    pub(crate) generation: u64,
    epoch: u64,
    pub(crate) by_account: BTreeMap<AccountId, RemoteSnapshot>,
}
#[derive(Default)]
pub(crate) struct RemoteSnapshot {
    pub(crate) cards: Vec<RemoteCard>,
    pub(crate) devices: Vec<Device>,
    pub(crate) groups: Vec<Group>,
    pub(crate) icons: Vec<Icon>,
    pub(crate) sync: BTreeMap<RemoteCollection, RemoteSyncState>,
    sync_generation: u64,
    icons_generation: u64,
}
enum CollectionResult {
    Cards(Vec<RemoteCard>),
    Devices(Vec<Device>),
    Groups(Vec<Group>),
    Icons(Vec<Icon>),
}
fn apply_collection(
    current: &mut RemoteSnapshot,
    collection: RemoteCollection,
    result: Result<CollectionResult>,
    failure: &mut Option<Failure>,
) {
    let state = match result {
        Ok(result) => {
            match result {
                CollectionResult::Cards(v) => current.cards = v,
                CollectionResult::Devices(v) => current.devices = v,
                CollectionResult::Groups(v) => current.groups = v,
                CollectionResult::Icons(v) => current.icons = v,
            }
            RemoteSyncState::Ready { synced: now() }
        }
        Err(error) => {
            if failure.is_none() {
                *failure = Some(error.clone());
            }
            RemoteSyncState::Failed { failure: error }
        }
    };
    current.sync.insert(collection, state);
}
enum PreviewCommand {
    Play {
        lease: Lease,
        cancel: Arc<std::sync::atomic::AtomicBool>,
        reply: std::sync::mpsc::Sender<Result<()>>,
        media: MediaId,
        start: MediaTime,
    },
    Stop,
}
impl Service {
    pub fn open(profile: PathBuf, tools: Tools) -> Result<Arc<Self>> {
        let profile = crate::canonical_profile(&profile)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(profile.join("instance.lock"))
            .map_err(storage)?;
        lock.try_lock_exclusive().map_err(|_| {
            Failure::Storage(
                "This catalogue is already open in another application instance.".into(),
            )
        })?;
        let key = crate::assets::library_key(&profile)?;
        Self::with_key(profile, tools, key, lock)
    }
    pub fn isolated(profile: PathBuf, tools: Tools, key: [u8; 32]) -> Result<Arc<Self>> {
        let profile = crate::canonical_profile(&profile)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(profile.join("instance.lock"))
            .map_err(storage)?;
        lock.try_lock_exclusive().map_err(storage)?;
        Self::with_key(profile, tools, key, lock)
    }
    fn with_key(profile: PathBuf, tools: Tools, key: [u8; 32], lock: File) -> Result<Arc<Self>> {
        let db = Database::open(&profile.join("catalogue.db"), &key)?;
        let namespace = crate::profile_namespace(&profile);
        let youtube = Arc::new(crate::youtube_tool::YoutubeTool::new(
            profile.join("tools").join("yt-dlp"),
            tools.youtube.clone(),
        ));
        let store = AssetStore::new(profile, key)?;
        let (tx, rx) = std::sync::mpsc::channel();
        let playback = Arc::new(Mutex::new(PreviewStatus::Stopped));
        let audio_state = playback.clone();
        std::thread::Builder::new()
            .name("yoto-audio-preview".into())
            .spawn(move || preview_loop(rx, audio_state))
            .map_err(storage)?;
        let mut caches = AccountCaches {
            active: db_account(&db)?,
            ..Default::default()
        };
        for account in db.list::<Account>(Table::Account)? {
            caches.by_account.insert(
                account.id.clone(),
                RemoteSnapshot {
                    cards: db.remote_cards(&account.id)?,
                    ..Default::default()
                },
            );
        }
        let service = Arc::new(Self {
            remote: Mutex::new(caches),
            db,
            store,
            tools,
            auth: TokenBroker::new(namespace)?,
            api: YotoGateway::new()?,
            snapshot_bytes: Mutex::new(None),
            interface_bytes: Mutex::new(None),
            notify: Notify::new(),
            _lock: lock,
            preview: tx,
            playback,
            preview_cancel: Mutex::new(None),
            asset_mutation: Mutex::new(()),
            cloud_mutation: tokio::sync::Mutex::new(()),
            jobs: Mutex::new(BTreeMap::new()),
            login_gate: tokio::sync::RwLock::new(()),
            login_session: Mutex::new(Arc::new(AccountSession::new())),
            sessions: Mutex::new(BTreeMap::new()),
            updater: yoto_updater::Updater::new().map_err(storage)?,
            youtube,
            players: tokio::sync::Mutex::new(BTreeMap::new()),
            update_gate: tokio::sync::RwLock::new(()),
            update_shutdown: false.into(),
        });
        for mut job in service.db.list::<Job>(Table::Job)? {
            match service.db.publication_outcome(&job)? {
                Some(PublicationOutcome::Possible | PublicationOutcome::Accepted { .. }) => {
                    job.state = JobState::Reconciling;
                    service.save_job(&job)?;
                    continue;
                }
                Some(PublicationOutcome::Confirmed { card, revision }) => {
                    job.state = JobState::Completed {
                        output: JobOutput::Published { card, revision },
                    };
                    job.checkpoint = None;
                    service.save_job(&job)?;
                    continue;
                }
                Some(PublicationOutcome::Prepared | PublicationOutcome::Rejected) | None => {}
            }
            match &job.state {
                JobState::Running { phase } => {
                    job.state = match phase {
                        JobPhase::Committing | JobPhase::Verifying { .. } => JobState::Reconciling,
                        JobPhase::Acquiring
                        | JobPhase::Analyzing
                        | JobPhase::Encoding
                        | JobPhase::Uploading { .. }
                        | JobPhase::Ingesting { .. } => JobState::Queued,
                    };
                    service.save_job(&job)?;
                }
                JobState::Queued
                | JobState::Retry { .. }
                | JobState::WaitingForAuthentication
                | JobState::Conflict { .. }
                | JobState::Reconciling
                | JobState::Completed { .. }
                | JobState::Failed { .. }
                | JobState::Cancelled => {}
            }
        }
        service.collect_garbage()?;
        Ok(service)
    }
    pub fn revision(&self) -> Result<String> {
        Ok(format!(
            "{}:{}",
            self.db.generation(),
            self.remote.lock().map_err(storage)?.generation
        ))
    }
    pub fn snapshot_chunk(
        &self,
        revision: Option<String>,
        offset: usize,
    ) -> Result<crate::bridge::SnapshotChunk> {
        let mut cache = self.snapshot_bytes.lock().map_err(storage)?;
        let current = self.revision()?;
        if cache.as_ref().is_none_or(|(rev, _)| rev != &current) {
            let mut stable = None;
            for _ in 0..5 {
                let before = self.revision()?;
                let bytes = serde_json::to_vec(&self.snapshot()?).map_err(storage)?;
                if self.revision()? == before {
                    stable = Some((before, Arc::new(bytes)));
                    break;
                }
            }
            *cache = Some(stable.ok_or(Failure::Conflict)?);
        }
        let (current, bytes) = cache.as_ref().ok_or(Failure::Conflict)?;
        if revision.as_ref().is_some_and(|rev| rev != current) {
            return Err(Failure::Conflict);
        }
        if offset > bytes.len() {
            return Err(Failure::Validation(
                "Snapshot page offset is invalid".into(),
            ));
        }
        let end = (offset + 256_000).min(bytes.len());
        Ok(crate::bridge::SnapshotChunk {
            revision: current.clone(),
            data: {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.encode(&bytes[offset..end])
            },
            next: (end < bytes.len()).then_some(end),
        })
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let remote = self.remote.lock().map_err(storage)?;
        let empty = RemoteSnapshot::default();
        let current = remote
            .active
            .as_ref()
            .and_then(|id| remote.by_account.get(id))
            .unwrap_or(&empty);
        Ok(Snapshot {
            active_account: remote.active.clone(),
            media: self.db.list(Table::Media)?,
            recipes: self.db.list(Table::Recipe)?,
            playlists: self.db.list(Table::Playlist)?,
            jobs: self.db.list(Table::Job)?,
            accounts: self.db.list(Table::Account)?,
            remote: current.cards.clone(),
            devices: current.devices.clone(),
            groups: current.groups.clone(),
            icons: current.icons.clone(),
            remote_sync: current.sync.clone(),
            mappings: self.db.list(Table::Mapping)?,
            bindings: self.db.list(Table::Binding)?,
            sync_states: BTreeMap::new(),
        })
    }
    fn check_device(&self, account: &AccountId, device: &DeviceId) -> Result<()> {
        let cache = self.remote.lock().map_err(storage)?;
        if cache
            .by_account
            .get(account)
            .is_some_and(|c| c.devices.iter().any(|d| &d.id == device))
        {
            Ok(())
        } else {
            Err(Failure::Authentication)
        }
    }
    pub async fn prepare_update_shutdown(&self) -> Result<()> {
        let _gate = self.update_gate.write().await;
        if self
            .update_shutdown
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(Failure::Validation(messages::RESTART_PENDING.into()));
        }
        for job in self.db.list::<Job>(Table::Job)? {
            match job.state {
                JobState::Queued
                | JobState::Running { .. }
                | JobState::Retry { .. }
                | JobState::Reconciling => {
                    return Err(Failure::Validation(messages::RESTART_BUSY.into()));
                }
                JobState::WaitingForAuthentication
                | JobState::Conflict { .. }
                | JobState::Completed { .. }
                | JobState::Failed { .. }
                | JobState::Cancelled => {}
            }
        }
        self.update_shutdown
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    pub fn cancel_update_shutdown(&self) {
        self.update_shutdown
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
    pub fn shutdown(&self) {
        if let Ok(jobs) = self.jobs.lock() {
            for flag in jobs.values() {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let _ = self.stop_preview();
    }
    pub fn start(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        tokio::spawn(async move {
            loop {
                let db = service.clone();
                let jobs = tokio::task::spawn_blocking(move || db.db.list::<Job>(Table::Job)).await;
                let jobs = match jobs {
                    Ok(Ok(j)) => j,
                    Ok(Err(_)) | Err(_) => break,
                };
                for job in jobs {
                    let due = match &job.state {
                        JobState::Queued => true,
                        JobState::Retry { at, .. } => *at <= now(),
                        JobState::Running { .. }
                        | JobState::WaitingForAuthentication
                        | JobState::Conflict { .. }
                        | JobState::Reconciling
                        | JobState::Completed { .. }
                        | JobState::Failed { .. }
                        | JobState::Cancelled => false,
                    };
                    if due {
                        service.run_job(job).await;
                    }
                }
                tokio::select! {_ = service.notify.notified()=>{},_ = tokio::time::sleep(std::time::Duration::from_secs(1))=>{}}
            }
        })
    }
    fn account_session(&self, account: &AccountId) -> Result<Arc<AccountSession>> {
        if self
            .db
            .get::<Account>(Table::Account, account.as_str())?
            .is_none()
        {
            return Err(Failure::Authentication);
        }
        Ok(self
            .sessions
            .lock()
            .map_err(storage)?
            .entry(account.clone())
            .or_insert_with(|| Arc::new(AccountSession::new()))
            .clone())
    }
    fn command_account(&self, command: &Command) -> Result<Option<AccountId>> {
        Ok(match command {
            Command::SelectAccount(a)
            | Command::Sync(a)
            | Command::RefreshPlaylists(a)
            | Command::RefreshIcons(a)
            | Command::FamilyImages(a) => Some(a.clone()),
            Command::CreateYotoPlaylist { account, .. }
            | Command::UpdateYotoPlaylist { account, .. }
            | Command::AddYotoMedia { account, .. }
            | Command::DuplicateYotoPlaylist { account, .. }
            | Command::DeviceConfig { account, .. }
            | Command::DeviceStatus { account, .. }
            | Command::SaveDeviceConfig { account, .. }
            | Command::SaveShortcuts { account, .. }
            | Command::LoadRemote { account, .. }
            | Command::DeleteRemote { account, .. }
            | Command::Publish { account, .. }
            | Command::Player { account, .. }
            | Command::RenameDevice { account, .. }
            | Command::SaveGroup { account, .. }
            | Command::DeleteGroup { account, .. }
            | Command::FamilyImage { account, .. }
            | Command::LoadGroup { account, .. }
            | Command::UploadFamilyImage { account, .. } => Some(account.clone()),
            Command::Retry(id)
            | Command::Reconcile(id)
            | Command::Cancel(id)
            | Command::Resolve { job: id, .. }
            | Command::AttachPublicationReceipt { job: id, .. } => match self.job(*id)?.request {
                JobRequest::Publish { account, .. } => Some(account),
                JobRequest::Import { .. } | JobRequest::Encode { .. } => None,
            },
            Command::AppUpdate(_)
            | Command::YoutubeTool(_)
            | Command::Import(_)
            | Command::InspectYoutube(_)
            | Command::NewPlaylist(_)
            | Command::SavePlaylist { .. }
            | Command::Duplicate(_)
            | Command::SaveCopy(_)
            | Command::DeletePlaylist(_)
            | Command::AddMedia { .. }
            | Command::UpdateMedia { .. }
            | Command::DeleteMedia { .. }
            | Command::DeleteRecipe { .. }
            | Command::DeleteMediaBatch { .. }
            | Command::CollectGarbage
            | Command::ReleaseAsset(_)
            | Command::JobsPage { .. }
            | Command::Backup { .. }
            | Command::Restore { .. }
            | Command::Prepare { .. }
            | Command::Waveform(_)
            | Command::Preview(_)
            | Command::PreviewTrim { .. }
            | Command::StopPreview
            | Command::PreviewStatus
            | Command::Export { .. }
            | Command::Connect
            | Command::CancelConnect
            | Command::Disconnect(_)
            | Command::SavePixelArt(_)
            | Command::ReadPixelArt(_)
            | Command::Thumbnail(_)
            | Command::StoreImage { .. }
            | Command::ImportIcon { .. }
            | Command::SetCover { .. } => None,
        })
    }
    pub async fn execute(self: &Arc<Self>, command: Command) -> Result<Reply> {
        if let Command::AppUpdate(action) = &command {
            self.updater.request(*action);
            return Ok(Reply::AppUpdate(self.updater.state()));
        }
        if let Command::YoutubeTool(action) = &command {
            return Ok(Reply::YoutubeTool(self.youtube.request(*action)));
        }
        let _update_guard = self.update_gate.read().await;
        if self
            .update_shutdown
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(Failure::Validation(messages::RESTARTING.into()));
        }
        if matches!(&command, Command::StopPreview) {
            self.stop_preview()?;
            return Ok(Reply::Updated);
        }
        if matches!(&command, Command::PreviewStatus) {
            return Ok(Reply::Data(
                serde_json::to_value(&*self.playback.lock().map_err(storage)?).map_err(storage)?,
            ));
        }
        if matches!(&command, Command::CancelConnect) {
            self.login_session.lock().map_err(storage)?.cancel();
            // The running sign-in holds the gate until it has dropped its callback listener,
            // so a new `Connect` sent after this reply finds the port free.
            let _login = self.login_gate.write().await;
            return Ok(Reply::Updated);
        }
        if matches!(&command, Command::Connect) {
            let _login = self.login_gate.read().await;
            let session = {
                let mut session = self.login_session.lock().map_err(storage)?;
                if session.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                    *session = Arc::new(AccountSession::new());
                }
                session.clone()
            };
            return tokio::select! {biased;_=session.cancelled()=>Err(Failure::Cancelled),result=self.execute_inner(command)=>result};
        }
        if let Command::Disconnect(account) = &command {
            self.login_session.lock().map_err(storage)?.cancel();
            let _login = self.login_gate.write().await;
            let session = self.account_session(account)?;
            session.cancel();
            for job in self.db.list::<Job>(Table::Job)? {
                if matches!(&job.request,JobRequest::Publish{account:a,..} if a==account)
                    && let Some(flag) = self.jobs.lock().map_err(storage)?.get(&job.id)
                {
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
            let _owner = session.gate.write().await;
            return self.execute_inner(command).await;
        }
        let account = self.command_account(&command)?;
        if let Some(account) = account {
            let session = self.account_session(&account)?;
            let _owner = session.gate.read().await;
            if session.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(Failure::Cancelled);
            }
            tokio::select! { biased; _ = session.cancelled() => Err(Failure::Cancelled), result = self.execute_inner(command) => result }
        } else {
            self.execute_inner(command).await
        }
    }
    async fn activate_account_session(&self, account: &AccountId) -> Result<()> {
        let previous = self.sessions.lock().map_err(storage)?.get(account).cloned();
        if let Some(previous) = previous {
            if !previous.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(());
            }
            let _drained = previous.gate.write().await;
        }
        self.sessions
            .lock()
            .map_err(storage)?
            .insert(account.clone(), Arc::new(AccountSession::new()));
        Ok(())
    }
    async fn connected_account(&self, mut account: Account) -> Result<()> {
        // Reconnecting without a name from Yoto keeps the one read before.
        if account.label == crate::auth::fallback_label(&account.id)
            && let Some(known) = self
                .db
                .get::<Account>(Table::Account, account.id.as_str())?
        {
            account.label = known.label;
        }
        self.db.put(Table::Account, account.id.as_str(), &account)?;
        self.activate_account_session(&account.id).await?;
        self.select_account(account.id)?;
        Ok(())
    }
    fn select_account(&self, account: AccountId) -> Result<()> {
        if self
            .db
            .get::<Account>(Table::Account, account.as_str())?
            .is_none()
        {
            return Err(Failure::Authentication);
        }
        self.db.put(Table::Remote, "active-account", &account)?;
        let mut cache = self.remote.lock().map_err(storage)?;
        cache.active = Some(account);
        cache.epoch = cache.epoch.wrapping_add(1);
        cache.generation = cache.generation.wrapping_add(1);
        Ok(())
    }
    async fn execute_inner(self: &Arc<Self>, command: Command) -> Result<Reply> {
        // Admission and deletion must not cross while DELETE awaits the remote response.
        // Durable per-playlist journal exclusion takes over after a write is queued.
        let _cloud_mutation = if matches!(
            &command,
            Command::CreateYotoPlaylist { .. }
                | Command::UpdateYotoPlaylist { .. }
                | Command::AddYotoMedia { .. }
                | Command::DuplicateYotoPlaylist { .. }
                | Command::DeleteRemote { .. }
                | Command::Publish { .. }
        ) {
            Some(self.cloud_mutation.lock().await)
        } else {
            None
        };
        match command {
            Command::AppUpdate(_) | Command::YoutubeTool(_) | Command::CancelConnect => {
                unreachable!("Updater and sign-in commands were handled before mutation admission")
            }
            Command::Connect => {
                let account = self.auth.login().await?;
                self.connected_account(account.clone()).await?;
                for mut job in self.db.list::<Job>(Table::Job)? {
                    match job.state {
                        JobState::WaitingForAuthentication => {
                            if matches!(&job.request,JobRequest::Publish{account:id,..} if id==&account.id)
                            {
                                job.state = JobState::Queued;
                                match self.db.admit_job(&job) {
                                    Ok(()) => {}
                                    Err(Failure::Validation(_)) => {}
                                    Err(error) => return Err(error),
                                }
                            }
                        }
                        JobState::Queued
                        | JobState::Running { .. }
                        | JobState::Retry { .. }
                        | JobState::Conflict { .. }
                        | JobState::Reconciling
                        | JobState::Completed { .. }
                        | JobState::Failed { .. }
                        | JobState::Cancelled => {}
                    }
                }
                self.notify.notify_one();
                Ok(Reply::Updated)
            }
            Command::Disconnect(id) => {
                if self
                    .db
                    .get::<AccountId>(Table::Remote, "active-account")?
                    .as_ref()
                    == Some(&id)
                {
                    self.db.remove(Table::Remote, "active-account")?;
                }
                {
                    let mut cache = self.remote.lock().map_err(storage)?;
                    cache.generation = cache.generation.wrapping_add(1);
                    cache.epoch = cache.epoch.wrapping_add(1);
                    cache.by_account.remove(&id);
                    if cache.active.as_ref() == Some(&id) {
                        cache.active = None;
                    }
                }
                self.auth.disconnect(&id).await?;
                self.db.purge_account(&id)?;
                self.collect_garbage()?;
                Ok(Reply::Updated)
            }
            Command::SelectAccount(account) => {
                self.select_account(account)?;
                Ok(Reply::Updated)
            }
            Command::Sync(account) => {
                let (epoch, generation, icons_generation) = {
                    let mut cache = self.remote.lock().map_err(storage)?;
                    let current = cache.by_account.entry(account.clone()).or_default();
                    for collection in RemoteCollection::ALL {
                        current.sync.insert(*collection, RemoteSyncState::Syncing);
                    }
                    cache.generation = cache.generation.wrapping_add(1);
                    let epoch = cache.epoch;
                    let current = cache.by_account.entry(account.clone()).or_default();
                    current.sync_generation = current.sync_generation.wrapping_add(1);
                    current.icons_generation = current.icons_generation.wrapping_add(1);
                    (epoch, current.sync_generation, current.icons_generation)
                };
                // Independent collections must not discard successful MYO results when
                // another endpoint is unavailable or the account lacks its permission.
                let (cards, devices, groups, icons) = tokio::join!(
                    async {
                        let token = self.auth.access(&account, Scope::UserContentView).await?;
                        self.api.library(&token).await
                    },
                    async {
                        let token = self.auth.access(&account, Scope::FamilyDevicesView).await?;
                        self.api.devices(&token).await
                    },
                    async {
                        let token = self.auth.access(&account, Scope::FamilyLibraryView).await?;
                        self.api.groups(&token).await
                    },
                    async {
                        let token = self.auth.access(&account, Scope::UserIconsManage).await?;
                        self.api.icons(&token).await
                    },
                );
                let cards_loaded = cards.is_ok();
                let (failure, cards) = {
                    let mut cache = self.remote.lock().map_err(storage)?;
                    if cache.epoch != epoch
                        || cache
                            .by_account
                            .get(&account)
                            .is_none_or(|current| current.sync_generation != generation)
                        || self
                            .db
                            .get::<Account>(Table::Account, account.as_str())?
                            .is_none()
                    {
                        if let Some(current) = cache.by_account.get_mut(&account)
                            && current.sync_generation == generation
                        {
                            for state in current.sync.values_mut() {
                                if matches!(state, RemoteSyncState::Syncing) {
                                    *state = RemoteSyncState::Failed {
                                        failure: Failure::Cancelled,
                                    };
                                }
                            }
                            cache.generation = cache.generation.wrapping_add(1);
                        }
                        return Err(Failure::Cancelled);
                    }
                    let current = cache.by_account.entry(account.clone()).or_default();
                    if let Ok(cards) = &cards {
                        self.db.replace_remote_cards(&account, cards)?;
                    }
                    let mut failure = None;
                    apply_collection(
                        current,
                        RemoteCollection::Playlists,
                        cards.map(CollectionResult::Cards),
                        &mut failure,
                    );
                    apply_collection(
                        current,
                        RemoteCollection::Devices,
                        devices.map(CollectionResult::Devices),
                        &mut failure,
                    );
                    apply_collection(
                        current,
                        RemoteCollection::Groups,
                        groups.map(CollectionResult::Groups),
                        &mut failure,
                    );
                    if current.icons_generation == icons_generation {
                        apply_collection(
                            current,
                            RemoteCollection::Icons,
                            icons.map(CollectionResult::Icons),
                            &mut failure,
                        );
                    }
                    let cards = current.cards.clone();
                    cache.generation = cache.generation.wrapping_add(1);
                    (failure, cards)
                };
                let detail_failure = if cards_loaded {
                    self.reconcile_cloud_summaries(&account, &cards).await.err()
                } else {
                    None
                };
                if let Some(failure) = &detail_failure {
                    self.cloud_refresh_failed(&account, epoch, generation, failure)?;
                }
                if let Some(failure) = failure.or(detail_failure) {
                    Err(failure)
                } else {
                    Ok(Reply::Updated)
                }
            }
            Command::RefreshPlaylists(account) => self.refresh_cloud_playlists(&account).await,
            Command::RefreshIcons(account) => self.refresh_icons(&account).await,
            Command::CreateYotoPlaylist { account, title } => {
                self.auth.access(&account, Scope::UserContentManage).await?;
                self.enqueue_cloud(account, Playlist::new(title)?, CloudPlaylistWrite::Create)
            }
            Command::UpdateYotoPlaylist {
                account,
                mut draft,
                expected,
            } => {
                let current = self.playlist(draft.id)?;
                let binding = self
                    .db
                    .get::<PublicationBinding>(Table::Binding, &format!("{account}:{}", draft.id))?
                    .ok_or(Failure::Authentication)?;
                if current.revision != expected {
                    return Err(Failure::Conflict);
                }
                draft.origin = DraftOrigin::Existing {
                    base: RemoteBase {
                        account: account.clone(),
                        card: binding.card,
                        document: binding.document,
                    },
                };
                self.auth.access(&account, Scope::UserContentManage).await?;
                self.enqueue_cloud(account, *draft, CloudPlaylistWrite::Update { expected })
            }
            Command::AddYotoMedia {
                account,
                card,
                media,
                part_title,
            } => {
                if media.is_empty() {
                    return Err(Failure::Validation(messages::MEDIA_SELECTION_EMPTY.into()));
                }
                let id = self.load_cloud_playlist(&account, &card).await?;
                let mut draft = self.playlist(id)?;
                let expected = draft.revision;
                for (id, recipe) in media {
                    let media = self.media(id)?;
                    if !media.recipes.contains(&recipe) {
                        return Err(Failure::Validation(messages::RECIPE_NOT_OF_MEDIA.into()));
                    }
                    self.add_to_playlist(&mut draft, &media, recipe, part_title.as_deref())?;
                }
                self.auth.access(&account, Scope::UserContentManage).await?;
                self.enqueue_cloud(account, draft, CloudPlaylistWrite::Update { expected })
            }
            Command::DuplicateYotoPlaylist { account, card } => {
                let id = self.load_cloud_playlist(&account, &card).await?;
                let mut copy = self.playlist(id)?.duplicate()?;
                // RemoteAudio references already belong to this account and need no re-upload.
                copy.origin = DraftOrigin::New;
                self.auth.access(&account, Scope::UserContentManage).await?;
                self.enqueue_cloud(account, copy, CloudPlaylistWrite::Create)
            }
            Command::LoadRemote { account, card } => {
                let id = self.load_cloud_playlist(&account, &card).await?;
                Ok(Reply::OpenedPlaylist(id))
            }
            Command::DeleteRemote { account, card } => {
                for binding in self
                    .db
                    .list::<PublicationBinding>(Table::Binding)?
                    .iter()
                    .filter(|binding| binding.account == account && binding.card == card)
                {
                    if self.db.cloud_playlist_pending(&account, binding.playlist)? {
                        return Err(Failure::Validation(
                            "Une modification Yoto est en cours. Attendez sa confirmation.".into(),
                        ));
                    }
                }
                let token = self.auth.access(&account, Scope::UserContentManage).await?;
                self.api.delete_card(&token, &card).await?;
                {
                    let mut cache = self.remote.lock().map_err(storage)?;
                    cache
                        .by_account
                        .entry(account.clone())
                        .or_default()
                        .cards
                        .retain(|c| c.card != card);
                    cache.epoch = cache.epoch.wrapping_add(1);
                    cache.generation = cache.generation.wrapping_add(1);
                }
                self.db.evict_cloud_playlist(&account, &card)?;
                Ok(Reply::Updated)
            }
            Command::AttachPublicationReceipt { job: id, card } => {
                let mut job = self.job(id)?;
                if !matches!(job.state, JobState::Reconciling) {
                    return Err(Failure::Validation(
                        "Only an uncertain publication can accept a receipt".into(),
                    ));
                }
                let JobRequest::Publish { account, snapshot } = &job.request else {
                    return Err(Failure::Validation("Publication required".into()));
                };
                let pending: PendingPublication = self
                    .db
                    .get(Table::Remote, &format!("{account}:commit:{id}"))?
                    .ok_or(Failure::Ambiguous)?;
                if !pending.document.is_null() {
                    return Err(Failure::Validation(
                        "Use normal reconciliation while the complete checkpoint is available"
                            .into(),
                    ));
                }
                let token = self.auth.access(account, Scope::UserContentView).await?;
                let document = self.api.card(&token, &card).await?;
                let revision = self
                    .db
                    .get::<Playlist>(Table::Playlist, &snapshot.id.to_string())?
                    .map_or(snapshot.revision, |p| p.revision);
                let binding = PublicationBinding {
                    account: account.clone(),
                    playlist: snapshot.id,
                    card: card.clone(),
                    revision,
                    document,
                };
                self.db.confirm_publication(&job, &pending, &binding)?;
                self.install_confirmed_card(&binding.account)?;
                job.checkpoint = None;
                job.state = JobState::Completed {
                    output: JobOutput::Published { card, revision },
                };
                self.save_job(&job)?;
                Ok(Reply::Updated)
            }
            Command::Reconcile(id) => {
                let mut job = self.job(id)?;
                if !matches!(job.state, JobState::Reconciling) {
                    return Err(Failure::Validation(
                        "Only an ambiguous publication can be reconciled".into(),
                    ));
                }
                let JobRequest::Publish { account, snapshot } = &job.request else {
                    return Err(Failure::Validation("Publication required".into()));
                };
                let key = format!("{account}:commit:{id}");
                let mut pending: PendingPublication =
                    self.db.get(Table::Remote, &key)?.ok_or_else(|| {
                        Failure::Compatibility("Publication checkpoint missing".into())
                    })?;
                if pending.document.is_null() {
                    return Err(Failure::Compatibility(format!(
                        "Private checkpoint purged at logout. Explicitly attach the card receipt{}; this job will not be re-posted.",
                        pending
                            .outcome
                            .card()
                            .map(|c| format!(" {c}"))
                            .unwrap_or_default()
                    )));
                }
                let token = self.auth.access(account, Scope::UserContentView).await?;
                let mut candidates = vec![];
                match pending.outcome.card() {
                    Some(card) => {
                        let doc = self.api.card(&token, card).await?;
                        if contains_document(&doc, &pending.document) {
                            candidates.push((card.clone(), doc));
                        }
                    }
                    None => {
                        for card in self.api.library(&token).await? {
                            if card.title == snapshot.title {
                                let doc = self.api.card(&token, &card.card).await?;
                                if contains_document(&doc, &pending.document) {
                                    candidates.push((card.card, doc));
                                }
                            }
                        }
                    }
                }
                if candidates.len() != 1 {
                    return Err(Failure::Ambiguous);
                }
                let (card, document) = candidates.remove(0);
                pending.outcome = PublicationOutcome::Accepted { card: card.clone() };
                self.db.put(Table::Remote, &key, &pending)?;
                let binding = PublicationBinding {
                    account: account.clone(),
                    playlist: snapshot.id,
                    card: card.clone(),
                    revision: snapshot.revision,
                    document,
                };
                self.db.confirm_publication(&job, &pending, &binding)?;
                self.install_confirmed_card(&binding.account)?;
                job.checkpoint = None;
                job.state = JobState::Completed {
                    output: JobOutput::Published {
                        card,
                        revision: snapshot.revision,
                    },
                };
                self.save_job(&job)?;
                Ok(Reply::Updated)
            }
            Command::Player {
                account,
                device,
                command,
            } => {
                let token = self
                    .auth
                    .access(&account, Scope::FamilyDevicesControl)
                    .await?;
                let device = self
                    .remote
                    .lock()
                    .map_err(storage)?
                    .by_account
                    .get(&account)
                    .ok_or(Failure::Authentication)?
                    .devices
                    .iter()
                    .find(|d| d.id == device)
                    .cloned()
                    .ok_or_else(|| Failure::Validation(messages::SELECT_PLAYER.into()))?;
                Ok(Reply::Player({
                    let owner = self
                        .players
                        .lock()
                        .await
                        .entry((account.clone(), device.id.clone()))
                        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                        .clone();
                    let _guard = owner.lock().await;
                    crate::players::send(&device, &token, &command).await?
                }))
            }
            Command::DeviceConfig { account, device } => {
                self.check_device(&account, &device)?;
                let token = self
                    .auth
                    .access(&account, Scope::FamilyDevicesManage)
                    .await?;
                Ok(Reply::Data(
                    serde_json::to_value(self.api.device_config(&token, &device).await?)
                        .map_err(storage)?,
                ))
            }
            Command::DeviceStatus { account, device } => {
                self.check_device(&account, &device)?;
                let token = self
                    .auth
                    .access(&account, Scope::FamilyDeviceStatusView)
                    .await?;
                Ok(Reply::Data(self.api.device_status(&token, &device).await?))
            }
            Command::SaveDeviceConfig {
                account,
                device,
                patch,
            } => {
                self.check_device(&account, &device)?;
                let token = self
                    .auth
                    .access(&account, Scope::FamilyDevicesManage)
                    .await?;
                patch.validate()?;
                // Fetch immediately before merging, preserving options not managed by this editor.
                let old = self.api.device_config(&token, &device).await?;
                let request = crate::api::patch_device_config(old.device.config, &patch)?;
                self.api.update_device(&token, &device, request).await?;
                Ok(Reply::Data(
                    serde_json::to_value(self.api.device_config(&token, &device).await?)
                        .map_err(storage)?,
                ))
            }
            Command::SaveShortcuts {
                account,
                device,
                day,
                night,
            } => {
                self.check_device(&account, &device)?;
                let token = self
                    .auth
                    .access(&account, Scope::FamilyDevicesManage)
                    .await?;
                let old = self.api.device_config(&token, &device).await?;
                let shortcuts =
                    crate::api::patch_shortcuts(old.device.shortcuts.as_deref(), day, night)?;
                self.api.shortcuts(&token, &device, shortcuts).await?;
                Ok(Reply::Data(
                    serde_json::to_value(self.api.device_config(&token, &device).await?)
                        .map_err(storage)?,
                ))
            }
            Command::RenameDevice {
                account,
                device,
                name,
            } => {
                if name.trim().is_empty() {
                    return Err(Failure::Validation(messages::PLAYER_NAME_EMPTY.into()));
                }
                let token = self
                    .auth
                    .access(&account, Scope::FamilyDevicesManage)
                    .await?;
                self.check_device(&account, &device)?;
                let old = self.api.device_config(&token, &device).await?;
                self.api
                    .update_device(
                        &token,
                        &device,
                        yoto_api_generated::models::DeviceConfigWrite::new(old.device.config, name),
                    )
                    .await?;
                Ok(Reply::Updated)
            }
            Command::SaveGroup {
                account,
                id,
                name,
                cards,
                image,
            } => {
                let token = self
                    .auth
                    .access(&account, Scope::FamilyLibraryManage)
                    .await?;
                Ok(Reply::Data(
                    self.api
                        .save_group(
                            &token,
                            id.as_ref(),
                            &name,
                            &cards,
                            image.as_ref().map(FamilyImageId::as_str),
                        )
                        .await?,
                ))
            }
            Command::DeleteGroup { account, id } => {
                let token = self
                    .auth
                    .access(&account, Scope::FamilyLibraryManage)
                    .await?;
                self.api.delete_group(&token, &id).await?;
                Ok(Reply::Updated)
            }
            Command::FamilyImages(account) => {
                let token = self.auth.access(&account, Scope::FamilyLibraryView).await?;
                Ok(Reply::Images(self.api.family_images(&token).await?))
            }
            Command::LoadGroup { account, id } => {
                let token = self.auth.access(&account, Scope::FamilyLibraryView).await?;
                Ok(Reply::Data(self.api.group(&token, &id).await?))
            }
            Command::FamilyImage {
                account,
                image,
                size,
            } => {
                let token = self.auth.access(&account, Scope::FamilyLibraryView).await?;
                let bytes = self.api.family_image(&token, &image, size).await?;
                let service = self.clone();
                let session = self.account_session(&account)?;
                let gate = session.gate.clone().read_owned().await;
                tokio::task::spawn_blocking(move || {
                    let _gate = gate;
                    let _assets = service.protect_assets()?;
                    if session.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                        return Err(Failure::Cancelled);
                    }
                    let lease = service.store.lease("png")?;
                    std::fs::write(lease.path(), bytes).map_err(storage)?;
                    crate::media::read_image(lease.path())?;
                    if session.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                        return Err(Failure::Cancelled);
                    }
                    let asset = service.store.put(lease.path(), "png")?;
                    service.db.pin_draft_asset(&asset, Some(&account))?;
                    Ok(Reply::Asset(asset.id))
                })
                .await
                .map_err(storage)?
            }
            Command::UploadFamilyImage { account, path } => {
                let token = self
                    .auth
                    .access(&account, Scope::FamilyLibraryManage)
                    .await?;
                Ok(Reply::Image(
                    self.api.upload_family_image(&token, &path).await?,
                ))
            }
            local @ (Command::Import(_)
            | Command::InspectYoutube(_)
            | Command::NewPlaylist(_)
            | Command::SavePlaylist { .. }
            | Command::Duplicate(_)
            | Command::SaveCopy(_)
            | Command::DeletePlaylist(_)
            | Command::AddMedia { .. }
            | Command::DeleteMedia { .. }
            | Command::DeleteRecipe { .. }
            | Command::DeleteMediaBatch { .. }
            | Command::CollectGarbage
            | Command::ReleaseAsset(_)
            | Command::JobsPage { .. }
            | Command::Backup { .. }
            | Command::Restore { .. }
            | Command::UpdateMedia { .. }
            | Command::Prepare { .. }
            | Command::Waveform(_)
            | Command::Preview(_)
            | Command::PreviewTrim { .. }
            | Command::StopPreview
            | Command::PreviewStatus
            | Command::Export { .. }
            | Command::Publish { .. }
            | Command::Retry(_)
            | Command::Cancel(_)
            | Command::SavePixelArt(_)
            | Command::Thumbnail(_)
            | Command::ReadPixelArt(_)
            | Command::StoreImage { .. }
            | Command::ImportIcon { .. }
            | Command::SetCover { .. }
            | Command::Resolve { .. }) => {
                let account = self.command_account(&local)?;
                let session = account
                    .as_ref()
                    .map(|account| self.account_session(account))
                    .transpose()?;
                let gate = if let Some(session) = &session {
                    Some(session.gate.clone().read_owned().await)
                } else {
                    None
                };
                let service = self.clone();
                tokio::task::spawn_blocking(move || {
                    let _gate = gate;
                    if session
                        .as_ref()
                        .is_some_and(|s| s.cancelled.load(std::sync::atomic::Ordering::SeqCst))
                    {
                        return Err(Failure::Cancelled);
                    }
                    service.execute_local(local)
                })
                .await
                .map_err(storage)?
            }
        }
    }
    fn execute_local(&self, command: Command) -> Result<Reply> {
        let _assets = match &command {
            Command::AppUpdate(_) | Command::YoutubeTool(_) => None,
            Command::SavePlaylist { .. }
            | Command::SaveCopy(_)
            | Command::Duplicate(_)
            | Command::AddMedia { .. }
            | Command::Prepare { .. }
            | Command::Waveform(_)
            | Command::Preview(_)
            | Command::PreviewTrim { .. }
            | Command::Export { .. }
            | Command::Backup { .. }
            | Command::Publish { .. }
            | Command::Resolve { .. }
            | Command::SavePixelArt(_)
            | Command::ReadPixelArt(_)
            | Command::Thumbnail(_)
            | Command::StoreImage { .. }
            | Command::ImportIcon { .. }
            | Command::SetCover { .. } => Some(self.protect_assets()?),
            Command::Import(_)
            | Command::InspectYoutube(_)
            | Command::NewPlaylist(_)
            | Command::DeletePlaylist(_)
            | Command::DeleteMedia { .. }
            | Command::DeleteRecipe { .. }
            | Command::DeleteMediaBatch { .. }
            | Command::CollectGarbage
            | Command::ReleaseAsset(_)
            | Command::JobsPage { .. }
            | Command::Restore { .. }
            | Command::UpdateMedia { .. }
            | Command::StopPreview
            | Command::PreviewStatus
            | Command::Retry(_)
            | Command::Cancel(_)
            | Command::Connect
            | Command::CancelConnect
            | Command::Disconnect(_)
            | Command::SelectAccount(_)
            | Command::Sync(_)
            | Command::RefreshPlaylists(_)
            | Command::RefreshIcons(_)
            | Command::CreateYotoPlaylist { .. }
            | Command::UpdateYotoPlaylist { .. }
            | Command::AddYotoMedia { .. }
            | Command::DuplicateYotoPlaylist { .. }
            | Command::LoadRemote { .. }
            | Command::DeleteRemote { .. }
            | Command::Reconcile(_)
            | Command::AttachPublicationReceipt { .. }
            | Command::Player { .. }
            | Command::DeviceConfig { .. }
            | Command::DeviceStatus { .. }
            | Command::SaveDeviceConfig { .. }
            | Command::SaveShortcuts { .. }
            | Command::RenameDevice { .. }
            | Command::SaveGroup { .. }
            | Command::DeleteGroup { .. }
            | Command::FamilyImages(_)
            | Command::LoadGroup { .. }
            | Command::FamilyImage { .. }
            | Command::UploadFamilyImage { .. } => None,
        };
        match command {
            Command::AppUpdate(_) | Command::YoutubeTool(_) => Err(Failure::Validation(
                "Updater command requires the async service".into(),
            )),
            Command::Import(selections) => {
                for selection in selections {
                    self.enqueue(JobRequest::Import { selection })?;
                }
                Ok(Reply::Updated)
            }
            Command::InspectYoutube(url) => Ok(Reply::Entries(crate::sources::inspect_youtube(
                &self.youtube_tools(),
                &url,
            )?)),
            Command::NewPlaylist(title) => {
                let p = Playlist::new(title)?;
                self.db.save_playlist(&p)?;
                Ok(Reply::OpenedPlaylist(p.id))
            }
            Command::SavePlaylist { draft, expected } => {
                let revision = self.db.save_playlist_cas(&draft, expected)?;
                Ok(Reply::Saved(revision))
            }
            Command::SaveCopy(draft) => {
                let copy = draft.duplicate()?;
                self.db.save_playlist(&copy)?;
                Ok(Reply::OpenedPlaylist(copy.id))
            }
            Command::Duplicate(id) => {
                let p = self.playlist(id)?.duplicate()?;
                self.db.save_playlist(&p)?;
                Ok(Reply::OpenedPlaylist(p.id))
            }
            Command::DeletePlaylist(id) => {
                self.db.remove(Table::Playlist, &id.to_string())?;
                Ok(Reply::Updated)
            }
            Command::AddMedia {
                playlist,
                media,
                part_title,
            } => {
                let mut p = self.playlist(playlist)?;
                let base = p.revision;
                for (id, recipe) in media {
                    let m = self.media(id)?;
                    if !m.recipes.contains(&recipe) {
                        return Err(Failure::Validation(messages::RECIPE_NOT_OF_MEDIA.into()));
                    }
                    self.add_to_playlist(&mut p, &m, recipe, part_title.as_deref())?;
                }
                self.db.save_playlist_cas(&p, base)?;
                Ok(Reply::Updated)
            }
            Command::DeleteMediaBatch { media, policy } => {
                let kept = self.db.delete_media_batch(&media, policy)?;
                self.collect_garbage()?;
                Ok(Reply::Data(serde_json::json!({
                    "deleted": media.len() - kept.len(),
                    "kept": kept.len(),
                })))
            }
            Command::DeleteMedia { media, policy } => {
                self.db.delete_media(media, None, policy)?;
                self.collect_garbage()?;
                Ok(Reply::Updated)
            }
            Command::DeleteRecipe { recipe, policy } => {
                let r = self.recipe(recipe)?;
                self.db.delete_media(r.media, Some(recipe), policy)?;
                self.collect_garbage()?;
                Ok(Reply::Updated)
            }
            Command::Backup {
                destination,
                recovery_key,
            } => {
                let key = zeroize::Zeroizing::new(recovery_key);
                crate::recovery::backup(&self.db, &self.store, &destination, &key)?;
                Ok(Reply::Data(serde_json::json!({"backup":destination})))
            }
            Command::Restore {
                source,
                destination,
                recovery_key,
            } => {
                let key = zeroize::Zeroizing::new(recovery_key);
                crate::recovery::restore(&source, &destination, &key, true)?;
                Ok(Reply::Data(
                    serde_json::json!({"restored_profile":destination}),
                ))
            }
            Command::JobsPage { offset, limit } => {
                if !(1..=200).contains(&limit) {
                    return Err(Failure::Validation("Job page size must be 1..=200".into()));
                }
                Ok(Reply::Data(
                    serde_json::to_value(self.db.jobs_page(offset, limit as usize)?)
                        .map_err(storage)?,
                ))
            }
            Command::ReleaseAsset(asset) => {
                self.db.release_draft_asset(asset)?;
                Ok(Reply::Updated)
            }
            Command::CollectGarbage => {
                let count = self.collect_garbage()?;
                Ok(Reply::Data(serde_json::json!({"reclaimed_assets":count})))
            }
            Command::UpdateMedia { media, metadata } => {
                self.db.update_media_metadata(media, &metadata)?;
                Ok(Reply::Updated)
            }
            Command::Prepare {
                media,
                trim,
                normalization,
            } => {
                let m = self.media(media)?;
                TrimRange::new(trim.start(), trim.end(), m.info.duration)?;
                let r = MediaRecipe {
                    id: RecipeId::new(),
                    media,
                    original: m.original,
                    trim,
                    normalization,
                    result: None,
                    info: None,
                    tool_version: self.tools.validate()?,
                    part: None,
                };
                self.db.add_recipe(&r)?;
                self.enqueue(JobRequest::Encode { recipe: r.id })?;
                Ok(Reply::Prepared(r.id))
            }
            Command::Waveform(id) => {
                let asset: AssetId = self
                    .db
                    .get(Table::Remote, &format!("waveform:{id}"))?
                    .ok_or_else(|| Failure::Storage("Waveform missing".into()))?;
                let asset = self.asset(asset)?;
                let lease = self.store.materialize(&asset)?;
                let bytes = std::fs::read(lease.path()).map_err(storage)?;
                Ok(Reply::Waveform(
                    serde_json::from_slice(&bytes).map_err(storage)?,
                ))
            }
            Command::Preview(recipe) => {
                let recipe = self.recipe(recipe)?;
                let source = self.media(recipe.media)?;
                TrimRange::new(recipe.trim.start(), recipe.trim.end(), source.info.duration)?;
                let (asset, trim) = match (recipe.result, &recipe.info) {
                    (Some(id), Some(info)) => (
                        self.asset(id)?,
                        TrimRange::new(MediaTime::from_seconds(0.)?, info.duration, info.duration)?,
                    ),
                    (None, _) => (self.asset(recipe.original)?, recipe.trim),
                    (Some(_), None) => {
                        return Err(Failure::Storage("Prepared audio metadata missing".into()));
                    }
                };
                self.play_preview(recipe.media, recipe.trim.start(), &asset, trim)?;
                Ok(Reply::Updated)
            }
            Command::PreviewTrim { media, trim } => {
                let m = self.media(media)?;
                TrimRange::new(trim.start(), trim.end(), m.info.duration)?;
                let a = self.asset(m.original)?;
                self.play_preview(media, trim.start(), &a, trim)?;
                Ok(Reply::Updated)
            }
            Command::PreviewStatus => Ok(Reply::Data(
                serde_json::to_value(&*self.playback.lock().map_err(storage)?).map_err(storage)?,
            )),
            Command::StopPreview => {
                self.stop_preview()?;
                Ok(Reply::Updated)
            }
            Command::Export {
                media,
                mut destination,
            } => {
                let m = self.media(media)?;
                let a = self.asset(m.original)?;
                // A name typed without extension still opens as audio.
                if destination.extension().is_none() && !a.extension.is_empty() {
                    destination.set_extension(&a.extension);
                }
                self.store.export(&a, &destination)?;
                Ok(Reply::Updated)
            }
            Command::Publish { account, playlist } => {
                let mut p = self.playlist(playlist)?;
                if matches!(&p.origin,DraftOrigin::Existing{base} if base.account!=account) {
                    return Err(Failure::Validation(
                        "Duplicate a remote draft before publishing to another account".into(),
                    ));
                }
                if let Some(binding) = self
                    .db
                    .get::<PublicationBinding>(Table::Binding, &format!("{account}:{playlist}"))?
                {
                    p.origin = DraftOrigin::Existing {
                        base: RemoteBase {
                            account: account.clone(),
                            card: binding.card,
                            document: binding.document,
                        },
                    };
                }
                p.validate()?;
                self.enqueue(JobRequest::Publish {
                    account,
                    snapshot: Box::new(p),
                })?;
                Ok(Reply::Updated)
            }
            Command::Retry(id) => {
                let mut job = self.job(id)?;
                if self.db.publication_outcome(&job)?.is_some_and(|o| match o {
                    PublicationOutcome::Possible
                    | PublicationOutcome::Accepted { .. }
                    | PublicationOutcome::Confirmed { .. } => true,
                    PublicationOutcome::Prepared | PublicationOutcome::Rejected => false,
                }) {
                    return Err(Failure::Validation(messages::PUBLICATION_DURABLE.into()));
                }
                match &job.state {
                    JobState::Failed { .. }
                    | JobState::WaitingForAuthentication
                    | JobState::Retry { .. }
                    | JobState::Cancelled => {
                        job.state = JobState::Queued;
                        self.db.admit_job(&job)?;
                        self.notify.notify_one();
                        Ok(Reply::Updated)
                    }
                    JobState::Queued
                    | JobState::Running { .. }
                    | JobState::Conflict { .. }
                    | JobState::Reconciling
                    | JobState::Completed { .. } => {
                        Err(Failure::Validation(messages::TASK_NEEDS_RESOLUTION.into()))
                    }
                }
            }
            Command::Cancel(id) => {
                if let Some(cancel) = self.jobs.lock().map_err(storage)?.get(&id) {
                    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                let mut job = self.job(id)?;
                match &job.state {
                    JobState::Completed { .. } => {
                        return Err(Failure::Validation(messages::TASK_COMPLETED.into()));
                    }
                    JobState::Reconciling => return Err(Failure::Ambiguous),
                    JobState::Queued
                    | JobState::Running { .. }
                    | JobState::Retry { .. }
                    | JobState::WaitingForAuthentication
                    | JobState::Conflict { .. }
                    | JobState::Failed { .. }
                    | JobState::Cancelled => {}
                }
                job.state = if self
                    .db
                    .publication_outcome(&job)?
                    .is_some_and(|o| o.uncertain())
                {
                    JobState::Reconciling
                } else {
                    JobState::Cancelled
                };
                self.save_job(&job)?;
                Ok(Reply::Updated)
            }
            Command::SavePixelArt(art) => {
                let bytes = art
                    .pixels
                    .iter()
                    .flat_map(|row| row.iter().flat_map(|pixel| pixel.iter().copied()))
                    .collect::<Vec<_>>();
                let image = image::RgbaImage::from_raw(16, 16, bytes)
                    .ok_or_else(|| Failure::Media(messages::PIXEL_IMAGE_INVALID.into()))?;
                let lease = self.store.lease("png")?;
                image.save(lease.path()).map_err(crate::media_error)?;
                let asset = self.store.put(lease.path(), "png")?;
                self.db.pin_draft_asset(&asset, None)?;
                Ok(Reply::Asset(asset.id))
            }
            Command::ReadPixelArt(id) => {
                self.pixel_art(id, false).map(Box::new).map(Reply::PixelArt)
            }
            Command::Thumbnail(id) => self.pixel_art(id, true).map(Box::new).map(Reply::PixelArt),
            Command::StoreImage { path, pixel_art } => {
                let asset = if pixel_art {
                    let lease = self.store.lease("png")?;
                    crate::media::prepare_icon(&path, lease.path())?;
                    self.store.put(lease.path(), "png")?
                } else {
                    let metadata = std::fs::metadata(&path).map_err(storage)?;
                    if metadata.len() > 8_000_000 {
                        return Err(Failure::Validation(messages::IMAGE_TOO_LARGE.into()));
                    }
                    let decoded = crate::media::read_image(&path)?;
                    if decoded.width() > 8192 || decoded.height() > 8192 {
                        return Err(Failure::Validation(
                            messages::IMAGE_DIMENSIONS_EXCEED.into(),
                        ));
                    }
                    let lease = self.store.lease("png")?;
                    decoded.save(lease.path()).map_err(crate::media_error)?;
                    self.store.put(lease.path(), "png")?
                };
                self.db.pin_draft_asset(&asset, None)?;
                Ok(Reply::Asset(asset.id))
            }
            Command::ImportIcon {
                path,
                playlist,
                chapter,
                track,
            } => {
                let lease = self.store.lease("png")?;
                crate::media::prepare_icon(&path, lease.path())?;
                let asset = self.store.put(lease.path(), "png")?;
                self.db.put(Table::Asset, &asset.id.to_string(), &asset)?;
                let mut p = self.playlist(playlist)?;
                let base = p.revision;
                let c = p
                    .chapters
                    .iter_mut()
                    .find(|c| c.id == chapter)
                    .ok_or_else(|| Failure::Validation(messages::CHAPTER_ABSENT.into()))?;
                match track {
                    Some(id) => {
                        c.tracks
                            .iter_mut()
                            .find(|t| t.id == id)
                            .ok_or_else(|| Failure::Validation(messages::TRACK_ABSENT.into()))?
                            .icon = Some(IconReference::Local { asset: asset.id })
                    }
                    None => c.icon = Some(IconReference::Local { asset: asset.id }),
                };
                p.touch()?;
                self.db.save_playlist_cas(&p, base)?;
                Ok(Reply::Updated)
            }
            Command::SetCover { path, playlist } => {
                let a = self.store.put(
                    &path,
                    path.extension().and_then(|s| s.to_str()).unwrap_or("png"),
                )?;
                self.db.put(Table::Asset, &a.id.to_string(), &a)?;
                let mut p = self.playlist(playlist)?;
                let base = p.revision;
                p.metadata.cover = Some(a.id);
                p.touch()?;
                self.db.save_playlist_cas(&p, base)?;
                Ok(Reply::Updated)
            }
            Command::Resolve { job, choice } => {
                let mut job = self.job(job)?;
                let remote = match &job.state {
                    JobState::Conflict { remote } => remote.clone(),
                    JobState::Reconciling => return Err(Failure::Ambiguous),
                    JobState::Queued
                    | JobState::Running { .. }
                    | JobState::Retry { .. }
                    | JobState::WaitingForAuthentication
                    | JobState::Completed { .. }
                    | JobState::Failed { .. }
                    | JobState::Cancelled => {
                        return Err(Failure::Validation("No conflict to resolve".into()));
                    }
                };
                let JobRequest::Publish { account, snapshot } = &mut job.request else {
                    return Err(Failure::Validation("Not a publication".into()));
                };
                match choice {
                    ConflictChoice::KeepLocal => {
                        let card = CardId::new(
                            remote
                                .get("cardId")
                                .and_then(serde_json::Value::as_str)
                                .ok_or_else(|| {
                                    Failure::Compatibility("Conflict card missing".into())
                                })?,
                        )?;
                        snapshot.origin = DraftOrigin::Existing {
                            base: RemoteBase {
                                account: account.clone(),
                                card,
                                document: remote,
                            },
                        };
                        job.state = JobState::Queued;
                    }
                    ConflictChoice::UseRemote => {
                        let mut p = crate::api::read_playlist(&remote, account)?;
                        p.id = snapshot.id;
                        self.reassociate(&mut p)?;
                        let card = CardId::new(
                            remote
                                .get("cardId")
                                .and_then(serde_json::Value::as_str)
                                .ok_or_else(|| {
                                    Failure::Compatibility("Conflict card missing".into())
                                })?,
                        )?;
                        let binding = PublicationBinding {
                            account: account.clone(),
                            playlist: p.id,
                            card,
                            revision: p.revision,
                            document: remote,
                        };
                        let expected = snapshot.revision;
                        job.state = JobState::Cancelled;
                        self.db.accept_remote(&p, expected, &binding, &job)?;
                        return Ok(Reply::Updated);
                    }
                    ConflictChoice::CreateCopy => {
                        **snapshot = snapshot.duplicate()?;
                        self.db.save_playlist(snapshot)?;
                        job.state = JobState::Queued;
                    }
                }
                self.db.admit_job(&job)?;
                self.notify.notify_one();
                Ok(Reply::Updated)
            }
            Command::Connect
            | Command::CancelConnect
            | Command::Disconnect(_)
            | Command::SelectAccount(_)
            | Command::DeviceConfig { .. }
            | Command::DeviceStatus { .. }
            | Command::SaveDeviceConfig { .. }
            | Command::SaveShortcuts { .. }
            | Command::Sync(_)
            | Command::RefreshPlaylists(_)
            | Command::RefreshIcons(_)
            | Command::CreateYotoPlaylist { .. }
            | Command::UpdateYotoPlaylist { .. }
            | Command::AddYotoMedia { .. }
            | Command::DuplicateYotoPlaylist { .. }
            | Command::LoadRemote { .. }
            | Command::DeleteRemote { .. }
            | Command::Reconcile(_)
            | Command::AttachPublicationReceipt { .. }
            | Command::Player { .. }
            | Command::RenameDevice { .. }
            | Command::SaveGroup { .. }
            | Command::DeleteGroup { .. }
            | Command::FamilyImage { .. }
            | Command::LoadGroup { .. }
            | Command::FamilyImages(_)
            | Command::UploadFamilyImage { .. } => Err(Failure::Validation(
                "Network command reached local worker".into(),
            )),
        }
    }
    pub fn media(&self, id: MediaId) -> Result<MediaItem> {
        self.db
            .get(Table::Media, &id.to_string())?
            .ok_or_else(|| Failure::Storage(messages::MEDIA_NOT_FOUND.into()))
    }
    fn pixel_art(&self, id: AssetId, thumbnail: bool) -> Result<PixelArt> {
        let asset = self.asset(id)?;
        let lease = self.store.materialize(&asset)?;
        let mut image = crate::media::read_image(lease.path())?.to_rgba8();
        if thumbnail {
            image = image::imageops::resize(&image, 16, 16, image::imageops::FilterType::Triangle);
        }
        if image.width() != 16 || image.height() != 16 {
            return Err(Failure::Validation(messages::PIXEL_ICON_SIZE.into()));
        }
        let mut art = PixelArt::blank();
        for (y, row) in art.pixels.iter_mut().enumerate() {
            for (x, pixel) in row.iter_mut().enumerate() {
                *pixel = image.get_pixel(x as u32, y as u32).0;
            }
        }
        Ok(art)
    }
    fn stop_preview(&self) -> Result<()> {
        let mut active = self.preview_cancel.lock().map_err(storage)?;
        if let Some(cancel) = active.take() {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.preview.send(PreviewCommand::Stop).map_err(storage)
    }
    fn play_preview(
        &self,
        media: MediaId,
        start: MediaTime,
        asset: &Asset,
        trim: TrimRange,
    ) -> Result<()> {
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let mut active = self.preview_cancel.lock().map_err(storage)?;
            if let Some(previous) = active.replace(cancel.clone()) {
                previous.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            self.preview.send(PreviewCommand::Stop).map_err(storage)?;
        }
        let _guard = crate::process::OperationGuard::install(cancel.clone());
        let input = self.store.materialize(asset)?;
        let lease = self.store.lease("wav")?;
        self.tools.preview(input.path(), lease.path(), trim)?;
        let (reply, receive) = std::sync::mpsc::channel();
        {
            // The generation check and enqueue share Stop/new-preview's lock. A stale
            // decode can neither restart playback nor displace the newer selection.
            let active = self.preview_cancel.lock().map_err(storage)?;
            if !active.as_ref().is_some_and(|v| Arc::ptr_eq(v, &cancel))
                || cancel.load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err(Failure::Cancelled);
            }
            self.preview
                .send(PreviewCommand::Play {
                    lease,
                    media,
                    start,
                    cancel: cancel.clone(),
                    reply,
                })
                .map_err(storage)?;
        }
        match receive.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(result) => result,
            Err(_) => {
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                Err(Failure::Media(messages::AUDIO_OUTPUT_SILENT.into()))
            }
        }
    }
    fn protect_assets(&self) -> Result<crate::assets::AssetProtection> {
        let _commit = self.asset_mutation.lock().map_err(storage)?;
        Ok(self.store.protect())
    }
    fn collect_garbage(&self) -> Result<usize> {
        let _commit = self.asset_mutation.lock().map_err(storage)?;
        if self.store.has_active_owners() {
            return Ok(0);
        }
        self.db.expire_draft_assets()?;
        let roots = self.db.asset_roots()?;
        let mut count = 0;
        for id in self.store.inventory()? {
            if !roots.contains(&id) {
                self.db.remove(Table::Asset, &id.to_string())?;
                self.store.delete(id)?;
                count += 1;
            }
        }
        Ok(count)
    }
    fn reassociate(&self, p: &mut Playlist) -> Result<()> {
        let mappings = self.db.list::<PublicationMapping>(Table::Mapping)?;
        for track in p.chapters.iter_mut().flat_map(|c| &mut c.tracks) {
            if let TrackMedia::RemoteAudio {
                account: owner,
                media,
                ..
            } = &track.media
                && let Some(mapping) = mappings
                    .iter()
                    .find(|m| &m.account == owner && &m.media == media)
                && let Ok(recipe) = self.recipe(mapping.recipe)
            {
                track.media = TrackMedia::Local {
                    media: recipe.media,
                    recipe: recipe.id,
                };
            }
        }
        Ok(())
    }
    pub fn recipe(&self, id: RecipeId) -> Result<MediaRecipe> {
        self.db
            .get(Table::Recipe, &id.to_string())?
            .ok_or_else(|| Failure::Storage(messages::RECIPE_NOT_FOUND.into()))
    }
    pub fn asset(&self, id: AssetId) -> Result<Asset> {
        self.db
            .get(Table::Asset, &id.to_string())?
            .ok_or_else(|| Failure::Storage("Asset not found".into()))
    }
    pub fn playlist(&self, id: PlaylistId) -> Result<Playlist> {
        self.db
            .get(Table::Playlist, &id.to_string())?
            .ok_or_else(|| Failure::Storage(messages::PLAYLIST_NOT_FOUND.into()))
    }
    fn job(&self, id: JobId) -> Result<Job> {
        self.db
            .get(Table::Job, &id.to_string())?
            .ok_or_else(|| Failure::Storage(messages::TASK_NOT_FOUND.into()))
    }
    fn save_job(&self, job: &Job) -> Result<()> {
        self.db.put(Table::Job, &job.id.to_string(), job)
    }
    fn enqueue(&self, request: JobRequest) -> Result<JobId> {
        let j = Job {
            id: JobId::new(),
            request,
            state: JobState::Queued,
            attempts: 0,
            created: now(),
            checkpoint: None,
        };
        self.db.admit_job(&j)?;
        self.notify.notify_one();
        Ok(j.id)
    }
    fn phase(&self, job: &mut Job, phase: JobPhase) -> Result<()> {
        let current = self.job(job.id)?;
        if let JobRequest::Publish { account, .. } = &job.request
            && self
                .db
                .get::<Account>(Table::Account, account.as_str())?
                .is_none()
        {
            return Err(Failure::Authentication);
        }
        match current.state {
            JobState::Cancelled => return Err(Failure::Cancelled),
            JobState::Queued
            | JobState::Running { .. }
            | JobState::Retry { .. }
            | JobState::WaitingForAuthentication
            | JobState::Conflict { .. }
            | JobState::Reconciling
            | JobState::Completed { .. }
            | JobState::Failed { .. } => {}
        }
        job.checkpoint = Some(phase.clone());
        job.state = JobState::Running { phase };
        self.save_job(job)
    }
    async fn run_job(self: &Arc<Self>, mut job: Job) {
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.insert(job.id, cancel.clone());
        }
        job.attempts += 1;
        let account_session = match &job.request {
            JobRequest::Publish { account, .. } => self.account_session(account).ok(),
            JobRequest::Import { .. } | JobRequest::Encode { .. } => None,
        };
        let result = if let Some(session) = &account_session {
            let _guard = session.gate.read().await;
            tokio::select! { biased; _ = session.cancelled() => Err(Failure::Cancelled), result = self.process_job(&mut job, cancel.clone()) => result }
        } else {
            self.process_job(&mut job, cancel.clone()).await
        };
        let outcome = self.db.publication_outcome(&job).ok().flatten();
        let result = if cancel.load(std::sync::atomic::Ordering::SeqCst)
            && !outcome.as_ref().is_some_and(|o| o.uncertain())
            && !matches!(result, Ok(JobOutput::Published { .. }))
        {
            Err(Failure::Cancelled)
        } else {
            result
        };
        let state = match (outcome, result) {
            (Some(PublicationOutcome::Confirmed { card, revision }), _) => JobState::Completed {
                output: JobOutput::Published { card, revision },
            },
            (Some(PublicationOutcome::Possible | PublicationOutcome::Accepted { .. }), Err(_)) => {
                JobState::Reconciling
            }
            (Some(PublicationOutcome::Prepared | PublicationOutcome::Rejected) | None, result)
            | (
                Some(PublicationOutcome::Possible | PublicationOutcome::Accepted { .. }),
                result @ Ok(_),
            ) => match result {
                Ok(output) => {
                    if cancel.load(std::sync::atomic::Ordering::SeqCst)
                        && !matches!(output, JobOutput::Published { .. })
                    {
                        JobState::Cancelled
                    } else {
                        JobState::Completed { output }
                    }
                }
                Err(Failure::Authentication) => JobState::WaitingForAuthentication,
                Err(Failure::Ambiguous) => JobState::Reconciling,
                Err(Failure::Cancelled) => JobState::Cancelled,
                Err(failure) => match &job.state {
                    JobState::Conflict { remote } => JobState::Conflict {
                        remote: remote.clone(),
                    },
                    JobState::Queued
                    | JobState::Running { .. }
                    | JobState::Retry { .. }
                    | JobState::WaitingForAuthentication
                    | JobState::Reconciling
                    | JobState::Completed { .. }
                    | JobState::Failed { .. }
                    | JobState::Cancelled => match failure.retry_seconds() {
                        Some(seconds) if job.attempts < Rules::MAX_ATTEMPTS => JobState::Retry {
                            at: now() + seconds * 2u64.pow(job.attempts.min(6)),
                            failure,
                        },
                        Some(_) | None => JobState::Failed { failure },
                    },
                },
            },
        };
        job.state = state;
        if !account_session
            .as_ref()
            .is_some_and(|s| s.cancelled.load(std::sync::atomic::Ordering::SeqCst))
        {
            let _ = self.save_job(&job);
        }
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(&job.id);
        }
    }
    async fn process_job(
        self: &Arc<Self>,
        job: &mut Job,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<JobOutput> {
        match job.request.clone() {
            JobRequest::Import { selection } => {
                self.phase(job, JobPhase::Acquiring)?;
                let tools = self.youtube_tools();
                let media = match self.import(job, &selection, tools.clone(), &cancel).await {
                    // YouTube changed: fetch a newer downloader when there is one, then try once more.
                    Err(Failure::Media(message))
                        if message == messages::YOUTUBE_OUTDATED
                            && !cancel.load(std::sync::atomic::Ordering::SeqCst) =>
                    {
                        self.youtube
                            .update(crate::youtube_tool::Trigger::Outdated)
                            .await;
                        let refreshed = self.youtube_tools();
                        if refreshed.youtube == tools.youtube {
                            return Err(Failure::Media(message));
                        }
                        self.import(job, &selection, refreshed, &cancel).await?
                    }
                    result => result?,
                };
                Ok(JobOutput::Imported { media: media.id })
            }
            JobRequest::Encode { recipe } => {
                self.phase(job, JobPhase::Encoding)?;
                let asset = self
                    .encode_recipe(recipe, cancel.clone(), Some(job.clone()))
                    .await?;
                Ok(JobOutput::Encoded { asset })
            }
            JobRequest::Publish { account, snapshot } => {
                // A track over Yoto's limit would only be refused after its upload.
                for track in snapshot.chapters.iter().flat_map(|c| &c.tracks) {
                    if let TrackMedia::Local { recipe, .. } = &track.media
                        && self.recipe(*recipe)?.trim.duration().micros() > Rules::TRACK_MICROS_MAX
                    {
                        return Err(Failure::Validation(
                            messages::TRACK_LONGER_THAN_YOTO_ALLOWS.into(),
                        ));
                    }
                }
                let mappings = self
                    .upload_tracks(job, &account, &snapshot, cancel.clone())
                    .await?;
                let token = self.auth.access(&account, Scope::UserContentManage).await?;
                let mut icons = BTreeMap::new();
                for chapter in &snapshot.chapters {
                    for icon in
                        std::iter::once(&chapter.icon).chain(chapter.tracks.iter().map(|t| &t.icon))
                    {
                        match icon {
                            Some(IconReference::Local { asset }) => {
                                if !icons.contains_key(asset) {
                                    let service = self.clone();
                                    let a = self.asset(*asset)?;
                                    let lease = tokio::task::spawn_blocking(move || {
                                        service.store.materialize(&a)
                                    })
                                    .await
                                    .map_err(storage)??;
                                    icons.insert(
                                        *asset,
                                        self.api
                                            .upload_icon(
                                                &self
                                                    .auth
                                                    .access(&account, Scope::UserIconsManage)
                                                    .await?,
                                                lease.path(),
                                            )
                                            .await?,
                                    );
                                }
                            }
                            Some(IconReference::Yoto { .. }) | None => {}
                        }
                    }
                }
                let cover = match snapshot.metadata.cover {
                    Some(id) => {
                        let service = self.clone();
                        let a = self.asset(id)?;
                        let lease =
                            tokio::task::spawn_blocking(move || service.store.materialize(&a))
                                .await
                                .map_err(storage)??;
                        Some(self.api.upload_cover(&token, lease.path()).await?)
                    }
                    None => None,
                };
                let mut snapshot = snapshot;
                if matches!(snapshot.origin, DraftOrigin::New)
                    && self
                        .db
                        .get::<PublicationBinding>(
                            Table::Binding,
                            &format!("{account}:{}", snapshot.id),
                        )?
                        .is_none()
                    && let Some(card) = self.db.confirmed_card(&account, snapshot.id)?
                {
                    let read = self.auth.access(&account, Scope::UserContentView).await?;
                    let document = self.api.card(&read, &card).await?;
                    snapshot.origin = DraftOrigin::Existing {
                        base: RemoteBase {
                            account: account.clone(),
                            card,
                            document,
                        },
                    };
                }
                let document = crate::api::write_playlist(
                    &snapshot,
                    &account,
                    &mappings,
                    &icons,
                    cover.as_deref(),
                )?;
                let binding: Option<PublicationBinding> = self
                    .db
                    .get(Table::Binding, &format!("{account}:{}", snapshot.id))?;
                let base = match (&snapshot.origin, &binding) {
                    (DraftOrigin::Existing { base }, _) => Some((&base.card, &base.document)),
                    (DraftOrigin::New, Some(binding)) => Some((&binding.card, &binding.document)),
                    (DraftOrigin::New, None) => None,
                };
                let mut document = document;
                if let Some((card, base)) = base {
                    let read = self.auth.access(&account, Scope::UserContentView).await?;
                    let remote = self.api.card(&read, card).await?;
                    if meaningful(&remote) != meaningful(base) {
                        job.state = JobState::Conflict { remote };
                        return Err(Failure::Conflict);
                    }
                    document["cardId"] = serde_json::json!(card.as_str());
                }
                if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(Failure::Cancelled);
                }
                let key = format!("{account}:commit:{}", job.id);
                let mut pending = PendingPublication {
                    job: job.id,
                    account: account.clone(),
                    document: document.clone(),
                    outcome: PublicationOutcome::Prepared,
                };
                self.db.put(Table::Remote, &key, &pending)?;
                let wire = YotoGateway::prepare_card(&document)?;
                self.db.guard_publication(job)?;
                self.phase(job, JobPhase::Committing)?;
                if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                    job.checkpoint = None;
                    return Err(Failure::Cancelled);
                }
                pending.outcome = PublicationOutcome::Possible;
                self.db.put(Table::Remote, &key, &pending)?;
                let card = match self.api.write_prepared_card(&token, wire).await {
                    Ok(card) => card,
                    Err(Failure::Ambiguous) => return Err(Failure::Ambiguous),
                    Err(error) => {
                        pending.outcome = PublicationOutcome::Rejected;
                        self.db.put(Table::Remote, &key, &pending)?;
                        job.checkpoint = None;
                        return Err(error);
                    }
                };
                pending.outcome = PublicationOutcome::Accepted { card: card.clone() };
                self.db.put(Table::Remote, &key, &pending)?;
                self.phase(job, JobPhase::Verifying { card: card.clone() })?;
                let read = self
                    .auth
                    .access(&account, Scope::UserContentView)
                    .await
                    .map_err(|_| Failure::Ambiguous)?;
                let confirmed = self
                    .api
                    .card(&read, &card)
                    .await
                    .map_err(|_| Failure::Ambiguous)?;
                if !contains_document(&confirmed, &document) {
                    return Err(Failure::Ambiguous);
                }
                let binding = PublicationBinding {
                    account: account.clone(),
                    playlist: snapshot.id,
                    card: card.clone(),
                    revision: snapshot.revision,
                    document: confirmed,
                };
                self.db.confirm_publication(job, &pending, &binding)?;
                self.install_confirmed_card(&binding.account)?;
                job.checkpoint = None;
                Ok(JobOutput::Published {
                    card,
                    revision: snapshot.revision,
                })
            }
        }
    }
    /// Add a media version to a playlist. A version too long for one Yoto track becomes
    /// one track per part; parts already cut earlier are reused, so their prepared audio
    /// and Yoto uploads are too.
    fn add_to_playlist(
        &self,
        playlist: &mut Playlist,
        media: &MediaItem,
        recipe: RecipeId,
        part_title: Option<&str>,
    ) -> Result<()> {
        let version = self.recipe(recipe)?;
        let ranges = plan_parts(version.trim);
        if ranges.len() == 1 {
            return playlist.add_media(media, recipe);
        }
        let count = u16::try_from(ranges.len()).map_err(storage)?;
        let existing: Vec<MediaRecipe> = self
            .db
            .list::<MediaRecipe>(Table::Recipe)?
            .into_iter()
            .filter(|r| r.part.is_some_and(|p| p.of == recipe && p.count == count))
            .collect();
        let mut parts = Vec::with_capacity(ranges.len());
        for (index, trim) in (1..=count).zip(ranges) {
            let part = RecipePart {
                of: recipe,
                index,
                count,
            };
            let reused = existing.iter().find(|r| {
                r.part == Some(part) && r.trim == trim && r.normalization == version.normalization
            });
            let id = match reused {
                Some(r) => r.id,
                None => {
                    let r = MediaRecipe {
                        id: RecipeId::new(),
                        media: media.id,
                        original: version.original,
                        trim,
                        normalization: version.normalization,
                        result: None,
                        info: None,
                        tool_version: version.tool_version.clone(),
                        part: Some(part),
                    };
                    self.db.add_recipe(&r)?;
                    r.id
                }
            };
            parts.push(id);
        }
        playlist.add_media_parts(media, &parts, part_title)
    }
    /// Acquire, analyse and record one import with the given tools.
    async fn import(
        self: &Arc<Self>,
        job: &Job,
        selection: &ImportSelection,
        tools: Tools,
        cancel: &Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<MediaItem> {
        let service = self.clone();
        let worker_cancel = cancel.clone();
        let frozen_job = job.clone();
        let selection = selection.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = crate::process::OperationGuard::install(worker_cancel.clone());
            let _assets = service.protect_assets()?;
            let (media, asset, recipe, waveform) =
                crate::sources::import(&tools, &service.store, &selection)?;
            let _completion = service.jobs.lock().map_err(storage)?;
            if worker_cancel.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(Failure::Cancelled);
            }
            service
                .db
                .finish_import(&frozen_job, &media, &asset, &recipe, &waveform)?;
            Ok(media)
        })
        .await
        .map_err(storage)?
    }
    /// The bundled tools, with the YouTube downloader currently chosen for imports.
    pub fn youtube_tools(&self) -> Tools {
        let mut tools = self.tools.clone();
        tools.youtube = self.youtube.path();
        tools
    }
    fn shared_phase(&self, job: &JobCell, phase: JobPhase) -> Result<()> {
        let mut job = job.lock().map_err(storage)?;
        self.phase(&mut job, phase)
    }
    /// Prepare, upload and follow the conversion of every local track of a publication
    /// through a bounded pipeline: encoding runs ahead one track at a time while earlier
    /// tracks upload and convert. Returns one mapping per distinct recipe, in track order.
    async fn upload_tracks(
        self: &Arc<Self>,
        job: &mut Job,
        account: &AccountId,
        snapshot: &Playlist,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<Vec<PublicationMapping>> {
        use crate::pipeline::{Gates, Halt};
        let mut seen = std::collections::BTreeSet::new();
        let tracks: Vec<(RecipeId, String)> = snapshot
            .chapters
            .iter()
            .flat_map(|c| &c.tracks)
            .filter_map(|t| match &t.media {
                TrackMedia::Local { recipe, .. } => {
                    seen.insert(*recipe).then(|| (*recipe, t.title.clone()))
                }
                TrackMedia::RemoteAudio { .. } | TrackMedia::Stream { .. } => None,
            })
            .collect();
        let cell: JobCell = Arc::new(Mutex::new(job.clone()));
        let prepare = {
            let service = self.clone();
            let account = account.clone();
            move |(recipe, title): (RecipeId, String), halt: Halt| {
                let service = service.clone();
                let account = account.clone();
                async move {
                    let asset = service
                        .encode_recipe_until(recipe, halt.flags(), None)
                        .await?;
                    let asset = service.asset(asset)?;
                    let key = format!("{account}:{recipe}:{}", asset.sha256);
                    let known = service.db.get(Table::Mapping, &key)?;
                    Ok(Outgoing {
                        recipe,
                        title,
                        asset,
                        key,
                        known,
                    })
                }
            }
        };
        let send = {
            let service = self.clone();
            let account = account.clone();
            let cell = cell.clone();
            move |track: Outgoing, _halt: Halt| {
                let (service, account, cell) = (service.clone(), account.clone(), cell.clone());
                async move {
                    if let Some(mapping) = track.known {
                        return Ok(Sent::Known(mapping));
                    }
                    let token = service
                        .auth
                        .access(&account, Scope::UserContentManage)
                        .await?;
                    let upload = service
                        .send_audio(&cell, &token, track.recipe, &track.asset, &track.title)
                        .await?;
                    Ok(Sent::Uploaded { track, upload })
                }
            }
        };
        let finish = {
            let service = self.clone();
            let account = account.clone();
            let cell = cell.clone();
            move |sent: Sent, gates: Gates| {
                let (service, account, cell) = (service.clone(), account.clone(), cell.clone());
                async move {
                    let (track, mut upload) = match sent {
                        Sent::Known(mapping) => return Ok(mapping),
                        Sent::Uploaded { track, upload } => (track, upload),
                    };
                    let audio_seconds = service
                        .recipe(track.recipe)?
                        .info
                        .map_or(0., |info| info.duration.seconds());
                    let budget = crate::transcode::budget(audio_seconds, track.asset.bytes);
                    let mut sent_again = false;
                    let ingested = loop {
                        let (end, percent) = service
                            .await_transcode(
                                &cell,
                                &account,
                                track.recipe,
                                &upload,
                                budget,
                                gates.halt(),
                            )
                            .await?;
                        let problem = match end {
                            TranscodeEnd::Ready(ingested) => break ingested,
                            TranscodeEnd::Stalled if !sent_again => {
                                sent_again = true;
                                let _slot = gates.upload().await?;
                                let token = service
                                    .auth
                                    .access(&account, Scope::UserContentManage)
                                    .await?;
                                upload = service
                                    .send_audio(
                                        &cell,
                                        &token,
                                        track.recipe,
                                        &track.asset,
                                        &track.title,
                                    )
                                    .await?;
                                continue;
                            }
                            TranscodeEnd::Stalled => TranscodeProblem::Stalled,
                            TranscodeEnd::OutOfTime => TranscodeProblem::TimedOut,
                            TranscodeEnd::Failed => TranscodeProblem::Failed,
                        };
                        return Err(Failure::Transcode {
                            track: track.title.clone(),
                            problem,
                            percent,
                        });
                    };
                    let mapping = PublicationMapping {
                        account: account.clone(),
                        recipe: track.recipe,
                        digest: track.asset.sha256,
                        info: ingested.audio_info()?,
                        media: ingested.media,
                    };
                    service.db.put(Table::Mapping, &track.key, &mapping)?;
                    Ok(mapping)
                }
            }
        };
        let result = crate::pipeline::run(
            tracks,
            crate::pipeline::PUBLISH,
            cancel,
            prepare,
            send,
            finish,
        )
        .await;
        // Keep the phase and checkpoint the stages recorded.
        *job = cell.lock().map_err(storage)?.clone();
        result
    }
    /// Reserve an upload for a prepared track and send the file when Yoto does not have it.
    async fn send_audio(
        self: &Arc<Self>,
        job: &JobCell,
        token: &str,
        recipe: RecipeId,
        asset: &Asset,
        title: &str,
    ) -> Result<UploadId> {
        self.shared_phase(
            job,
            JobPhase::Uploading {
                recipe,
                upload: None,
            },
        )?;
        let reservation = self.api.reserve(token, &asset.sha256, title).await?;
        self.shared_phase(
            job,
            JobPhase::Uploading {
                recipe,
                upload: Some(reservation.id.clone()),
            },
        )?;
        if let Some(url) = reservation.url {
            let service = self.clone();
            let file = asset.clone();
            let lease = tokio::task::spawn_blocking(move || service.store.materialize(&file))
                .await
                .map_err(storage)??;
            self.api.put_signed(&url, lease.path(), asset.bytes).await?;
        }
        Ok(reservation.id)
    }
    /// Wait for Yoto to convert an upload, showing its progress on the job.
    /// Returns how the wait ended and the last percentage Yoto reported.
    async fn await_transcode(
        self: &Arc<Self>,
        job: &JobCell,
        account: &AccountId,
        recipe: RecipeId,
        upload: &UploadId,
        budget: std::time::Duration,
        halt: &crate::pipeline::Halt,
    ) -> Result<(TranscodeEnd, Option<u8>)> {
        use crate::{
            api::Ingestion,
            transcode::{Verdict, Watch, poll_interval},
        };
        let ingesting = |percent| JobPhase::Ingesting {
            recipe,
            upload: upload.clone(),
            percent,
        };
        self.shared_phase(job, ingesting(None))?;
        let started = std::time::Instant::now();
        let mut watch = Watch::new(budget);
        loop {
            if halt.halted() {
                return Err(Failure::Cancelled);
            }
            let token = self.auth.access(account, Scope::UserContentManage).await?;
            let progress = match self.api.ingestion(&token, upload).await? {
                Ingestion::Ready(ingested) => {
                    return Ok((TranscodeEnd::Ready(Box::new(ingested)), Some(100)));
                }
                Ingestion::Failed(progress) => {
                    return Ok((TranscodeEnd::Failed, progress.percent.or(watch.percent())));
                }
                Ingestion::Working(progress) => progress,
            };
            let shown = watch.percent();
            let verdict = watch.observe(started.elapsed(), &progress);
            if watch.percent() != shown {
                self.shared_phase(job, ingesting(watch.percent()))?;
            }
            match verdict {
                Verdict::Wait => {}
                Verdict::Stalled => return Ok((TranscodeEnd::Stalled, watch.percent())),
                Verdict::OutOfTime => return Ok((TranscodeEnd::OutOfTime, watch.percent())),
            }
            tokio::time::sleep(poll_interval(started.elapsed())).await;
        }
    }
    async fn encode_recipe(
        self: &Arc<Self>,
        id: RecipeId,
        cancel: Arc<std::sync::atomic::AtomicBool>,
        completion: Option<Job>,
    ) -> Result<AssetId> {
        self.encode_recipe_until(id, vec![cancel], completion).await
    }
    /// Prepare a recipe's audio unless it is ready, stopping when any of `stops` is raised.
    async fn encode_recipe_until(
        self: &Arc<Self>,
        id: RecipeId,
        stops: Vec<Arc<std::sync::atomic::AtomicBool>>,
        completion: Option<Job>,
    ) -> Result<AssetId> {
        let recipe = self.recipe(id)?;
        if let Some(id) = recipe.result {
            return Ok(id);
        }
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = crate::process::OperationGuard::install_any(stops.clone());
            let _assets = service.protect_assets()?;
            let mut r = service.recipe(id)?;
            let m = service.media(r.media)?;
            TrimRange::new(r.trim.start(), r.trim.end(), m.info.duration)?;
            let a = service.asset(r.original)?;
            let source = service.store.materialize(&a)?;
            let output = service.store.lease("mp3")?;
            let info = service.tools.encode(source.path(), output.path(), &r)?;
            let asset = service.store.put(output.path(), "mp3")?;
            let _completion = service.jobs.lock().map_err(storage)?;
            if stops
                .iter()
                .any(|s| s.load(std::sync::atomic::Ordering::SeqCst))
            {
                return Err(Failure::Cancelled);
            }
            r.result = Some(asset.id);
            r.info = Some(info);
            service.db.finish_encode(&asset, &r, completion.as_ref())
        })
        .await
        .map_err(storage)?
    }
}
fn meaningful(v: &serde_json::Value) -> serde_json::Value {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        for key in ["createdAt", "updatedAt", "_id", "userId"] {
            o.remove(key);
        }
    }
    v
}
fn contains_document(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
    match (actual, expected) {
        (serde_json::Value::Object(a), serde_json::Value::Object(e)) => e
            .iter()
            .all(|(k, v)| a.get(k).is_some_and(|a| contains_document(a, v))),
        (serde_json::Value::Array(a), serde_json::Value::Array(e)) => {
            a.len() == e.len() && a.iter().zip(e).all(|(a, e)| contains_document(a, e))
        }
        (a, e) => a == e,
    }
}
fn preview_loop(
    receiver: std::sync::mpsc::Receiver<PreviewCommand>,
    status: Arc<Mutex<PreviewStatus>>,
) {
    let mut timeline: Option<(MediaId, MediaTime)> = None;
    let mut stream = None;
    let mut player = None;
    let mut lease = None;
    let mut active_cancel: Option<Arc<std::sync::atomic::AtomicBool>> = None;
    loop {
        let command = match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(command) => command,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if player.as_ref().is_some_and(rodio::Player::empty)
                    || active_cancel
                        .as_ref()
                        .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
                {
                    player.take();
                    lease.take();
                    active_cancel.take();
                    timeline = None;
                    if let Ok(mut state) = status.lock() {
                        *state = PreviewStatus::Stopped;
                    }
                } else if let (Some(player), Some((media, start))) = (&player, timeline)
                    && let Ok(mut state) = status.lock()
                {
                    let position =
                        MediaTime::from_seconds(start.seconds() + player.get_pos().as_secs_f64())
                            .unwrap_or(start);
                    *state = PreviewStatus::Playing { media, position };
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match command {
            PreviewCommand::Play {
                lease: next,
                media,
                start,
                cancel,
                reply,
            } => {
                player.take();
                lease.take();
                let result = (|| {
                    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                        return Err(Failure::Cancelled);
                    }
                    if stream.is_none() {
                        stream =
                            Some(rodio::DeviceSinkBuilder::open_default_sink().map_err(|e| {
                                Failure::Media(format!("Audio output unavailable: {e}"))
                            })?);
                    }
                    if let Some(s) = &stream {
                        let file = File::open(next.path()).map_err(storage)?;
                        let decoder = rodio::Decoder::try_from(file)
                            .map_err(|e| Failure::Media(format!("Preview decoding failed: {e}")))?;
                        let p = rodio::Player::connect_new(s.mixer());
                        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                            return Err(Failure::Cancelled);
                        }
                        p.append(decoder);
                        player = Some(p);
                        lease = Some(next);
                        active_cancel = Some(cancel);
                        timeline = Some((media, start));
                        *status.lock().map_err(storage)? = PreviewStatus::Playing {
                            media,
                            position: start,
                        };
                    }
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            PreviewCommand::Stop => {
                player.take();
                lease.take();
                active_cancel.take();
                timeline = None;
                if let Ok(mut state) = status.lock() {
                    *state = PreviewStatus::Stopped;
                }
            }
        }
    }
}

fn db_account(db: &Database) -> Result<Option<AccountId>> {
    if let Some(account) = db.get::<AccountId>(Table::Remote, "active-account")?
        && db
            .get::<Account>(Table::Account, account.as_str())?
            .is_some()
    {
        return Ok(Some(account));
    }
    Ok(db
        .list::<Account>(Table::Account)?
        .first()
        .map(|a| a.id.clone()))
}
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};
    pub(super) fn streaming() -> Playlist {
        let mut p = Playlist::new("Publication test".into()).unwrap();
        p.chapters.push(Chapter {
            id: ChapterId::new(),
            key: ChapterKey::new("chapter").unwrap(),
            title: "Chapter".into(),
            icon: None,
            tracks: vec![Track {
                events: None,
                id: TrackId::new(),
                key: TrackKey::new("track").unwrap(),
                title: "Track".into(),
                icon: None,
                media: TrackMedia::Stream {
                    url: "https://example.com/test.mp3".into(),
                    format: AudioFormat::Mp3,
                },
                extra: Extra::new(),
            }],
            extra: Extra::new(),
        });
        p
    }
    pub(super) fn fixture(profile: PathBuf, host: String) -> (Arc<Service>, AccountId) {
        let tools = Tools::at(std::env::var_os("YOTO_TEST_TOOLS").unwrap().into());
        let mut service = Service::isolated(profile, tools, [7; 32]).unwrap();
        let account = AccountId::new("test-account").unwrap();
        let inner = Arc::get_mut(&mut service).unwrap();
        inner.auth = TokenBroker::test_broker(
            host.clone(),
            &account,
            &crate::auth::Grant {
                access: "test-token".into(),
                refresh: None,
                scopes: vec![Scope::UserContentView, Scope::UserContentManage],
                unknown_scopes: vec![],
                expires: now() + 3600,
            },
        );
        inner.api.host = host;
        service
            .db
            .put(
                Table::Account,
                account.as_str(),
                &Account {
                    id: account.clone(),
                    label: "Test".into(),
                    scopes: vec![Scope::UserContentManage],
                },
            )
            .unwrap();
        (service, account)
    }
    #[tokio::test]
    async fn post_receipt_persists_and_failed_verify_reconciles_after_restart() {
        let server = MockServer::start().await;
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("catalogue");
        let (service, account) = fixture(profile.clone(), server.uri());
        let p = streaming();
        let mut confirmed =
            crate::api::write_playlist(&p, &account, &[], &Default::default(), None).unwrap();
        confirmed["cardId"] = serde_json::json!("created-card");
        service.db.save_playlist(&p).unwrap();
        Mock::given(method("POST"))
            .and(path("/content"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"card":{"cardId":"created-card"}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/content/created-card"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&server)
            .await;
        let id = service
            .enqueue(JobRequest::Publish {
                account: account.clone(),
                snapshot: Box::new(p.clone()),
            })
            .unwrap();
        service.run_job(service.job(id).unwrap()).await;
        assert!(matches!(
            service.job(id).unwrap().state,
            JobState::Reconciling
        ));
        let receipt: PendingPublication = service
            .db
            .get(Table::Remote, &format!("{account}:commit:{id}"))
            .unwrap()
            .unwrap();
        assert_eq!(receipt.outcome.card().unwrap().as_str(), "created-card");
        assert!(service.execute(Command::Retry(id)).await.is_err());
        drop(service);
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/content/created-card"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"card":confirmed})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let (service, _) = fixture(profile, server.uri());
        service.execute(Command::Reconcile(id)).await.unwrap();
        assert!(matches!(
            service.job(id).unwrap().state,
            JobState::Completed {
                output: JobOutput::Published { .. }
            }
        ));
        let binding: PublicationBinding = service
            .db
            .get(Table::Binding, &format!("{account}:{}", p.id))
            .unwrap()
            .unwrap();
        assert_eq!(binding.card.as_str(), "created-card");
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| r.method != "POST")
        );
    }
    #[tokio::test]
    async fn lost_post_receipt_reconciles_by_remote_identity_without_reposting() {
        let server = MockServer::start().await;
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("catalogue");
        let (service, account) = fixture(profile.clone(), server.uri());
        let p = streaming();
        let mut confirmed =
            crate::api::write_playlist(&p, &account, &[], &Default::default(), None).unwrap();
        confirmed["cardId"] = serde_json::json!("created-card");
        // The remote accepts the operation but its response body is lost/corrupt.
        Mock::given(method("POST"))
            .and(path("/content"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{"))
            .expect(1)
            .mount(&server)
            .await;
        let id = service
            .enqueue(JobRequest::Publish {
                account: account.clone(),
                snapshot: Box::new(p.clone()),
            })
            .unwrap();
        service.run_job(service.job(id).unwrap()).await;
        assert!(matches!(
            service.job(id).unwrap().state,
            JobState::Reconciling
        ));
        let pending: PendingPublication = service
            .db
            .get(Table::Remote, &format!("{account}:commit:{id}"))
            .unwrap()
            .unwrap();
        assert!(pending.outcome.card().is_none());
        assert!(service.execute(Command::Retry(id)).await.is_err());
        drop(service);
        Mock::given(method("GET"))
            .and(path("/content/mine"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"cards":[{"cardId":"created-card","title":p.title}]}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/content/created-card"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"card":confirmed})),
            )
            .mount(&server)
            .await;
        let (service, _) = fixture(profile, server.uri());
        service.execute(Command::Reconcile(id)).await.unwrap();
        assert!(matches!(
            service.job(id).unwrap().state,
            JobState::Completed {
                output: JobOutput::Published { .. }
            }
        ));
        assert_eq!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|r| r.method == "POST")
                .count(),
            1
        );
        let binding: PublicationBinding = service
            .db
            .get(Table::Binding, &format!("{account}:{}", p.id))
            .unwrap()
            .unwrap();
        assert_eq!(binding.card.as_str(), "created-card");
    }
    #[tokio::test]
    async fn cancellation_after_post_does_not_enable_duplicate_publication() {
        let server = MockServer::start().await;
        let temp = tempfile::tempdir().unwrap();
        let (service, account) = fixture(temp.path().join("catalogue"), server.uri());
        let p = streaming();
        let mut confirmed =
            crate::api::write_playlist(&p, &account, &[], &Default::default(), None).unwrap();
        confirmed["cardId"] = serde_json::json!("created-card");
        Mock::given(method("POST"))
            .and(path("/content"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_millis(200))
                    .set_body_json(serde_json::json!({"card":{"cardId":"created-card"}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/content/created-card"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"card":confirmed})),
            )
            .mount(&server)
            .await;
        let id = service
            .enqueue(JobRequest::Publish {
                account,
                snapshot: Box::new(p),
            })
            .unwrap();
        let runner = service.clone();
        let run = tokio::spawn(async move { runner.run_job(runner.job(id).unwrap()).await });
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while server.received_requests().await.unwrap().is_empty() {
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        service.execute(Command::Cancel(id)).await.unwrap();
        assert!(matches!(
            service.job(id).unwrap().state,
            JobState::Reconciling
        ));
        run.await.unwrap();
        assert!(matches!(
            service.job(id).unwrap().state,
            JobState::Completed {
                output: JobOutput::Published { .. }
            }
        ));
        assert!(service.execute(Command::Retry(id)).await.is_err());
    }
    #[tokio::test]
    async fn publication_admission_concurrent_ambiguous_edited_restart_and_legacy_queue() {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("catalogue");
        let (service, account) = fixture(profile.clone(), server.uri());
        let p = streaming();
        service.db.save_playlist(&p).unwrap();
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{"))
            .expect(1)
            .mount(&server)
            .await;
        let (a, b) = tokio::join!(
            service.execute(Command::Publish {
                account: account.clone(),
                playlist: p.id
            }),
            service.execute(Command::Publish {
                account: account.clone(),
                playlist: p.id
            })
        );
        assert_ne!(a.is_ok(), b.is_ok());
        let first = service.db.list::<Job>(Table::Job).unwrap().remove(0);
        let mut legacy = first.clone();
        legacy.id = JobId::new();
        legacy.created += 1;
        service.save_job(&legacy).unwrap();
        service.run_job(first).await;
        service.run_job(legacy).await;
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        let mut edited = p.clone();
        edited.title = "Changed draft".into();
        service.db.save_playlist_cas(&edited, p.revision).unwrap();
        assert!(
            service
                .execute(Command::Publish {
                    account: account.clone(),
                    playlist: p.id
                })
                .await
                .is_err()
        );
        drop(service);
        let (service, account) = fixture(profile, server.uri());
        assert!(
            service
                .execute(Command::Publish {
                    account,
                    playlist: p.id
                })
                .await
                .is_err()
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
    #[tokio::test]
    async fn definitive_write_rejections_can_retry_but_corrupt_success_cannot() {
        for code in [400, 401, 403, 422] {
            let server = MockServer::start().await;
            let root = tempfile::tempdir().unwrap();
            let (service, account) = fixture(root.path().join("catalogue"), server.uri());
            let p = streaming();
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(code))
                .mount(&server)
                .await;
            let id = service
                .enqueue(JobRequest::Publish {
                    account,
                    snapshot: Box::new(p),
                })
                .unwrap();
            service.run_job(service.job(id).unwrap()).await;
            assert!(!matches!(
                service.job(id).unwrap().state,
                JobState::Reconciling
            ));
            assert!(service.job(id).unwrap().checkpoint.is_none());
            service.execute(Command::Retry(id)).await.unwrap();
        }
        assert!(matches!(
            YotoGateway::prepare_card(&serde_json::json!({"title":123})),
            Err(Failure::Validation(_))
        ));
    }
    #[tokio::test]
    async fn delayed_remote_load_and_reconcile_cannot_restore_logout_state() {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let (service, account) = fixture(root.path().join("catalogue"), server.uri());
        let mut doc =
            crate::api::write_playlist(&streaming(), &account, &[], &Default::default(), None)
                .unwrap();
        doc["cardId"] = serde_json::json!("private-card");
        Mock::given(method("GET"))
            .and(path("/content/private-card"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_millis(150))
                    .set_body_json(serde_json::json!({"card":doc})),
            )
            .mount(&server)
            .await;
        let client = service.clone();
        let owner = account.clone();
        let load = tokio::spawn(async move {
            client
                .execute(Command::LoadRemote {
                    account: owner,
                    card: CardId::new("private-card").unwrap(),
                })
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        service.execute(Command::Disconnect(account)).await.unwrap();
        assert!(matches!(load.await.unwrap(), Err(Failure::Cancelled)));
        let snapshot = service.snapshot().unwrap();
        assert!(
            snapshot.accounts.is_empty()
                && snapshot.playlists.is_empty()
                && snapshot.bindings.is_empty()
        );
    }

    #[tokio::test]
    async fn reconcile_racing_logout_keeps_only_receipt_and_explicit_attach_never_posts() {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("catalogue");
        let (service, account) = fixture(profile.clone(), server.uri());
        let p = streaming();
        service.db.save_playlist(&p).unwrap();
        let mut doc =
            crate::api::write_playlist(&p, &account, &[], &Default::default(), None).unwrap();
        doc["cardId"] = serde_json::json!("receipt-card");
        doc["privateExtension"] = serde_json::json!("remove-this-at-logout");
        let id = service
            .enqueue(JobRequest::Publish {
                account: account.clone(),
                snapshot: Box::new(p.clone()),
            })
            .unwrap();
        let mut job = service.job(id).unwrap();
        job.state = JobState::Reconciling;
        job.checkpoint = Some(JobPhase::Verifying {
            card: CardId::new("receipt-card").unwrap(),
        });
        service.save_job(&job).unwrap();
        service
            .db
            .put(
                Table::Remote,
                &format!("{account}:commit:{id}"),
                &PendingPublication {
                    job: id,
                    account: account.clone(),
                    outcome: PublicationOutcome::Accepted {
                        card: CardId::new("receipt-card").unwrap(),
                    },
                    document: doc.clone(),
                },
            )
            .unwrap();
        Mock::given(method("GET"))
            .and(path("/content/receipt-card"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_millis(150))
                    .set_body_json(serde_json::json!({"card":doc})),
            )
            .mount(&server)
            .await;
        let client = service.clone();
        let active = tokio::spawn(async move { client.execute(Command::Reconcile(id)).await });
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        service
            .execute(Command::Disconnect(account.clone()))
            .await
            .unwrap();
        assert!(matches!(active.await.unwrap(), Err(Failure::Cancelled)));
        let snapshot = service.snapshot().unwrap();
        assert!(snapshot.accounts.is_empty() && snapshot.bindings.is_empty());
        assert_eq!(snapshot.playlists[0].id, p.id);
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains("remove-this-at-logout")
        );
        let minimal: PendingPublication = service
            .db
            .get(Table::Remote, &format!("{account}:commit:{id}"))
            .unwrap()
            .unwrap();
        assert!(minimal.document.is_null());
        assert_eq!(minimal.outcome.card().unwrap().as_str(), "receipt-card");
        drop(service);
        let (service, account) = fixture(profile, server.uri());
        assert!(
            service
                .execute(Command::Publish {
                    account,
                    playlist: p.id
                })
                .await
                .is_err()
        );
        assert!(service.execute(Command::Reconcile(id)).await.is_err());
        service
            .execute(Command::AttachPublicationReceipt {
                job: id,
                card: CardId::new("receipt-card").unwrap(),
            })
            .await
            .unwrap();
        assert!(matches!(
            service.job(id).unwrap().state,
            JobState::Completed { .. }
        ));
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| r.method.as_str() == "GET")
        );
    }
    #[tokio::test]
    async fn authenticated_local_worker_keeps_epoch_gate_until_completion_and_player_owner_serializes()
     {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let (service, account) = fixture(root.path().join("catalogue"), server.uri());
        let session = service.account_session(&account).unwrap();
        let gate = session.gate.clone().read_owned().await;
        let (started, wait) = tokio::sync::oneshot::channel();
        let worker = tokio::task::spawn_blocking(move || {
            let _gate = gate;
            started.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(75));
            assert!(session.cancelled.load(std::sync::atomic::Ordering::SeqCst));
        });
        wait.await.unwrap();
        service
            .execute(Command::Disconnect(account.clone()))
            .await
            .unwrap();
        worker.await.unwrap();
        assert!(
            service
                .db
                .get::<Account>(Table::Account, account.as_str())
                .unwrap()
                .is_none()
        );
        let id = DeviceId::new("synthetic-player").unwrap();
        let owner = service
            .players
            .lock()
            .await
            .entry((account, id))
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let first = owner.clone().lock_owned().await;
        let second = tokio::spawn(async move {
            let _guard = owner.lock().await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        assert!(!second.is_finished());
        drop(first);
        second.await.unwrap();
    }
    #[tokio::test]
    async fn use_remote_updates_binding_atomically_and_next_publish_accepts_base() {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let (service, account) = fixture(root.path().join("catalogue"), server.uri());
        let p = streaming();
        service.db.save_playlist(&p).unwrap();
        let mut remote =
            crate::api::write_playlist(&p, &account, &[], &Default::default(), None).unwrap();
        remote["cardId"] = serde_json::json!("card");
        remote["title"] = serde_json::json!("Remote changed");
        let id = service
            .enqueue(JobRequest::Publish {
                account: account.clone(),
                snapshot: Box::new(p.clone()),
            })
            .unwrap();
        let mut job = service.job(id).unwrap();
        job.state = JobState::Conflict {
            remote: remote.clone(),
        };
        service.save_job(&job).unwrap();
        service
            .execute(Command::Resolve {
                job: id,
                choice: ConflictChoice::UseRemote,
            })
            .await
            .unwrap();
        let accepted = service.playlist(p.id).unwrap();
        assert_eq!(accepted.revision, p.revision.next().unwrap());
        let binding: PublicationBinding = service
            .db
            .get(Table::Binding, &format!("{account}:{}", p.id))
            .unwrap()
            .unwrap();
        assert_eq!(binding.document, remote);
        assert_eq!(binding.revision, accepted.revision);
        service
            .execute(Command::Publish {
                account: account.clone(),
                playlist: p.id,
            })
            .await
            .unwrap();
        let new = service.db.list::<Job>(Table::Job).unwrap().pop().unwrap();
        let JobRequest::Publish { snapshot, .. } = new.request else {
            panic!()
        };
        let DraftOrigin::Existing { base } = snapshot.origin else {
            panic!()
        };
        assert_eq!(base.document, remote);
    }
}

#[cfg(test)]
#[path = "service_regressions.rs"]
mod final_regressions;

#[cfg(test)]
#[path = "service_playlist_tests.rs"]
mod playlist_flow_tests;

#[path = "service_cloud.rs"]
mod cloud;

#[cfg(test)]
#[path = "service_remote_ui_fixture.rs"]
mod service_remote_ui_fixture;

#[cfg(test)]
#[path = "service_cloud_tests.rs"]
mod cloud_flow_tests;

#[path = "service_icons.rs"]
mod icons;

#[cfg(test)]
#[path = "service_icons_tests.rs"]
mod icon_refresh_tests;

#[cfg(test)]
#[path = "service_update_tests.rs"]
mod update_tests;
