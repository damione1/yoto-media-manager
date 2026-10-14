use crate::*;
use crate::{
    assets::AssetStore,
    db::{Database, Table},
};
use yoto_domain::*;
#[test]
fn encrypted_assets_detect_tamper_truncation_and_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let store = AssetStore::new(root.path().to_owned(), [3; 32]).unwrap();
    let file = root.path().join("plain.mp3");
    let bytes = vec![77; 150_000];
    std::fs::write(&file, &bytes).unwrap();
    let asset = store.put(&file, "mp3").unwrap();
    let lease = store.materialize(&asset).unwrap();
    assert_eq!(std::fs::read(lease.path()).unwrap(), bytes);
    let path = root.path().join("assets").join(format!("{}.yma", asset.id));
    let mut encrypted = std::fs::read(&path).unwrap();
    assert!(!encrypted.windows(64).any(|v| v == &bytes[..64]));
    encrypted[100] ^= 1;
    std::fs::write(&path, &encrypted).unwrap();
    assert!(store.materialize(&asset).is_err());
    encrypted.truncate(45);
    std::fs::write(&path, &encrypted).unwrap();
    assert!(store.materialize(&asset).is_err());
    let workspace = store.workspace().unwrap();
    let dir = workspace.path().to_owned();
    std::fs::write(dir.join("source.part"), b"partial").unwrap();
    drop(workspace);
    assert!(!dir.exists());
}
#[test]
fn sqlcipher_revision_cas_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("db");
    let db = Database::open(&path, &[4; 32]).unwrap();
    let mut p = Playlist::new("Before".into()).unwrap();
    db.save_playlist(&p).unwrap();
    let base = p.revision;
    p.title = "After".into();
    p.touch().unwrap();
    db.save_playlist_cas(&p, base).unwrap();
    let mut stale = p.clone();
    stale.title = "Stale".into();
    assert!(matches!(
        db.save_playlist_cas(&stale, base),
        Err(Failure::Conflict)
    ));
    drop(db);
    assert!(
        !std::fs::read(&path)
            .unwrap()
            .starts_with(b"SQLite format 3")
    );
    assert!(Database::open(&path, &[5; 32]).is_err());
    let db = Database::open(&path, &[4; 32]).unwrap();
    assert_eq!(
        db.get::<Playlist>(Table::Playlist, &p.id.to_string())
            .unwrap()
            .unwrap()
            .title,
        "After"
    );
}
fn tools() -> crate::media::Tools {
    crate::media::Tools::at(
        std::env::var_os("YOTO_TEST_TOOLS")
            .expect("Set YOTO_TEST_TOOLS to verified bundled tools")
            .into(),
    )
}
#[tokio::test]
async fn real_import_trim_and_selected_recipe() {
    crate::diagnostics::smoke(tools()).await.unwrap();
}
#[test]
fn opus_preview_uses_exact_transient_pcm_and_validates_cancel_and_duration() {
    use std::sync::{Arc, atomic::AtomicBool};
    let root = tempfile::tempdir().unwrap();
    let tools = tools();
    let input = root.path().join("fixture.opus");
    let mut generate = crate::media::command(&tools.ffmpeg);
    generate
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=6",
            "-ac",
            "2",
            "-c:a",
            "opus",
            "-strict",
            "-2",
            "-y",
        ])
        .arg(&input);
    assert!(
        crate::process::output(generate, 100_000, std::time::Duration::from_secs(30))
            .unwrap()
            .status
            .success()
    );
    let source = tools.probe(&input).unwrap();
    assert_eq!(source.codec, "opus");
    let store = AssetStore::new(root.path().join("catalogue"), [79; 32]).unwrap();
    let lease = store.lease("wav").unwrap();
    let path = lease.path().to_owned();
    let trim = TrimRange::new(
        MediaTime::from_seconds(1.).unwrap(),
        MediaTime::from_seconds(4.).unwrap(),
        source.duration,
    )
    .unwrap();
    let decoded = tools.preview(&input, lease.path(), trim).unwrap();
    assert_eq!(decoded.codec, "pcm_s16le");
    assert_eq!(decoded.duration, MediaTime::from_seconds(3.).unwrap());
    let decoder = rodio::Decoder::try_from(std::fs::File::open(lease.path()).unwrap()).unwrap();
    assert_eq!(decoder.count(), 3 * Rules::SAMPLE_RATE as usize * 2);
    let too_long: TrimRange =
        serde_json::from_value(serde_json::json!([1_000_000, 9_000_000])).unwrap();
    assert!(matches!(
        tools.preview(&input, lease.path(), too_long),
        Err(Failure::Validation(_))
    ));
    let _guard = crate::process::OperationGuard::install(Arc::new(AtomicBool::new(true)));
    assert!(matches!(
        tools.preview(&input, lease.path(), trim),
        Err(Failure::Cancelled)
    ));
    drop(lease);
    assert!(!path.exists());
}
#[test]
fn whole_webm_opus_preview_accepts_the_shorter_decoded_tail() {
    // WebM announces a millisecond-rounded duration that counts Opus pre-skip and padding,
    // so the decoded audio ends a few milliseconds early, as with YouTube downloads.
    let root = tempfile::tempdir().unwrap();
    let tools = tools();
    let input = root.path().join("fixture.webm");
    let mut generate = crate::media::command(&tools.ffmpeg);
    generate
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=6.01",
            "-ac",
            "2",
            "-c:a",
            "libopus",
            "-y",
        ])
        .arg(&input);
    assert!(
        crate::process::output(generate, 100_000, std::time::Duration::from_secs(30))
            .unwrap()
            .status
            .success()
    );
    let source = tools.probe(&input).unwrap();
    let whole = TrimRange::new(MediaTime::ZERO, source.duration, source.duration).unwrap();
    let store = AssetStore::new(root.path().join("catalogue"), [80; 32]).unwrap();
    let lease = store.lease("wav").unwrap();
    let decoded = tools.preview(&input, lease.path(), whole).unwrap();
    assert!(decoded.duration <= source.duration);
    assert!(source.duration.seconds() - decoded.duration.seconds() < 0.1);
}
#[tokio::test]
async fn device_contract_requires_online_and_preserves_unmanaged_settings() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    Mock::given(method("GET")).and(path("/device-v2/devices/mine"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"devices":[{"deviceId":"player","name":"Player","online":false,"deviceType":"v2","futureCapability":42}]})))
        .mount(&server).await;
    let devices = api.devices("token").await.unwrap();
    assert!(!devices[0].online);
    assert_eq!(devices[0].extra["futureCapability"], 42);
    Mock::given(method("GET")).and(path("/device-v2/player/config"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"device":{"deviceId":"player","config":{"repeatAll":true,"alarms":[{"futureAlarm":"preserve"}],"clockFace":"digital-sun","futureSetting":42}}})))
        .mount(&server).await;
    let device = DeviceId::new("player").unwrap();
    let mut config = api
        .device_config("token", &device)
        .await
        .unwrap()
        .device
        .config;
    config.repeat_all = Some(false);
    Mock::given(method("PUT")).and(path("/device-v2/player/config"))
        .and(body_json(serde_json::json!({"name":"Renamed","config":{"repeatAll":false,"alarms":[{"futureAlarm":"preserve"}],"clockFace":"digital-sun","futureSetting":42}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status":"ok"}))).expect(1).mount(&server).await;
    api.update_device(
        "token",
        &device,
        yoto_api_generated::models::DeviceConfigWrite::new(config, "Renamed".into()),
    )
    .await
    .unwrap();
    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/device-v2/devices/mine"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"devices":[{"deviceId":"player","name":"Player"}]}),
            ),
        )
        .mount(&server)
        .await;
    assert!(matches!(
        api.devices("token").await,
        Err(Failure::Compatibility(_))
    ));
}
#[tokio::test]
async fn group_card_items_and_binary_family_image_follow_generated_contract() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    Mock::given(method("POST"))
        .and(path("/card/family/library/groups"))
        .and(body_json(
            serde_json::json!({"name":"Group","items":[{"contentId":"card-one"}]}),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"group-one"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    api.save_group(
        "token",
        None,
        "Group",
        &[CardId::new("card-one").unwrap()],
        None,
    )
    .await
    .unwrap();
    Mock::given(method("GET")).and(path("/card/family/library/groups"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([{"id":"group-one","name":"Group","items":[{"contentId":"card-one"}]}]))).mount(&server).await;
    assert_eq!(
        api.groups("token").await.unwrap()[0].cards[0].as_str(),
        "card-one"
    );
    let temp = tempfile::tempdir().unwrap();
    let png = temp.path().join("image.png");
    image::RgbaImage::new(16, 16).save(&png).unwrap();
    Mock::given(method("POST")).and(path("/media/family/images"))
        .and(header("content-type","application/octet-stream")).and(body_bytes(std::fs::read(&png).unwrap()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"imageId":"image-one","url":"https://api.yotoplay.com/media/family/images/image-one"}))).expect(1).mount(&server).await;
    assert_eq!(
        api.upload_family_image("token", &png)
            .await
            .unwrap()
            .as_str(),
        "image-one"
    );
    Mock::given(method("GET"))
        .and(path("/media/family/images/image-one"))
        .and(query_param("width", "320"))
        .and(query_param("height", "320"))
        .and(header("authorization", "Bearer token"))
        .respond_with(ResponseTemplate::new(302).insert_header(
            "location",
            "https://media.example/image?signature=short-lived",
        ))
        .expect(1)
        .mount(&server)
        .await;
    let location = yoto_api_generated::apis::family_api::get_a_family_image(
        &api.configuration("token"),
        "image-one",
        320,
        320,
    )
    .await
    .unwrap();
    assert_eq!(
        location,
        "https://media.example/image?signature=short-lived"
    );
}
#[tokio::test]
async fn competing_ui_mcp_sessions_share_one_owner() {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    let (a, b) = tokio::join!(
        Session::connect(profile.clone(), tools(), true),
        Session::connect(profile, tools(), true)
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_ne!(a.is_owner(), b.is_owner());
    a.execute(Command::NewPlaylist("UI".into())).await.unwrap();
    b.execute(Command::NewPlaylist("MCP".into())).await.unwrap();
    assert_eq!(a.snapshot().await.unwrap().playlists.len(), 2);
    assert_eq!(b.snapshot().await.unwrap().playlists.len(), 2);
    b.shutdown();
    a.shutdown();
}
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "Uses the unlocked native macOS vault; run explicitly on a development Mac"]
async fn fresh_native_vault_clients_share_one_owner() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("native-vault-profile");
    let (a, b) = tokio::join!(
        Session::connect(profile.clone(), tools(), false),
        Session::connect(profile.clone(), tools(), false)
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_ne!(a.is_owner(), b.is_owner());
    b.execute(Command::NewPlaylist("Native vault race".into()))
        .await
        .unwrap();
    assert_eq!(a.snapshot().await.unwrap().playlists.len(), 1);
    assert_eq!(b.snapshot().await.unwrap().playlists.len(), 1);
    a.shutdown();
    b.shutdown();
    drop(a);
    drop(b);
    let namespace = hex::encode(Sha256::digest(
        canonical_profile(&profile)
            .unwrap()
            .as_os_str()
            .as_encoded_bytes(),
    ));
    for service in ["dev.yoto.desktop.library", "dev.yoto.desktop.ipc"] {
        keyring::Entry::new(service, &namespace)
            .unwrap()
            .delete_credential()
            .unwrap();
    }
}
#[tokio::test]
async fn playlist_revision_is_assigned_by_owner_and_rejects_stale_clients() {
    let root = tempfile::tempdir().unwrap();
    let session = Session::connect(root.path().join("profile"), tools(), true)
        .await
        .unwrap();
    session
        .execute(Command::NewPlaylist("CAS".into()))
        .await
        .unwrap();
    let mut first = session.snapshot().await.unwrap().playlists.remove(0);
    let expected = first.revision;
    let mut stale = first.clone();
    first.metadata.description = "First client".into();
    first.revision = Revision(u64::MAX); // A client cannot control the canonical revision.
    let reply = session
        .execute(Command::SavePlaylist {
            draft: Box::new(first),
            expected,
        })
        .await
        .unwrap();
    assert!(matches!(reply,crate::Reply::Saved(r) if r==expected.next().unwrap()));
    stale.metadata.description = "Stale client".into();
    stale.revision = Revision(0);
    assert!(matches!(
        session
            .execute(Command::SavePlaylist {
                draft: Box::new(stale),
                expected
            })
            .await,
        Err(Failure::Conflict)
    ));
    let current = session.snapshot().await.unwrap().playlists.remove(0);
    assert_eq!(current.metadata.description, "First client");
    assert_eq!(current.revision, expected.next().unwrap());
    session.shutdown();
    assert!(Revision(u64::MAX).next().is_err());
}
#[test]
fn commit_recovery_never_requeues_post() {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    let service = Service::isolated(profile.clone(), tools(), [9; 32]).unwrap();
    let account = AccountId::new("a").unwrap();
    let mut j = Job {
        id: JobId::new(),
        request: JobRequest::Publish {
            account,
            snapshot: Box::new(Playlist::new("Frozen".into()).unwrap()),
        },
        state: JobState::Running {
            phase: JobPhase::Committing,
        },
        attempts: 1,
        created: 1,
        checkpoint: Some(JobPhase::Committing),
    };
    service.db.put(Table::Job, &j.id.to_string(), &j).unwrap();
    j.id = JobId::new();
    j.state = JobState::Running {
        phase: JobPhase::Verifying {
            card: CardId::new("card1").unwrap(),
        },
    };
    service.db.put(Table::Job, &j.id.to_string(), &j).unwrap();
    drop(service);
    let recovered = Service::isolated(profile, tools(), [9; 32]).unwrap();
    assert!(
        recovered
            .snapshot()
            .unwrap()
            .jobs
            .iter()
            .all(|j| matches!(j.state, JobState::Reconciling))
    );
}
#[test]
fn rich_card_preserves_extensions_and_removes_icons() {
    let account = AccountId::new("owner").unwrap();
    let doc = serde_json::json!({"cardId":"card1","title":"Before","futureTop":{"enabled":true},"metadata":{"author":"Author","category":"stories","minAge":3,"maxAge":9,"futureMetadata":42},"content":{"playbackType":"linear","futureContent":"v2","config":{"autoadvance":"next","resumeTimeout":12,"futureConfig":7},"chapters":[{"key":"chapter1","title":"Chapter","futureChapter":true,"display":{"icon16x16":"yoto:#icon1","futureDisplay":"keep"},"tracks":[{"key":"track1","title":"Track","trackUrl":"yoto:#remote1","type":"audio","format":"mp3","duration":3.,"fileSize":123,"channels":"stereo","ambient":null,"display":{"icon16x16":"yoto:#icon2","futureDisplay":5},"events":{"onEnd":{"cmd":"repeat","futureEvent":true},"futureEventType":{}},"futureTrack":{"x":1}}]}]}});
    let mut p = crate::api::read_playlist(&doc, &account).unwrap();
    p.title = "Edited".into();
    p.chapters[0].icon = None;
    p.chapters[0].tracks[0].icon = None;
    let out = crate::api::write_playlist(&p, &account, &[], &Default::default(), None).unwrap();
    assert_eq!(out["futureTop"], doc["futureTop"]);
    assert_eq!(out["metadata"]["minAge"], 3.);
    assert_eq!(
        out["content"]["chapters"][0]["display"]["futureDisplay"],
        "keep"
    );
    assert!(
        out["content"]["chapters"][0]["display"]
            .get("icon16x16")
            .is_none()
    );
    let track = &out["content"]["chapters"][0]["tracks"][0];
    assert!(track["display"].get("icon16x16").is_none());
    assert_eq!(track["ambient"], serde_json::Value::Null);
    assert_eq!(
        track["events"],
        doc["content"]["chapters"][0]["tracks"][0]["events"]
    );
    let typed: yoto_api_generated::models::Card = serde_json::from_value(out).unwrap();
    let channels: AudioChannels = typed.content.chapters[0].tracks[0].channels.unwrap();
    assert_eq!(channels, AudioChannels::Stereo);
}
#[tokio::test]
async fn upload_reservation_sends_the_digest_as_lowercase_hexadecimal() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    let digest = "9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08";
    Mock::given(method("GET"))
        .and(path("/media/transcode/audio/uploadUrl"))
        .and(query_param("sha256", digest.to_lowercase()))
        .and(query_param("filename", "Chanson"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"upload":{"uploadId":"upload1","uploadUrl":"https://storage.test/put"}}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let reservation = api.reserve("API-TOKEN", digest, "Chanson").await.unwrap();
    assert_eq!(reservation.url.as_deref(), Some("https://storage.test/put"));
    assert!(api.reserve("API-TOKEN", "abcd", "Chanson").await.is_err());
}
#[tokio::test]
async fn upload_contract_has_no_storage_bearer_and_null_reservation() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    Mock::given(method("GET"))
        .and(path("/media/transcode/audio/uploadUrl"))
        .and(header("authorization", "Bearer API-TOKEN"))
        .and(query_param("sha256", "0".repeat(64)))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"upload":{"uploadId":"upload1","uploadUrl":null}}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    let reservation = api
        .reserve("API-TOKEN", &"00".repeat(32), "name")
        .await
        .unwrap();
    assert!(reservation.url.is_none());
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"audio").unwrap();
    Mock::given(method("PUT"))
        .and(path("/signed"))
        .and(header("content-type", "audio/mpeg"))
        .and(header("content-length", "5"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    api.put_signed(&format!("{}/signed", server.uri()), file.path(), 5)
        .await
        .unwrap();
    let requests = server.received_requests().await.unwrap();
    let put = requests.iter().find(|r| r.method == "PUT").unwrap();
    assert!(!put.headers.contains_key("authorization"));
    Mock::given(method("GET"))
        .and(path("/media/upload/upload1/transcoded"))
        .and(query_param("loudnorm", "false"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"transcode":{"transcodedSha256":null}})),
        )
        .mount(&server)
        .await;
    assert!(matches!(
        api.ingestion("API-TOKEN", &reservation.id).await.unwrap(),
        crate::api::Ingestion::Working(_)
    ));
}
#[tokio::test]
async fn a_failed_transcode_is_reported_with_its_progress() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    Mock::given(method("GET"))
        .and(path("/media/upload/upload2/transcoded"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "transcode":{"transcodedSha256":null,"progress":{"phase":"failed","percent":37}}
        })))
        .mount(&server)
        .await;
    let upload = UploadId::new("upload2").unwrap();
    match api.ingestion("API-TOKEN", &upload).await.unwrap() {
        crate::api::Ingestion::Failed(progress) => assert_eq!(progress.percent, Some(37)),
        other @ (crate::api::Ingestion::Ready(_) | crate::api::Ingestion::Working(_)) => {
            panic!("expected a failed transcode, got {other:?}")
        }
    }
}
#[tokio::test]
async fn covers_are_converted_by_yoto_to_the_card_format() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    Mock::given(method("POST"))
        .and(path("/media/coverImage/user/me/upload"))
        .and(query_param("autoconvert", "true"))
        .and(query_param("coverType", "default"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "coverImage":{"mediaId":"cover1","mediaUrl":"https://example.test/pub/cover1"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"png").unwrap();
    assert_eq!(
        api.upload_cover("API-TOKEN", file.path()).await.unwrap(),
        "https://example.test/pub/cover1"
    );
}
#[test]
fn yoto_limit_refusals_become_plain_messages() {
    use crate::api::limit_message as limit;
    let cases = [
        (
            r#"{"error":{"code":"bad-request","message":"Card cannot have more than 100 tracks"}}"#,
            Some(messages::PLAYLIST_TRACKS_MAX),
        ),
        (
            "Too many tracks in content",
            Some(messages::PLAYLIST_TRACKS_MAX),
        ),
        (
            "Track duration exceeds the 60 minute limit",
            Some(messages::YOTO_TRACK_TOO_LONG),
        ),
        (
            "Audio file size exceeds maximum of 100MB",
            Some(messages::YOTO_TRACK_TOO_LARGE),
        ),
        (
            "Total duration of the card exceeds 5 hours",
            Some(messages::YOTO_PLAYLIST_TOO_LONG),
        ),
        (
            "Playlist total size is larger than 500 MB",
            Some(messages::YOTO_PLAYLIST_TOO_LARGE),
        ),
        (
            "Track exceeds the maximum duration and file size",
            Some(messages::TRACK_EXCEEDS_LIMITS),
        ),
        ("Invalid cardId", None),
        ("title is required", None),
        ("", None),
    ];
    for (body, expected) in cases {
        assert_eq!(limit(body), expected, "{body}");
    }
}
#[tokio::test]
async fn a_refused_card_names_the_limit_it_broke() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    Mock::given(method("POST"))
        .and(path("/content"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error":{"code":"bad-request","message":"Total duration exceeds the 5 hour limit"}
        })))
        .mount(&server)
        .await;
    let failure = api
        .write_card(
            "API-TOKEN",
            &serde_json::json!({"title":"T","content":{"chapters":[]}}),
        )
        .await
        .unwrap_err();
    assert!(matches!(failure, Failure::Validation(m) if m == messages::YOTO_PLAYLIST_TOO_LONG));
}
#[test]
fn retry_after_accepts_seconds_and_dates_within_bounds() {
    use crate::api::retry_after_seconds as wait;
    use std::time::Duration;
    let now = httpdate::parse_http_date("Tue, 13 Oct 2026 10:00:00 GMT").unwrap();
    assert_eq!(wait(Some("120"), now), 120);
    assert_eq!(wait(Some(" 7 "), now), 7);
    assert_eq!(wait(Some("0"), now), 1);
    assert_eq!(wait(Some("86400"), now), 300);
    assert_eq!(wait(Some("Tue, 13 Oct 2026 10:01:30 GMT"), now), 90);
    assert_eq!(wait(Some("Tue, 13 Oct 2026 09:00:00 GMT"), now), 1);
    assert_eq!(wait(Some("Wed, 14 Oct 2026 10:00:00 GMT"), now), 300);
    assert_eq!(wait(Some("soon"), now), 30);
    assert_eq!(wait(Some("-5"), now), 30);
    assert_eq!(wait(None, now), 30);
    let later = now + Duration::from_millis(400);
    assert_eq!(wait(Some("Tue, 13 Oct 2026 10:00:10 GMT"), later), 10);
}
#[tokio::test]
async fn a_rate_limited_request_waits_as_long_as_yoto_asks() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    Mock::given(method("GET"))
        .and(path("/device-v2/devices/mine"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "75"))
        .mount(&server)
        .await;
    let failure = api.devices("API-TOKEN").await.unwrap_err();
    assert!(matches!(
        failure,
        Failure::RateLimit {
            retry_after_seconds: 75
        }
    ));
}
#[test]
fn upload_time_grows_with_the_file_within_bounds() {
    use crate::api::upload_timeout;
    use std::time::Duration;
    assert_eq!(upload_timeout(5), Duration::from_secs(5 * 60));
    assert_eq!(upload_timeout(40_000_000), Duration::from_secs(800));
    assert_eq!(upload_timeout(100_000_000), Duration::from_secs(2000));
    assert_eq!(upload_timeout(u64::MAX), Duration::from_secs(45 * 60));
}
#[test]
fn mcp_redacts_capabilities() {
    let value = crate::bridge::sanitize(
        serde_json::json!({"refresh_token":"secret","nested":{"uploadUrl":"https://example.test?X-Amz-Signature=secret"},"url":"https://example.test?X-Amz-Credential=secret","id":"opaque"}),
    );
    let text = value.to_string();
    assert!(!text.contains("secret"));
    assert_eq!(value["id"], "opaque");
}
#[cfg(unix)]
#[test]
fn child_timeout_kills_descendants_and_cancellation_is_typed() {
    use std::sync::{Arc, atomic::AtomicBool};
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", "sleep 60 & wait"]);
    let start = std::time::Instant::now();
    let output = crate::process::output(cmd, 1024, std::time::Duration::from_millis(100)).unwrap();
    assert!(!output.status.success());
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    let flag = Arc::new(AtomicBool::new(true));
    let _guard = crate::process::OperationGuard::install(flag);
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", "sleep 60"]);
    assert!(matches!(
        crate::process::output(cmd, 1024, std::time::Duration::from_secs(10)),
        Err(Failure::Cancelled)
    ));
}

pub(crate) fn catalogue_fixture(
    service: &std::sync::Arc<Service>,
    asset: AssetId,
    title: &str,
) -> (MediaItem, MediaRecipe) {
    let media = MediaItem {
        id: MediaId::new(),
        title: title.into(),
        author: "Author".into(),
        tags: vec![],
        original: asset,
        info: AudioInfo {
            duration: MediaTime::from_seconds(6.).unwrap(),
            bytes: 100,
            codec: "mp3".into(),
            sample_rate: 44100,
            channels: 2,
        },
        provenance: Provenance::LocalFile {
            name: "fixture.mp3".into(),
        },
        recipes: vec![],
    };
    service
        .db
        .put(Table::Media, &media.id.to_string(), &media)
        .unwrap();
    let recipe = MediaRecipe {
        id: RecipeId::new(),
        media: media.id,
        original: asset,
        trim: TrimRange::new(MediaTime::ZERO, media.info.duration, media.info.duration).unwrap(),
        normalization: Normalization::Preserve,
        result: None,
        info: None,
        tool_version: "fixture".into(),
        part: None,
    };
    service.db.add_recipe(&recipe).unwrap();
    (media, recipe)
}
#[tokio::test]
async fn deletion_reference_policy_shared_original_and_failed_import_orphans() {
    let root = tempfile::tempdir().unwrap();
    let service = Service::isolated(root.path().join("profile"), tools(), [11; 32]).unwrap();
    let input = root.path().join("audio");
    std::fs::write(&input, b"encrypted source fixture").unwrap();
    let asset = service.store.put(&input, "mp3").unwrap();
    service
        .db
        .put(Table::Asset, &asset.id.to_string(), &asset)
        .unwrap();
    let (a, recipe) = catalogue_fixture(&service, asset.id, "A");
    let (b, _) = catalogue_fixture(&service, asset.id, "B shared original");
    let mut p = Playlist::new("References A".into()).unwrap();
    p.add_media(&a, recipe.id).unwrap();
    service.db.save_playlist(&p).unwrap();
    assert!(
        service
            .execute(Command::DeleteMedia {
                media: a.id,
                policy: ReferencePolicy::RejectReferenced
            })
            .await
            .is_err()
    );
    service
        .execute(Command::DeleteMedia {
            media: a.id,
            policy: ReferencePolicy::RemovePlaylistTracks,
        })
        .await
        .unwrap();
    assert!(
        service
            .db
            .get::<MediaItem>(Table::Media, &a.id.to_string())
            .unwrap()
            .is_none()
    );
    assert!(service.playlist(p.id).unwrap().chapters.is_empty());
    assert_eq!(
        std::fs::read(service.store.materialize(&asset).unwrap().path()).unwrap(),
        b"encrypted source fixture"
    );
    let orphan = service.store.put(&input, "mp3").unwrap(); // Failed acquisition persisted a file before committing any record.
    service.execute(Command::CollectGarbage).await.unwrap();
    assert!(service.store.materialize(&orphan).is_err());
    assert!(service.store.materialize(&asset).is_ok());
    service
        .execute(Command::DeleteMedia {
            media: b.id,
            policy: ReferencePolicy::RejectReferenced,
        })
        .await
        .unwrap();
    assert!(service.store.materialize(&asset).is_err());
}
#[tokio::test]
async fn batch_deletion_keeps_referenced_media_and_deletes_the_rest() {
    let root = tempfile::tempdir().unwrap();
    let service = Service::isolated(root.path().join("profile"), tools(), [12; 32]).unwrap();
    let input = root.path().join("audio");
    std::fs::write(&input, b"encrypted source fixture").unwrap();
    let asset = service.store.put(&input, "mp3").unwrap();
    service
        .db
        .put(Table::Asset, &asset.id.to_string(), &asset)
        .unwrap();
    let (used, recipe) = catalogue_fixture(&service, asset.id, "In a playlist");
    let (free, _) = catalogue_fixture(&service, asset.id, "Unused");
    let mut p = Playlist::new("Keeps one".into()).unwrap();
    p.add_media(&used, recipe.id).unwrap();
    service.db.save_playlist(&p).unwrap();
    let reply = service
        .execute(Command::DeleteMediaBatch {
            media: vec![used.id, free.id],
            policy: ReferencePolicy::RejectReferenced,
        })
        .await
        .unwrap();
    let Reply::Data(counts) = reply else {
        panic!("batch deletion reports its counts");
    };
    assert_eq!(counts["deleted"], 1);
    assert_eq!(counts["kept"], 1);
    let exists = |id: MediaId| {
        service
            .db
            .get::<MediaItem>(Table::Media, &id.to_string())
            .unwrap()
            .is_some()
    };
    assert!(exists(used.id));
    assert!(!exists(free.id));
    assert_eq!(service.playlist(p.id).unwrap().chapters.len(), 1);
}
#[tokio::test]
async fn ten_thousand_catalogue_rows_page_across_bounded_ipc_and_aliases_share_owner() {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    let service = Service::isolated(profile.clone(), tools(), [41; 32]).unwrap();
    let file = root.path().join("asset");
    std::fs::write(&file, b"audio fixture").unwrap();
    let asset = service.store.put(&file, "mp3").unwrap();
    service
        .db
        .put(Table::Asset, &asset.id.to_string(), &asset)
        .unwrap();
    for index in 0..10_000 {
        let media = MediaItem {
            id: MediaId::new(),
            title: format!("Media {index}"),
            author: "A".repeat(120),
            tags: vec!["capacity".repeat(30)],
            original: asset.id,
            info: AudioInfo {
                duration: MediaTime::from_seconds(6.).unwrap(),
                bytes: 13,
                codec: "mp3".into(),
                sample_rate: 44100,
                channels: 2,
            },
            provenance: Provenance::LocalFile {
                name: "fixture.mp3".into(),
            },
            recipes: vec![],
        };
        service
            .db
            .put(Table::Media, &media.id.to_string(), &media)
            .unwrap();
        let job = Job {
            id: JobId::new(),
            request: JobRequest::Import {
                selection: ImportSelection::LocalFile {
                    path: "fixture".into(),
                },
            },
            state: JobState::Completed {
                output: JobOutput::Imported { media: media.id },
            },
            attempts: 1,
            created: 0,
            checkpoint: None,
        };
        service
            .db
            .put(Table::Job, &job.id.to_string(), &job)
            .unwrap();
    }
    assert!(
        serde_json::to_vec(&service.snapshot().unwrap())
            .unwrap()
            .len()
            > 4_000_000
    );
    let ipc = bridge::start(service.clone(), profile.clone(), true).unwrap();
    let alias = profile.join("..").join("profile");
    assert_eq!(
        canonical_profile(&alias).unwrap(),
        canonical_profile(&profile).unwrap()
    );
    let proxy = Session::connect(alias, tools(), true).await.unwrap();
    assert!(!proxy.is_owner());
    let snapshot = proxy.snapshot().await.unwrap();
    assert_eq!(snapshot.media.len(), 10_000);
    assert_eq!(snapshot.jobs.len(), 10_000);
    let delta = proxy.interface_changes(vec![]).await.unwrap();
    let mut ui = Snapshot::default();
    let revisions = delta.apply(&mut ui);
    assert_eq!(ui.media.len(), 10_000);
    assert_eq!(ui.jobs.len(), 200);
    assert!(
        proxy
            .interface_changes(revisions.clone())
            .await
            .unwrap()
            .parts
            .is_empty()
    );
    let mut changed = snapshot.jobs[0].clone();
    changed.state = JobState::Cancelled;
    service
        .db
        .put(Table::Job, &changed.id.to_string(), &changed)
        .unwrap();
    let update = proxy.interface_changes(revisions).await.unwrap();
    assert!(!update.parts.iter().any(|part| matches!(
        part,
        crate::projection::InterfacePart::Media(_) | crate::projection::InterfacePart::Recipes(_)
    )));
    assert!(serde_json::to_vec(&update).unwrap().len() < 200_000);
    let Reply::Data(page) = proxy
        .execute(Command::JobsPage {
            offset: 9800,
            limit: 200,
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(page.as_array().unwrap().len(), 200);
    let revision = proxy.revision().await.unwrap();
    proxy
        .execute(Command::NewPlaylist("From proxy".into()))
        .await
        .unwrap();
    assert_ne!(proxy.revision().await.unwrap(), revision);
    assert_eq!(proxy.snapshot().await.unwrap().playlists.len(), 1);
    ipc.abort();
}
#[test]
fn complete_encrypted_backup_restores_assets_with_recovery_and_detects_tamper() {
    let root = tempfile::tempdir().unwrap();
    let service = Service::isolated(root.path().join("profile"), tools(), [17; 32]).unwrap();
    let source = root.path().join("audio");
    std::fs::write(&source, b"original audio bytes").unwrap();
    let asset = service.store.put(&source, "mp3").unwrap();
    service
        .db
        .put(Table::Asset, &asset.id.to_string(), &asset)
        .unwrap();
    let (media, _) = catalogue_fixture(&service, asset.id, "Portable");
    let destination = root.path().join("backup");
    let recovery = hex::encode([71; 32]);
    crate::recovery::backup(&service.db, &service.store, &destination, &recovery).unwrap();
    assert!(
        !std::fs::read(destination.join("catalogue.db"))
            .unwrap()
            .starts_with(b"SQLite format 3")
    );
    assert!(
        !String::from_utf8_lossy(&std::fs::read(destination.join("manifest.ybk")).unwrap())
            .contains(&hex::encode([17; 32]))
    );
    assert!(
        crate::recovery::restore(
            &destination,
            &root.path().join("wrong"),
            &hex::encode([72; 32]),
            false
        )
        .is_err()
    );
    let restored = root.path().join("restored");
    crate::recovery::restore(&destination, &restored, &recovery, false).unwrap();
    let db = Database::open(&restored.join("catalogue.db"), &[17; 32]).unwrap();
    assert_eq!(
        db.get::<MediaItem>(Table::Media, &media.id.to_string())
            .unwrap()
            .unwrap()
            .title,
        "Portable"
    );
    let store = AssetStore::new(restored, [17; 32]).unwrap();
    assert_eq!(
        std::fs::read(store.materialize(&asset).unwrap().path()).unwrap(),
        b"original audio bytes"
    );
    std::fs::write(destination.join("catalogue.db"), b"altered").unwrap();
    assert!(
        crate::recovery::restore(
            &destination,
            &root.path().join("tampered"),
            &recovery,
            false
        )
        .is_err()
    );
}
#[test]
fn migration_failure_keeps_v1_and_recoverable_encrypted_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    std::fs::create_dir(&profile).unwrap();
    let path = profile.join("catalogue.db");
    let key = [29; 32];
    let c = rusqlite::Connection::open(&path).unwrap();
    c.pragma_update(None, "key", format!("x'{}'", hex::encode(key)))
        .unwrap();
    c.execute_batch(include_str!("../../../db/migrations/001.sql"))
        .unwrap();
    let store = AssetStore::new(profile.clone(), key).unwrap();
    let original = root.path().join("original");
    std::fs::write(&original, b"v1 original audio bytes").unwrap();
    let asset = store.put(&original, "mp3").unwrap();
    let media = MediaItem {
        id: MediaId::new(),
        title: "V1 representative".into(),
        author: "Owner".into(),
        tags: vec!["preserve".into()],
        original: asset.id,
        info: AudioInfo {
            duration: MediaTime::from_seconds(6.).unwrap(),
            bytes: 23,
            codec: "mp3".into(),
            sample_rate: 44100,
            channels: 1,
        },
        provenance: Provenance::LocalFile {
            name: "original.mp3".into(),
        },
        recipes: vec![RecipeId::new()],
    };
    let recipe = MediaRecipe {
        id: media.recipes[0],
        media: media.id,
        original: asset.id,
        trim: TrimRange::new(MediaTime::ZERO, media.info.duration, media.info.duration).unwrap(),
        normalization: Normalization::Preserve,
        result: None,
        info: None,
        tool_version: "v1".into(),
        part: None,
    };
    for (table, id, value) in [
        (
            "assets",
            asset.id.to_string(),
            serde_json::to_value(&asset).unwrap(),
        ),
        (
            "media_items",
            media.id.to_string(),
            serde_json::to_value(&media).unwrap(),
        ),
        (
            "recipes",
            recipe.id.to_string(),
            serde_json::to_value(&recipe).unwrap(),
        ),
    ] {
        c.execute(
            &format!("INSERT INTO {table}(id,payload) VALUES(?1,?2)"),
            rusqlite::params![
                id,
                serde_json::to_string(&serde_json::json!({"version":1,"value":value})).unwrap()
            ],
        )
        .unwrap();
    }
    drop(c);

    assert!(
        Database::open_injected_upgrade(
            &path,
            &key,
            "CREATE TABLE failure_test(id); SELECT missing_function();"
        )
        .is_err()
    );
    let c = rusqlite::Connection::open(&path).unwrap();
    c.pragma_update(None, "key", format!("x'{}'", hex::encode(key)))
        .unwrap();
    assert_eq!(
        c.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        1
    );
    drop(c);
    let backup = std::fs::read_dir(profile.join("backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let restored = root.path().join("restored");
    crate::recovery::restore(&backup, &restored, &hex::encode(key), false).unwrap();
    let restored_db = Database::open(&restored.join("catalogue.db"), &key).unwrap();
    restored_db.verify().unwrap();
    assert_eq!(
        restored_db.list::<MediaItem>(Table::Media).unwrap()[0].id,
        media.id
    );
    assert_eq!(
        restored_db.list::<MediaRecipe>(Table::Recipe).unwrap()[0].id,
        recipe.id
    );
    let restored_store = AssetStore::new(restored, key).unwrap();
    assert_eq!(
        std::fs::read(restored_store.materialize(&asset).unwrap().path()).unwrap(),
        b"v1 original audio bytes"
    );
    Database::open(&path, &key).unwrap().verify().unwrap();
}
#[test]
fn shortcuts_patch_preserves_unedited_template_mode_and_extensions() {
    let original = serde_json::json!({"versionId":"v","custom":9,"modes":{"customMode":{"x":1},"day":{"modeFlag":true,"content":[{"cmd":"track-play","params":{"card":"day","chapter":"c","track":"t"}}]},"night":{"modeFlag":false,"content":[{"cmd":"track-play","params":{"card":"template:night","chapter":"c","track":"t","custom":5}}]}}});
    let old = serde_json::from_value(original.clone()).unwrap();
    let unchanged = crate::api::patch_shortcuts(Some(&old), None, None).unwrap();
    assert_eq!(
        serde_json::to_value(unchanged).unwrap()["shortcuts"],
        original
    );
    let patched = crate::api::patch_shortcuts(
        Some(&old),
        Some(vec![Shortcut {
            card: CardId::new("new-day").unwrap(),
            chapter: ChapterKey::new("c").unwrap(),
            track: TrackKey::new("t").unwrap(),
        }]),
        None,
    )
    .unwrap();
    let data = serde_json::to_value(patched).unwrap();
    assert_eq!(
        data["shortcuts"]["modes"]["night"],
        original["modes"]["night"]
    );
    assert_eq!(data["shortcuts"]["modes"]["day"]["modeFlag"], true);
    let cleared = crate::api::patch_shortcuts(Some(&old), None, Some(vec![])).unwrap();
    assert_eq!(
        serde_json::to_value(cleared).unwrap()["shortcuts"]["modes"]["night"]["content"],
        serde_json::json!([])
    );
}
#[test]
fn helper_manifest_is_required_before_execution_and_checks_dependency_hashes() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("youtube")).unwrap();
    let target = "x86_64-pc-windows-msvc";
    let mut files = serde_json::Map::new();
    for name in [
        "ffmpeg.exe",
        "ffprobe.exe",
        "deno.exe",
        "youtube/yt-dlp.exe",
        "avcodec-63.dll",
        "avformat-63.dll",
        "avutil-61.dll",
        "avfilter-12.dll",
        "avdevice-63.dll",
        "swresample-7.dll",
        "swscale-10.dll",
    ] {
        std::fs::write(root.path().join(name), name).unwrap();
        files.insert(
            name.into(),
            serde_json::json!(hex::encode(Sha256::digest(name.as_bytes()))),
        );
    }
    std::fs::write(
        root.path().join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({"target":target,"files":files,"links":{}})).unwrap(),
    )
    .unwrap();
    crate::media::verify_manifest(root.path(), target).unwrap();
    assert!(crate::media::verify_manifest(root.path(), "aarch64-apple-darwin").is_err());
    std::fs::write(root.path().join("avcodec-63.dll"), b"altered").unwrap();
    assert!(crate::media::verify_manifest(root.path(), target).is_err());
    std::fs::remove_file(root.path().join("deno.exe")).unwrap();
    assert!(crate::media::verify_manifest(root.path(), target).is_err());
}
#[tokio::test]
async fn group_rename_keeps_duplicates_and_wire_order_and_explicit_reorder_round_trips() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};
    let server = MockServer::start().await;
    let mut api = crate::api::YotoGateway::new().unwrap();
    api.host = server.uri();
    let id = GroupId::new("ordered-group").unwrap();
    let cards = ["z", "a", "m", "a"]
        .map(|c| CardId::new(c).unwrap())
        .to_vec();
    Mock::given(method("GET")).and(path("/card/family/library/groups/ordered-group")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"ordered-group","name":"Original","items":[{"contentId":"z"},{"contentId":"a"},{"contentId":"m"},{"contentId":"a"}]}))).mount(&server).await;
    for (name, order) in [
        ("Renamed", cards.clone()),
        (
            "Reordered",
            vec![
                cards[2].clone(),
                cards[0].clone(),
                cards[1].clone(),
                cards[3].clone(),
            ],
        ),
    ] {
        Mock::given(method("PUT")).and(path("/card/family/library/groups/ordered-group")).and(body_json(serde_json::json!({"name":name,"items":order.iter().map(|card|serde_json::json!({"contentId":card})).collect::<Vec<_>>()}))).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"ordered-group"}))).expect(1).mount(&server).await;
        api.save_group("test", Some(&id), name, &order, None)
            .await
            .unwrap();
    }
}
#[test]
fn full_settings_patch_validates_once_preserves_open_values_and_unmanaged_fields() {
    let original = serde_json::json!({"alarms":[{"unknownAlarm":true}],"unmanaged":{"preserve":1},"clockFace":"old","locale":"old","hourFormat":"old","dayDisplayBrightness":"old","nightDisplayBrightness":"old","displayDimBrightness":"old","dayYotoDaily":"old","nightYotoDaily":"old","dayYotoRadio":"old","nightYotoRadio":"old","volumeLevel":"old","bluetoothEnabled":"old"});
    let baseline = serde_json::json!({"name":"Device","dayTime":"07:30","nightTime":"22:01","maxVolumeLimit":"16","nightMaxVolumeLimit":"16","ambientColour":"#ff3900","nightAmbientColour":"#ff0000","displayDimTimeout":"60","shutdownTimeout":"3600","repeatAll":false,"btHeadphonesEnabled":true,"headphonesVolumeLimited":false});
    for field in [
        "clockFace",
        "locale",
        "hourFormat",
        "dayDisplayBrightness",
        "nightDisplayBrightness",
        "displayDimBrightness",
        "dayYotoDaily",
        "nightYotoDaily",
        "dayYotoRadio",
        "nightYotoRadio",
        "volumeLevel",
        "bluetoothEnabled",
    ] {
        let mut data = baseline.clone();
        data[field] = serde_json::json!("future-open-value");
        let patch: DeviceConfiguration = serde_json::from_value(data).unwrap();
        let output = crate::api::patch_device_config(
            serde_json::from_value(original.clone()).unwrap(),
            &patch,
        )
        .unwrap();
        let config = serde_json::to_value(output).unwrap()["config"].clone();
        assert_eq!(config[field], "future-open-value");
        for other in original
            .as_object()
            .unwrap()
            .keys()
            .filter(|other| *other != field)
        {
            assert_eq!(config[other], original[other]);
        }
    }
    for (field, value) in [
        ("dayTime", "25:10"),
        ("nightTime", "07:99"),
        ("ambientColour", "#xyzxyz"),
        ("maxVolumeLimit", "-1"),
        ("shutdownTimeout", "NaN"),
    ] {
        let mut data = baseline.clone();
        data[field] = serde_json::json!(value);
        let patch: DeviceConfiguration = serde_json::from_value(data).unwrap();
        assert!(patch.validate().is_err());
        assert!(
            crate::api::patch_device_config(
                yoto_api_generated::models::DeviceSettings::new(),
                &patch
            )
            .is_err()
        );
    }
}
#[tokio::test]
async fn long_media_is_added_as_reused_parts_and_removed_with_its_version() {
    let root = tempfile::tempdir().unwrap();
    let service = Service::isolated(root.path().join("profile"), tools(), [13; 32]).unwrap();
    let input = root.path().join("audio");
    std::fs::write(&input, b"long source fixture").unwrap();
    let asset = service.store.put(&input, "mp3").unwrap();
    service
        .db
        .put(Table::Asset, &asset.id.to_string(), &asset)
        .unwrap();
    let (mut media, _) = catalogue_fixture(&service, asset.id, "Livre audio");
    media.info.duration = MediaTime::from_seconds(7_200.).unwrap();
    service
        .db
        .put(Table::Media, &media.id.to_string(), &media)
        .unwrap();
    let version = MediaRecipe {
        id: RecipeId::new(),
        media: media.id,
        original: asset.id,
        trim: TrimRange::new(MediaTime::ZERO, media.info.duration, media.info.duration).unwrap(),
        normalization: Normalization::Loudness,
        result: None,
        info: None,
        tool_version: "fixture".into(),
        part: None,
    };
    service.db.add_recipe(&version).unwrap();
    let playlist = Playlist::new("Soirée".into()).unwrap();
    service.db.save_playlist(&playlist).unwrap();
    for _ in 0..2 {
        service
            .execute(Command::AddMedia {
                playlist: playlist.id,
                media: vec![(media.id, version.id)],
                part_title: None,
            })
            .await
            .unwrap();
    }
    let saved = service.playlist(playlist.id).unwrap();
    let parts = |chapter: &Chapter| -> Vec<RecipeId> {
        chapter
            .tracks
            .iter()
            .map(|t| match t.media {
                TrackMedia::Local { recipe, .. } => recipe,
                TrackMedia::RemoteAudio { .. } | TrackMedia::Stream { .. } => panic!("local"),
            })
            .collect()
    };
    assert_eq!(saved.chapters.len(), 2);
    assert_eq!(
        saved.chapters[0].tracks[2].title,
        "Livre audio — partie 3/3"
    );
    // Adding it again reuses the same parts, so nothing is prepared or uploaded twice.
    assert_eq!(parts(&saved.chapters[0]), parts(&saved.chapters[1]));
    let first = parts(&saved.chapters[0]);
    for (index, id) in first.iter().enumerate() {
        let part = service.recipe(*id).unwrap();
        assert_eq!(part.normalization, Normalization::Loudness);
        assert_eq!(part.trim.duration().seconds(), 2_400.);
        assert_eq!(part.part.unwrap().index as usize, index + 1);
    }
    // Parts are not versions a person picks.
    assert!(
        service
            .media(media.id)
            .unwrap()
            .recipes
            .iter()
            .all(|id| !first.contains(id))
    );
    service
        .execute(Command::DeleteRecipe {
            recipe: version.id,
            policy: ReferencePolicy::RemovePlaylistTracks,
        })
        .await
        .unwrap();
    assert!(service.playlist(playlist.id).unwrap().chapters.is_empty());
    assert!(first.iter().all(|id| service.recipe(*id).is_err()));
}
