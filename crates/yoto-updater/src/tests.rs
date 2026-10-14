use super::*;
use base64::Engine;
fn signature() -> String {
    base64::engine::general_purpose::STANDARD.encode(include_bytes!(
        "../../../tests/fixtures/updater/signature-check.txt.minisig"
    ))
}
#[test]
fn authentic_signature_and_tampering() {
    let bytes = include_bytes!("../../../tests/fixtures/updater/signature-check.txt");
    assert!(verify(bytes, &signature(), PUBLIC_KEY).is_ok());
    assert!(verify(b"modified", &signature(), PUBLIC_KEY).is_err());
    let mut key_bytes = base64::engine::general_purpose::STANDARD
        .decode(PUBLIC_KEY.lines().nth(1).unwrap())
        .unwrap();
    key_bytes[20] ^= 1;
    let wrong_key = format!(
        "untrusted comment: different public key\n{}",
        base64::engine::general_purpose::STANDARD.encode(key_bytes)
    );
    assert!(minisign_verify::PublicKey::decode(&wrong_key).is_ok());
    assert!(verify(bytes, &signature(), &wrong_key).is_err());
}
fn manifest(version: &str, platform: Platform) -> Manifest {
    let version = Version::parse(version).unwrap();
    Manifest { version: version.clone(), notes: "Release".into(), platforms: [(platform, Artifact { url: format!("https://github.com/damione1/yoto-media-manager/releases/download/v{version}/{}", platform.archive_names(&version)[0]), signature: signature(), size: 100 })].into() }
}
#[test]
fn no_downgrade_and_platform_is_exact() {
    let current = AppVersion::Release(Version::parse("0.2.0").unwrap());
    assert!(
        select_release(
            manifest("0.1.0", Platform::MacArm),
            &current,
            Platform::MacArm
        )
        .unwrap()
        .is_none()
    );
    assert!(
        select_release(
            manifest("0.2.0", Platform::MacArm),
            &current,
            Platform::MacArm
        )
        .unwrap()
        .is_none()
    );
    assert!(
        select_release(
            manifest("0.3.0", Platform::MacArm),
            &current,
            Platform::MacIntel
        )
        .is_err()
    );
    for platform in [Platform::MacArm, Platform::MacIntel, Platform::Windows] {
        assert!(
            select_release(manifest("0.3.0", platform), &current, platform)
                .unwrap()
                .is_some()
        );
    }
}
#[test]
fn external_urls_and_excessive_size_rejected() {
    let current = AppVersion::Release(Version::parse("0.1.0").unwrap());
    let mut value = manifest("0.2.0", Platform::Windows);
    value.platforms.get_mut(&Platform::Windows).unwrap().url =
        "https://attacker.invalid/update.zip".into();
    assert!(select_release(value, &current, Platform::Windows).is_err());
    let mut value = manifest("0.2.0", Platform::Windows);
    value.platforms.get_mut(&Platform::Windows).unwrap().size = MAX_ARCHIVE + 1;
    assert!(select_release(value, &current, Platform::Windows).is_err());
}

#[test]
fn signed_identity_cannot_be_relabelled_as_a_new_release_or_platform() {
    assert!(verify_identity(&signature(), "YotoMediaManager test fixture").is_ok());
    for platform in [Platform::MacArm, Platform::MacIntel, Platform::Windows] {
        for name in platform.archive_names(&Version::parse("99.0.0").unwrap()) {
            let expected = format!("YotoMediaManager {name}");
            assert!(verify_identity(&signature(), &expected).is_err());
        }
    }
    // Altering the trusted comment to the expected release invalidates its global signature.
    let text = include_str!("../../../tests/fixtures/updater/signature-check.txt.minisig").replace(
        "YotoMediaManager test fixture",
        "YotoMediaManager Yoto-99.0.0-windows-x86_64.zip",
    );
    let forged = base64::engine::general_purpose::STANDARD.encode(text);
    assert!(
        verify(
            include_bytes!("../../../tests/fixtures/updater/signature-check.txt"),
            &forged,
            PUBLIC_KEY
        )
        .is_err()
    );
}

#[test]
fn build_version_parsing_is_strict() {
    assert_eq!(version::parse_release("1.2.3"), Some((1, 2, 3)));
    assert_eq!(version::parse_release("0.10.0"), Some((0, 10, 0)));
    for text in [
        "",
        "nightly",
        "1.2",
        "1.2.3.4",
        "01.2.3",
        "1.02.3",
        "1.2.03",
        "v1.2.3",
        "1.2.3-beta.1",
        "1.2.3+build",
        " 1.2.3",
        "1.2.3\n",
        "1..3",
        "1.2.x",
        "+1.2.3",
    ] {
        assert_eq!(version::parse_release(text), None, "{text:?}");
    }
    assert_eq!(
        AppVersion::from_build("1.2.3"),
        AppVersion::Release(Version::new(1, 2, 3))
    );
    assert_eq!(
        AppVersion::from_build(version::NIGHTLY),
        AppVersion::Nightly
    );
    // Tests are built without YOTO_APP_VERSION unless a release build sets it.
    match option_env!("YOTO_APP_VERSION") {
        Some(value) if !value.is_empty() => assert_eq!(CURRENT_VERSION, value),
        _ => {
            assert_eq!(CURRENT_VERSION, "nightly");
            assert!(AppVersion::current().is_nightly());
        }
    }
}
#[test]
fn nightly_is_offered_the_latest_release_and_never_current() {
    for version in ["0.0.1", "0.1.0", "99.0.0"] {
        let release = select_release(
            manifest(version, Platform::MacArm),
            &AppVersion::Nightly,
            Platform::MacArm,
        )
        .unwrap()
        .expect("a nightly is offered every published release");
        assert_eq!(release.version, Version::parse(version).unwrap());
    }
    // The manifest is still validated for a nightly.
    let mut value = manifest("0.3.0", Platform::Windows);
    value.platforms.get_mut(&Platform::Windows).unwrap().url =
        "https://attacker.invalid/update.zip".into();
    assert!(select_release(value, &AppVersion::Nightly, Platform::Windows).is_err());
}
#[tokio::test]
async fn nightly_refuses_to_download_and_replace_itself() {
    let updater = Updater::for_version(AppVersion::Nightly).unwrap();
    let release = Arc::new(
        select_release(
            manifest("1.0.0", Platform::Windows),
            &AppVersion::Nightly,
            Platform::Windows,
        )
        .unwrap()
        .unwrap(),
    );
    updater.download(release).await;
    assert!(matches!(
        updater.state().as_ref(),
        UpdateState::Failed(error) if matches!(error.as_ref(), UpdateError::Install(_))
    ));
    assert!(updater.prepared().is_err());
}
#[test]
fn current_and_legacy_archive_names_are_accepted_and_bound_to_the_signature() {
    let current = AppVersion::Release(Version::parse("0.1.0").unwrap());
    let cases = [
        (
            Platform::MacArm,
            "YotoMediaManager-0.2.0-darwin-aarch64.tar.gz",
        ),
        (Platform::MacArm, "Yoto-0.2.0-darwin-aarch64.tar.gz"),
        (
            Platform::MacIntel,
            "YotoMediaManager-0.2.0-darwin-x86_64.tar.gz",
        ),
        (
            Platform::Windows,
            "YotoMediaManager-0.2.0-windows-x86_64.zip",
        ),
        (Platform::Windows, "Yoto-0.2.0-windows-x86_64.zip"),
    ];
    for (platform, name) in cases {
        let mut value = manifest("0.2.0", platform);
        value.platforms.get_mut(&platform).unwrap().url = format!(
            "https://github.com/damione1/yoto-media-manager/releases/download/v0.2.0/{name}"
        );
        let release = select_release(value, &current, platform).unwrap().unwrap();
        // download() requires the trusted comment to name this exact file.
        assert_eq!(release.archive_name(), name);
    }
    for name in [
        "Other-0.2.0-windows-x86_64.zip",
        "YotoMediaManager-0.1.0-windows-x86_64.zip",
        "YotoMediaManager-0.2.0-darwin-aarch64.tar.gz",
        "yotomediamanager-0.2.0-windows-x86_64.zip",
        "x/YotoMediaManager-0.2.0-windows-x86_64.zip",
    ] {
        let mut value = manifest("0.2.0", Platform::Windows);
        value.platforms.get_mut(&Platform::Windows).unwrap().url = format!(
            "https://github.com/damione1/yoto-media-manager/releases/download/v0.2.0/{name}"
        );
        assert!(
            select_release(value, &current, Platform::Windows).is_err(),
            "{name}"
        );
    }
}
