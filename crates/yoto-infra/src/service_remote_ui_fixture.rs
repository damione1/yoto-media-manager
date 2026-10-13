//! Explicitly invoked native UI fixture; never included in a production build.
use super::tests::fixture;
use super::*;
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::*};

#[tokio::test]
#[ignore = "native UI fixture: requires YOTO_TEST_UI_PROFILE and an explicit stop file"]
async fn native_remote_playlist_fixture() {
    let profile = PathBuf::from(std::env::var_os("YOTO_TEST_UI_PROFILE").unwrap());
    assert!(
        profile
            .to_string_lossy()
            .starts_with("/tmp/yoto-root-remote-ui-")
    );
    assert!(!profile.join("catalogue.db").exists());
    let server = MockServer::start().await;
    let (mut service, account) = fixture(profile.clone(), server.uri());
    Arc::get_mut(&mut service).unwrap().auth = TokenBroker::test_broker(
        server.uri(),
        &account,
        &crate::auth::Grant {
            access: "test-token".into(),
            refresh: None,
            scopes: vec![
                Scope::UserContentView,
                Scope::UserContentManage,
                Scope::UserIconsManage,
            ],
            unknown_scopes: vec![],
            expires: now() + 3600,
        },
    );
    service
        .db
        .put(
            Table::Account,
            account.as_str(),
            &Account {
                id: account.clone(),
                label: "Compte de démonstration".into(),
                scopes: vec![
                    Scope::UserContentView,
                    Scope::UserContentManage,
                    Scope::UserIconsManage,
                ],
            },
        )
        .unwrap();
    service
        .execute(Command::SelectAccount(account))
        .await
        .unwrap();
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        16,
        16,
        image::Rgba([120, 40, 10, 255]),
    ));
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    std::fs::write(profile.join("mon-icone.png"), png.get_ref()).unwrap();
    Mock::given(method("GET"))
        .and(path("/icons/sample.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "image/png")
                .set_body_bytes(png.into_inner()),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/media/displayIcons/user/yoto"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"displayIcons":[{
            "mediaId":"public-cat", "displayIconId":"public-display-cat", "title":"Chat", "public":true,
            "url":format!("{}/icons/sample.png", server.uri()), "userId":"yoto"
        }]})))
        .mount(&server).await;
    let own_icons = Arc::new(Mutex::new(vec![json!({
        "mediaId":"own-star", "displayIconId":"own-display-star", "title":"Mon étoile", "public":false,
        "url":format!("{}/icons/sample.png", server.uri()), "userId":"test-account"
    })]));
    let own = own_icons.clone();
    Mock::given(method("GET"))
        .and(path("/media/displayIcons/user/me"))
        .respond_with(move |_: &Request| {
            ResponseTemplate::new(200).set_body_json(json!({"displayIcons":*own.lock().unwrap()}))
        })
        .mount(&server)
        .await;
    let own = own_icons.clone();
    let icon_url = format!("{}/icons/sample.png", server.uri());
    Mock::given(method("POST"))
        .and(path("/media/displayIcons/user/me/upload"))
        .respond_with(move |request: &Request| {
            assert!(request.body.starts_with(b"\x89PNG"));
            let icon = json!({"mediaId":"uploaded-own", "displayIconId":"uploaded-display", "title":"Mon image", "url":icon_url, "userId":"test-account", "public":false});
            own.lock().unwrap().push(icon.clone());
            ResponseTemplate::new(200).set_body_json(json!({"displayIcon":icon})).set_delay(Duration::from_secs(2))
        })
        .mount(&server).await;
    let documents = Arc::new(Mutex::new(BTreeMap::from([(
        "songs-card".to_owned(),
        json!({
            "cardId":"songs-card", "title":"Chansons de démonstration", "metadata":{"category":"music"},
            "content":{"playbackType":"linear","config":{"autoadvance":"next","resumeTimeout":2592000},
                "chapters":[{"key":"1","title":"Chanson existante","tracks":[{
                    "key":"1","title":"Chanson existante","type":"audio","trackUrl":"yoto:#existing-audio",
                    "format":"mp3","duration":6.0,"fileSize":1234,"channels":"stereo"
                }]}]}
        }),
    )])));
    let cards = documents.clone();
    Mock::given(method("GET"))
        .and(path("/content/mine"))
        .respond_with(move |_: &Request| {
            let cards = cards
                .lock()
                .unwrap()
                .values()
                .map(|card| json!({"cardId":card["cardId"],"title":card["title"]}))
                .collect::<Vec<_>>();
            ResponseTemplate::new(200).set_body_json(json!({"cards":cards}))
        })
        .mount(&server)
        .await;
    let detail = documents.clone();
    Mock::given(method("GET"))
        .and(path_regex("^/content/(songs-card|created-[0-9]+)$"))
        .respond_with(move |request: &Request| {
            let id = request.url.path().trim_start_matches("/content/");
            match detail.lock().unwrap().get(id) {
                Some(card) => ResponseTemplate::new(200).set_body_json(json!({"card":card})),
                None => ResponseTemplate::new(404),
            }
        })
        .mount(&server)
        .await;
    let create = documents.clone();
    Mock::given(method("POST"))
        .and(path("/content"))
        .respond_with(move |request: &Request| {
            let mut card: Value = serde_json::from_slice(&request.body).unwrap();
            let mut cards = create.lock().unwrap();
            let id = card["cardId"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("created-{}", cards.len()));
            card["cardId"] = json!(id);
            cards.insert(id, card.clone());
            ResponseTemplate::new(200)
                .set_body_json(json!({"card":card}))
                .set_delay(Duration::from_secs(8))
        })
        .mount(&server)
        .await;
    let delete = documents.clone();
    Mock::given(method("DELETE"))
        .and(path_regex("^/content/(songs-card|created-[0-9]+)$"))
        .respond_with(move |request: &Request| {
            delete
                .lock()
                .unwrap()
                .remove(request.url.path().trim_start_matches("/content/"));
            ResponseTemplate::new(200).set_body_json(json!({}))
        })
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/media/transcode/audio/uploadUrl"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"upload":{
            "uploadId":"fixture-upload","uploadUrl":format!("{}/signed-storage",server.uri())
        }})))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/signed-storage"))
        .respond_with(|request: &Request| {
            assert!(request.headers.get("Authorization").is_none());
            assert!(!request.body.is_empty());
            ResponseTemplate::new(200).set_delay(Duration::from_secs(2))
        })
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/media/upload/fixture-upload/transcoded"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"transcode":{
                "transcodedSha256":"fixture-uploaded-audio","transcodedInfo":{
                    "duration":6.0,"fileSize":1024,"channels":"stereo","format":"mp3"
                }
            }})),
        )
        .mount(&server)
        .await;
    let worker = service.start();
    for (name, frequency) in [("Comptine du matin", "440"), ("Histoire du soir", "660")] {
        let input = profile.join(format!("{name}.mp3"));
        let mut generate = crate::media::command(&service.tools.ffmpeg);
        generate
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &format!("sine=frequency={frequency}:duration=6"),
                "-ac",
                "2",
                "-y",
            ])
            .arg(&input);
        assert!(
            crate::process::output(generate, 100_000, Duration::from_secs(30))
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
    }
    for _ in 0..200 {
        if service.snapshot().unwrap().media.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(service.snapshot().unwrap().media.len(), 2);
    // The UI must fetch MYO itself; no pre-populated remote list or editable cache.
    assert!(service.snapshot().unwrap().remote.is_empty());
    assert!(service.snapshot().unwrap().playlists.is_empty());
    let ipc = crate::bridge::start(service.clone(), profile.clone(), true).unwrap();
    std::fs::write(profile.join("ready"), server.uri()).unwrap();
    println!("Native remote fixture ready: {}", profile.display());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1200);
    while !profile.join("stop").exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "native fixture exceeded 20 minutes"
        );
        let requests = server.received_requests().await.unwrap();
        let audit = requests.iter().map(|r| json!({"method":r.method.to_string(),"path":r.url.path(),"body":serde_json::from_slice::<Value>(&r.body).ok()})).collect::<Vec<_>>();
        std::fs::write(
            profile.join("api-audit.json"),
            serde_json::to_vec_pretty(&audit).unwrap(),
        )
        .unwrap();
        if profile.join("external-change").exists() {
            if let Some(card) = documents.lock().unwrap().get_mut("songs-card") {
                card["title"] = json!("Chansons modifiées ailleurs");
                card["content"]["chapters"][0]["title"] = json!("Chapitre modifié ailleurs");
            }
            std::fs::remove_file(profile.join("external-change")).unwrap();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    ipc.abort();
    worker.abort();
}
