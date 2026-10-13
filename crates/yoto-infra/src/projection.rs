//! Revisioned interface projections. Historical jobs are explicitly paged, never polled in full.
use crate::{
    Service, Snapshot,
    api::{Group, Icon, RemoteCard},
    db::Table,
    storage,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use yoto_domain::*;
#[derive(Serialize, Deserialize)]
pub struct InterfaceDelta {
    pub revisions: Vec<u64>,
    pub parts: Vec<InterfacePart>,
}
#[derive(Serialize, Deserialize)]
pub enum InterfacePart {
    Media(Vec<MediaItem>),
    Recipes(Vec<MediaRecipe>),
    Playlists(Vec<Playlist>),
    Jobs(Vec<Job>),
    Accounts(Vec<Account>),
    Mappings(Vec<PublicationMapping>),
    Bindings(Vec<PublicationBinding>),
    Remote {
        active: Option<AccountId>,
        cards: Vec<RemoteCard>,
        devices: Vec<Device>,
        groups: Vec<Group>,
        icons: Vec<Icon>,
        sync: BTreeMap<RemoteCollection, RemoteSyncState>,
    },
    Sync(BTreeMap<RecipeId, MediaSyncState>),
}
impl InterfaceDelta {
    pub fn apply(self, snapshot: &mut Snapshot) -> Vec<u64> {
        for part in self.parts {
            match part {
                InterfacePart::Media(v) => snapshot.media = v,
                InterfacePart::Recipes(v) => snapshot.recipes = v,
                InterfacePart::Playlists(v) => snapshot.playlists = v,
                InterfacePart::Jobs(v) => snapshot.jobs = v,
                InterfacePart::Accounts(v) => snapshot.accounts = v,
                InterfacePart::Mappings(v) => snapshot.mappings = v,
                InterfacePart::Bindings(v) => snapshot.bindings = v,
                InterfacePart::Sync(v) => snapshot.sync_states = v,
                InterfacePart::Remote {
                    active,
                    cards,
                    devices,
                    groups,
                    icons,
                    sync,
                } => {
                    snapshot.active_account = active;
                    snapshot.remote = cards;
                    snapshot.devices = devices;
                    snapshot.groups = groups;
                    snapshot.icons = icons;
                    snapshot.remote_sync = sync;
                }
            }
        }
        self.revisions
    }
}
impl Service {
    fn interface_revisions(&self) -> Result<Vec<u64>> {
        let mut versions = Table::ALL
            .iter()
            .map(|t| self.db.table_generation(*t))
            .collect::<Vec<_>>();
        versions.push(self.remote.lock().map_err(storage)?.generation + 1);
        Ok(versions)
    }
    pub fn interface_changes(&self, since: &[u64]) -> Result<InterfaceDelta> {
        for _ in 0..5 {
            let versions = self.interface_revisions()?;
            let changed = |table: Table| since.get(table as usize) != versions.get(table as usize);
            let mut parts = vec![];
            if changed(Table::Media) {
                parts.push(InterfacePart::Media(self.db.list(Table::Media)?));
            }
            if changed(Table::Recipe) {
                parts.push(InterfacePart::Recipes(self.db.list(Table::Recipe)?));
            }
            if changed(Table::Playlist) {
                parts.push(InterfacePart::Playlists(self.db.list(Table::Playlist)?));
            }
            if changed(Table::Job) {
                parts.push(InterfacePart::Jobs(self.db.jobs_page(0, 200)?));
            }
            if changed(Table::Account) {
                parts.push(InterfacePart::Accounts(self.db.list(Table::Account)?));
            }
            if changed(Table::Mapping) {
                parts.push(InterfacePart::Mappings(self.db.list(Table::Mapping)?));
            }
            if changed(Table::Binding) {
                parts.push(InterfacePart::Bindings(self.db.list(Table::Binding)?));
            }
            let remote_changed = since.get(Table::ALL.len()) != versions.get(Table::ALL.len());
            if remote_changed {
                let remote = self.remote.lock().map_err(storage)?;
                let current = remote
                    .active
                    .as_ref()
                    .and_then(|id| remote.by_account.get(id));
                parts.push(InterfacePart::Remote {
                    active: remote.active.clone(),
                    cards: current.map(|c| c.cards.clone()).unwrap_or_default(),
                    devices: current.map(|c| c.devices.clone()).unwrap_or_default(),
                    groups: current.map(|c| c.groups.clone()).unwrap_or_default(),
                    icons: current.map(|c| c.icons.clone()).unwrap_or_default(),
                    sync: current.map(|c| c.sync.clone()).unwrap_or_default(),
                });
            }
            if remote_changed || changed(Table::Mapping) || changed(Table::Job) {
                let active = self.remote.lock().map_err(storage)?.active.clone();
                let states = if let Some(active) = active {
                    MediaSyncState::project_all(
                        &active,
                        &self.db.list(Table::Mapping)?,
                        &self.db.list(Table::Job)?,
                    )
                } else {
                    BTreeMap::new()
                };
                parts.push(InterfacePart::Sync(states));
            }
            if versions == self.interface_revisions()? {
                return Ok(InterfaceDelta {
                    revisions: versions,
                    parts,
                });
            }
        }
        Err(Failure::Conflict)
    }
    pub fn interface_chunk(
        &self,
        since: Vec<u64>,
        revision: Option<String>,
        offset: usize,
    ) -> Result<crate::bridge::SnapshotChunk> {
        let mut cache = self.interface_bytes.lock().map_err(storage)?;
        let current = self.revision()?;
        if cache
            .as_ref()
            .is_none_or(|(rev, previous, _)| rev != &current || previous != &since)
        {
            let mut stable = None;
            for _ in 0..5 {
                let key = self.revision()?;
                let delta = self.interface_changes(&since)?;
                let bytes = serde_json::to_vec(&delta).map_err(storage)?;
                if key == self.revision()? {
                    stable = Some((key, since.clone(), Arc::new(bytes)));
                    break;
                }
            }
            *cache = Some(stable.ok_or(Failure::Conflict)?);
        }
        let (key, _, bytes) = cache.as_ref().ok_or(Failure::Conflict)?;
        if revision.as_ref().is_some_and(|r| r != key) {
            return Err(Failure::Conflict);
        }
        crate::bridge::chunk(key, bytes, offset)
    }
}
