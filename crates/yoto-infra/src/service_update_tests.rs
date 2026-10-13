use super::*;
#[tokio::test]
async fn updater_shutdown_waits_for_admitted_commands_and_refuses_active_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let tools = Tools::at(std::env::var_os("YOTO_TEST_TOOLS").unwrap().into());
    let service = Service::isolated(directory.path().to_path_buf(), tools, [7; 32]).unwrap();
    let queued = service
        .enqueue(JobRequest::Import {
            selection: ImportSelection::LocalFile {
                path: directory.path().join("input.mp3"),
            },
        })
        .unwrap();
    assert!(service.prepare_update_shutdown().await.is_err());
    assert!(
        !service
            .update_shutdown
            .load(std::sync::atomic::Ordering::SeqCst)
    );
    let mut job = service.job(queued).unwrap();
    job.state = JobState::Cancelled;
    service
        .db
        .put(Table::Job, &queued.to_string(), &job)
        .unwrap();
    // Simulate an admitted command holding the same read gate as execute().
    let admitted = service.update_gate.read().await;
    let owner = service.clone();
    let shutdown = tokio::spawn(async move { owner.prepare_update_shutdown().await });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(!shutdown.is_finished());
    drop(admitted);
    shutdown.await.unwrap().unwrap();
    assert!(
        service
            .execute(Command::NewPlaylist("Must not be admitted".into()))
            .await
            .is_err()
    );
    assert!(
        service.prepare_update_shutdown().await.is_err(),
        "A second helper must never be armed"
    );
    // Read-only update state remains discoverable to the UI/MCP during restart.
    assert!(matches!(
        service
            .execute(Command::AppUpdate(yoto_updater::UpdateAction::State))
            .await
            .unwrap(),
        Reply::AppUpdate(_)
    ));
    service.cancel_update_shutdown();
    assert!(
        service
            .execute(Command::NewPlaylist("Allowed after abort".into()))
            .await
            .is_ok()
    );
}
