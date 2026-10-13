use super::tests::fixture;
use super::*;
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::*};

fn remote_document() -> serde_json::Value {
    serde_json::json!({
        "cardId":"songs-card", "title":"Chansons", "futureTop":{"keep":true},
        "metadata":{"category":"music","description":"Collection existante"},
        "content":{"playbackType":"linear","config":{"autoadvance":"next","resumeTimeout":2592000},
            "chapters":[
                {"key":"1","title":"Chanson une","display":{"icon16x16":"yoto:#icon1"},"futureChapter":42,"tracks":[
                    {"key":"1","title":"Première chanson","type":"audio","trackUrl":"yoto:#remote-audio-one","format":"mp3","duration":3.0,"fileSize":123,"channels":"stereo","futureTrack":{"retain":true}}]},
                {"key":"2","title":"Chanson deux","tracks":[
                    {"key":"1","title":"Deuxième chanson","type":"audio","trackUrl":"yoto:#remote-audio-two","format":"mp3","duration":4.0,"fileSize":456,"channels":"stereo"}]}
            ]}
    })
}

#[tokio::test]
async fn synced_myo_opens_edits_adds_trimmed_audio_and_updates_the_same_cloud_card() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("catalogue");
    let (service, account) = fixture(profile.clone(), server.uri());
    service
        .execute(Command::SelectAccount(account.clone()))
        .await
        .unwrap();
    let document = Arc::new(Mutex::new(remote_document()));
    let cards = document.clone();
    Mock::given(method("GET")).and(path("/content/mine"))
        .respond_with(move|_:&Request|ResponseTemplate::new(200).set_body_json(serde_json::json!({"cards":[{"cardId":"songs-card","title":cards.lock().unwrap()["title"]}]})))
        .expect(2).mount(&server).await;
    let detail = document.clone();
    Mock::given(method("GET"))
        .and(path("/content/songs-card"))
        .respond_with(move |_: &Request| {
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"card":detail.lock().unwrap().clone()}))
        })
        .expect(8)
        .mount(&server)
        .await;
    // Unrelated missing scopes are a partial sync error. MYO still reaches Ready.
    assert!(
        service
            .execute(Command::Sync(account.clone()))
            .await
            .is_err()
    );
    let snapshot = service.snapshot().unwrap();
    assert_eq!(snapshot.remote.len(), 1);
    assert!(matches!(
        snapshot.remote_sync.get(&RemoteCollection::Playlists),
        Some(RemoteSyncState::Ready { .. })
    ));
    let opened = service
        .execute(Command::LoadRemote {
            account: account.clone(),
            card: CardId::new("songs-card").unwrap(),
        })
        .await
        .unwrap();
    let Reply::OpenedPlaylist(playlist) = opened else {
        panic!("opening must select the editable draft");
    };
    let original = service.playlist(playlist).unwrap();
    assert_eq!(original.chapters[0].key.as_str(), "1");
    assert_eq!(original.chapters[0].tracks[0].key.as_str(), "1");
    assert_eq!(original.chapters[1].tracks[0].key.as_str(), "1");
    assert_ne!(
        original.chapters[0].tracks[0].id,
        original.chapters[1].tracks[0].id
    );

    // Generate only synthetic audio and run the same durable import used by UI/MCP.
    let input = root.path().join("Nouvelle chanson.wav");
    let mut generate = crate::media::command(&service.tools.ffmpeg);
    generate
        .args([
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-ac",
            "2",
            "-y",
        ])
        .arg(&input);
    assert!(
        crate::process::output(generate, 100_000, std::time::Duration::from_secs(30))
            .unwrap()
            .status
            .success()
    );
    service
        .execute(Command::Import(vec![ImportSelection::LocalFile {
            path: input,
        }]))
        .await
        .unwrap();
    let import = service.db.jobs_page(0, 1).unwrap().remove(0);
    service.run_job(import.clone()).await;
    let JobState::Completed {
        output: JobOutput::Imported { media },
    } = service.job(import.id).unwrap().state
    else {
        panic!("synthetic audio import failed");
    };
    let item = service.media(media).unwrap();
    let Reply::Prepared(recipe) = service
        .execute(Command::Prepare {
            media,
            trim: TrimRange::new(
                MediaTime::from_seconds(1.0).unwrap(),
                MediaTime::from_seconds(2.0).unwrap(),
                item.info.duration,
            )
            .unwrap(),
            normalization: Normalization::Preserve,
        })
        .await
        .unwrap()
    else {
        panic!("prepared recipe missing");
    };
    // Prepare queues local encoding; complete it before adding the selected version.
    let encode = service.db.jobs_page(0, 1).unwrap().remove(0);
    service.run_job(encode).await;
    assert!(service.recipe(recipe).unwrap().result.is_some());
    Mock::given(method("GET")).and(path("/media/transcode/audio/uploadUrl"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"upload":{"uploadId":"fixture-upload","uploadUrl":format!("{}/signed-storage",server.uri())}})))
        .expect(1).mount(&server).await;
    Mock::given(method("PUT"))
        .and(path("/signed-storage"))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(150)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/media/upload/fixture-upload/transcoded"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"transcode":{"transcodedSha256":"new-uploaded-audio","transcodedInfo":{"duration":1.0,"fileSize":1024,"channels":"stereo","format":"mp3"}}})))
        .expect(1).mount(&server).await;
    let cloud = document.clone();
    Mock::given(method("POST"))
        .and(path("/content"))
        .respond_with(move |request: &Request| {
            // Validate the real generated DTO and preserve both remote key paths.
            let typed: yoto_api_generated::models::Card =
                serde_json::from_slice(&request.body).unwrap();
            assert_eq!(typed.card_id.as_deref(), Some("songs-card"));
            assert_eq!(typed.content.chapters[0].key, "1");
            assert_eq!(typed.content.chapters[0].tracks[0].key, "1");
            assert_eq!(typed.content.chapters[1].tracks[0].key, "1");
            if typed.title == "Chansons du soir" {
                assert_eq!(
                    typed.content.chapters[0].tracks[0].title,
                    "Chanson renommée"
                );
            } else {
                assert_eq!(typed.title, "Chansons");
            }
            assert_eq!(
                typed.content.chapters[2].tracks[0].track_url,
                "yoto:#new-uploaded-audio"
            );

            *cloud.lock().unwrap() = serde_json::to_value(typed).unwrap();
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"card":{"cardId":"songs-card"}}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let Reply::PlaylistOperation {
        playlist: added_playlist,
        job: add_job,
    } = service
        .execute(Command::AddYotoMedia {
            account: account.clone(),
            card: CardId::new("songs-card").unwrap(),
            media: vec![(media, recipe)],
        })
        .await
        .unwrap()
    else {
        panic!("cloud add must return typed operation");
    };
    assert_eq!(added_playlist, playlist);
    assert_eq!(
        service.playlist(playlist).unwrap().chapters.len(),
        2,
        "queued media must not appear before confirmation"
    );
    assert!(
        service
            .execute(Command::DeleteRemote {
                account: account.clone(),
                card: CardId::new("songs-card").unwrap()
            })
            .await
            .is_err()
    );
    let worker = service.clone();
    let upload = tokio::spawn(async move { worker.run_job(worker.job(add_job).unwrap()).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.method == "PUT")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        service.job(add_job).unwrap().state,
        JobState::Running {
            phase: JobPhase::Uploading { .. }
        }
    ));
    assert_eq!(
        service.playlist(playlist).unwrap().chapters.len(),
        2,
        "audio in flight remains hidden"
    );
    upload.await.unwrap();
    assert!(matches!(
        service.job(add_job).unwrap().state,
        JobState::Completed { .. }
    ));
    let mut draft = service.playlist(playlist).unwrap();
    assert_eq!(draft.chapters.len(), 3);
    assert!(
        matches!(draft.chapters[2].tracks[0].media, TrackMedia::Local { recipe:r, .. } if r==recipe)
    );
    let expected = draft.revision;
    draft.title = "Chansons du soir".into();
    draft.chapters[0].tracks[0].title = "Chanson renommée".into();
    draft.metadata.author = "Famille".into();
    let Reply::PlaylistOperation {
        playlist: updated,
        job: publish_id,
    } = service
        .execute(Command::UpdateYotoPlaylist {
            account: account.clone(),
            draft: Box::new(draft),
            expected,
        })
        .await
        .unwrap()
    else {
        panic!("cloud edit missing operation");
    };
    assert_eq!(updated, playlist);
    assert_eq!(service.playlist(playlist).unwrap().title, "Chansons");
    let publish = service.job(publish_id).unwrap();
    service.run_job(publish.clone()).await;
    let revision = expected.next().unwrap();
    assert!(
        matches!(service.job(publish.id).unwrap().state,JobState::Completed{output:JobOutput::Published{card,revision:r}} if card.as_str()=="songs-card"&&r==revision)
    );
    let snapshot = service.snapshot().unwrap();
    assert_eq!(snapshot.bindings.len(), 1);
    assert_eq!(snapshot.bindings[0].revision, revision);
    assert_eq!(snapshot.mappings.len(), 1);
    assert_eq!(snapshot.mappings[0].recipe, recipe);
    assert_eq!(
        document.lock().unwrap()["futureTop"],
        serde_json::json!({"keep":true})
    );
    assert_eq!(
        document.lock().unwrap()["content"]["chapters"][0]["futureChapter"],
        42
    );
    assert_eq!(
        document.lock().unwrap()["content"]["chapters"][0]["tracks"][0]["futureTrack"],
        serde_json::json!({"retain":true})
    );
    let requests = server.received_requests().await.unwrap();
    assert!(
        !requests
            .iter()
            .find(|r| r.method == "PUT")
            .unwrap()
            .headers
            .contains_key("authorization")
    );
    assert!(
        service
            .execute(Command::Sync(account.clone()))
            .await
            .is_err()
    );
    assert_eq!(
        service.snapshot().unwrap().remote[0].title,
        "Chansons du soir"
    );
    drop(service);
    let (reopened, _) = fixture(profile, server.uri());
    assert_eq!(reopened.playlist(playlist).unwrap().revision, revision);
    assert_eq!(
        reopened.snapshot().unwrap().bindings[0].card.as_str(),
        "songs-card"
    );
    assert!(
        matches!(reopened.execute(Command::LoadRemote{account,card:CardId::new("songs-card").unwrap()}).await.unwrap(),Reply::OpenedPlaylist(id) if id==playlist)
    );
}

#[tokio::test]
async fn an_empty_remote_myo_remains_editable_when_reloaded() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = fixture(root.path().join("catalogue"), server.uri());
    Mock::given(method("GET")).and(path("/content/empty-card"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"card":{"cardId":"empty-card","title":"À compléter","content":{"chapters":[]}}})))
        .expect(2).mount(&server).await;
    let Reply::OpenedPlaylist(id) = service
        .execute(Command::LoadRemote {
            account: account.clone(),
            card: CardId::new("empty-card").unwrap(),
        })
        .await
        .unwrap()
    else {
        panic!("draft not opened");
    };
    assert!(service.playlist(id).unwrap().chapters.is_empty());
    assert!(matches!(
        service
            .execute(Command::Publish {
                account: account.clone(),
                playlist: id
            })
            .await,
        Err(Failure::Validation(_))
    ));
    assert!(
        matches!(service.execute(Command::LoadRemote{account,card:CardId::new("empty-card").unwrap()}).await.unwrap(),Reply::OpenedPlaylist(same) if same==id)
    );
}
