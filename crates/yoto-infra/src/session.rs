//! One service owner per catalogue, with UI/MCP clients using the authenticated local bridge.
use crate::{
    Command, Reply, Service, Snapshot,
    bridge::{self, Request},
    media::Tools,
};
use std::{path::PathBuf, sync::Arc};
use yoto_domain::*;
pub struct Session {
    profile: PathBuf,
    isolated: bool,
    owner: Option<Arc<Service>>,
    worker: Option<tokio::task::JoinHandle<()>>,
    ipc: Option<tokio::task::JoinHandle<()>>,
}
impl Session {
    pub async fn connect(profile: PathBuf, tools: Tools, isolated: bool) -> Result<Arc<Self>> {
        let profile = crate::canonical_profile(&profile)?;
        // A successful probe selects proxy mode. Otherwise the OS catalogue lock is acquired
        // before opening SQLCipher, cleaning leases or removing any stale socket.
        if bridge::call(&profile, isolated, Request::Health)
            .await
            .is_ok()
        {
            return Ok(Arc::new(Self {
                profile,
                isolated,
                owner: None,
                worker: None,
                ipc: None,
            }));
        }
        let attempt = if isolated && cfg!(debug_assertions) {
            Service::isolated(profile.clone(), tools, [41; 32])
        } else {
            Service::open(profile.clone(), tools)
        };
        let owner = match attempt {
            Ok(owner) => owner,
            Err(original) => {
                // Another process may have acquired the lock between our probe and open.
                for _ in 0..30 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    if bridge::call(&profile, isolated, Request::Health)
                        .await
                        .is_ok()
                    {
                        return Ok(Arc::new(Self {
                            profile,
                            isolated,
                            owner: None,
                            worker: None,
                            ipc: None,
                        }));
                    }
                }
                return Err(original);
            }
        };
        let worker = owner.start();
        let ipc = bridge::start(owner.clone(), profile.clone(), isolated)?;
        Ok(Arc::new(Self {
            profile,
            isolated,
            owner: Some(owner),
            worker: Some(worker),
            ipc: Some(ipc),
        }))
    }
    pub async fn execute(&self, command: Command) -> Result<Reply> {
        match &self.owner {
            Some(owner) => owner.execute(command).await,
            None => {
                let value =
                    bridge::call(&self.profile, self.isolated, Request::Execute(command)).await?;
                serde_json::from_value(value).map_err(crate::storage)
            }
        }
    }
    pub async fn snapshot(&self) -> Result<Snapshot> {
        match &self.owner {
            Some(owner) => {
                let owner = owner.clone();
                tokio::task::spawn_blocking(move || owner.snapshot())
                    .await
                    .map_err(crate::storage)?
            }
            None => {
                for _ in 0..5 {
                    let mut bytes = Vec::new();
                    let mut revision = None;
                    let mut offset = 0;
                    loop {
                        let chunk = bridge::call(
                            &self.profile,
                            self.isolated,
                            Request::SnapshotChunk {
                                revision: revision.clone(),
                                offset,
                            },
                        )
                        .await;
                        let chunk: bridge::SnapshotChunk = match chunk {
                            Ok(value) => serde_json::from_value(value).map_err(crate::storage)?,
                            Err(Failure::Conflict) => break,
                            Err(error) => return Err(error),
                        };
                        revision = Some(chunk.revision);
                        {
                            use base64::Engine;
                            bytes.extend(
                                base64::engine::general_purpose::STANDARD
                                    .decode(chunk.data)
                                    .map_err(crate::storage)?,
                            );
                        }
                        if bytes.len() > 512_000_000 {
                            return Err(Failure::Storage(
                                "Snapshot exceeds the supported 512 MB projection capacity".into(),
                            ));
                        }
                        match chunk.next {
                            Some(next) => offset = next,
                            None => return serde_json::from_slice(&bytes).map_err(crate::storage),
                        }
                    }
                }
                Err(Failure::Conflict)
            }
        }
    }
    pub async fn interface_changes(
        &self,
        since: Vec<u64>,
    ) -> Result<crate::projection::InterfaceDelta> {
        if let Some(owner) = &self.owner {
            let owner = owner.clone();
            return tokio::task::spawn_blocking(move || owner.interface_changes(&since))
                .await
                .map_err(crate::storage)?;
        }
        for _ in 0..5 {
            let mut bytes = vec![];
            let mut revision = None;
            let mut offset = 0;
            loop {
                let response = bridge::call(
                    &self.profile,
                    self.isolated,
                    Request::InterfaceChunk {
                        since: since.clone(),
                        revision: revision.clone(),
                        offset,
                    },
                )
                .await;
                let chunk: bridge::SnapshotChunk = match response {
                    Ok(v) => serde_json::from_value(v).map_err(crate::storage)?,
                    Err(Failure::Conflict) => break,
                    Err(e) => return Err(e),
                };
                revision = Some(chunk.revision);
                use base64::Engine;
                bytes.extend(
                    base64::engine::general_purpose::STANDARD
                        .decode(chunk.data)
                        .map_err(crate::storage)?,
                );
                if bytes.len() > 512_000_000 {
                    return Err(Failure::Validation(
                        "Interface projection exceeds 512 MB".into(),
                    ));
                }
                match chunk.next {
                    Some(next) => offset = next,
                    None => return serde_json::from_slice(&bytes).map_err(crate::storage),
                }
            }
        }
        Err(Failure::Conflict)
    }
    pub async fn revision(&self) -> Result<String> {
        match &self.owner {
            Some(owner) => owner.revision(),
            None => bridge::call(&self.profile, self.isolated, Request::Health)
                .await?
                .get("revision")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .ok_or(Failure::Connection),
        }
    }
    /// Stop accepting mutations only after all admitted work has finished.
    /// A proxy cannot replace the app while another process owns its workers.
    pub async fn prepare_update_shutdown(&self) -> Result<()> {
        match &self.owner {
            Some(owner) => owner.prepare_update_shutdown().await,
            None => Err(Failure::Validation(messages::UPDATE_OTHER_INSTANCE.into())),
        }
    }
    pub fn cancel_update_shutdown(&self) {
        if let Some(owner) = &self.owner {
            owner.cancel_update_shutdown();
        }
    }
    pub async fn install_update(&self) -> Result<()> {
        let owner = self
            .owner
            .clone()
            .ok_or_else(|| Failure::Validation(messages::UPDATE_OTHER_INSTANCE.into()))?;
        let prepared = owner.updater.prepared().map_err(crate::storage)?;
        if let Err(error) = owner.prepare_update_shutdown().await {
            owner.updater.install_blocked(error.to_string());
            return Err(error);
        }
        owner.updater.installing();
        let launched = tokio::task::spawn_blocking(move || prepared.launch_helper()).await;
        match launched {
            Ok(Ok(())) => {
                self.shutdown();
                Ok(())
            }
            Ok(Err(error)) => {
                owner.cancel_update_shutdown();
                owner.updater.failed(error);
                Err(Failure::Validation(messages::UPDATE_HELPER_FAILED.into()))
            }
            Err(error) => {
                owner.cancel_update_shutdown();
                owner
                    .updater
                    .failed(yoto_updater::UpdateError::Install(error.to_string()));
                Err(crate::storage(error))
            }
        }
    }
    pub fn is_owner(&self) -> bool {
        self.owner.is_some()
    }
    pub fn shutdown(&self) {
        if let Some(owner) = &self.owner {
            owner.shutdown();
        }
        if let Some(worker) = &self.worker {
            worker.abort();
        }
        if let Some(ipc) = &self.ipc {
            ipc.abort();
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.shutdown();
    }
}
