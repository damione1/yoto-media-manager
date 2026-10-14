use super::*;
impl Service {
    pub(super) fn enqueue_cloud(
        &self,
        account: AccountId,
        mut draft: Playlist,
        intent: CloudPlaylistWrite,
    ) -> Result<Reply> {
        draft.validate_remote_write()?;
        draft.revision = match intent {
            CloudPlaylistWrite::Create => Revision(1),
            CloudPlaylistWrite::Update { expected } => expected.next()?,
        };
        let playlist = draft.id;
        let job = Job {
            id: JobId::new(),
            request: JobRequest::Publish {
                account,
                snapshot: Box::new(draft),
            },
            state: JobState::Queued,
            attempts: 0,
            created: now(),
            checkpoint: None,
            dismissed: false,
        };
        self.db.admit_cloud_job(&job, &intent)?;
        self.notify.notify_one();
        Ok(Reply::PlaylistOperation {
            playlist,
            job: job.id,
        })
    }
    pub(super) fn install_confirmed_card(&self, account: &AccountId) -> Result<()> {
        let cards = self.db.remote_cards(account)?;
        let mut cache = self.remote.lock().map_err(storage)?;
        cache.by_account.entry(account.clone()).or_default().cards = cards;
        cache.generation = cache.generation.wrapping_add(1);
        Ok(())
    }
    pub(super) async fn load_cloud_playlist(
        &self,
        account: &AccountId,
        card: &CardId,
    ) -> Result<PlaylistId> {
        let existing = self
            .db
            .list::<PublicationBinding>(Table::Binding)?
            .into_iter()
            .find(|binding| &binding.account == account && &binding.card == card);
        if let Some(binding) = &existing
            && self.db.cloud_playlist_pending(account, binding.playlist)?
        {
            return Ok(binding.playlist);
        }
        let token = self.auth.access(account, Scope::UserContentView).await?;
        let document = self.api.card(&token, card).await?;
        self.cache_cloud_document(account, card, existing, document)
    }
    fn cache_cloud_document(
        &self,
        account: &AccountId,
        card: &CardId,
        existing: Option<PublicationBinding>,
        document: serde_json::Value,
    ) -> Result<PlaylistId> {
        let mut playlist = crate::api::read_playlist(&document, account)?;
        if let Some(binding) = &existing
            && let Some(old) = self
                .db
                .get::<Playlist>(Table::Playlist, &binding.playlist.to_string())?
        {
            if meaningful(&document) == meaningful(&binding.document)
                && old.revision == binding.revision
            {
                return Ok(old.id);
            }
            playlist.retain_ids_from(&old);
        }
        self.reassociate(&mut playlist)?;
        let binding = PublicationBinding {
            account: account.clone(),
            playlist: playlist.id,
            card: card.clone(),
            revision: playlist.revision,
            document,
        };
        match existing {
            Some(previous) => {
                self.db
                    .refresh_cloud_playlist(&playlist, &binding, previous.revision)
            }
            None => self.db.import_remote(&playlist, &binding),
        }
    }
    pub(super) async fn refresh_cloud_playlists(&self, account: &AccountId) -> Result<Reply> {
        let (epoch, generation) = {
            let mut cache = self.remote.lock().map_err(storage)?;
            let current = cache.by_account.entry(account.clone()).or_default();
            current
                .sync
                .insert(RemoteCollection::Playlists, RemoteSyncState::Syncing);
            current.sync_generation = current.sync_generation.wrapping_add(1);
            let generation = current.sync_generation;
            cache.generation = cache.generation.wrapping_add(1);
            (cache.epoch, generation)
        };
        let result = async {
            let token = self.auth.access(account, Scope::UserContentView).await?;
            self.api.library(&token).await
        }
        .await;
        let cards = {
            let mut cache = self.remote.lock().map_err(storage)?;
            if cache.epoch != epoch
                || cache
                    .by_account
                    .get(account)
                    .is_none_or(|current| current.sync_generation != generation)
            {
                return Err(Failure::Cancelled);
            }
            let cards = match result {
                Ok(cards) => cards,
                Err(failure) => {
                    cache
                        .by_account
                        .entry(account.clone())
                        .or_default()
                        .sync
                        .insert(
                            RemoteCollection::Playlists,
                            RemoteSyncState::Failed {
                                failure: failure.clone(),
                            },
                        );
                    cache.generation = cache.generation.wrapping_add(1);
                    return Err(failure);
                }
            };
            self.db.replace_remote_cards(account, &cards)?;
            let current = cache.by_account.entry(account.clone()).or_default();
            current.cards = cards.clone();
            current.sync.insert(
                RemoteCollection::Playlists,
                RemoteSyncState::Ready { synced: now() },
            );
            cache.generation = cache.generation.wrapping_add(1);
            cards
        };
        if let Err(failure) = self.reconcile_cloud_summaries(account, &cards).await {
            self.cloud_refresh_failed(account, epoch, generation, &failure)?;
            return Err(failure);
        }
        Ok(Reply::Updated)
    }
    pub(super) fn cloud_refresh_failed(
        &self,
        account: &AccountId,
        epoch: u64,
        generation: u64,
        failure: &Failure,
    ) -> Result<()> {
        let mut cache = self.remote.lock().map_err(storage)?;
        if cache.epoch == epoch
            && cache
                .by_account
                .get(account)
                .is_some_and(|current| current.sync_generation == generation)
        {
            cache
                .by_account
                .entry(account.clone())
                .or_default()
                .sync
                .insert(
                    RemoteCollection::Playlists,
                    RemoteSyncState::Failed {
                        failure: failure.clone(),
                    },
                );
            cache.generation = cache.generation.wrapping_add(1);
        }
        Ok(())
    }
    pub(super) async fn reconcile_cloud_summaries(
        &self,
        account: &AccountId,
        cards: &[RemoteCard],
    ) -> Result<()> {
        // Only already-opened card details need reconciliation; summaries are sufficient for listing.
        let bindings = self.db.list::<PublicationBinding>(Table::Binding)?;
        for binding in bindings
            .iter()
            .filter(|binding| &binding.account == account)
        {
            if cards.iter().any(|card| card.card == binding.card) {
                self.load_cloud_playlist(account, &binding.card).await?;
            } else if !self.db.cloud_playlist_pending(account, binding.playlist)? {
                // MYO indexing can lag a confirmed create. Only an authoritative detail
                // 404 proves removal; a missing index entry alone cannot discard a receipt.
                let token = self.auth.access(account, Scope::UserContentView).await?;
                if let Some(document) = self.api.card_if_present(&token, &binding.card).await? {
                    self.cache_cloud_document(
                        account,
                        &binding.card,
                        Some(binding.clone()),
                        document.clone(),
                    )?;
                    let summary = RemoteCard {
                        card: binding.card.clone(),
                        title: document
                            .get("title")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        document,
                        synced: now(),
                    };
                    self.db.put(
                        Table::Remote,
                        &format!("{account}:{}", binding.card),
                        &summary,
                    )?;
                    self.install_confirmed_card(account)?;
                } else {
                    self.db.evict_cloud_playlist(account, &binding.card)?;
                }
            }
        }
        Ok(())
    }
}
