use crate::{Command, Service, db::Table, media::Tools};
use yoto_domain::*;
/// Offline bundle smoke: actual sidecars, SQLCipher, encrypted originals, exact recipe, durable jobs.
pub async fn smoke(tools: Tools) -> Result<()> {
    let temp = tempfile::tempdir().map_err(crate::storage)?;
    let audio = temp.path().join("fixture.mp3");
    let mut generate = std::process::Command::new(&tools.ffmpeg);
    generate
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=6",
            "-c:a",
            "libmp3lame",
            "-y",
        ])
        .arg(&audio);
    let generated = crate::process::output(generate, 100_000, std::time::Duration::from_secs(30))?;
    if !generated.status.success() {
        return Err(Failure::Tool("Synthetic audio generation failed".into()));
    }
    let service = Service::isolated(temp.path().join("catalogue"), tools, [92; 32])?;
    let worker = service.start();
    service
        .execute(Command::Import(vec![ImportSelection::LocalFile {
            path: audio,
        }]))
        .await?;
    wait(&service).await?;
    let snapshot = service.snapshot()?;
    let media = snapshot
        .media
        .first()
        .ok_or_else(|| Failure::Storage("Import did not produce a media".into()))?;
    let trim = TrimRange::new(
        MediaTime::from_seconds(1.)?,
        MediaTime::from_seconds(4.)?,
        media.info.duration,
    )?;
    let crate::Reply::Prepared(recipe) = service
        .execute(Command::Prepare {
            media: media.id,
            trim,
            normalization: Normalization::Preserve,
        })
        .await?
    else {
        return Err(Failure::Storage("Preparation reply invalid".into()));
    };
    wait(&service).await?;
    let r = service.recipe(recipe)?;
    // The editor can retain metadata while a preparation finishes in the background.
    // Saving that metadata must never replace the media's newly appended recipe references.
    service
        .execute(Command::UpdateMedia {
            media: media.id,
            metadata: MediaMetadata {
                title: "Renamed after preparation".into(),
                author: "Self-test".into(),
                tags: vec!["trim".into()],
            },
        })
        .await?;
    let updated = service.media(media.id)?;
    if !updated.recipes.contains(&recipe) || updated.recipes.len() != 2 {
        return Err(Failure::Storage(
            "Metadata save lost the prepared recipe".into(),
        ));
    }
    let info = r
        .info
        .ok_or_else(|| Failure::Media("Prepared audio missing".into()))?;
    if (info.duration.seconds() - 3.).abs() > 0.02 {
        return Err(Failure::Media(format!(
            "Exact trim failed: {} seconds",
            info.duration.seconds()
        )));
    }
    service
        .execute(Command::NewPlaylist("Bundle self-test".into()))
        .await?;
    let p = service.snapshot()?.playlists[0].id;
    service
        .execute(Command::AddMedia {
            playlist: p,
            media: vec![(media.id, recipe)],
            part_title: None,
            chapter: None,
        })
        .await?;
    let draft = service.playlist(p)?;
    draft.validate()?;
    if !matches!(draft.chapters[0].tracks[0].media,TrackMedia::Local{recipe:r,..} if r==recipe) {
        return Err(Failure::Storage("Selected recipe lost".into()));
    }
    let header = std::fs::read(&service.db.path).map_err(crate::storage)?;
    if header.starts_with(b"SQLite format 3") {
        return Err(Failure::Storage("Database is plaintext".into()));
    }
    service.shutdown();
    worker.abort();
    Ok(())
}
async fn wait(service: &Service) -> Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(40);
    loop {
        let jobs = service.db.list::<Job>(Table::Job)?;
        let mut done = true;
        for job in jobs {
            match job.state {
                JobState::Completed { .. } => {}
                JobState::Queued | JobState::Running { .. } | JobState::Retry { .. } => {
                    done = false
                }
                JobState::Failed { failure } => return Err(failure),
                JobState::WaitingForAuthentication => return Err(Failure::Authentication),
                JobState::Conflict { .. } => return Err(Failure::Conflict),
                JobState::Reconciling => return Err(Failure::Ambiguous),
                JobState::Cancelled => return Err(Failure::Cancelled),
            }
        }
        if done {
            return Ok(());
        }
        if tokio::time::Instant::now() > deadline {
            return Err(Failure::Media("Bundle smoke job timed out".into()));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
