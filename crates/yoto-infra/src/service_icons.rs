use super::*;

impl Service {
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
