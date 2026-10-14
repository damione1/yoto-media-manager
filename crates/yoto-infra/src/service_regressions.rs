use super::{
    tests::{fixture, streaming},
    *,
};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};

fn returned_asset(reply: Reply) -> AssetId {
    match reply {
        Reply::Asset(asset) => asset,
        Reply::AppUpdate(_)
        | Reply::YoutubeTool(_)
        | Reply::Updated
        | Reply::Saved(_)
        | Reply::PlaylistOperation { .. }
        | Reply::OpenedPlaylist(_)
        | Reply::Entries(_)
        | Reply::Waveform(_)
        | Reply::Prepared(_)
        | Reply::PixelArt(_)
        | Reply::Image(_)
        | Reply::Images(_)
        | Reply::Player(_)
        | Reply::Data(_) => panic!("expected an asset reply"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn blocked_provider_encoder_and_preview_allow_controls_and_cancel_descendants_before_commit()
{
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let real = Tools::at(std::env::var_os("YOTO_TEST_TOOLS").unwrap().into());
    let input = root.path().join("tone.mp3");
    assert!(
        std::process::Command::new(&real.ffmpeg)
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=6",
                "-codec:a",
                "libmp3lame",
                "-y"
            ])
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    let blocked = root.path().join("block");
    let started = root.path().join("started");
    let descendant = root.path().join("child");
    let wrapper = root.path().join("ffmpeg-wrapper");
    let quote = |p: &std::path::Path| format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"));
    std::fs::write(&wrapper,format!("#!/bin/sh\nmode=$(cat {} 2>/dev/null)\ncase \"$mode:$*\" in\nwaveform:*'-f f32le'*|encode:*'-codec:a libmp3lame'*|preview:*'-codec:a pcm_s16le'*)\necho $$ > {}\nsleep 20 &\necho $! > {}\nwait\n;;\nesac\nexec {} \"$@\"\n",quote(&blocked),quote(&started),quote(&descendant),quote(&real.ffmpeg))).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut tools = real;
    tools.ffmpeg = wrapper;
    let profile = root.path().join("profile");
    let service = Service::isolated(profile.clone(), tools, [41; 32]).unwrap();
    let original = service.store.put(&input, "mp3").unwrap();
    service
        .db
        .put(Table::Asset, &original.id.to_string(), &original)
        .unwrap();
    let (media, recipe) = crate::tests::catalogue_fixture(&service, original.id, "Control media");
    let ipc = crate::bridge::start(service.clone(), profile.clone(), true).unwrap();
    for mode in ["waveform", "encode"] {
        let _ = std::fs::remove_file(&started);
        let _ = std::fs::remove_file(&descendant);
        std::fs::write(&blocked, mode).unwrap();
        let request = if mode == "waveform" {
            JobRequest::Import {
                selection: ImportSelection::LocalFile {
                    path: input.clone(),
                },
            }
        } else {
            JobRequest::Encode { recipe: recipe.id }
        };
        let id = service.enqueue(request).unwrap();
        let run = service.clone();
        let job = service.job(id).unwrap();
        let worker = tokio::spawn(async move { run.run_job(job).await });
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !descendant.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await
            }
        })
        .await
        .unwrap();
        let before = std::time::Instant::now();
        for command in [
            Command::JobsPage {
                offset: 0,
                limit: 20,
            },
            Command::UpdateMedia {
                media: media.id,
                metadata: MediaMetadata {
                    title: format!("Edited during {mode}"),
                    author: String::new(),
                    tags: vec![],
                },
            },
            Command::Import(vec![ImportSelection::LocalFile {
                path: input.clone(),
            }]),
            Command::CollectGarbage,
            Command::Cancel(id),
        ] {
            crate::bridge::call(&profile, true, crate::bridge::Request::Execute(command))
                .await
                .unwrap();
        }
        assert!(
            before.elapsed() < std::time::Duration::from_secs(1),
            "controls waited behind {mode}"
        );
        tokio::time::timeout(std::time::Duration::from_secs(3), worker)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            service.job(id).unwrap().state,
            JobState::Cancelled
        ));
        assert_eq!(service.db.list::<MediaItem>(Table::Media).unwrap().len(), 1);
        assert!(service.recipe(recipe.id).unwrap().result.is_none());
        let pid: i32 = std::fs::read_to_string(&descendant)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while unsafe { libc::kill(pid, 0) } == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await
            }
        })
        .await
        .unwrap();
        service.execute(Command::CollectGarbage).await.unwrap();
        assert_eq!(service.store.inventory().unwrap(), vec![original.id]);
    }
    let _ = std::fs::remove_file(&descendant);
    std::fs::write(&blocked, "preview").unwrap();
    let old = service.clone();
    let id = media.id;
    let first = tokio::spawn(async move {
        old.execute(Command::PreviewTrim {
            media: id,
            trim: TrimRange::new(
                MediaTime::ZERO,
                MediaTime::from_seconds(1.).unwrap(),
                MediaTime::from_seconds(6.).unwrap(),
            )
            .unwrap(),
        })
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while !descendant.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await
        }
    })
    .await
    .unwrap();
    std::fs::remove_file(&blocked).unwrap();
    let newer = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        service.execute(Command::PreviewTrim {
            media: media.id,
            trim: TrimRange::new(
                MediaTime::from_seconds(2.).unwrap(),
                MediaTime::from_seconds(3.).unwrap(),
                media.info.duration,
            )
            .unwrap(),
        }),
    )
    .await
    .unwrap();
    if let Err(error) = newer {
        assert!(
            error.to_string().contains("Audio output unavailable"),
            "{error}"
        );
    }
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(3), first)
            .await
            .unwrap()
            .unwrap(),
        Err(Failure::Cancelled)
    ));
    service.execute(Command::StopPreview).await.unwrap();
    ipc.abort();
    let _ = ipc.await;
}

#[tokio::test]
async fn accepted_post_survives_each_local_sql_failure_stale_state_and_restart_without_repost() {
    for point in [
        crate::db::InjectedWrite::Receipt,
        crate::db::InjectedWrite::Verifying,
        crate::db::InjectedWrite::Binding,
    ] {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("catalogue");
        let (service, account) = fixture(profile.clone(), server.uri());
        let playlist = streaming();
        service.db.save_playlist(&playlist).unwrap();
        let mut remote =
            crate::api::write_playlist(&playlist, &account, &[], &BTreeMap::new(), None).unwrap();
        remote["cardId"] = serde_json::json!("accepted-card");
        Mock::given(method("POST"))
            .and(path("/content"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"card":{"cardId":"accepted-card"}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/content/accepted-card"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"card":remote})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/content/mine"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"cards":[{"cardId":"accepted-card","title":playlist.title}]}),
            ))
            .mount(&server)
            .await;
        let id = service
            .enqueue(JobRequest::Publish {
                account: account.clone(),
                snapshot: Box::new(playlist.clone()),
            })
            .unwrap();
        service.db.inject_sql_failure(point);
        service.run_job(service.job(id).unwrap()).await;
        assert!(matches!(
            service.job(id).unwrap().state,
            JobState::Reconciling
        ));
        assert!(
            service
                .db
                .publication_outcome(&service.job(id).unwrap())
                .unwrap()
                .unwrap()
                .uncertain()
        );
        // A historical state alone cannot authorize a replay, including legacy/bad saves.
        for state in [
            JobState::Failed {
                failure: Failure::Storage("local write".into()),
            },
            JobState::Cancelled,
        ] {
            let mut stale = service.job(id).unwrap();
            stale.state = state;
            service.save_job(&stale).unwrap();
            assert!(service.execute(Command::Retry(id)).await.is_err());
            assert!(
                service
                    .execute(Command::Publish {
                        account: account.clone(),
                        playlist: playlist.id
                    })
                    .await
                    .is_err()
            );
        }
        drop(service);
        let (service, account) = fixture(profile, server.uri());
        assert!(matches!(
            service.job(id).unwrap().state,
            JobState::Reconciling
        ));
        assert!(service.execute(Command::Retry(id)).await.is_err());
        assert!(
            service
                .execute(Command::Publish {
                    account: account.clone(),
                    playlist: playlist.id
                })
                .await
                .is_err()
        );
        service.execute(Command::Reconcile(id)).await.unwrap();
        assert!(matches!(
            service.job(id).unwrap().state,
            JobState::Completed { .. }
        ));
        assert!(matches!(
            service
                .db
                .publication_outcome(&service.job(id).unwrap())
                .unwrap(),
            Some(PublicationOutcome::Confirmed { .. })
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
    }
}

#[tokio::test]
async fn repeated_account_authorization_keeps_epoch_and_late_old_work_cannot_survive_logout() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = fixture(root.path().join("profile"), server.uri());
    let old = service.account_session(&account).unwrap();
    let gate = old.gate.clone().read_owned().await;
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let worker_service = service.clone();
    let worker_epoch = old.clone();
    let worker_account = account.clone();
    let worker = tokio::task::spawn_blocking(move || {
        let _gate = gate;
        started.send(()).unwrap();
        wait.recv().unwrap();
        if !worker_epoch
            .cancelled
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            worker_service
                .db
                .put(
                    Table::Remote,
                    &format!("{worker_account}:late"),
                    &serde_json::json!({"private":true}),
                )
                .unwrap();
        }
    });
    ready.await.unwrap();
    // This is the common activation path after TokenBroker has replaced the grant
    // under its own account lock. It must retain the owner of this existing worker.
    service.activate_account_session(&account).await.unwrap();
    assert!(Arc::ptr_eq(
        &old,
        &service.account_session(&account).unwrap()
    ));
    let closing = service.clone();
    let id = account.clone();
    let logout = tokio::spawn(async move { closing.execute(Command::Disconnect(id)).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), old.cancelled())
        .await
        .unwrap();
    assert!(!logout.is_finished());
    release.send(()).unwrap();
    worker.await.unwrap();
    logout.await.unwrap().unwrap();
    assert!(
        service
            .db
            .get::<serde_json::Value>(Table::Remote, &format!("{account}:late"))
            .unwrap()
            .is_none()
    );
    assert!(
        service
            .db
            .get::<Account>(Table::Account, account.as_str())
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn rich_remote_replacement_and_privacy_purge_bypass_only_local_edit_semantics() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = fixture(root.path().join("profile"), server.uri());
    let file = root.path().join("original");
    std::fs::write(&file, b"preserved local original").unwrap();
    let asset = service.store.put(&file, "mp3").unwrap();
    service
        .db
        .put(Table::Asset, &asset.id.to_string(), &asset)
        .unwrap();
    let (media, recipe) = crate::tests::catalogue_fixture(&service, asset.id, "Local");
    let mut playlist = streaming();
    playlist.add_media(&media, recipe.id).unwrap();
    playlist.chapters[0].tracks[0].media = TrackMedia::RemoteAudio {
        account: account.clone(),
        media: YotoMediaId::new("remote").unwrap(),
        info: media.info.clone(),
    };
    let mut event = Event::new(EventCommand::Goto);
    event.params = Some(std::collections::HashMap::from([(
        "chapter".into(),
        serde_json::json!("private"),
    )]));
    let mut events = TrackEvents::new();
    events.on_end = Some(Box::new(event));
    playlist.chapters[1].tracks[0].events = Some(events);
    playlist.playback.shuffle = vec![ShuffleGroup {
        start: 0,
        end: 1,
        limit: 2,
    }];
    playlist.origin = DraftOrigin::Existing {
        base: RemoteBase {
            account: account.clone(),
            card: CardId::new("card").unwrap(),
            document: serde_json::json!({"private":"old"}),
        },
    };
    service.db.save_playlist(&playlist).unwrap();
    let mut local_edit = playlist.clone();
    local_edit.chapters.swap(0, 1);
    assert!(
        service
            .db
            .save_playlist_cas(&local_edit, playlist.revision)
            .is_err()
    );
    let mut authoritative = playlist.clone();
    authoritative.chapters.swap(0, 1);
    let id = service
        .enqueue(JobRequest::Publish {
            account: account.clone(),
            snapshot: Box::new(playlist.clone()),
        })
        .unwrap();
    let mut resolved = service.job(id).unwrap();
    resolved.state = JobState::Cancelled;
    let binding = PublicationBinding {
        account: account.clone(),
        playlist: playlist.id,
        card: CardId::new("card").unwrap(),
        revision: playlist.revision,
        document: serde_json::json!({"remote":"accepted"}),
    };
    service
        .db
        .accept_remote(&authoritative, playlist.revision, &binding, &resolved)
        .unwrap();
    let accepted = service.playlist(playlist.id).unwrap();
    assert_eq!(accepted.chapters[0].id, authoritative.chapters[0].id);
    let mut unsafe_again = accepted.clone();
    unsafe_again.chapters.swap(0, 1);
    assert!(
        service
            .db
            .save_playlist_cas(&unsafe_again, accepted.revision)
            .is_err()
    );
    let private_image = service.store.put(&file, "png").unwrap();
    service
        .db
        .pin_draft_asset(&private_image, Some(&account))
        .unwrap();
    service
        .execute(Command::Disconnect(account.clone()))
        .await
        .unwrap();
    assert!(service.asset(private_image.id).is_err());
    let purged = service.playlist(playlist.id).unwrap();
    assert_eq!(purged.chapters.len(), 1);
    assert_eq!(purged.chapters[0].tracks.len(), 1);
    assert!(matches!(
        purged.chapters[0].tracks[0].media,
        TrackMedia::Local { .. }
    ));
    assert!(purged.chapters[0].tracks[0].events.is_none());
    assert!(purged.playback.shuffle.is_empty());
    assert_eq!(
        std::fs::read(service.store.materialize(&asset).unwrap().path()).unwrap(),
        b"preserved local original"
    );
    assert!(
        service
            .db
            .get::<Account>(Table::Account, account.as_str())
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn draft_images_survive_second_client_gc_failed_save_restart_and_explicit_abandonment() {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    let service = Service::isolated(
        profile.clone(),
        Tools::at(std::env::var_os("YOTO_TEST_TOOLS").unwrap().into()),
        [41; 32],
    )
    .unwrap();
    let ipc = crate::bridge::start(service.clone(), profile.clone(), true).unwrap();
    let image = returned_asset(
        service
            .execute(Command::SavePixelArt(Box::new(PixelArt::blank())))
            .await
            .unwrap(),
    );
    crate::bridge::call(
        &profile,
        true,
        crate::bridge::Request::Execute(Command::CollectGarbage),
    )
    .await
    .unwrap();
    service.execute(Command::ReadPixelArt(image)).await.unwrap();
    let mut playlist = streaming();
    service.db.save_playlist(&playlist).unwrap();
    playlist.chapters[0].icon = Some(IconReference::Local { asset: image });
    let mut invalid = playlist.clone();
    invalid.title = String::new();
    assert!(
        service
            .execute(Command::SavePlaylist {
                draft: Box::new(invalid),
                expected: playlist.revision
            })
            .await
            .is_err()
    );
    assert!(
        service
            .execute(Command::SavePlaylist {
                draft: Box::new(playlist.clone()),
                expected: Revision(999)
            })
            .await
            .is_err()
    );
    service.execute(Command::CollectGarbage).await.unwrap();
    service.execute(Command::ReadPixelArt(image)).await.unwrap();
    ipc.abort();
    let _ = ipc.await;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while Arc::strong_count(&service) > 1 {
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    drop(service);
    let service = Service::isolated(
        profile,
        Tools::at(std::env::var_os("YOTO_TEST_TOOLS").unwrap().into()),
        [41; 32],
    )
    .unwrap();
    service.execute(Command::ReadPixelArt(image)).await.unwrap();
    service
        .execute(Command::SavePlaylist {
            draft: Box::new(playlist.clone()),
            expected: playlist.revision,
        })
        .await
        .unwrap();
    service.execute(Command::ReleaseAsset(image)).await.unwrap();
    service.execute(Command::CollectGarbage).await.unwrap();
    service.execute(Command::ReadPixelArt(image)).await.unwrap();
    let abandoned = returned_asset(
        service
            .execute(Command::SavePixelArt(Box::new(PixelArt::blank())))
            .await
            .unwrap(),
    );
    let identical = returned_asset(
        service
            .execute(Command::SavePixelArt(Box::new(PixelArt::blank())))
            .await
            .unwrap(),
    );
    assert_ne!(abandoned, identical);
    assert_eq!(
        service.asset(abandoned).unwrap().sha256,
        service.asset(identical).unwrap().sha256
    );
    service
        .execute(Command::ReleaseAsset(abandoned))
        .await
        .unwrap();
    service.execute(Command::CollectGarbage).await.unwrap();
    service
        .execute(Command::ReadPixelArt(identical))
        .await
        .unwrap();
    service
        .execute(Command::ReleaseAsset(identical))
        .await
        .unwrap();
    service.execute(Command::CollectGarbage).await.unwrap();
    assert!(service.asset(identical).is_err());
    assert!(
        service
            .execute(Command::ReadPixelArt(abandoned))
            .await
            .is_err()
    );
    let mut current = service.playlist(playlist.id).unwrap();
    let expected = current.revision;
    current.metadata.cover = Some(abandoned);
    assert!(
        service
            .execute(Command::SavePlaylist {
                draft: Box::new(current),
                expected
            })
            .await
            .is_err()
    );
    let expired = returned_asset(
        service
            .execute(Command::SavePixelArt(Box::new(PixelArt::blank())))
            .await
            .unwrap(),
    );
    service
        .db
        .put(
            Table::Remote,
            &format!("draft-asset:{expired}"),
            &DraftAssetLease {
                asset: expired,
                account: None,
                expires: 0,
            },
        )
        .unwrap();
    service.execute(Command::CollectGarbage).await.unwrap();
    assert!(service.asset(expired).is_err());
}

#[tokio::test]
async fn confirmed_ambiguous_and_rejected_logout_effects_are_distinct_and_confirmed_reuses_card() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    let (service, account) = fixture(profile.clone(), server.uri());
    let playlist = streaming();
    service.db.save_playlist(&playlist).unwrap();
    let mut document =
        crate::api::write_playlist(&playlist, &account, &[], &BTreeMap::new(), None).unwrap();
    document["cardId"] = serde_json::json!("confirmed");
    Mock::given(method("POST"))
        .and(path("/content"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"card":{"cardId":"confirmed"}})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/content/confirmed"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"card":document})),
        )
        .mount(&server)
        .await;
    let confirmed = service
        .enqueue(JobRequest::Publish {
            account: account.clone(),
            snapshot: Box::new(playlist.clone()),
        })
        .unwrap();
    service.run_job(service.job(confirmed).unwrap()).await;
    assert!(matches!(
        service.job(confirmed).unwrap().state,
        JobState::Completed { .. }
    ));
    let mut other = streaming();
    other.title = "Other effect".into();
    let ambiguous = service
        .enqueue(JobRequest::Publish {
            account: account.clone(),
            snapshot: Box::new(other),
        })
        .unwrap();
    service
        .db
        .put(
            Table::Remote,
            &format!("{account}:commit:{ambiguous}"),
            &PendingPublication {
                job: ambiguous,
                account: account.clone(),
                document: serde_json::json!({"private":true}),
                outcome: PublicationOutcome::Possible,
            },
        )
        .unwrap();
    let rejected = service
        .enqueue(JobRequest::Publish {
            account: account.clone(),
            snapshot: Box::new(streaming()),
        })
        .unwrap();
    service
        .db
        .put(
            Table::Remote,
            &format!("{account}:commit:{rejected}"),
            &PendingPublication {
                job: rejected,
                account: account.clone(),
                document: serde_json::json!({"private":true}),
                outcome: PublicationOutcome::Rejected,
            },
        )
        .unwrap();
    service
        .execute(Command::Disconnect(account.clone()))
        .await
        .unwrap();
    assert!(matches!(
        service.job(confirmed).unwrap().state,
        JobState::Completed { .. }
    ));
    assert!(matches!(
        service.job(ambiguous).unwrap().state,
        JobState::Reconciling
    ));
    assert!(service.job(rejected).is_err());
    let receipt: PendingPublication = service
        .db
        .get(Table::Remote, &format!("{account}:commit:{confirmed}"))
        .unwrap()
        .unwrap();
    assert!(receipt.document.is_null());
    assert!(matches!(
        receipt.outcome,
        PublicationOutcome::Confirmed { .. }
    ));
    drop(service);
    let (service, account) = fixture(profile, server.uri());
    service
        .execute(Command::Publish {
            account: account.clone(),
            playlist: playlist.id,
        })
        .await
        .unwrap();
    let queued = service.db.list::<Job>(Table::Job).unwrap().pop().unwrap();
    service.run_job(queued).await;
    let posts: Vec<_> = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method == "POST")
        .collect();
    assert_eq!(posts.len(), 2);
    let second: serde_json::Value = serde_json::from_slice(&posts[1].body).unwrap();
    assert_eq!(second["cardId"], "confirmed");
}

#[cfg(unix)]
#[tokio::test]
async fn deep_unicode_and_restored_profile_paths_share_short_authenticated_endpoint() {
    let root = tempfile::tempdir().unwrap();
    let parent = root
        .path()
        .join("bibliothèque-音楽".repeat(5))
        .join("bibliothèque-音楽".repeat(5));
    let profile = parent.join("restaurée");
    let tools = Tools::at(std::env::var_os("YOTO_TEST_TOOLS").unwrap().into());
    let owner = crate::session::Session::connect(profile.clone(), tools.clone(), true)
        .await
        .unwrap();
    assert!(profile.as_os_str().as_encoded_bytes().len() > 104);
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&profile, &alias).unwrap();
    let proxy = crate::session::Session::connect(alias, tools.clone(), true)
        .await
        .unwrap();
    assert!(owner.is_owner());
    assert!(!proxy.is_owner());
    proxy
        .execute(Command::NewPlaylist("Deep catalogue".into()))
        .await
        .unwrap();
    assert_eq!(owner.snapshot().await.unwrap().playlists.len(), 1);
    let backup = root.path().join("backup");
    let recovery = hex::encode([13; 32]);
    owner
        .execute(Command::Backup {
            destination: backup.clone(),
            recovery_key: recovery.clone(),
        })
        .await
        .unwrap();
    let restored = parent.join("restauration-深い");
    crate::recovery::restore(&backup, &restored, &recovery, false).unwrap();
    // The synthetic backup has the isolated key, so no native credentials are involved.
    let new_owner = crate::session::Session::connect(restored.clone(), tools.clone(), true)
        .await
        .unwrap();
    let new_proxy = crate::session::Session::connect(restored, tools, true)
        .await
        .unwrap();
    assert!(!new_proxy.is_owner());
    assert_eq!(new_proxy.snapshot().await.unwrap().playlists.len(), 1);
    new_proxy.shutdown();
    new_owner.shutdown();
    proxy.shutdown();
    owner.shutdown();
}

fn opened_playlist(reply: Reply) -> PlaylistId {
    match reply {
        Reply::OpenedPlaylist(id) => id,
        Reply::AppUpdate(_)
        | Reply::YoutubeTool(_)
        | Reply::Updated
        | Reply::Saved(_)
        | Reply::PlaylistOperation { .. }
        | Reply::Entries(_)
        | Reply::Waveform(_)
        | Reply::Prepared(_)
        | Reply::Asset(_)
        | Reply::PixelArt(_)
        | Reply::Image(_)
        | Reply::Images(_)
        | Reply::Player(_)
        | Reply::Data(_) => panic!("expected an opened playlist"),
    }
}

#[tokio::test]
async fn new_login_activates_shared_account_and_selection_and_remote_cache_survive_restart() {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    let tools = Tools::at(std::env::var_os("YOTO_TEST_TOOLS").unwrap().into());
    let service = Service::isolated(profile.clone(), tools.clone(), [7; 32]).unwrap();
    let previous = service.interface_changes(&[]).unwrap().revisions;
    assert!(service.snapshot().unwrap().active_account.is_none());
    let a = AccountId::new("account-a").unwrap();
    let b = AccountId::new("account-b").unwrap();
    for id in [&a, &b] {
        service
            .connected_account(Account {
                id: id.clone(),
                label: id.to_string(),
                scopes: vec![Scope::UserContentView],
            })
            .await
            .unwrap();
    }
    let mut shared = Snapshot::default();
    service
        .interface_changes(&previous)
        .unwrap()
        .apply(&mut shared);
    assert_eq!(shared.active_account, Some(b.clone()));
    assert_eq!(shared.accounts.len(), 2);
    let card = RemoteCard {
        card: CardId::new("card-a").unwrap(),
        title: "My remote playlist".into(),
        document: serde_json::json!({}),
        synced: now(),
    };
    service.db.replace_remote_cards(&a, &[card]).unwrap();
    service
        .execute(Command::SelectAccount(a.clone()))
        .await
        .unwrap();
    drop(service);
    let restored = Service::isolated(profile, tools, [7; 32]).unwrap();
    assert_eq!(restored.snapshot().unwrap().active_account, Some(a.clone()));
    assert_eq!(
        restored.snapshot().unwrap().remote[0].card.as_str(),
        "card-a"
    );
    restored.execute(Command::SelectAccount(b)).await.unwrap();
    assert!(restored.snapshot().unwrap().remote.is_empty());
    restored.execute(Command::SelectAccount(a)).await.unwrap();
    assert_eq!(restored.snapshot().unwrap().remote.len(), 1);
}

#[tokio::test]
async fn partial_sync_keeps_myo_cards_when_other_scopes_are_unavailable_and_projects_errors() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = fixture(root.path().join("profile"), server.uri());
    service
        .execute(Command::SelectAccount(account.clone()))
        .await
        .unwrap();
    let previous = service.interface_changes(&[]).unwrap().revisions;
    Mock::given(method("GET")).and(path("/content/mine"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"cards":[{"cardId":"myo-existing","title":"Existing online playlist"}]})))
        .expect(1).mount(&server).await;
    assert!(matches!(
        service.execute(Command::Sync(account.clone())).await,
        Err(Failure::Permission(_))
    ));
    let snapshot = service.snapshot().unwrap();
    assert_eq!(snapshot.remote.len(), 1);
    assert_eq!(snapshot.remote[0].title, "Existing online playlist");
    assert!(matches!(
        snapshot.remote_sync[&RemoteCollection::Playlists],
        RemoteSyncState::Ready { .. }
    ));
    for collection in [
        RemoteCollection::Devices,
        RemoteCollection::Groups,
        RemoteCollection::Icons,
    ] {
        assert!(matches!(
            snapshot.remote_sync[&collection],
            RemoteSyncState::Failed {
                failure: Failure::Permission(_)
            }
        ));
    }
    let mut shared = Snapshot::default();
    service
        .interface_changes(&previous)
        .unwrap()
        .apply(&mut shared);
    assert_eq!(shared.remote[0].card.as_str(), "myo-existing");
    assert_eq!(shared.remote_sync.len(), 4);
    assert_eq!(service.db.remote_cards(&account).unwrap().len(), 1);
    assert!(
        snapshot.playlists.is_empty(),
        "synchronization must not create or overwrite editable drafts"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn switching_account_or_logout_during_sync_ignores_old_results_and_clears_progress() {
    for logout in [false, true] {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let (service, account) = fixture(root.path().join("profile"), server.uri());
        service
            .execute(Command::SelectAccount(account.clone()))
            .await
            .unwrap();
        let other = AccountId::new("other-account").unwrap();
        service
            .db
            .put(
                Table::Account,
                other.as_str(),
                &Account {
                    id: other.clone(),
                    label: "Other".into(),
                    scopes: vec![],
                },
            )
            .unwrap();
        Mock::given(method("GET")).and(path("/content/mine"))
            .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(200)).set_body_json(serde_json::json!({"cards":[{"cardId":"private-card","title":"Private playlist"}]})))
            .expect(1).mount(&server).await;
        let worker = service.clone();
        let id = account.clone();
        let sync = tokio::spawn(async move { worker.execute(Command::Sync(id)).await });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while server.received_requests().await.unwrap().is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            service.snapshot().unwrap().remote_sync[&RemoteCollection::Playlists],
            RemoteSyncState::Syncing
        ));
        if logout {
            service
                .execute(Command::Disconnect(account.clone()))
                .await
                .unwrap();
        } else {
            service
                .execute(Command::SelectAccount(other.clone()))
                .await
                .unwrap();
        }
        assert!(matches!(sync.await.unwrap(), Err(Failure::Cancelled)));
        assert!(service.snapshot().unwrap().remote.is_empty());
        assert!(service.db.remote_cards(&account).unwrap().is_empty());
        if !logout {
            assert_eq!(service.snapshot().unwrap().active_account, Some(other));
            service
                .execute(Command::SelectAccount(account))
                .await
                .unwrap();
            assert!(matches!(
                service.snapshot().unwrap().remote_sync[&RemoteCollection::Playlists],
                RemoteSyncState::Failed {
                    failure: Failure::Cancelled
                }
            ));
        } else {
            assert!(service.snapshot().unwrap().active_account.is_none());
            assert!(service.snapshot().unwrap().remote_sync.is_empty());
        }
    }
}

#[tokio::test]
async fn remote_open_refreshes_authoritative_state_and_archives_legacy_edits() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = fixture(root.path().join("profile"), server.uri());
    let card = CardId::new("remote-card").unwrap();
    let mut doc =
        crate::api::write_playlist(&streaming(), &account, &[], &Default::default(), None).unwrap();
    doc["cardId"] = serde_json::json!(card);
    Mock::given(method("GET"))
        .and(path("/content/remote-card"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"card":doc})))
        .expect(2)
        .mount(&server)
        .await;
    let id = opened_playlist(
        service
            .execute(Command::LoadRemote {
                account: account.clone(),
                card: card.clone(),
            })
            .await
            .unwrap(),
    );
    let mut draft: Playlist = service
        .db
        .get(Table::Playlist, &id.to_string())
        .unwrap()
        .unwrap();
    let image = returned_asset(
        service
            .execute(Command::SavePixelArt(Box::new(PixelArt::blank())))
            .await
            .unwrap(),
    );
    draft.chapters[0].icon = Some(IconReference::Local { asset: image });
    draft.title = "My local title must survive".into();
    draft.metadata.description = "My local changes".into();
    let revision = service
        .db
        .save_playlist_cas(&draft, draft.revision)
        .unwrap();
    let reopened = opened_playlist(
        service
            .execute(Command::LoadRemote {
                account: account.clone(),
                card: card.clone(),
            })
            .await
            .unwrap(),
    );
    assert_eq!(reopened, id);
    let preserved: Playlist = service
        .db
        .get(Table::Playlist, &id.to_string())
        .unwrap()
        .unwrap();
    assert_ne!(preserved.title, "My local title must survive");
    let archived: Playlist = service
        .db
        .get(
            Table::Remote,
            &format!("{account}:legacy-playlist:{id}:{}", revision.0),
        )
        .unwrap()
        .unwrap();
    assert_eq!(archived.title, "My local title must survive");
    assert_eq!(archived.metadata.description, "My local changes");
    assert_eq!(preserved.revision, revision.next().unwrap());
    assert_eq!(service.snapshot().unwrap().playlists.len(), 1);
    assert_eq!(service.snapshot().unwrap().bindings.len(), 1);
    // The transaction also guards two concurrent fresh imports of the same remote card.
    let other = crate::api::read_playlist(&doc, &account).unwrap();
    let duplicate_binding = PublicationBinding {
        account: account.clone(),
        playlist: other.id,
        card: card.clone(),
        revision: other.revision,
        document: doc,
    };
    assert_eq!(
        service
            .db
            .import_remote(&other, &duplicate_binding)
            .unwrap(),
        id
    );
    assert_eq!(service.snapshot().unwrap().playlists.len(), 1);
    assert!(
        service.db.asset_roots().unwrap().contains(&image),
        "archived draft retains its image after canonical refresh"
    );
    service.db.evict_cloud_playlist(&account, &card).unwrap();
    assert!(
        service.db.asset_roots().unwrap().contains(&image),
        "archive is independent of the visible cache"
    );
    service.db.purge_account(&account).unwrap();
    assert!(
        !service.db.asset_roots().unwrap().contains(&image),
        "account purge removes private archive roots"
    );
    assert!(
        service
            .db
            .get::<Playlist>(
                Table::Remote,
                &format!("{account}:legacy-playlist:{id}:{}", revision.0)
            )
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn newer_sync_retains_progress_when_an_older_response_finishes_and_full_sync_clears_removed_cards()
 {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (mut service, account) = fixture(root.path().join("profile"), server.uri());
    Arc::get_mut(&mut service).unwrap().auth = TokenBroker::test_broker(
        server.uri(),
        &account,
        &crate::auth::Grant {
            access: "test-token".into(),
            refresh: None,
            scopes: Scope::ALL.to_vec(),
            unknown_scopes: vec![],
            expires: now() + 3600,
        },
    );
    service
        .execute(Command::SelectAccount(account.clone()))
        .await
        .unwrap();
    for (url, body) in [
        ("/device-v2/devices/mine", serde_json::json!({"devices":[]})),
        ("/card/family/library/groups", serde_json::json!([])),
        (
            "/media/displayIcons/user/yoto",
            serde_json::json!({"displayIcons":[]}),
        ),
        (
            "/media/displayIcons/user/me",
            serde_json::json!({"displayIcons":[]}),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(url))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
    }
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let responses = counter.clone();
    Mock::given(method("GET")).and(path("/content/mine"))
        .respond_with(move |_: &wiremock::Request| {
            let number = responses.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(if number == 0 { 200 } else { 500 }))
                .set_body_json(serde_json::json!({"cards":[{"cardId": if number == 0 { "old-card" } else { "latest-card" },"title":"Remote playlist"}]}))
        }).expect(2).mount(&server).await;
    let first_service = service.clone();
    let first_account = account.clone();
    let first =
        tokio::spawn(async move { first_service.execute(Command::Sync(first_account)).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while counter.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let second_service = service.clone();
    let second_account = account.clone();
    let second =
        tokio::spawn(async move { second_service.execute(Command::Sync(second_account)).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while counter.load(std::sync::atomic::Ordering::SeqCst) < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(first.await.unwrap(), Err(Failure::Cancelled)));
    assert!(matches!(
        service.snapshot().unwrap().remote_sync[&RemoteCollection::Playlists],
        RemoteSyncState::Syncing
    ));
    second.await.unwrap().unwrap();
    assert_eq!(
        service.snapshot().unwrap().remote[0].card.as_str(),
        "latest-card"
    );
    assert!(
        service
            .snapshot()
            .unwrap()
            .remote_sync
            .values()
            .all(|s| matches!(s, RemoteSyncState::Ready { .. }))
    );
    assert_eq!(
        service.db.remote_cards(&account).unwrap()[0].card.as_str(),
        "latest-card"
    );
    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/content/mine"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"cards":[]})))
        .mount(&server)
        .await;
    // Missing unrelated endpoints still cannot resurrect cards removed from the authoritative MYO list.
    assert!(
        service
            .execute(Command::Sync(account.clone()))
            .await
            .is_err()
    );
    assert!(service.snapshot().unwrap().remote.is_empty());
    assert!(service.db.remote_cards(&account).unwrap().is_empty());
}
