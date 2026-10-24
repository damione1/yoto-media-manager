//! Exercise the real Windows APIs in CI, with synthetic data and temporary profiles.
use super::*;
use std::fs;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};

struct Fixture {
    profile: tempfile::TempDir,
    id: AccountId,
}
impl Fixture {
    fn new() -> Self {
        Self {
            profile: tempfile::tempdir().unwrap(),
            id: AccountId::new(format!("synthetic-{}", uuid::Uuid::new_v4())).unwrap(),
        }
    }
    fn broker(&self) -> TokenBroker {
        TokenBroker::new(self.profile.path()).unwrap()
    }
    fn path(&self) -> std::path::PathBuf {
        self.profile.path().join("oauth").join(format!(
            "{}.dpapi",
            hex::encode(Sha256::digest(self.id.as_str()))
        ))
    }
    fn legacy(&self, text: &str) {
        self.broker()
            .entry(&self.id)
            .unwrap()
            .set_password(text)
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.broker().entry(&self.id).unwrap().delete_credential();
    }
}
fn grant(access: &str, expires: u64) -> Grant {
    Grant {
        access: access.into(),
        refresh: Some("synthetic-refresh".into()),
        scopes: vec![Scope::UserContentView],
        unknown_scopes: vec!["future:scope".into()],
        expires,
    }
}

#[tokio::test]
async fn large_grants_survive_restart_rotation_and_disconnect() {
    let f = Fixture::new();
    let access = "synthetic-access-".repeat(1024);
    let refresh = "synthetic-refresh-".repeat(512);
    let mut original = grant(&access, 0);
    original.refresh = Some(refresh.clone());
    let text = Zeroizing::new(serde_json::to_string(&original).unwrap());
    // Demonstrate the exact old failure, then exercise the production storage.
    assert!(matches!(
        f.broker().entry(&f.id).unwrap().set_password(&text),
        Err(keyring::Error::TooLong(_, 2560))
    ));
    f.broker().save(&f.id, &original).unwrap();
    let disk = fs::read(f.path()).unwrap();
    assert!(!disk.windows(64).any(|w| w == &access.as_bytes()[..64]));
    assert!(!disk.windows(64).any(|w| w == &refresh.as_bytes()[..64]));

    let server = MockServer::start().await;
    let rotated = "rotated-access-".repeat(1024);
    let rotated_refresh = "rotated-refresh-".repeat(512);
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": rotated, "refresh_token": rotated_refresh,
            "expires_in": 3600, "token_type": "Bearer",
            "scope": "user:content:view future:scope"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut reopened = f.broker();
    assert_eq!(reopened.load(&f.id).unwrap().access, access);
    reopened.host = server.uri();
    assert_eq!(
        reopened
            .access(&f.id, Scope::UserContentView)
            .await
            .unwrap()
            .as_str(),
        rotated.as_str()
    );
    let request = &server.received_requests().await.unwrap()[0];
    let fields: HashMap<_, _> = url::form_urlencoded::parse(&request.body)
        .into_owned()
        .collect();
    assert_eq!(fields["refresh_token"], refresh);
    drop(reopened);

    let reopened = f.broker();
    let saved = reopened.load(&f.id).unwrap();
    assert_eq!(saved.access, rotated);
    assert_eq!(saved.refresh.as_deref(), Some(rotated_refresh.as_str()));
    assert_eq!(saved.unknown_scopes, ["future:scope"]);
    assert!(saved.expires > now() + 60);
    assert_eq!(
        fs::read_dir(f.profile.path().join("oauth"))
            .unwrap()
            .count(),
        1
    );
    reopened.disconnect(&f.id).await.unwrap();
    reopened.disconnect(&f.id).await.unwrap();
    assert!(!f.path().exists());
    assert!(matches!(
        f.broker().load(&f.id),
        Err(Failure::Authentication)
    ));
}

#[test]
fn legacy_grants_migrate_without_losing_fields() {
    let f = Fixture::new();
    let original = grant("legacy-access", now() + 3600);
    let text = Zeroizing::new(serde_json::to_string(&original).unwrap());
    f.legacy(&text);
    assert!(!f.path().exists());
    let migrated = f.broker().load(&f.id).unwrap();
    assert_eq!(serde_json::to_string(&migrated).unwrap(), *text);
    assert!(f.path().exists());
    assert!(matches!(
        f.broker().entry(&f.id).unwrap().get_password(),
        Err(keyring::Error::NoEntry)
    ));
    assert_eq!(f.broker().load(&f.id).unwrap().access, "legacy-access");
}

#[test]
fn unavailable_file_storage_preserves_the_legacy_session() {
    let f = Fixture::new();
    let text = Zeroizing::new(serde_json::to_string(&grant("legacy", now() + 3600)).unwrap());
    f.legacy(&text);
    // A file where the directory belongs forces a real filesystem write error.
    fs::write(f.profile.path().join("oauth"), b"blocked").unwrap();
    assert!(matches!(
        f.broker().save(&f.id, &grant("replacement", now() + 3600)),
        Err(Failure::Storage(_))
    ));
    assert!(matches!(f.broker().load(&f.id), Err(Failure::Storage(_))));
    assert_eq!(
        f.broker().entry(&f.id).unwrap().get_password().unwrap(),
        *text
    );
    fs::remove_file(f.profile.path().join("oauth")).unwrap();
    assert_eq!(f.broker().load(&f.id).unwrap().access, "legacy");
}

#[tokio::test]
async fn corrupt_files_do_not_restore_legacy_sessions_and_logout_cleans_both() {
    let f = Fixture::new();
    let g = grant("current", now() + 3600);
    f.broker().save(&f.id, &g).unwrap();
    f.legacy(&serde_json::to_string(&grant("stale", now() + 3600)).unwrap());
    let mut bytes = fs::read(f.path()).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(f.path(), bytes).unwrap();
    assert!(matches!(
        f.broker().load(&f.id),
        Err(Failure::Authentication)
    ));
    f.broker().disconnect(&f.id).await.unwrap();
    assert!(!f.path().exists());
    assert!(matches!(
        f.broker().load(&f.id),
        Err(Failure::Authentication)
    ));
    assert!(matches!(
        f.broker().entry(&f.id).unwrap().get_password(),
        Err(keyring::Error::NoEntry)
    ));
}

#[tokio::test]
async fn files_are_bound_to_the_profile_and_account() {
    let a = Fixture::new();
    let b = Fixture {
        profile: tempfile::tempdir().unwrap(),
        id: a.id.clone(),
    };
    let other = AccountId::new(format!("synthetic-other-{}", uuid::Uuid::new_v4())).unwrap();
    let broker = a.broker();
    broker
        .save(&a.id, &grant("account-a", now() + 3600))
        .unwrap();
    broker
        .save(&other, &grant("account-b", now() + 3600))
        .unwrap();
    assert_eq!(broker.load(&a.id).unwrap().access, "account-a");
    assert_eq!(broker.load(&other).unwrap().access, "account-b");
    let copied = fs::read(a.path()).unwrap();
    b.broker()
        .save(&b.id, &grant("profile-b", now() + 3600))
        .unwrap();
    fs::write(b.path(), &copied).unwrap();
    assert!(matches!(
        b.broker().load(&b.id),
        Err(Failure::Authentication)
    ));
    // Same profile, different account: substituting the file must also fail.
    let other_path = a.profile.path().join("oauth").join(format!(
        "{}.dpapi",
        hex::encode(Sha256::digest(other.as_str()))
    ));
    fs::write(other_path, copied).unwrap();
    assert!(matches!(broker.load(&other), Err(Failure::Authentication)));
    broker.disconnect(&a.id).await.unwrap();
    broker.disconnect(&other).await.unwrap();
}
