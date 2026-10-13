use crate::{now, storage};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Mutex,
};
use yoto_auth_generated::apis::{authentication_api, configuration::Configuration};
use yoto_domain::*;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};
#[derive(Clone, Deserialize)]
pub struct PublicConfig {
    pub oauth: OAuthConfig,
}
#[derive(Clone, Deserialize)]
pub struct OAuthConfig {
    #[serde(skip)]
    pub client_id: String,
    pub redirect_uri: String,
    pub requested_scopes: Vec<Scope>,
}
impl PublicConfig {
    pub fn embedded() -> Result<Self> {
        let mut c: Self =
            toml::from_str(include_str!("../../../config/yoto.toml")).map_err(storage)?;
        c.oauth.client_id = env!(
            "YOTO_CLIENT_ID",
            "Set YOTO_CLIENT_ID before building YotoMediaManager"
        )
        .trim()
        .to_owned();
        let u = url::Url::parse(&c.oauth.redirect_uri).map_err(storage)?;
        if u.scheme() != "http"
            || u.host_str() != Some("127.0.0.1")
            || u.path() != "/callback"
            || u.port().is_none()
        {
            return Err(Failure::Validation(
                "OAuth requires the registered loopback callback".into(),
            ));
        }
        if c.oauth.client_id.is_empty() {
            return Err(Failure::Validation("OAuth client is missing".into()));
        }
        Ok(c)
    }
}
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Grant {
    pub access: String,
    pub refresh: Option<String>,
    #[zeroize(skip)]
    pub scopes: Vec<Scope>,
    pub unknown_scopes: Vec<String>,
    pub expires: u64,
}
enum TokenExchange<'a> {
    Authorization { code: &'a str, verifier: &'a str },
    Refresh { refresh: &'a str },
}
#[cfg(test)]
type TestVault = Arc<std::sync::Mutex<HashMap<String, Zeroizing<String>>>>;
pub struct TokenBroker {
    namespace: String,
    config: PublicConfig,
    http: reqwest::Client,
    host: String,
    locks: Mutex<HashMap<AccountId, Arc<Mutex<()>>>>,
    #[cfg(test)]
    test_vault: Option<TestVault>,
}
impl TokenBroker {
    pub fn new(namespace: String) -> Result<Self> {
        Ok(Self {
            namespace,
            config: PublicConfig::embedded()?,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(storage)?,
            host: "https://login.yotoplay.com".into(),
            locks: Mutex::new(HashMap::new()),
            #[cfg(test)]
            test_vault: None,
        })
    }
    fn configuration(&self) -> Configuration {
        Configuration {
            base_path: self.host.clone(),
            client: self.http.clone(),
            bearer_access_token: None,
            ..Configuration::default()
        }
    }
    fn vault_key(&self, id: &AccountId) -> String {
        format!("{}:{id}", self.namespace)
    }
    fn entry(&self, id: &AccountId) -> Result<keyring::Entry> {
        keyring::Entry::new("dev.yoto.desktop.oauth", &self.vault_key(id)).map_err(storage)
    }
    fn load(&self, id: &AccountId) -> Result<Grant> {
        #[cfg(test)]
        if let Some(vault) = &self.test_vault {
            let vault = vault.lock().map_err(storage)?;
            let text = vault
                .get(&self.vault_key(id))
                .ok_or(Failure::Authentication)?;
            return serde_json::from_str(text).map_err(|_| Failure::Authentication);
        }
        let s = Zeroizing::new(
            self.entry(id)?
                .get_password()
                .map_err(|_| Failure::Authentication)?,
        );
        serde_json::from_str(&s).map_err(|_| Failure::Authentication)
    }
    fn save(&self, id: &AccountId, g: &Grant) -> Result<()> {
        let text = Zeroizing::new(serde_json::to_string(g).map_err(storage)?);
        #[cfg(test)]
        if let Some(vault) = &self.test_vault {
            vault
                .lock()
                .map_err(storage)?
                .insert(self.vault_key(id), text);
            return Ok(());
        }
        self.entry(id)?.set_password(&text).map_err(storage)
    }
    pub async fn disconnect(&self, id: &AccountId) -> Result<()> {
        let gate = self
            .locks
            .lock()
            .await
            .entry(id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = gate.lock().await;
        #[cfg(test)]
        if let Some(vault) = &self.test_vault {
            vault.lock().map_err(storage)?.remove(&self.vault_key(id));
            return Ok(());
        }
        match self.entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(storage(e)),
        }
    }
    fn grant(response: yoto_auth_generated::models::PostOauthToken200Response) -> Result<Grant> {
        if response.expires_in <= 0 {
            return Err(Failure::Compatibility("Invalid token expiration".into()));
        }
        let mut scopes = vec![];
        let mut unknown = vec![];
        for raw in response.scope.unwrap_or_default().split_whitespace() {
            match serde_json::from_value(serde_json::Value::String(raw.into())) {
                Ok(scope) => scopes.push(scope),
                Err(_) => unknown.push(raw.into()),
            }
        }
        Ok(Grant {
            access: response.access_token,
            refresh: response.refresh_token,
            scopes,
            unknown_scopes: unknown,
            expires: now() + response.expires_in as u64,
        })
    }
    async fn exchange(
        &self,
        exchange: TokenExchange<'_>,
    ) -> Result<yoto_auth_generated::models::PostOauthToken200Response> {
        let (code, verifier, kind, redirect, refresh) = match exchange {
            TokenExchange::Authorization { code, verifier } => (
                Some(code),
                Some(verifier),
                "authorization_code",
                Some(self.config.oauth.redirect_uri.as_str()),
                None,
            ),
            TokenExchange::Refresh { refresh } => {
                (None, None, "refresh_token", None, Some(refresh))
            }
        };
        authentication_api::post_oauth_token(
            &self.configuration(),
            Some(&self.config.oauth.client_id),
            None,
            code,
            verifier,
            Some(kind),
            redirect,
            refresh,
            None,
            None,
            None,
        )
        .await
        .map_err(|_| Failure::Authentication)
    }
    #[cfg(test)]
    pub(crate) fn test_broker(host: String, id: &AccountId, grant: &Grant) -> Self {
        let mut broker = Self::new("test".into()).unwrap();
        broker.host = host;
        broker.test_vault = Some(Arc::new(std::sync::Mutex::new(HashMap::new())));
        broker.save(id, grant).unwrap();
        broker
    }
    pub async fn access(&self, id: &AccountId, scope: Scope) -> Result<Zeroizing<String>> {
        let gate = self
            .locks
            .lock()
            .await
            .entry(id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = gate.lock().await;
        let mut grant = self.load(id)?;
        if grant.expires <= now() + 60 {
            let refresh = grant.refresh.as_deref().ok_or(Failure::Authentication)?;
            let r = self.exchange(TokenExchange::Refresh { refresh }).await?;
            let mut rotated = Self::grant(r)?;
            if rotated.refresh.is_none() {
                return Err(Failure::Authentication);
            }
            if rotated.scopes.is_empty() {
                rotated.scopes = grant.scopes.clone();
            }
            self.save(id, &rotated)?;
            grant = rotated;
        }
        if !grant.scopes.contains(&scope) {
            return Err(Failure::Permission(scope.wire().into()));
        }
        Ok(Zeroizing::new(grant.access.clone()))
    }
    pub async fn login(&self) -> Result<Account> {
        let config = &self.config.oauth;
        let callback = url::Url::parse(&config.redirect_uri).map_err(storage)?;
        let listener=TcpListener::bind(("127.0.0.1",callback.port().ok_or_else(||Failure::Validation("No callback port".into()))?)).await.map_err(|_|Failure::Validation("OAuth callback port 8787 is occupied. Close the other connection attempt and retry.".into()))?;
        let mut random = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut random);
        let verifier = Zeroizing::new(URL_SAFE_NO_PAD.encode(random));
        rand::thread_rng().fill_bytes(&mut random);
        let state = Zeroizing::new(URL_SAFE_NO_PAD.encode(random));
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut authorize =
            url::Url::parse(&format!("{}/authorize", self.host)).map_err(storage)?;
        authorize.query_pairs_mut().extend_pairs([
            ("audience", "https://api.yotoplay.com"),
            (
                "scope",
                &config
                    .requested_scopes
                    .iter()
                    .map(|s| s.wire())
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            ("response_type", "code"),
            ("client_id", &config.client_id),
            ("redirect_uri", &config.redirect_uri),
            ("state", state.as_str()),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ]);
        webbrowser::open(authorize.as_str())
            .map_err(|_| Failure::Validation("Could not open the browser".into()))?;
        let deadline = Instant::now() + Duration::from_secs(300);
        let code = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Failure::Authentication);
            }
            let (mut socket, _) = tokio::time::timeout(remaining, listener.accept())
                .await
                .map_err(|_| Failure::Authentication)?
                .map_err(storage)?;
            let mut bytes = [0; 8192];
            let n = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut bytes))
                .await
                .map_err(|_| Failure::Authentication)?
                .map_err(storage)?;
            let request = String::from_utf8_lossy(&bytes[..n]);
            let path = request
                .lines()
                .next()
                .and_then(|s| s.strip_prefix("GET "))
                .and_then(|s| s.split_whitespace().next());
            let result = match path {
                Some(p) => validate_callback(p, &state),
                None => Err(Failure::Authentication),
            };
            let response = if result.is_ok() {
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\nConnected. You can return to the Yoto application."
            } else {
                "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid authorization response."
            };
            let _ = socket.write_all(response.as_bytes()).await;
            match result {
                Ok(code) => break code,
                Err(Failure::Authentication) => continue,
                Err(e) => return Err(e),
            }
        };
        drop(listener);
        let response = self
            .exchange(TokenExchange::Authorization {
                code: &code,
                verifier: &verifier,
            })
            .await?;
        let grant = Self::grant(response)?;
        // The subject is only a namespace extracted from an authority-issued token. It is
        // never trusted for scopes or authentication; scopes come from the token response.
        let payload = grant.access.split('.').nth(1).ok_or_else(|| {
            Failure::Compatibility("Token does not expose an account namespace".into())
        })?;
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).map_err(storage)?)
                .map_err(storage)?;
        let subject = claims
            .get("sub")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Failure::Compatibility("Token subject is missing".into()))?;
        let id = AccountId::new(hex::encode(Sha256::digest(subject.as_bytes())))?;
        let gate = self
            .locks
            .lock()
            .await
            .entry(id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = gate.lock().await;
        self.save(&id, &grant)?;
        let label = format!("Yoto {}", &id.as_str()[..8]);
        Ok(Account {
            id,
            label,
            scopes: grant.scopes.clone(),
        })
    }
}
pub fn validate_callback(path: &str, state: &str) -> Result<String> {
    let u =
        url::Url::parse(&format!("http://127.0.0.1{path}")).map_err(|_| Failure::Authentication)?;
    if u.path() != "/callback" {
        return Err(Failure::Authentication);
    }
    let q: HashMap<_, _> = u.query_pairs().into_owned().collect();
    if q.get("state").map(String::as_str) != Some(state) {
        return Err(Failure::Authentication);
    }
    if q.contains_key("error") {
        return Err(Failure::Permission("Authorization was declined".into()));
    }
    q.get("code")
        .filter(|s| !s.is_empty() && s.len() < 4096)
        .cloned()
        .ok_or(Failure::Authentication)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::*};
    #[test]
    fn callback_checks_state_path_error_and_code() {
        assert!(validate_callback("/callback?state=wrong&code=x", "expected").is_err());
        assert!(validate_callback("/else?state=expected&code=x", "expected").is_err());
        assert!(matches!(
            validate_callback("/callback?state=expected&error=access_denied", "expected"),
            Err(Failure::Permission(_))
        ));
        assert_eq!(
            validate_callback("/callback?state=expected&code=hello%20world", "expected").unwrap(),
            "hello world"
        );
    }
    #[test]
    fn pkce_rfc7636_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
    #[tokio::test]
    async fn refresh_is_single_flight_rotated_and_logout_cannot_resurrect() {
        let server = MockServer::start().await;
        let mut broker = TokenBroker::new("test".into()).unwrap();
        broker.host = server.uri();
        broker.test_vault = Some(Arc::new(std::sync::Mutex::new(HashMap::new())));
        let broker = Arc::new(broker);
        let id = AccountId::new("owner").unwrap();
        broker
            .save(
                &id,
                &Grant {
                    access: "expired".into(),
                    refresh: Some("old-refresh".into()),
                    scopes: vec![Scope::UserContentManage],
                    unknown_scopes: vec![],
                    expires: 0,
                },
            )
            .unwrap();
        Mock::given(method("POST")).and(path("/oauth/token")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(100)).set_body_json(serde_json::json!({"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600,"token_type":"Bearer","scope":"user:content:manage future:scope"}))).expect(1).mount(&server).await;
        let (a, b) = tokio::join!(
            broker.access(&id, Scope::UserContentManage),
            broker.access(&id, Scope::UserContentManage)
        );
        assert_eq!(&**a.as_ref().unwrap(), "new-access");
        assert_eq!(&**b.as_ref().unwrap(), "new-access");
        assert_eq!(
            broker.load(&id).unwrap().refresh.as_deref(),
            Some("new-refresh")
        );
        assert_eq!(broker.load(&id).unwrap().unknown_scopes, ["future:scope"]);
        let request = &server.received_requests().await.unwrap()[0];
        assert!(!request.headers.contains_key("authorization"));
        let fields: HashMap<_, _> = url::form_urlencoded::parse(&request.body)
            .into_owned()
            .collect();
        assert_eq!(fields["grant_type"], "refresh_token");
        assert_eq!(fields["refresh_token"], "old-refresh");
        assert_eq!(fields["client_id"], broker.config.oauth.client_id);
        assert!(!fields.contains_key("client_secret"));
        broker.disconnect(&id).await.unwrap();
        assert!(matches!(
            broker.access(&id, Scope::UserContentManage).await,
            Err(Failure::Authentication)
        ));
        // Logout queued behind an in-flight refresh removes its rotated result before returning.
        server.reset().await;
        broker
            .save(
                &id,
                &Grant {
                    access: "expired".into(),
                    refresh: Some("r".into()),
                    scopes: vec![Scope::UserContentManage],
                    unknown_scopes: vec![],
                    expires: 0,
                },
            )
            .unwrap();
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(100)).set_body_json(serde_json::json!({"access_token":"rotated","refresh_token":"rotated-r","expires_in":3600,"token_type":"Bearer","scope":"user:content:manage"}))).mount(&server).await;
        let b = broker.clone();
        let owner = id.clone();
        let refresh = tokio::spawn(async move { b.access(&owner, Scope::UserContentManage).await });
        tokio::time::sleep(Duration::from_millis(25)).await;
        broker.disconnect(&id).await.unwrap();
        refresh.await.unwrap().unwrap();
        assert!(broker.load(&id).is_err());
    }
    #[tokio::test]
    async fn profiles_do_not_share_rotation_replacement_or_logout() {
        let server = MockServer::start().await;
        let vault = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let id = AccountId::new("same-subject").unwrap();
        let mut a = TokenBroker::new("canonical-profile-a".into()).unwrap();
        a.host = server.uri();
        a.test_vault = Some(vault.clone());
        let mut b = TokenBroker::new("canonical-profile-b".into()).unwrap();
        b.host = server.uri();
        b.test_vault = Some(vault.clone());
        let grant = |access: &str, expired| Grant {
            access: access.into(),
            refresh: Some(format!("{access}-refresh")),
            scopes: vec![Scope::UserContentView],
            unknown_scopes: vec![],
            expires: if expired { 0 } else { now() + 3600 },
        };
        a.save(&id, &grant("a", true)).unwrap();
        b.save(&id, &grant("b", false)).unwrap();
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token":"a-rotated","refresh_token":"a-rotated-refresh","expires_in":3600,"token_type":"Bearer","scope":"user:content:view"}))).expect(1).mount(&server).await;
        assert_eq!(
            &*a.access(&id, Scope::UserContentView).await.unwrap(),
            "a-rotated"
        );
        assert_eq!(&*b.access(&id, Scope::UserContentView).await.unwrap(), "b");
        a.save(&id, &grant("a-replaced", false)).unwrap();
        assert_eq!(b.load(&id).unwrap().access, "b");
        a.disconnect(&id).await.unwrap();
        assert!(a.load(&id).is_err());
        assert_eq!(b.load(&id).unwrap().access, "b");
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "Creates and deletes synthetic records in the unlocked native vault"]
    async fn native_profile_grants_are_independent_and_cleaned() {
        struct Cleanup(Vec<keyring::Entry>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for entry in &self.0 {
                    let _ = entry.delete_credential();
                }
            }
        }
        let root = tempfile::tempdir().unwrap();
        let a_profile = crate::canonical_profile(&root.path().join("a")).unwrap();
        let b_profile = crate::canonical_profile(&root.path().join("b")).unwrap();
        let id = AccountId::new(format!("synthetic-test-{}", uuid::Uuid::new_v4())).unwrap();
        let server = MockServer::start().await;
        let mut a = TokenBroker::new(crate::profile_namespace(&a_profile)).unwrap();
        a.host = server.uri();
        let b = TokenBroker::new(crate::profile_namespace(&b_profile)).unwrap();
        let _cleanup = Cleanup(vec![a.entry(&id).unwrap(), b.entry(&id).unwrap()]);
        let grant = |access: &str, expires| Grant {
            access: access.into(),
            refresh: Some(format!("{access}-synthetic-refresh")),
            scopes: vec![Scope::UserContentView],
            unknown_scopes: vec![],
            expires,
        };
        a.save(&id, &grant("synthetic-a", 0)).unwrap();
        b.save(&id, &grant("synthetic-b", now() + 3600)).unwrap();
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token":"synthetic-rotated","refresh_token":"synthetic-rotation","expires_in":3600,"token_type":"Bearer","scope":"user:content:view"}))).expect(1).mount(&server).await;
        assert_eq!(
            &*a.access(&id, Scope::UserContentView).await.unwrap(),
            "synthetic-rotated"
        );
        a.save(&id, &grant("synthetic-replaced", now() + 3600))
            .unwrap();
        assert_eq!(b.load(&id).unwrap().access, "synthetic-b");
        a.disconnect(&id).await.unwrap();
        assert!(a.load(&id).is_err());
        assert_eq!(b.load(&id).unwrap().access, "synthetic-b");
    }
}
