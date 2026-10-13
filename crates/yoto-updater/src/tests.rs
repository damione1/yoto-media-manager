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
    Manifest { version: version.clone(), notes: "Release".into(), platforms: [(platform, Artifact { url: format!("https://github.com/damione1/yoto-media-manager/releases/download/v{version}/{}", platform.archive_name(&version)), signature: signature(), size: 100 })].into() }
}
#[test]
fn no_downgrade_and_platform_is_exact() {
    let current = Version::parse("0.2.0").unwrap();
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
    let current = Version::parse("0.1.0").unwrap();
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
        let expected = format!(
            "YotoMediaManager {}",
            platform.archive_name(&Version::parse("99.0.0").unwrap())
        );
        assert!(verify_identity(&signature(), &expected).is_err());
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
