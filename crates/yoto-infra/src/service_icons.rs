use super::*;

impl Service {
    pub(super) async fn store_icon_cover(
        &self,
        account: &AccountId,
        media: &YotoMediaId,
        collection: crate::api::IconCollection,
    ) -> Result<Reply> {
        // Resolve the image from this account's authoritative catalogue, never a caller URL.
        self.auth.access(account, Scope::UserIconsManage).await?;
        let url = self
            .remote
            .lock()
            .map_err(storage)?
            .by_account
            .get(account)
            .and_then(|cache| {
                cache
                    .icons
                    .iter()
                    .find(|icon| &icon.media == media && icon.collection == collection)
            })
            .and_then(|icon| icon.url.clone())
            .ok_or_else(|| {
                Failure::Validation("Icon unavailable in this account catalogue".into())
            })?;
        let bytes = self.api.icon_image(&url).await?;
        let _guard = self.protect_assets()?;
        let source = self.store.lease("png")?;
        std::fs::write(source.path(), &bytes).map_err(storage)?;
        let image = crate::media::read_image(source.path())?;
        let cover = crate::media::icon_cover(image);
        let output = self.store.lease("png")?;
        cover.save(output.path()).map_err(crate::media_error)?;
        let asset = self.store.put(output.path(), "png")?;
        self.db.pin_draft_asset(&asset, None)?;
        Ok(Reply::Asset(asset.id))
    }
    pub(super) async fn refresh_icons(&self, account: &AccountId) -> Result<Reply> {
        let (epoch, generation) = {
            let mut cache = self.remote.lock().map_err(storage)?;
            let current = cache.by_account.entry(account.clone()).or_default();
            current.icons_generation = current.icons_generation.wrapping_add(1);
            current
                .sync
                .insert(RemoteCollection::Icons, RemoteSyncState::Syncing);
            let generation = current.icons_generation;
            cache.generation = cache.generation.wrapping_add(1);
            (cache.epoch, generation)
        };
        let result = async {
            let token = self.auth.access(account, Scope::UserIconsManage).await?;
            self.api.icons(&token).await
        }
        .await;
        let mut cache = self.remote.lock().map_err(storage)?;
        if cache.epoch != epoch
            || cache
                .by_account
                .get(account)
                .is_none_or(|current| current.icons_generation != generation)
        {
            if let Some(current) = cache.by_account.get_mut(account)
                && current.icons_generation == generation
            {
                current.sync.insert(
                    RemoteCollection::Icons,
                    RemoteSyncState::Failed {
                        failure: Failure::Cancelled,
                    },
                );
                cache.generation = cache.generation.wrapping_add(1);
            }
            return Err(Failure::Cancelled);
        }
        let mut failure = None;
        apply_collection(
            cache.by_account.entry(account.clone()).or_default(),
            RemoteCollection::Icons,
            result.map(CollectionResult::Icons),
            &mut failure,
        );
        cache.generation = cache.generation.wrapping_add(1);
        match failure {
            Some(failure) => Err(failure),
            None => Ok(Reply::Updated),
        }
    }
}
