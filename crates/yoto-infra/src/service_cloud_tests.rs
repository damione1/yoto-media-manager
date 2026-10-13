use super::{
    tests::{fixture, streaming},
    *,
};
use std::sync::atomic::{AtomicBool, Ordering};
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::*};

fn operation(reply: Reply) -> (PlaylistId, JobId) {
    let Reply::PlaylistOperation { playlist, job } = reply else {
        panic!("expected typed remote operation");
    };
    (playlist, job)
}

#[tokio::test]
async fn cloud_create_empty_edit_duplicate_refresh_external_changes_and_delete_are_authoritative() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = fixture(root.path().join("profile"), server.uri());
    service
        .execute(Command::SelectAccount(account.clone()))
        .await
        .unwrap();
    let documents = Arc::new(Mutex::new(BTreeMap::<String, serde_json::Value>::new()));
    let hide_index = Arc::new(AtomicBool::new(false));
    let hiding = hide_index.clone();
    let cards = documents.clone();
    Mock::given(method("GET"))
        .and(path("/content/mine"))
        .respond_with(move |_: &Request| {
            let cards: Vec<_> = cards
                .lock()
                .unwrap()
                .iter()
                .filter(|_| !hiding.load(Ordering::SeqCst))
                .map(|(card, doc)| serde_json::json!({"cardId":card,"title":doc["title"]}))
                .collect();
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"cards":cards}))
        })
        .mount(&server)
        .await;
    let cards = documents.clone();
    Mock::given(method("GET"))
        .and(path_regex("^/content/cloud-[0-9]+$"))
        .respond_with(move |request: &Request| {
            let key = request.url.path().rsplit('/').next().unwrap();
            match cards.lock().unwrap().get(key) {
                Some(card) => {
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"card":card}))
                }
                None => ResponseTemplate::new(404),
            }
        })
        .mount(&server)
        .await;
    let cards = documents.clone();
    Mock::given(method("POST"))
        .and(path("/content"))
        .respond_with(move |request: &Request| {
            let mut card: yoto_api_generated::models::Card =
                serde_json::from_slice(&request.body).unwrap();
            let mut cards = cards.lock().unwrap();
            let key = card
                .card_id
                .clone()
                .unwrap_or_else(|| format!("cloud-{}", cards.len() + 1));
            card.card_id = Some(key.clone());
            cards.insert(key.clone(), serde_json::to_value(card).unwrap());
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"card":{"cardId":key}}))
        })
        .expect(3)
        .mount(&server)
        .await;
    let cards = documents.clone();
    Mock::given(method("DELETE"))
        .and(path("/content/cloud-2"))
        .respond_with(move |_: &Request| {
            cards.lock().unwrap().remove("cloud-2");
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"success":true}))
        })
        .expect(1)
        .mount(&server)
        .await;

    service
        .execute(Command::RefreshPlaylists(account.clone()))
        .await
        .unwrap();
    assert!(service.snapshot().unwrap().remote.is_empty());
    let (playlist, create) = operation(
        service
            .execute(Command::CreateYotoPlaylist {
                account: account.clone(),
                title: "Chansons".into(),
            })
            .await
            .unwrap(),
    );
    assert!(
        service.snapshot().unwrap().playlists.is_empty(),
        "a pending create is only a durable journal"
    );
    assert!(service.snapshot().unwrap().remote.is_empty());
    service.run_job(service.job(create).unwrap()).await;
    assert!(matches!(
        service.job(create).unwrap().state,
        JobState::Completed { .. }
    ));
    let snapshot = service.snapshot().unwrap();
    assert_eq!(snapshot.remote.len(), 1);
    assert_eq!(snapshot.bindings[0].card.as_str(), "cloud-1");
    let mut canonical = service.playlist(playlist).unwrap();
    assert!(canonical.chapters.is_empty());
    let expected = canonical.revision;
    canonical.chapters = streaming().chapters;
    canonical.title = "Chansons du soir".into();
    let (_, edit) = operation(
        service
            .execute(Command::UpdateYotoPlaylist {
                account: account.clone(),
                draft: Box::new(canonical),
                expected,
            })
            .await
            .unwrap(),
    );
    assert!(service.playlist(playlist).unwrap().chapters.is_empty());
    service.run_job(service.job(edit).unwrap()).await;
    assert!(matches!(
        service.job(edit).unwrap().state,
        JobState::Completed { .. }
    ));
    let chapter = service.playlist(playlist).unwrap().chapters[0].id;
    let track = service.playlist(playlist).unwrap().chapters[0].tracks[0].id;
    {
        let mut cards = documents.lock().unwrap();
        let card = cards.get_mut("cloud-1").unwrap();
        card["title"] = "Modifié dans Yoto".into();
        card["content"]["chapters"][0]["tracks"][0]["title"] = "Titre externe".into();
    }
    service
        .execute(Command::RefreshPlaylists(account.clone()))
        .await
        .unwrap();
    let fresh = service.playlist(playlist).unwrap();
    assert_eq!(fresh.title, "Modifié dans Yoto");
    assert_eq!(fresh.chapters[0].tracks[0].title, "Titre externe");
    assert_eq!(fresh.chapters[0].id, chapter);
    assert_eq!(fresh.chapters[0].tracks[0].id, track);
    let card = CardId::new("cloud-1").unwrap();
    let (duplicate, copy) = operation(
        service
            .execute(Command::DuplicateYotoPlaylist {
                account: account.clone(),
                card,
            })
            .await
            .unwrap(),
    );
    assert_ne!(duplicate, playlist);
    assert_eq!(service.snapshot().unwrap().playlists.len(), 1);
    service.run_job(service.job(copy).unwrap()).await;
    assert!(matches!(
        service.job(copy).unwrap().state,
        JobState::Completed { .. }
    ));
    assert_eq!(service.snapshot().unwrap().remote.len(), 2);
    service
        .execute(Command::DeleteRemote {
            account: account.clone(),
            card: CardId::new("cloud-2").unwrap(),
        })
        .await
        .unwrap();
    assert!(service.playlist(duplicate).is_err());
    assert_eq!(service.snapshot().unwrap().remote.len(), 1);
    hide_index.store(true, Ordering::SeqCst);
    service
        .execute(Command::RefreshPlaylists(account.clone()))
        .await
        .unwrap();
    assert_eq!(
        service.snapshot().unwrap().remote.len(),
        1,
        "a temporarily missing MYO index must not hide a GET-confirmed card"
    );
    assert!(service.playlist(playlist).is_ok());
    documents.lock().unwrap().remove("cloud-1");
    service
        .execute(Command::RefreshPlaylists(account))
        .await
        .unwrap();
    let final_snapshot = service.snapshot().unwrap();
    assert!(final_snapshot.remote.is_empty());
    assert!(final_snapshot.playlists.is_empty());
    assert!(final_snapshot.bindings.is_empty());
}

#[tokio::test]
async fn uncertain_cloud_edit_keeps_confirmed_state_and_reconciles_without_a_second_post() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = fixture(root.path().join("profile"), server.uri());
    service
        .execute(Command::SelectAccount(account.clone()))
        .await
        .unwrap();
    let card = CardId::new("cloud-1").unwrap();
    let mut doc =
        crate::api::write_playlist(&streaming(), &account, &[], &Default::default(), None).unwrap();
    doc["cardId"] = serde_json::json!(card);
    let doc = Arc::new(Mutex::new(doc));
    let unavailable = Arc::new(AtomicBool::new(false));
    let state = doc.clone();
    let failing = unavailable.clone();
    Mock::given(method("GET"))
        .and(path("/content/cloud-1"))
        .respond_with(move |_: &Request| {
            if failing.load(Ordering::SeqCst) {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"card":state.lock().unwrap().clone()}))
            }
        })
        .mount(&server)
        .await;
    let state = doc.clone();
    Mock::given(method("GET")).and(path("/content/mine"))
        .respond_with(move |_: &Request| ResponseTemplate::new(200).set_body_json(serde_json::json!({"cards":[{"cardId":"cloud-1","title":state.lock().unwrap()["title"]}]})))
        .mount(&server).await;
    let state = doc.clone();
    let failing = unavailable.clone();
    Mock::given(method("POST"))
        .and(path("/content"))
        .respond_with(move |request: &Request| {
            *state.lock().unwrap() = serde_json::from_slice(&request.body).unwrap();
            failing.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"card":{"cardId":"cloud-1"}}))
        })
        .expect(1)
        .mount(&server)
        .await;
    let Reply::OpenedPlaylist(id) = service
        .execute(Command::LoadRemote {
            account: account.clone(),
            card: card.clone(),
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    let mut proposed = service.playlist(id).unwrap();
    let expected = proposed.revision;
    proposed.title = "Demandé mais pas encore confirmé".into();
    let (_, job) = operation(
        service
            .execute(Command::UpdateYotoPlaylist {
                account: account.clone(),
                draft: Box::new(proposed.clone()),
                expected,
            })
            .await
            .unwrap(),
    );
    service.run_job(service.job(job).unwrap()).await;
    assert!(matches!(
        service.job(job).unwrap().state,
        JobState::Reconciling
    ));
    assert_eq!(service.playlist(id).unwrap().title, "Publication test");
    assert!(
        service
            .execute(Command::UpdateYotoPlaylist {
                account: account.clone(),
                draft: Box::new(proposed),
                expected
            })
            .await
            .is_err()
    );
    service
        .execute(Command::RefreshPlaylists(account.clone()))
        .await
        .unwrap();
    assert_eq!(
        service.playlist(id).unwrap().title,
        "Publication test",
        "refresh cannot expose an unverified write"
    );
    unavailable.store(false, Ordering::SeqCst);
    service.execute(Command::Reconcile(job)).await.unwrap();
    assert_eq!(
        service.playlist(id).unwrap().title,
        "Demandé mais pas encore confirmé"
    );
    assert_eq!(service.snapshot().unwrap().bindings[0].card, card);
    assert!(matches!(
        service.job(job).unwrap().state,
        JobState::Completed { .. }
    ));
}

#[tokio::test]
async fn delete_and_remote_write_admission_cannot_cross_the_http_await() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = fixture(root.path().join("profile"), server.uri());
    let card = CardId::new("cloud-1").unwrap();
    let mut document =
        crate::api::write_playlist(&streaming(), &account, &[], &Default::default(), None).unwrap();
    document["cardId"] = serde_json::json!(card);
    Mock::given(method("GET"))
        .and(path("/content/cloud-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"card":document})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/content/cloud-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"success":true}))
                .set_delay(std::time::Duration::from_millis(150)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let Reply::OpenedPlaylist(id) = service
        .execute(Command::LoadRemote {
            account: account.clone(),
            card: card.clone(),
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    let mut proposed = service.playlist(id).unwrap();
    let expected = proposed.revision;
    proposed.title = "Concurrent edit".into();
    let client = service.clone();
    let owner = account.clone();
    let deletion = tokio::spawn(async move {
        client
            .execute(Command::DeleteRemote {
                account: owner,
                card,
            })
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.method == "DELETE")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let client = service.clone();
    let edit = tokio::spawn(async move {
        client
            .execute(Command::UpdateYotoPlaylist {
                account,
                draft: Box::new(proposed),
                expected,
            })
            .await
    });
    deletion.await.unwrap().unwrap();
    assert!(edit.await.unwrap().is_err());
    let snapshot = service.snapshot().unwrap();
    assert!(snapshot.bindings.is_empty());
    assert!(snapshot.playlists.is_empty());
    assert!(
        snapshot.jobs.is_empty(),
        "a stale concurrent mutation must not enter the durable queue"
    );
    assert!(
        !server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|request| request.method == "POST")
    );
}
