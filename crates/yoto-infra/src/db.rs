use crate::storage;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
use yoto_domain::*;
pub struct Database {
    connection: Mutex<Connection>,
    pub path: PathBuf,
    generation: std::sync::atomic::AtomicU64,
    table_generations: [std::sync::atomic::AtomicU64; Table::ALL.len()],
    #[cfg(test)]
    injected_failure: Mutex<Option<InjectedWrite>>,
}
#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) enum InjectedWrite {
    Receipt,
    Verifying,
    Binding,
}
impl Database {
    pub fn open(path: &Path, key: &[u8; 32]) -> Result<Self> {
        Self::open_with_upgrade(path, key, include_str!("../../../db/migrations/002.sql"))
    }
    #[cfg(test)]
    pub(crate) fn open_injected_upgrade(path: &Path, key: &[u8; 32], sql: &str) -> Result<Self> {
        Self::open_with_upgrade(path, key, sql)
    }
    fn open_with_upgrade(path: &Path, key: &[u8; 32], upgrade: &str) -> Result<Self> {
        let mut c = Connection::open(path).map_err(storage)?;
        c.pragma_update(None, "key", format!("x'{}'", hex::encode(key)))
            .map_err(storage)?;
        c.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(storage)?;
        let cipher: String = c
            .query_row("PRAGMA cipher_version", [], |r| r.get(0))
            .map_err(storage)?;
        if cipher.is_empty() {
            return Err(Failure::Storage("SQLCipher is required".into()));
        }
        c.pragma_update(None, "foreign_keys", "ON")
            .map_err(storage)?;
        c.pragma_update(None, "journal_mode", "WAL")
            .map_err(storage)?;
        c.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(storage)?;
        let version: i64 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(storage)?;
        if version > 2 {
            return Err(Failure::Storage(
                "This catalogue needs a newer application. It was not modified.".into(),
            ));
        }
        if version == 0 {
            let tx = c.transaction().map_err(storage)?;
            tx.execute_batch(include_str!("../../../db/migrations/001.sql"))
                .map_err(storage)?;
            tx.commit().map_err(storage)?;
        }
        if version > 0 && version < 2 {
            crate::recovery::migration_backup(&c, path, key, version as u32)?;
        }
        if version < 2 {
            let tx = c.transaction().map_err(storage)?;
            tx.execute_batch(upgrade).map_err(storage)?;
            tx.commit().map_err(storage)?;
        }
        Ok(Self {
            connection: Mutex::new(c),
            path: path.to_owned(),
            generation: std::sync::atomic::AtomicU64::new(1),
            table_generations: std::array::from_fn(|_| std::sync::atomic::AtomicU64::new(1)),
            #[cfg(test)]
            injected_failure: Mutex::new(None),
        })
    }
    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn changed(&self) {
        self.changed_tables(&Table::ALL);
    }
    fn changed_tables(&self, tables: &[Table]) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        for table in tables {
            self.table_generations[*table as usize]
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    pub fn table_generation(&self, table: Table) -> u64 {
        self.table_generations[table as usize].load(std::sync::atomic::Ordering::SeqCst)
    }
    pub fn jobs_page(&self, offset: usize, limit: usize) -> Result<Vec<Job>> {
        let c = self.lock()?;
        let mut query = c
            .prepare("SELECT payload FROM jobs ORDER BY rowid DESC LIMIT ?1 OFFSET ?2")
            .map_err(storage)?;
        query
            .query_map(
                params![
                    i64::try_from(limit).map_err(storage)?,
                    i64::try_from(offset).map_err(storage)?
                ],
                |r| r.get::<_, String>(0),
            )
            .map_err(storage)?
            .map(|r| decode(&r.map_err(storage)?))
            .collect()
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(storage)
    }
    pub fn put<T: Serialize>(&self, table: Table, id: &str, value: &T) -> Result<()> {
        let json = serde_json::to_string(&Envelope { version: 1, value }).map_err(storage)?;
        let connection = self.lock()?;
        #[cfg(test)]
        self.fail_injected_write(&connection, table, &json)?;
        connection.execute(&format!("INSERT INTO {}(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",table.name()),params![id,json]).map_err(storage)?;
        self.changed_tables(&[table]);
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn inject_sql_failure(&self, point: InjectedWrite) {
        *self.injected_failure.lock().unwrap() = Some(point);
    }
    #[cfg(test)]
    fn fail_injected_write(&self, connection: &Connection, table: Table, json: &str) -> Result<()> {
        let mut failure = self.injected_failure.lock().map_err(storage)?;
        let value: serde_json::Value = serde_json::from_str(json).map_err(storage)?;
        let matches = match (&*failure, table) {
            (Some(InjectedWrite::Receipt), Table::Remote) => {
                value["value"]["outcome"].get("Accepted").is_some()
            }
            (Some(InjectedWrite::Verifying), Table::Job) => {
                value["value"]["state"]["Running"]["phase"]
                    .get("Verifying")
                    .is_some()
            }
            (Some(InjectedWrite::Binding), Table::Binding) => true,
            (
                Some(InjectedWrite::Receipt | InjectedWrite::Verifying | InjectedWrite::Binding)
                | None,
                _,
            ) => false,
        };
        if matches {
            failure.take();
            // Exercise an actual SQLite failure, consumed once, at the selected write.
            connection
                .execute("INSERT INTO __injected_missing_table VALUES(1)", [])
                .map_err(storage)?;
        }
        Ok(())
    }
    pub fn get<T: DeserializeOwned>(&self, table: Table, id: &str) -> Result<Option<T>> {
        let json: Option<String> = self
            .lock()?
            .query_row(
                &format!("SELECT payload FROM {} WHERE id=?1", table.name()),
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        json.map(|j| decode(&j)).transpose()
    }
    pub fn list<T: DeserializeOwned>(&self, table: Table) -> Result<Vec<T>> {
        let c = self.lock()?;
        let mut q = c
            .prepare(&format!(
                "SELECT payload FROM {} ORDER BY rowid",
                table.name()
            ))
            .map_err(storage)?;
        q.query_map([], |r| r.get::<_, String>(0))
            .map_err(storage)?
            .map(|r| decode(&r.map_err(storage)?))
            .collect()
    }
    pub fn remove(&self, table: Table, id: &str) -> Result<()> {
        self.lock()?
            .execute(&format!("DELETE FROM {} WHERE id=?1", table.name()), [id])
            .map_err(storage)?;
        self.changed_tables(&[table]);
        Ok(())
    }
    pub fn save_playlist(&self, p: &Playlist) -> Result<()> {
        self.save_playlist_inner(p, None).map(|_| ())
    }
    pub fn save_playlist_cas(&self, p: &Playlist, expected: Revision) -> Result<Revision> {
        self.save_playlist_inner(p, Some(expected))
    }
    fn save_playlist_inner(&self, p: &Playlist, expected: Option<Revision>) -> Result<Revision> {
        Rules::title(&p.title)?;
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        let revision = write_playlist(&tx, p, expected, PlaylistWriteIntent::LocalEdit)?;
        tx.commit().map_err(storage)?;
        self.changed_tables(&[Table::Playlist]);
        Ok(revision)
    }
    pub fn accept_remote(
        &self,
        p: &Playlist,
        expected: Revision,
        binding: &PublicationBinding,
        job: &Job,
    ) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        let revision = write_playlist(
            &tx,
            p,
            Some(expected),
            PlaylistWriteIntent::AuthoritativeReplacement,
        )?;
        let mut binding = binding.clone();
        binding.revision = revision;
        for (table, id, value) in [
            (
                Table::Binding,
                format!("{}:{}", binding.account, binding.playlist),
                serde_json::to_value(&binding).map_err(storage)?,
            ),
            (
                Table::Job,
                job.id.to_string(),
                serde_json::to_value(job).map_err(storage)?,
            ),
        ] {
            tx.execute(&format!("INSERT INTO {}(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",table.name()),params![id,serde_json::to_string(&Envelope{version:1,value}).map_err(storage)?]).map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(())
    }
    /// Import one remote card once. Repeated opens reuse its local draft, including edits.
    pub fn import_remote(&self, p: &Playlist, binding: &PublicationBinding) -> Result<PlaylistId> {
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        let mut existing = tx
            .prepare("SELECT payload FROM bindings")
            .map_err(storage)?;
        let rows = existing
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(storage)?;
        for row in rows {
            let previous: PublicationBinding = decode(&row.map_err(storage)?)?;
            if previous.account == binding.account
                && previous.card == binding.card
                && tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM playlists WHERE id=?1)",
                        [previous.playlist.to_string()],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(storage)?
            {
                return Ok(previous.playlist);
            }
        }
        drop(existing);
        write_playlist(&tx, p, None, PlaylistWriteIntent::AuthoritativeReplacement)?;
        tx.execute(
            "INSERT INTO bindings(id,payload) VALUES(?1,?2)",
            params![
                format!("{}:{}", binding.account, binding.playlist),
                serde_json::to_string(&Envelope {
                    version: 1,
                    value: binding
                })
                .map_err(storage)?
            ],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.changed_tables(&[Table::Playlist, Table::Binding]);
        Ok(p.id)
    }
    pub fn remote_cards(&self, account: &AccountId) -> Result<Vec<crate::api::RemoteCard>> {
        let c = self.lock()?;
        let mut q = c.prepare("SELECT payload FROM remote_cache WHERE substr(id,1,length(?1))=?1 AND json_type(payload,'$.value.card')='text'").map_err(storage)?;
        q.query_map([format!("{account}:")], |r| r.get::<_, String>(0))
            .map_err(storage)?
            .map(|r| decode(&r.map_err(storage)?))
            .collect()
    }
    pub fn replace_remote_cards(
        &self,
        account: &AccountId,
        cards: &[crate::api::RemoteCard],
    ) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        tx.execute("DELETE FROM remote_cache WHERE substr(id,1,length(?1))=?1 AND json_type(payload,'$.value.card')='text'", [format!("{account}:")]).map_err(storage)?;
        for card in cards {
            tx.execute(
                "INSERT INTO remote_cache(id,payload) VALUES(?1,?2)",
                params![
                    format!("{account}:{}", card.card),
                    serde_json::to_string(&Envelope {
                        version: 1,
                        value: card
                    })
                    .map_err(storage)?
                ],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        self.changed_tables(&[Table::Remote]);
        Ok(())
    }
    /// Admission and retry use the same durable account/playlist exclusion under one transaction.
    pub fn admit_job(&self, job: &Job) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        publication_guard(&tx, job, false)?;
        let json = serde_json::to_string(&Envelope {
            version: 1,
            value: job,
        })
        .map_err(storage)?;
        tx.execute("INSERT INTO jobs(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", params![job.id.to_string(),json]).map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.changed_tables(&[Table::Job]);
        Ok(())
    }
    pub fn admit_cloud_job(&self, job: &Job, intent: &CloudPlaylistWrite) -> Result<()> {
        let JobRequest::Publish { account, snapshot } = &job.request else {
            return Err(Failure::Validation(
                "A cloud edit must be a publication".into(),
            ));
        };
        snapshot.validate_remote_write()?;
        let mut connection = self.lock()?;
        let tx = connection.transaction().map_err(storage)?;
        publication_guard(&tx, job, false)?;
        if let CloudPlaylistWrite::Update { expected } = intent {
            let current: String = tx
                .query_row(
                    "SELECT payload FROM playlists WHERE id=?1",
                    [snapshot.id.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage)?
                .ok_or(Failure::Conflict)?;
            let current: Playlist = decode(&current)?;
            if current.revision != *expected {
                return Err(Failure::Conflict);
            }
            snapshot.validate_structure_against(&current)?;
        }
        for (table, id, value) in [
            (
                Table::Job,
                job.id.to_string(),
                serde_json::to_value(job).map_err(storage)?,
            ),
            (
                Table::Remote,
                format!("{account}:cloud-write:{}", job.id),
                serde_json::to_value(intent).map_err(storage)?,
            ),
        ] {
            tx.execute(
                &format!("INSERT INTO {}(id,payload) VALUES(?1,?2)", table.name()),
                params![
                    id,
                    serde_json::to_string(&Envelope { version: 1, value }).map_err(storage)?
                ],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(())
    }
    /// A detail refresh replaces only the confirmed cache; a publication in
    /// progress keeps both its last confirmed base and its proposed journal.
    pub fn refresh_cloud_playlist(
        &self,
        playlist: &Playlist,
        binding: &PublicationBinding,
        observed: Revision,
    ) -> Result<PlaylistId> {
        let mut connection = self.lock()?;
        let tx = connection.transaction().map_err(storage)?;
        let current_binding: Option<String> = tx
            .query_row(
                "SELECT payload FROM bindings WHERE id=?1",
                [format!("{}:{}", binding.account, binding.playlist)],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;
        // A late HTTP detail must not replace a newer confirmed receipt or refresh.
        let current = current_binding.ok_or(Failure::Cancelled)?;
        if decode::<PublicationBinding>(&current)?.revision != observed {
            return Ok(binding.playlist);
        }

        if cloud_playlist_pending(&tx, &binding.account, playlist.id)? {
            return Ok(playlist.id);
        }
        let old: Option<String> = tx
            .query_row(
                "SELECT payload FROM playlists WHERE id=?1",
                [playlist.id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;
        let old = old.map(|json| decode::<Playlist>(&json)).transpose()?;
        if let Some(old) = &old {
            archive_legacy_draft(&tx, &binding.account, old)?;
        }
        let expected = old.map(|old| old.revision);
        let revision = write_playlist(
            &tx,
            playlist,
            expected,
            PlaylistWriteIntent::AuthoritativeReplacement,
        )?;
        let mut binding = binding.clone();
        binding.revision = revision;
        tx.execute("INSERT INTO bindings(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![format!("{}:{}",binding.account,binding.playlist),serde_json::to_string(&Envelope{version:1,value:&binding}).map_err(storage)?]).map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(playlist.id)
    }
    pub fn cloud_playlist_pending(
        &self,
        account: &AccountId,
        playlist: PlaylistId,
    ) -> Result<bool> {
        cloud_playlist_pending(&*self.lock()?, account, playlist)
    }
    pub fn evict_cloud_playlist(&self, account: &AccountId, card: &CardId) -> Result<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction().map_err(storage)?;
        let mut query = tx
            .prepare("SELECT payload FROM bindings")
            .map_err(storage)?;
        let bindings = query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage)?
            .map(|row| decode::<PublicationBinding>(&row.map_err(storage)?))
            .collect::<Result<Vec<_>>>()?;
        drop(query);
        for binding in bindings
            .iter()
            .filter(|b| &b.account == account && &b.card == card)
        {
            if cloud_playlist_pending(&tx, account, binding.playlist)? {
                return Err(Failure::Validation(
                    "Une modification Yoto est en cours pour cette playlist.".into(),
                ));
            }
            if let Some(old) = tx
                .query_row(
                    "SELECT payload FROM playlists WHERE id=?1",
                    [binding.playlist.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(storage)?
            {
                archive_legacy_draft(&tx, account, &decode::<Playlist>(&old)?)?;
            }
            tx.execute(
                "DELETE FROM playlists WHERE id=?1",
                [binding.playlist.to_string()],
            )
            .map_err(storage)?;
            tx.execute(
                "DELETE FROM bindings WHERE id=?1",
                [format!("{account}:{}", binding.playlist)],
            )
            .map_err(storage)?;
        }
        tx.execute(
            "DELETE FROM remote_cache WHERE id=?1",
            [format!("{account}:{card}")],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(())
    }
    pub fn guard_publication(&self, job: &Job) -> Result<()> {
        publication_guard(&*self.lock()?, job, true)
    }
    pub fn publication_outcome(&self, job: &Job) -> Result<Option<PublicationOutcome>> {
        read_publication_outcome(&*self.lock()?, job)
    }
    pub fn confirmed_card(
        &self,
        account: &AccountId,
        playlist: PlaylistId,
    ) -> Result<Option<CardId>> {
        let jobs = self.list::<Job>(Table::Job)?;
        for job in jobs.iter().rev() {
            if matches!(&job.request, JobRequest::Publish { account:a, snapshot } if a==account && snapshot.id==playlist)
                && let Some(PublicationOutcome::Confirmed { card, .. }) =
                    self.publication_outcome(job)?
            {
                return Ok(Some(card));
            }
        }
        Ok(None)
    }
    /// Receipt, canonical binding and completed job commit together. If any write
    /// fails, the previously durable Possible/Accepted effect remains authoritative.
    pub fn confirm_publication(
        &self,
        job: &Job,
        pending: &PendingPublication,
        binding: &PublicationBinding,
    ) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        let mut binding = binding.clone();
        let intent: Option<String> = tx
            .query_row(
                "SELECT payload FROM remote_cache WHERE id=?1",
                [format!("{}:cloud-write:{}", binding.account, job.id)],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;
        if let Some(intent) = intent {
            let intent: CloudPlaylistWrite = decode(&intent)?;
            let JobRequest::Publish { snapshot, .. } = &job.request else {
                return Err(Failure::Compatibility(
                    "Cloud receipt has no playlist".into(),
                ));
            };
            let mut canonical = crate::api::read_playlist(&binding.document, &binding.account)?;
            canonical.retain_ids_from(snapshot);
            for chapter in &mut canonical.chapters {
                if let Some(old) = snapshot.chapters.iter().find(|c| c.key == chapter.key) {
                    if matches!(old.icon, Some(IconReference::Local { .. })) {
                        chapter.icon = old.icon.clone();
                    }
                    for track in &mut chapter.tracks {
                        if let Some(old) = old.tracks.iter().find(|t| t.key == track.key) {
                            if matches!(old.media, TrackMedia::Local { .. }) {
                                track.media = old.media.clone();
                            }
                            if matches!(old.icon, Some(IconReference::Local { .. })) {
                                track.icon = old.icon.clone();
                            }
                        }
                    }
                }
            }
            canonical.metadata.cover = snapshot.metadata.cover;
            let expected = match intent {
                CloudPlaylistWrite::Create => None,
                CloudPlaylistWrite::Update { expected } => Some(expected),
            };
            binding.revision = write_playlist(
                &tx,
                &canonical,
                expected,
                PlaylistWriteIntent::AuthoritativeReplacement,
            )?;
        }
        let summary = crate::api::RemoteCard {
            card: binding.card.clone(),
            title: binding
                .document
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            document: binding.document.clone(),
            synced: crate::now(),
        };
        tx.execute("INSERT INTO remote_cache(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![format!("{}:{}",binding.account,binding.card),serde_json::to_string(&Envelope{version:1,value:summary}).map_err(storage)?]).map_err(storage)?;
        let mut pending = pending.clone();
        pending.outcome = PublicationOutcome::Confirmed {
            card: binding.card.clone(),
            revision: binding.revision,
        };
        let mut job = job.clone();
        job.checkpoint = None;
        job.state = JobState::Completed {
            output: JobOutput::Published {
                card: binding.card.clone(),
                revision: binding.revision,
            },
        };
        for (table, id, value) in [
            (
                Table::Binding,
                format!("{}:{}", binding.account, binding.playlist),
                serde_json::to_value(&binding).map_err(storage)?,
            ),
            (
                Table::Remote,
                format!("{}:commit:{}", binding.account, job.id),
                serde_json::to_value(pending).map_err(storage)?,
            ),
            (
                Table::Job,
                job.id.to_string(),
                serde_json::to_value(job).map_err(storage)?,
            ),
        ] {
            let json = serde_json::to_string(&Envelope { version: 1, value }).map_err(storage)?;
            #[cfg(test)]
            self.fail_injected_write(&tx, table, &json)?;
            tx.execute(&format!("INSERT INTO {}(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", table.name()), params![id,json]).map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(())
    }
    pub fn update_media_metadata(&self, id: MediaId, metadata: &MediaMetadata) -> Result<()> {
        Rules::title(&metadata.title)?;
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        let json: String = tx
            .query_row(
                "SELECT payload FROM media_items WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let mut media: MediaItem = decode(&json)?;
        media.title = metadata.title.clone();
        media.author = metadata.author.clone();
        media.tags = metadata.tags.clone();
        let json = serde_json::to_string(&Envelope {
            version: 1,
            value: media,
        })
        .map_err(storage)?;
        tx.execute(
            "UPDATE media_items SET payload=?2 WHERE id=?1",
            params![id.to_string(), json],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(())
    }
    pub fn add_recipe(&self, recipe: &MediaRecipe) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        let json: String = tx
            .query_row(
                "SELECT payload FROM media_items WHERE id=?1",
                [recipe.media.to_string()],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let mut media: MediaItem = decode(&json)?;
        if recipe.original != media.original {
            return Err(Failure::Validation(
                "Recipe must refer to its original media".into(),
            ));
        }
        TrimRange::new(recipe.trim.start(), recipe.trim.end(), media.info.duration)?;
        let json = serde_json::to_string(&Envelope {
            version: 1,
            value: recipe,
        })
        .map_err(storage)?;
        tx.execute(
            "INSERT INTO recipes(id,payload) VALUES(?1,?2)",
            params![recipe.id.to_string(), json],
        )
        .map_err(storage)?;
        // A part is not a version a person chooses: it stays out of the media's list.
        if recipe.part.is_none() {
            media.recipes.push(recipe.id);
        }
        let json = serde_json::to_string(&Envelope {
            version: 1,
            value: media,
        })
        .map_err(storage)?;
        tx.execute(
            "UPDATE media_items SET payload=?2 WHERE id=?1",
            params![recipe.media.to_string(), json],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(())
    }
    pub fn finish_import(
        &self,
        job: &Job,
        media: &MediaItem,
        original: &Asset,
        recipe: &MediaRecipe,
        waveform: &Asset,
    ) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        let put = |table: Table, id: &str, value: serde_json::Value| -> Result<()> {
            let json = serde_json::to_string(&Envelope { version: 1, value }).map_err(storage)?;
            tx.execute(&format!("INSERT INTO {}(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",table.name()),params![id,json]).map_err(storage)?;
            Ok(())
        };
        for asset in [original, waveform] {
            put(
                Table::Asset,
                &asset.id.to_string(),
                serde_json::to_value(asset).map_err(storage)?,
            )?;
        }
        put(
            Table::Media,
            &media.id.to_string(),
            serde_json::to_value(media).map_err(storage)?,
        )?;
        put(
            Table::Recipe,
            &recipe.id.to_string(),
            serde_json::to_value(recipe).map_err(storage)?,
        )?;
        put(
            Table::Remote,
            &format!("waveform:{}", media.id),
            serde_json::to_value(waveform.id).map_err(storage)?,
        )?;
        let mut complete = job.clone();
        complete.state = JobState::Completed {
            output: JobOutput::Imported { media: media.id },
        };
        put(
            Table::Job,
            &job.id.to_string(),
            serde_json::to_value(complete).map_err(storage)?,
        )?;
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(())
    }
    pub fn delete_media(
        &self,
        media: MediaId,
        recipe: Option<RecipeId>,
        policy: ReferencePolicy,
    ) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        delete_media_tx(&tx, media, recipe, policy)?;
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(())
    }
    pub fn finish_encode(
        &self,
        asset: &Asset,
        recipe: &MediaRecipe,
        completion: Option<&Job>,
    ) -> Result<AssetId> {
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        let stored: String = tx
            .query_row(
                "SELECT payload FROM recipes WHERE id=?1",
                [recipe.id.to_string()],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let current: MediaRecipe = decode(&stored)?;
        if let Some(result) = current.result {
            return Ok(result);
        }
        for (table, id, value) in [
            (
                Table::Asset,
                asset.id.to_string(),
                serde_json::to_value(asset).map_err(storage)?,
            ),
            (
                Table::Recipe,
                recipe.id.to_string(),
                serde_json::to_value(recipe).map_err(storage)?,
            ),
        ] {
            tx.execute(&format!("INSERT INTO {}(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",table.name()),params![id,serde_json::to_string(&Envelope{version:1,value}).map_err(storage)?]).map_err(storage)?;
        }
        if let Some(job) = completion {
            let mut job = job.clone();
            job.state = JobState::Completed {
                output: JobOutput::Encoded { asset: asset.id },
            };
            tx.execute(
                "UPDATE jobs SET payload=?2 WHERE id=?1",
                params![
                    job.id.to_string(),
                    serde_json::to_string(&Envelope {
                        version: 1,
                        value: job
                    })
                    .map_err(storage)?
                ],
            )
            .map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(asset.id)
    }
    /// Deletes every media that can go and keeps the others (still in a playlist, used by a
    /// job, or awaiting reconciliation). Returns the media that were kept.
    pub fn delete_media_batch(
        &self,
        media: &[MediaId],
        policy: ReferencePolicy,
    ) -> Result<Vec<MediaId>> {
        let mut c = self.lock()?;
        let mut tx = c.transaction().map_err(storage)?;
        let mut kept = Vec::new();
        for id in media {
            let savepoint = tx.savepoint().map_err(storage)?;
            match delete_media_tx(&savepoint, *id, None, policy) {
                Ok(()) => savepoint.commit().map_err(storage)?,
                // Dropping the savepoint rolls back this media only.
                Err(Failure::Validation(_)) => kept.push(*id),
                Err(error) => return Err(error),
            }
        }
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(kept)
    }
    pub fn pin_draft_asset(&self, asset: &Asset, account: Option<&AccountId>) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        let lease = DraftAssetLease {
            asset: asset.id,
            account: account.cloned(),
            expires: crate::now() + DraftAssetLease::LIFETIME_SECONDS,
        };
        for (table, id, value) in [
            (
                Table::Asset,
                asset.id.to_string(),
                serde_json::to_value(asset).map_err(storage)?,
            ),
            (
                Table::Remote,
                format!("draft-asset:{}", asset.id),
                serde_json::to_value(lease).map_err(storage)?,
            ),
        ] {
            tx.execute(&format!("INSERT INTO {}(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",table.name()),params![id,serde_json::to_string(&Envelope{version:1,value}).map_err(storage)?]).map_err(storage)?;
        }
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(())
    }
    pub fn release_draft_asset(&self, asset: AssetId) -> Result<()> {
        self.remove(Table::Remote, &format!("draft-asset:{asset}"))
    }
    pub fn expire_draft_assets(&self) -> Result<()> {
        self.lock()?.execute("DELETE FROM remote_cache WHERE id LIKE 'draft-asset:%' AND json_extract(payload,'$.value.expires')<=?1",[i64::try_from(crate::now()).map_err(storage)?]).map_err(storage)?;
        Ok(())
    }
    pub fn asset_roots(&self) -> Result<std::collections::BTreeSet<AssetId>> {
        let mut roots = std::collections::BTreeSet::new();
        {
            let c = self.lock()?;
            let mut q = c
                .prepare("SELECT payload FROM remote_cache WHERE id LIKE 'draft-asset:%'")
                .map_err(storage)?;
            for row in q
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(storage)?
            {
                let lease: DraftAssetLease = decode(&row.map_err(storage)?)?;
                if lease.expires > crate::now() {
                    roots.insert(lease.asset);
                }
            }
        }
        for m in self.list::<MediaItem>(Table::Media)? {
            roots.insert(m.original);
            if let Some(w) = self.get::<AssetId>(Table::Remote, &format!("waveform:{}", m.id))? {
                roots.insert(w);
            }
        }
        for r in self.list::<MediaRecipe>(Table::Recipe)? {
            roots.insert(r.original);
            roots.extend(r.result);
        }
        {
            let connection = self.lock()?;
            let mut query = connection
                .prepare("SELECT payload FROM remote_cache WHERE id LIKE '%:legacy-playlist:%'")
                .map_err(storage)?;
            for row in query
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(storage)?
            {
                roots.extend(decode::<Playlist>(&row.map_err(storage)?)?.image_assets());
            }
        }
        let mut playlist = |p: &Playlist| roots.extend(p.image_assets());
        for p in self.list::<Playlist>(Table::Playlist)? {
            playlist(&p);
        }
        let jobs = self.list::<Job>(Table::Job)?;
        for j in &jobs {
            if let JobRequest::Publish { snapshot, .. } = &j.request {
                playlist(snapshot);
            }
        }
        for j in jobs {
            if let JobState::Completed {
                output: JobOutput::Encoded { asset },
            } = j.state
            {
                roots.insert(asset);
            }
        }
        Ok(roots)
    }
    pub fn purge_account(&self, account: &AccountId) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction().map_err(storage)?;
        tx.execute("DELETE FROM remote_cache WHERE id LIKE 'draft-asset:%' AND json_extract(payload,'$.value.account')=?1",[account.as_str()]).map_err(storage)?;
        let receipts = {
            let mut q = tx
                .prepare("SELECT id,payload FROM remote_cache WHERE id LIKE ?1")
                .map_err(storage)?;
            q.query_map([format!("{}:commit:%", account.as_str())], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(storage)?
            .map(|row| {
                let (id, data) = row.map_err(storage)?;
                Ok((id, decode::<PendingPublication>(&data)?))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>>>()?
        };
        for table in [Table::Mapping, Table::Binding, Table::Remote] {
            tx.execute(
                &format!("DELETE FROM {} WHERE id LIKE ?1", table.name()),
                [format!("{}:%", account.as_str())],
            )
            .map_err(storage)?;
        }
        // Keep only a minimal ambiguity tombstone; it prevents a new POST after reconnection.
        let jobs = {
            let mut q = tx.prepare("SELECT payload FROM jobs").map_err(storage)?;
            q.query_map([], |r| r.get::<_, String>(0))
                .map_err(storage)?
                .map(|r| decode::<Job>(&r.map_err(storage)?))
                .collect::<Result<Vec<_>>>()?
        };
        for mut job in jobs {
            let JobRequest::Publish { account: a, .. } = &job.request else {
                continue;
            };
            if a != account {
                continue;
            }
            let receipt_id = format!("{account}:commit:{}", job.id);
            let outcome = match &job.state {
                JobState::Completed {
                    output: JobOutput::Published { card, revision },
                } => Some(PublicationOutcome::Confirmed {
                    card: card.clone(),
                    revision: *revision,
                }),
                JobState::Queued
                | JobState::Running { .. }
                | JobState::Retry { .. }
                | JobState::WaitingForAuthentication
                | JobState::Conflict { .. }
                | JobState::Reconciling
                | JobState::Completed { .. }
                | JobState::Failed { .. }
                | JobState::Cancelled => receipts.get(&receipt_id).map(|p| p.outcome.clone()),
            }
            .unwrap_or_else(|| match &job.state {
                JobState::Completed {
                    output: JobOutput::Published { card, revision },
                } => PublicationOutcome::Confirmed {
                    card: card.clone(),
                    revision: *revision,
                },
                JobState::Reconciling => PublicationOutcome::Possible,
                JobState::Queued
                | JobState::Running { .. }
                | JobState::Retry { .. }
                | JobState::WaitingForAuthentication
                | JobState::Conflict { .. }
                | JobState::Completed { .. }
                | JobState::Failed { .. }
                | JobState::Cancelled => PublicationOutcome::Prepared,
            });
            match &outcome {
                PublicationOutcome::Possible
                | PublicationOutcome::Accepted { .. }
                | PublicationOutcome::Confirmed { .. } => {
                    let JobRequest::Publish { snapshot, .. } = &mut job.request else {
                        unreachable!("Publication filtered above")
                    };
                    let id = snapshot.id;
                    **snapshot = Playlist::new("Disconnected publication receipt".into())?;
                    snapshot.id = id;
                    job.checkpoint = None;
                    job.state = match &outcome {
                        PublicationOutcome::Confirmed { card, revision } => JobState::Completed {
                            output: JobOutput::Published {
                                card: card.clone(),
                                revision: *revision,
                            },
                        },
                        PublicationOutcome::Possible | PublicationOutcome::Accepted { .. } => {
                            JobState::Reconciling
                        }
                        PublicationOutcome::Prepared | PublicationOutcome::Rejected => {
                            unreachable!("Effect filtered above")
                        }
                    };
                    let receipt = PendingPublication {
                        job: job.id,
                        account: account.clone(),
                        document: serde_json::Value::Null,
                        outcome,
                    };
                    for (table, id, value) in [
                        (
                            Table::Remote,
                            receipt_id,
                            serde_json::to_value(receipt).map_err(storage)?,
                        ),
                        (
                            Table::Job,
                            job.id.to_string(),
                            serde_json::to_value(job).map_err(storage)?,
                        ),
                    ] {
                        tx.execute(&format!("INSERT INTO {}(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",table.name()),params![id,serde_json::to_string(&Envelope{version:1,value}).map_err(storage)?]).map_err(storage)?;
                    }
                }
                PublicationOutcome::Prepared | PublicationOutcome::Rejected => {
                    tx.execute("DELETE FROM jobs WHERE id=?1", [job.id.to_string()])
                        .map_err(storage)?;
                }
            }
        }
        let playlists = {
            let mut q = tx
                .prepare("SELECT payload FROM playlists")
                .map_err(storage)?;
            q.query_map([], |r| r.get::<_, String>(0))
                .map_err(storage)?
                .map(|r| decode::<Playlist>(&r.map_err(storage)?))
                .collect::<Result<Vec<_>>>()?
        };
        for mut p in playlists {
            let owned = matches!(&p.origin,DraftOrigin::Existing{base} if &base.account==account);
            let has_remote =
                p.chapters.iter().flat_map(|c| &c.tracks).any(
                    |t| matches!(&t.media,TrackMedia::RemoteAudio{account:a,..} if a==account),
                );
            let has_icon = p.chapters.iter().any(|c| {
                matches!(&c.icon,Some(IconReference::Yoto{account:a,..}) if a==account)
                    || c.tracks.iter().any(
                        |t| matches!(&t.icon,Some(IconReference::Yoto{account:a,..}) if a==account),
                    )
            });
            if !owned && !has_remote && !has_icon {
                continue;
            }
            p.chapters.iter_mut().for_each(|c| {
                if owned {
                    c.extra.clear();
                }
                if matches!(&c.icon,Some(IconReference::Yoto{account:a,..}) if a==account) {
                    c.icon = None;
                }
                c.tracks.retain(
                    |t| !matches!(&t.media,TrackMedia::RemoteAudio{account:a,..} if a==account),
                );
                for t in &mut c.tracks {
                    if owned {
                        t.extra.clear();
                    }
                    if owned || has_remote {
                        t.events = None;
                    }
                    if matches!(&t.icon,Some(IconReference::Yoto{account:a,..}) if a==account) {
                        t.icon = None;
                    }
                }
            });
            p.chapters.retain(|c| !c.tracks.is_empty());
            if p.chapters.is_empty() {
                tx.execute("DELETE FROM playlists WHERE id=?1", [p.id.to_string()])
                    .map_err(storage)?;
            } else {
                if has_remote {
                    p.playback.shuffle.clear();
                }
                if owned {
                    p.origin = DraftOrigin::New;
                    p.extra.clear();
                    p.content_extra.clear();
                    p.metadata.extra.clear();
                    p.playback = Playback::standard();
                }
                write_playlist(
                    &tx,
                    &p,
                    Some(p.revision),
                    PlaylistWriteIntent::ProvenancePurge,
                )?;
            }
        }
        tx.execute("DELETE FROM accounts WHERE id=?1", [account.as_str()])
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.changed();
        Ok(())
    }
    pub fn verify(&self) -> Result<()> {
        let result: String = self
            .lock()?
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(storage)?;
        if result != "ok" {
            return Err(Failure::Storage(
                "Catalogue integrity verification failed".into(),
            ));
        }
        Ok(())
    }
    pub fn backup(&self, destination: &Path, key: &[u8; 32]) -> Result<()> {
        if destination.exists() {
            return Err(Failure::Validation(
                messages::BACKUP_DESTINATION_EXISTS.into(),
            ));
        }
        let mut out = Connection::open(destination).map_err(storage)?;
        out.pragma_update(None, "key", format!("x'{}'", hex::encode(key)))
            .map_err(storage)?;
        let c = self.lock()?;
        let backup = rusqlite::backup::Backup::new(&c, &mut out).map_err(storage)?;
        backup
            .run_to_completion(128, std::time::Duration::from_millis(10), None)
            .map_err(storage)
    }
}
fn delete_media_tx(
    tx: &Connection,
    media: MediaId,
    recipe: Option<RecipeId>,
    policy: ReferencePolicy,
) -> Result<()> {
    let records = |table: Table| -> Result<Vec<serde_json::Value>> {
        let mut q = tx
            .prepare(&format!("SELECT payload FROM {}", table.name()))
            .map_err(storage)?;
        q.query_map([], |r| r.get::<_, String>(0))
            .map_err(storage)?
            .map(|r| decode::<serde_json::Value>(&r.map_err(storage)?))
            .collect()
    };
    let recipes = records(Table::Recipe)?
        .into_iter()
        .map(|v| serde_json::from_value::<MediaRecipe>(v).map_err(storage))
        .collect::<Result<Vec<_>>>()?;
    let ids = recipes
        .iter()
        // Removing a version also removes the parts it was cut into.
        .filter(|r| {
            r.media == media
                && recipe.is_none_or(|id| id == r.id || r.part.is_some_and(|p| p.of == id))
        })
        .map(|r| r.id)
        .collect::<std::collections::BTreeSet<_>>();
    if ids.is_empty() {
        return Err(Failure::Validation(messages::MEDIA_ABSENT.into()));
    }
    let refers = |p: &Playlist| {
        p.chapters
            .iter()
            .flat_map(|c| &c.tracks)
            .any(|t| matches!(t.media,TrackMedia::Local{recipe,..} if ids.contains(&recipe)))
    };
    for value in records(Table::Job)? {
        let job: Job = serde_json::from_value(value).map_err(storage)?;
        let related = match &job.request {
            JobRequest::Encode { recipe } => ids.contains(recipe),
            JobRequest::Publish { snapshot, .. } => refers(snapshot),
            JobRequest::Import { .. } => {
                matches!(job.state,JobState::Completed{output:JobOutput::Imported{media:id}} if id==media)
            }
        };
        if related {
            match job.state {
                JobState::Completed { .. } | JobState::Failed { .. } | JobState::Cancelled => {
                    match read_publication_outcome(tx, &job)? {
                        Some(
                            PublicationOutcome::Possible | PublicationOutcome::Accepted { .. },
                        ) => {
                            return Err(Failure::Validation(
                                messages::MEDIA_AWAITS_RECONCILE.into(),
                            ));
                        }
                        Some(PublicationOutcome::Confirmed { .. }) => {
                            let mut receipt_job = job.clone();
                            if let JobRequest::Publish { snapshot, .. } = &mut receipt_job.request {
                                let id = snapshot.id;
                                **snapshot = Playlist::new("Publication receipt".into())?;
                                snapshot.id = id;
                            }
                            tx.execute(
                                "UPDATE jobs SET payload=?2 WHERE id=?1",
                                params![
                                    job.id.to_string(),
                                    serde_json::to_string(&Envelope {
                                        version: 1,
                                        value: receipt_job
                                    })
                                    .map_err(storage)?
                                ],
                            )
                            .map_err(storage)?;
                        }
                        Some(PublicationOutcome::Prepared | PublicationOutcome::Rejected)
                        | None => {
                            if let JobRequest::Publish { account, .. } = &job.request {
                                tx.execute(
                                    "DELETE FROM remote_cache WHERE id=?1",
                                    [format!("{account}:commit:{}", job.id)],
                                )
                                .map_err(storage)?;
                            }
                            tx.execute("DELETE FROM jobs WHERE id=?1", [job.id.to_string()])
                                .map_err(storage)?;
                        }
                    }
                }
                JobState::Queued
                | JobState::Running { .. }
                | JobState::Retry { .. }
                | JobState::WaitingForAuthentication
                | JobState::Conflict { .. }
                | JobState::Reconciling => {
                    return Err(Failure::Validation(messages::MEDIA_IN_JOB.into()));
                }
            }
        }
    }
    for value in records(Table::Playlist)? {
        let mut p: Playlist = serde_json::from_value(value).map_err(storage)?;
        if !refers(&p) {
            continue;
        }
        match policy {
            ReferencePolicy::RejectReferenced => {
                return Err(Failure::Validation(messages::MEDIA_IN_PLAYLIST.into()));
            }
            ReferencePolicy::RemovePlaylistTracks => {}
        }
        let tracks = p
            .chapters
            .iter()
            .flat_map(|c| &c.tracks)
            .filter(|t| matches!(t.media,TrackMedia::Local{recipe,..} if ids.contains(&recipe)))
            .map(|t| t.id)
            .collect::<Vec<_>>();
        let expected = p.revision;
        for track in tracks {
            p.remove_track(track)?;
        }
        write_playlist(tx, &p, Some(expected), PlaylistWriteIntent::LocalEdit)?;
    }
    for id in &ids {
        tx.execute(
            "DELETE FROM mappings WHERE id IN (SELECT mapping FROM mapping_refs WHERE recipe=?1)",
            [id.to_string()],
        )
        .map_err(storage)?;
        tx.execute("DELETE FROM recipes WHERE id=?1", [id.to_string()])
            .map_err(storage)?;
    }
    if recipe.is_none() {
        tx.execute(
            "DELETE FROM remote_cache WHERE id=?1",
            [format!("waveform:{media}")],
        )
        .map_err(storage)?;
        tx.execute("DELETE FROM media_items WHERE id=?1", [media.to_string()])
            .map_err(storage)?;
    } else {
        let json: String = tx
            .query_row(
                "SELECT payload FROM media_items WHERE id=?1",
                [media.to_string()],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let mut item: MediaItem = decode(&json)?;
        item.recipes.retain(|id| !ids.contains(id));
        tx.execute(
            "UPDATE media_items SET payload=?2 WHERE id=?1",
            params![
                media.to_string(),
                serde_json::to_string(&Envelope {
                    version: 1,
                    value: item
                })
                .map_err(storage)?
            ],
        )
        .map_err(storage)?;
    }
    Ok(())
}
enum PlaylistWriteIntent {
    LocalEdit,
    AuthoritativeReplacement,
    ProvenancePurge,
}
fn write_playlist(
    tx: &Connection,
    p: &Playlist,
    expected: Option<Revision>,
    intent: PlaylistWriteIntent,
) -> Result<Revision> {
    Rules::title(&p.title)?;
    if !p.chapters.is_empty() {
        p.validate()?;
    }
    if let Some(expected) = expected {
        let old: Option<String> = tx
            .query_row(
                "SELECT payload FROM playlists WHERE id=?1",
                [p.id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        let old: Playlist = old
            .map(|j| decode(&j))
            .transpose()?
            .ok_or(Failure::Conflict)?;
        if old.revision != expected {
            return Err(Failure::Conflict);
        }
        match intent {
            PlaylistWriteIntent::LocalEdit => p.validate_structure_against(&old)?,
            PlaylistWriteIntent::AuthoritativeReplacement
            | PlaylistWriteIntent::ProvenancePurge => {}
        }
    }
    let mut canonical = p.clone();
    let image_assets = p.image_assets();
    for id in image_assets {
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM assets WHERE id=?1)",
                [id.to_string()],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if !exists {
            return Err(Failure::Validation(format!(
                "Local image asset {id} is missing. Import it again before saving."
            )));
        }
        tx.execute(
            "DELETE FROM remote_cache WHERE id=?1",
            [format!("draft-asset:{id}")],
        )
        .map_err(storage)?;
    }
    if let Some(expected) = expected {
        canonical.revision = expected.next()?;
    }
    let payload = serde_json::to_string(&Envelope {
        version: 1,
        value: &canonical,
    })
    .map_err(storage)?;
    tx.execute("INSERT INTO playlists(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![p.id.to_string(),payload]).map_err(storage)?;
    tx.execute("DELETE FROM chapters WHERE playlist=?1", [p.id.to_string()])
        .map_err(storage)?;
    for (ci, ch) in p.chapters.iter().enumerate() {
        tx.execute(
            "INSERT INTO chapters(id,playlist,position) VALUES(?1,?2,?3)",
            params![ch.id.to_string(), p.id.to_string(), ci as i64],
        )
        .map_err(storage)?;
        for (ti, t) in ch.tracks.iter().enumerate() {
            tx.execute(
                "INSERT INTO tracks(id,chapter,position) VALUES(?1,?2,?3)",
                params![t.id.to_string(), ch.id.to_string(), ti as i64],
            )
            .map_err(storage)?;
            if let TrackMedia::Local { recipe, media } = &t.media {
                let json: String = tx
                    .query_row(
                        "SELECT payload FROM recipes WHERE id=?1",
                        [recipe.to_string()],
                        |r| r.get(0),
                    )
                    .map_err(storage)?;
                let canonical: MediaRecipe = decode(&json)?;
                if canonical.media != *media {
                    return Err(Failure::Validation(
                        "Track recipe belongs to another media".into(),
                    ));
                }
                tx.execute(
                    "INSERT INTO track_recipes(track,recipe) VALUES(?1,?2)",
                    params![t.id.to_string(), recipe.to_string()],
                )
                .map_err(storage)?;
            }
        }
    }
    Ok(canonical.revision)
}
fn read_publication_outcome(c: &Connection, job: &Job) -> Result<Option<PublicationOutcome>> {
    let JobRequest::Publish { account, .. } = &job.request else {
        return Ok(None);
    };
    if let JobState::Completed {
        output: JobOutput::Published { card, revision },
    } = &job.state
    {
        return Ok(Some(PublicationOutcome::Confirmed {
            card: card.clone(),
            revision: *revision,
        }));
    }
    let stored: Option<String> = c
        .query_row(
            "SELECT payload FROM remote_cache WHERE id=?1",
            [format!("{account}:commit:{}", job.id)],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    stored
        .map(|s| decode::<PendingPublication>(&s).map(|p| p.outcome))
        .transpose()
}
fn publication_guard(c: &Connection, job: &Job, runner: bool) -> Result<()> {
    let JobRequest::Publish { account, snapshot } = &job.request else {
        return Ok(());
    };
    if read_publication_outcome(c, job)?.is_some_and(|o| match o {
        PublicationOutcome::Possible
        | PublicationOutcome::Accepted { .. }
        | PublicationOutcome::Confirmed { .. } => true,
        PublicationOutcome::Prepared | PublicationOutcome::Rejected => false,
    }) {
        return Err(Failure::Validation(messages::PUBLICATION_DURABLE.into()));
    }
    let mut q = c
        .prepare("SELECT payload FROM jobs ORDER BY rowid")
        .map_err(storage)?;
    let rows = q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(storage)?;
    for row in rows {
        let other: Job = decode(&row.map_err(storage)?)?;
        if other.id == job.id {
            continue;
        }
        let JobRequest::Publish {
            account: a,
            snapshot: p,
        } = &other.request
        else {
            continue;
        };
        if a != account || p.id != snapshot.id {
            continue;
        }
        let blocked = read_publication_outcome(c, &other)?.is_some_and(|o| o.uncertain())
            || match other.state {
                JobState::Reconciling | JobState::Conflict { .. } => true,
                JobState::Running { .. } => true,
                JobState::Queued | JobState::Retry { .. } | JobState::WaitingForAuthentication => {
                    !runner || (other.created, other.id) < (job.created, job.id)
                }
                JobState::Completed { .. } | JobState::Failed { .. } | JobState::Cancelled => false,
            };
        if blocked {
            return Err(Failure::Validation(messages::PUBLICATION_PENDING.into()));
        }
    }
    Ok(())
}
fn cloud_playlist_pending(
    connection: &Connection,
    account: &AccountId,
    playlist: PlaylistId,
) -> Result<bool> {
    let mut query = connection
        .prepare("SELECT payload FROM jobs")
        .map_err(storage)?;
    for row in query
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(storage)?
    {
        let job: Job = decode(&row.map_err(storage)?)?;
        if matches!(&job.request,JobRequest::Publish{account:owner,snapshot} if owner==account&&snapshot.id==playlist)
        {
            if read_publication_outcome(connection, &job)?
                .is_some_and(|outcome| outcome.uncertain())
            {
                return Ok(true);
            }
            match job.state {
                JobState::Queued
                | JobState::Running { .. }
                | JobState::Retry { .. }
                | JobState::WaitingForAuthentication
                | JobState::Conflict { .. }
                | JobState::Reconciling => return Ok(true),
                JobState::Completed { .. } | JobState::Failed { .. } | JobState::Cancelled => {}
            }
        }
    }
    Ok(false)
}
#[derive(Serialize, serde::Deserialize)]
struct Envelope<T> {
    version: u32,
    value: T,
}
fn decode<T: DeserializeOwned>(json: &str) -> Result<T> {
    let e: Envelope<T> = serde_json::from_str(json).map_err(storage)?;
    if e.version != 1 {
        return Err(Failure::Storage("Unsupported record version".into()));
    }
    Ok(e.value)
}
#[derive(Clone, Copy)]
pub enum Table {
    Media,
    Asset,
    Recipe,
    Playlist,
    Job,
    Account,
    Mapping,
    Binding,
    Remote,
}
impl Table {
    pub const ALL: [Self; 9] = [
        Self::Media,
        Self::Asset,
        Self::Recipe,
        Self::Playlist,
        Self::Job,
        Self::Account,
        Self::Mapping,
        Self::Binding,
        Self::Remote,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::Media => "media_items",
            Self::Asset => "assets",
            Self::Recipe => "recipes",
            Self::Playlist => "playlists",
            Self::Job => "jobs",
            Self::Account => "accounts",
            Self::Mapping => "mappings",
            Self::Binding => "bindings",
            Self::Remote => "remote_cache",
        }
    }
}

fn archive_legacy_draft(
    connection: &Connection,
    account: &AccountId,
    playlist: &Playlist,
) -> Result<()> {
    let previous: Option<String> = connection
        .query_row(
            "SELECT payload FROM bindings WHERE id=?1",
            [format!("{account}:{}", playlist.id)],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?;
    if let Some(previous) = previous
        && decode::<PublicationBinding>(&previous)?.revision != playlist.revision
    {
        connection
            .execute(
                "INSERT INTO remote_cache(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO NOTHING",
                params![
                    format!(
                        "{account}:legacy-playlist:{}:{}",
                        playlist.id, playlist.revision.0
                    ),
                    serde_json::to_string(&Envelope {
                        version: 1,
                        value: playlist
                    })
                    .map_err(storage)?
                ],
            )
            .map_err(storage)?;
    }
    Ok(())
}
