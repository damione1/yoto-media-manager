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

#[tokio::test]
async fn catalogue_cover_is_account_scoped_and_downloads_selected_collection_without_token() {
    use crate::api::{Icon, IconCollection};
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = icon_fixture(root.path().join("profile"), server.uri());
    service
        .execute(Command::SelectAccount(account.clone()))
        .await
        .unwrap();
    let media = YotoMediaId::new("same-id").unwrap();
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        16,
        16,
        image::Rgba([51, 102, 153, 255]),
    ));
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    Mock::given(method("GET"))
        .and(path("/personal.png"))
        .respond_with(move |request: &Request| {
            assert!(!request.headers.contains_key("authorization"));
            ResponseTemplate::new(200).set_body_bytes(png.get_ref().clone())
        })
        .expect(1)
        .mount(&server)
        .await;
    service
        .remote
        .lock()
        .unwrap()
        .by_account
        .entry(account.clone())
        .or_default()
        .icons = vec![
        Icon {
            media: media.clone(),
            title: "Public".into(),
            url: Some(format!("{}/public.png", server.uri())),
            collection: IconCollection::Public,
        },
        Icon {
            media: media.clone(),
            title: "Personal".into(),
            url: Some(format!("{}/personal.png", server.uri())),
            collection: IconCollection::User,
        },
    ];
    let asset = service
        .execute(Command::StoreIconCover {
            account: account.clone(),
            media,
            collection: IconCollection::User,
        })
        .await
        .unwrap()
        .into_asset()
        .unwrap();
    let stored: Asset = service
        .db
        .get(Table::Asset, &asset.to_string())
        .unwrap()
        .unwrap();
    let file = service.store.materialize(&stored).unwrap();
    let image = crate::media::read_image(file.path()).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (512, 512));
    assert_eq!(image.get_pixel(400, 400).0, [51, 102, 153, 255]);
    let unrelated = service
        .execute(Command::StoreIconCover {
            account,
            media: YotoMediaId::new("missing").unwrap(),
            collection: IconCollection::Public,
        })
        .await;
    assert!(matches!(unrelated, Err(Failure::Validation(_))));
    let other_account = service
        .execute(Command::StoreIconCover {
            account: AccountId::new("unconnected-account").unwrap(),
            media: YotoMediaId::new("same-id").unwrap(),
            collection: IconCollection::User,
        })
        .await;
    assert!(matches!(other_account, Err(Failure::Authentication)));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn catalogue_cover_rejects_oversized_download_before_storing_asset() {
    use crate::api::{Icon, IconCollection};
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = icon_fixture(root.path().join("profile"), server.uri());
    service
        .execute(Command::SelectAccount(account.clone()))
        .await
        .unwrap();
    let media = YotoMediaId::new("huge").unwrap();
    service
        .remote
        .lock()
        .unwrap()
        .by_account
        .entry(account.clone())
        .or_default()
        .icons = vec![Icon {
        media: media.clone(),
        title: "Huge".into(),
        url: Some(format!("{}/huge.png", server.uri())),
        collection: IconCollection::Public,
    }];
    Mock::given(method("GET"))
        .and(path("/huge.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0; 8_000_001]))
        .expect(1)
        .mount(&server)
        .await;
    let result = service
        .execute(Command::StoreIconCover {
            account,
            media,
            collection: IconCollection::Public,
        })
        .await;
    assert!(matches!(result, Err(Failure::Validation(_))));
    assert!(service.store.inventory().unwrap().is_empty());
}

#[tokio::test]
async fn catalogue_cover_follows_redirects_without_leaking_account_token() {
    use crate::api::{Icon, IconCollection};
    let source = MockServer::start().await;
    let destination = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let (service, account) = icon_fixture(root.path().join("profile"), source.uri());
    service
        .execute(Command::SelectAccount(account.clone()))
        .await
        .unwrap();
    let media = YotoMediaId::new("redirected-icon").unwrap();
    service
        .remote
        .lock()
        .unwrap()
        .by_account
        .entry(account.clone())
        .or_default()
        .icons = vec![Icon {
        media: media.clone(),
        title: "Redirected".into(),
        url: Some(format!("{}/redirect.png", source.uri())),
        collection: IconCollection::Public,
    }];
    let target = format!("{}/real.png", destination.uri());
    Mock::given(method("GET"))
        .and(path("/redirect.png"))
        .respond_with(move |request: &Request| {
            assert!(!request.headers.contains_key("authorization"));
            assert_eq!(
                request.headers.get("user-agent").unwrap(),
                "yoto-media-manager"
            );
            ResponseTemplate::new(302).insert_header("Location", &target)
        })
        .expect(1)
        .mount(&source)
        .await;
    let pixels = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        16,
        16,
        image::Rgba([11, 22, 33, 255]),
    ));
    let mut png = std::io::Cursor::new(Vec::new());
    pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
    Mock::given(method("GET"))
        .and(path("/real.png"))
        .respond_with(move |request: &Request| {
            assert!(!request.headers.contains_key("authorization"));
            assert_eq!(
                request.headers.get("user-agent").unwrap(),
                "yoto-media-manager"
            );
            ResponseTemplate::new(200).set_body_bytes(png.get_ref().clone())
        })
        .expect(1)
        .mount(&destination)
        .await;
    let asset = service
        .execute(Command::StoreIconCover {
            account,
            media,
            collection: IconCollection::Public,
        })
        .await
        .unwrap()
        .into_asset()
        .unwrap();
    let stored: Asset = service
        .db
        .get(Table::Asset, &asset.to_string())
        .unwrap()
        .unwrap();
    let file = service.store.materialize(&stored).unwrap();
    let image = crate::media::read_image(file.path()).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (512, 512));
    assert_eq!(image.get_pixel(300, 300).0, [11, 22, 33, 255]);
}

#[tokio::test]
async fn catalogue_cover_http_rejection_is_not_reported_as_offline() {
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    Mock::given(method("GET"))
        .and(path("/denied.png"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    let failure = api
        .icon_image(&format!("{}/denied.png", server.uri()))
        .await
        .unwrap_err();
    assert!(
        matches!(failure, Failure::Validation(message) if message == messages::image_download_rejected(403))
    );
}

#[tokio::test]
async fn catalogue_cover_redirect_loop_is_bounded_and_reports_image_failure() {
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    Mock::given(method("GET"))
        .and(path("/loop.png"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/loop.png"))
        .mount(&server)
        .await;
    let failure = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        api.icon_image(&format!("{}/loop.png", server.uri())),
    )
    .await
    .expect("redirect limit must stop the loop promptly")
    .unwrap_err();
    assert!(
        matches!(failure, Failure::Validation(message) if message == messages::IMAGE_DOWNLOAD_FAILED)
    );
    let requests = server.received_requests().await.unwrap().len();
    assert!(
        (2..=6).contains(&requests),
        "unexpected redirect count: {requests}"
    );
}

#[tokio::test]
async fn catalogue_cover_redirect_keeps_download_byte_limit() {
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    Mock::given(method("GET"))
        .and(path("/redirect.png"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/huge.png"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/huge.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0; 8_000_001]))
        .expect(1)
        .mount(&server)
        .await;
    let failure = api
        .icon_image(&format!("{}/redirect.png", server.uri()))
        .await
        .unwrap_err();
    assert!(
        matches!(failure, Failure::Validation(message) if message == messages::IMAGE_TOO_LARGE)
    );
}

#[tokio::test]
async fn catalogue_cover_rejects_non_https_urls_outside_local_fixture() {
    let api = crate::api::YotoGateway::new().unwrap();
    for url in [
        "file:///tmp/icon.png",
        "ftp://example.test/icon.png",
        "http://example.test/icon.png",
    ] {
        assert!(matches!(
            api.icon_image(url).await,
            Err(Failure::Validation(_))
        ));
    }
}
