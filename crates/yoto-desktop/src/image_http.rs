use futures::{AsyncReadExt, future::BoxFuture};
use gpui_kit::http_client::{
    AsyncBody, HttpClient, RedirectPolicy, Request, RequestTimeout, Response, Url,
    http::HeaderValue,
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

/// GPUI's image loader runs outside Tokio; use the application's existing runtime.
/// This client is independent of OAuth and never adds account credentials to image requests.
pub(crate) struct ImageHttpClient {
    runtime: Arc<tokio::runtime::Runtime>,
    follow: reqwest::Client,
    no_follow: reqwest::Client,
    /// One client per redirect limit, built once: each client owns a connection pool.
    limited: std::sync::Mutex<BTreeMap<usize, reqwest::Client>>,
}
impl ImageHttpClient {
    pub(crate) fn new(runtime: Arc<tokio::runtime::Runtime>) -> reqwest::Result<Self> {
        Ok(Self {
            runtime,
            follow: client(reqwest::redirect::Policy::custom(|attempt| {
                attempt.follow()
            }))?,
            no_follow: client(reqwest::redirect::Policy::none())?,
            limited: std::sync::Mutex::new(BTreeMap::new()),
        })
    }
    fn limited(&self, limit: usize) -> reqwest::Result<reqwest::Client> {
        let mut clients = self
            .limited
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(client) = clients.get(&limit) {
            return Ok(client.clone());
        }
        let created = client(reqwest::redirect::Policy::limited(limit))?;
        clients.insert(limit, created.clone());
        Ok(created)
    }
}
fn client(policy: reqwest::redirect::Policy) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("yoto-media-manager")
        .redirect(policy)
        .timeout(Duration::from_secs(30))
        .build()
}
impl HttpClient for ImageHttpClient {
    fn user_agent(&self) -> Option<&HeaderValue> {
        None
    }
    fn proxy(&self) -> Option<&Url> {
        None
    }
    fn send(
        &self,
        request: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        const MAX_BYTES: usize = 16 * 1024 * 1024;
        let (head, body) = request.into_parts();
        let selected = match head.extensions.get::<RedirectPolicy>() {
            Some(RedirectPolicy::FollowAll) => Ok(self.follow.clone()),
            Some(RedirectPolicy::FollowLimit(limit)) => self.limited(*limit as usize),
            Some(RedirectPolicy::NoFollow) | None => Ok(self.no_follow.clone()),
        };
        let runtime = self.runtime.clone();
        Box::pin(async move {
            let mut bytes = Vec::new();
            body.take(MAX_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .await?;
            anyhow::ensure!(bytes.len() <= MAX_BYTES, "Image request exceeds size limit");
            let client = selected?;
            runtime
                .spawn(async move {
                    let mut request = client
                        .request(head.method, head.uri.to_string())
                        .headers(head.headers)
                        .body(bytes);
                    if let Some(timeout) = head.extensions.get::<RequestTimeout>() {
                        request = request.timeout(timeout.0);
                    }
                    let mut response = request.send().await?;
                    let mut builder = Response::builder()
                        .status(response.status())
                        .version(response.version());
                    *builder.headers_mut().expect("valid HTTP response") =
                        response.headers().clone();
                    let mut bytes = Vec::new();
                    while let Some(chunk) = response.chunk().await? {
                        anyhow::ensure!(
                            bytes.len() + chunk.len() <= MAX_BYTES,
                            "Image response exceeds size limit"
                        );
                        bytes.extend_from_slice(&chunk);
                    }
                    Ok::<_, anyhow::Error>(builder.body(AsyncBody::from(bytes))?)
                })
                .await?
        })
    }
}
