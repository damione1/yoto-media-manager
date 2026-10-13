use super::{tests::fixture, *};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::*};

fn icon_fixture(profile: PathBuf, host: String) -> (Arc<Service>, AccountId) {
    let (mut service, account) = fixture(profile, host.clone());
    Arc::get_mut(&mut service).unwrap().auth = TokenBroker::test_broker(
        host,
        &account,
        &crate::auth::Grant {
            access: "icons-only-token".into(),
            refresh: None,
            scopes: vec![Scope::UserIconsManage],
            unknown_scopes: vec![],
            expires: now() + 3600,
        },
    );
    (service, account)
}

#[tokio::test]
async fn icon_refresh_uses_only_icon_scope_and_preserves_catalogues_on_retryable_failure() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = icon_fixture(root.path().join("profile"), server.uri());
    service
        .execute(Command::SelectAccount(account.clone()))
        .await
        .unwrap();
    Mock::given(method("GET")).and(path("/media/displayIcons/user/yoto"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"displayIcons":[{"mediaId":"same-id","title":"Mes icônes","url":"https://example.com/public.png"}]})))
        .expect(3).mount(&server).await;
    let fail = Arc::new(AtomicBool::new(false));
    let failing = fail.clone();
    Mock::given(method("GET")).and(path("/media/displayIcons/user/me"))
        .respond_with(move |_: &Request| if failing.load(Ordering::SeqCst) {ResponseTemplate::new(503)} else {ResponseTemplate::new(200).set_body_json(serde_json::json!({"displayIcons":[{"mediaId":"same-id","title":"Yoto","url":"https://example.com/own.png"}]}))})
        .expect(3).mount(&server).await;
    service
        .execute(Command::RefreshIcons(account.clone()))
        .await
        .unwrap();
    let snapshot = service.snapshot().unwrap();
    assert_eq!(snapshot.icons.len(), 2);
    assert_eq!(
        snapshot.icons[0].collection,
        crate::api::IconCollection::Public
    );
    assert_eq!(
        snapshot.icons[1].collection,
        crate::api::IconCollection::User
    );
    assert_eq!(
        snapshot.icons[0].media, snapshot.icons[1].media,
        "origin is not inferred from IDs/titles"
    );
    assert!(matches!(
        snapshot.remote_sync.get(&RemoteCollection::Icons),
        Some(RemoteSyncState::Ready { .. })
    ));
    assert_eq!(
        snapshot.remote_sync.len(),
        1,
        "narrow refresh must not require or touch playlists/devices/groups"
    );
    fail.store(true, Ordering::SeqCst);
    assert!(
        service
            .execute(Command::RefreshIcons(account.clone()))
            .await
            .is_err()
    );
    let failed = service.snapshot().unwrap();
    assert_eq!(failed.icons.len(), 2);
    assert!(matches!(
        failed.remote_sync.get(&RemoteCollection::Icons),
        Some(RemoteSyncState::Failed { .. })
    ));
    fail.store(false, Ordering::SeqCst);
    service
        .execute(Command::RefreshIcons(account))
        .await
        .unwrap();
    assert!(matches!(
        service
            .snapshot()
            .unwrap()
            .remote_sync
            .get(&RemoteCollection::Icons),
        Some(RemoteSyncState::Ready { .. })
    ));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 6);
    assert!(
        requests
            .iter()
            .all(|request| request.url.path().starts_with("/media/displayIcons/user/"))
    );
}

#[tokio::test]
async fn latest_narrow_icon_refresh_wins_over_older_narrow_and_full_sync() {
    for full in [false, true] {
        let server = MockServer::start().await;
        let root = tempfile::tempdir().unwrap();
        let (service, account) = icon_fixture(root.path().join("profile"), server.uri());
        service
            .execute(Command::SelectAccount(account.clone()))
            .await
            .unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        Mock::given(method("GET")).and(path("/media/displayIcons/user/yoto"))
            .respond_with(move |_: &Request| {
                let number = counter.fetch_add(1,Ordering::SeqCst);
                let response = ResponseTemplate::new(200).set_body_json(serde_json::json!({"displayIcons":[{"mediaId":format!("public-{number}"),"title":format!("Catalogue {number}")}]}));
                if number==0 {response.set_delay(std::time::Duration::from_millis(150))} else {response}
            }).expect(2).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/media/displayIcons/user/me"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"displayIcons":[]})),
            )
            .expect(2)
            .mount(&server)
            .await;
        let client = service.clone();
        let owner = account.clone();
        let first = tokio::spawn(async move {
            client
                .execute(if full {
                    Command::Sync(owner)
                } else {
                    Command::RefreshIcons(owner)
                })
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while server.received_requests().await.unwrap().is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        service
            .execute(Command::RefreshIcons(account))
            .await
            .unwrap();
        let old = first.await.unwrap();
        assert!(old.is_err());
        if !full {
            assert!(matches!(old, Err(Failure::Cancelled)));
        }
        let snapshot = service.snapshot().unwrap();
        assert_eq!(snapshot.icons.len(), 1);
        assert_eq!(snapshot.icons[0].media.as_str(), "public-1");
        assert!(matches!(
            snapshot.remote_sync.get(&RemoteCollection::Icons),
            Some(RemoteSyncState::Ready { .. })
        ));
    }
}

#[tokio::test]
async fn icon_refresh_finishing_after_account_switch_cannot_install_old_icons() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = icon_fixture(root.path().join("profile"), server.uri());
    service
        .execute(Command::SelectAccount(account.clone()))
        .await
        .unwrap();
    Mock::given(method("GET"))
        .and(path("/media/displayIcons/user/yoto"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(
                    serde_json::json!({"displayIcons":[{"mediaId":"private-to-old-account"}]}),
                )
                .set_delay(std::time::Duration::from_millis(100)),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/media/displayIcons/user/me"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"displayIcons":[]})),
        )
        .mount(&server)
        .await;
    let client = service.clone();
    let first = tokio::spawn(async move { client.execute(Command::RefreshIcons(account)).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while server.received_requests().await.unwrap().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
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
    service
        .execute(Command::SelectAccount(other))
        .await
        .unwrap();
    assert!(matches!(first.await.unwrap(), Err(Failure::Cancelled)));
    let snapshot = service.snapshot().unwrap();
    assert!(snapshot.icons.is_empty());
    assert!(!snapshot.remote_sync.contains_key(&RemoteCollection::Icons));
}
