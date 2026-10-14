use super::*;
async fn bounded(response: reqwest::Response, maximum: u64) -> Result<Vec<u8>, UpdateError> {
    let mut response = response
        .error_for_status()
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    if response.content_length().is_some_and(|n| n > maximum) {
        return Err(UpdateError::Manifest("Response exceeds size limit".into()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?
    {
        if bytes.len() as u64 + chunk.len() as u64 > maximum {
            return Err(UpdateError::Manifest("Response exceeds size limit".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
pub(super) async fn check(
    client: &reqwest::Client,
    current: &AppVersion,
) -> Result<Option<Release>, UpdateError> {
    let response = client
        .get(MANIFEST_URL)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let bytes = bounded(response, 128_000).await?;
    let manifest =
        serde_json::from_slice(&bytes).map_err(|e| UpdateError::Manifest(e.to_string()))?;
    select_release(manifest, current, Platform::current()?)
}
pub(super) async fn download(
    owner: &Updater,
    release: Arc<Release>,
) -> Result<PreparedUpdate, UpdateError> {
    let mut response = owner
        .client
        .get(&release.artifact.url)
        .send()
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?
        .error_for_status()
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    if response
        .content_length()
        .is_some_and(|n| n != release.artifact.size)
    {
        return Err(UpdateError::Archive(
            "Archive size differs from manifest".into(),
        ));
    }
    let mut bytes = Vec::new();
    loop {
        if owner.cancel.load(Ordering::SeqCst) {
            return Err(UpdateError::Cancelled);
        }
        let next = tokio::select! {
            next = response.chunk() => next.map_err(|e| UpdateError::Network(e.to_string()))?,
            _ = tokio::time::sleep(std::time::Duration::from_millis(150)) => continue,
        };
        let Some(chunk) = next else { break };
        if bytes.len() as u64 + chunk.len() as u64 > release.artifact.size {
            return Err(UpdateError::Archive("Archive exceeds declared size".into()));
        }
        bytes.extend_from_slice(&chunk);
        owner.set(UpdateState::Downloading {
            release: release.clone(),
            received: bytes.len() as u64,
            total: release.artifact.size,
        });
    }
    if bytes.len() as u64 != release.artifact.size {
        return Err(UpdateError::Archive("Incomplete download".into()));
    }
    owner.set(UpdateState::Verifying(release.clone()));
    let prepared = tokio::task::spawn_blocking(move || {
        verify(&bytes, &release.artifact.signature, PUBLIC_KEY)?;
        // Bind the signed payload to the manifest version/platform to prevent replay
        // of an older authentic archive under a forged newer manifest.
        verify_identity(
            &release.artifact.signature,
            &format!("YotoMediaManager {}", release.archive_name()),
        )?;
        install::prepare(&bytes, release.platform, release.version.clone())
    })
    .await
    .map_err(|e| UpdateError::Install(e.to_string()))??;
    if owner.cancel.load(Ordering::SeqCst) {
        return Err(UpdateError::Cancelled);
    }
    Ok(prepared)
}
