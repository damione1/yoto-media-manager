//! Local authenticated IPC. This process is the sole catalogue writer; proxies never open SQLite.
use crate::{Command, Service, storage};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use yoto_domain::*;
use zeroize::Zeroizing;
#[derive(Serialize, Deserialize)]
pub enum Request {
    Execute(Command),
    Snapshot,
    Health,
    InterfaceChunk {
        since: Vec<u64>,
        revision: Option<String>,
        offset: usize,
    },
    SnapshotChunk {
        revision: Option<String>,
        offset: usize,
    },
}
#[derive(Serialize, Deserialize)]
pub struct SnapshotChunk {
    pub revision: String,
    pub data: String,
    pub next: Option<usize>,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    credential: String,
    request: Request,
}
#[derive(Serialize, Deserialize)]
struct Response {
    result: Result<serde_json::Value>,
}
const MAX: usize = 4_000_000;
fn namespace(profile: &Path) -> String {
    crate::profile_namespace(profile)
}
fn endpoint(profile: &Path) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // sockaddr_un is much shorter than a filesystem path. The catalogue keeps
        // its lifetime lock; only the authenticated endpoint lives in this directory.
        let uid = unsafe { libc::geteuid() };
        let runtime = PathBuf::from(format!("/tmp/yoto-ipc-{uid}"));
        match std::fs::create_dir(&runtime) {
            Ok(()) => std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700))
                .map_err(storage)?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(Failure::Storage(format!(
                    "Cannot create private IPC directory {}: {error}",
                    runtime.display()
                )));
            }
        }
        let metadata = std::fs::symlink_metadata(&runtime).map_err(storage)?;
        if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o777 != 0o700 {
            return Err(Failure::Storage(format!(
                "IPC directory {} must be a private directory owned by this user (mode 0700)",
                runtime.display()
            )));
        }
        Ok(runtime.join(format!("{}.sock", namespace(profile))))
    }
    #[cfg(windows)]
    {
        Ok(PathBuf::from(format!(
            r"\\.\pipe\yoto-{}",
            namespace(profile)
        )))
    }
}
fn credential(profile: &Path, isolated: bool, create: bool) -> Result<Zeroizing<String>> {
    if isolated && cfg!(debug_assertions) {
        return Ok(Zeroizing::new(hex::encode([41; 32])));
    }
    let entry =
        keyring::Entry::new("dev.yoto.desktop.ipc", &namespace(profile)).map_err(storage)?;
    match entry.get_password() {
        Ok(s) => Ok(Zeroizing::new(s)),
        Err(keyring::Error::NoEntry) if create => {
            let mut bytes = [0; 32];
            rand::thread_rng().fill_bytes(&mut bytes);
            let token = Zeroizing::new(hex::encode(bytes));
            entry.set_password(&token).map_err(storage)?;
            Ok(token)
        }
        Err(keyring::Error::NoEntry) => Err(Failure::Connection),
        Err(e) => Err(storage(e)),
    }
}
async fn write<T: Serialize, W: AsyncWrite + Unpin>(stream: &mut W, data: &T) -> Result<()> {
    let bytes = serde_json::to_vec(data).map_err(storage)?;
    if bytes.len() > MAX {
        return Err(Failure::Validation("IPC message exceeds 4 MB".into()));
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        stream
            .write_u32(bytes.len() as u32)
            .await
            .map_err(storage)?;
        stream.write_all(&bytes).await.map_err(storage)?;
        stream.flush().await.map_err(storage)
    })
    .await
    .map_err(|_| Failure::Connection)?
}
async fn read<T: serde::de::DeserializeOwned, R: AsyncRead + Unpin>(stream: &mut R) -> Result<T> {
    let len = stream.read_u32().await.map_err(storage)? as usize;
    if len > MAX {
        return Err(Failure::Validation("IPC message exceeds 4 MB".into()));
    }
    let mut bytes = Zeroizing::new(vec![0; len]);
    stream.read_exact(&mut bytes).await.map_err(storage)?;
    serde_json::from_slice(&bytes).map_err(storage)
}
async fn connection<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    service: Arc<Service>,
    secret: Arc<Zeroizing<String>>,
) -> Result<()> {
    let envelope: Envelope =
        tokio::time::timeout(std::time::Duration::from_secs(5), read(&mut stream))
            .await
            .map_err(|_| Failure::Connection)??;
    // Constant-time comparison for the fixed-size local credential.
    let a = envelope.credential.as_bytes();
    let b = secret.as_bytes();
    let equal = a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (a, b)| diff | (a ^ b)) == 0;
    if !equal {
        return Err(Failure::Authentication);
    }
    let result = match envelope.request {
        Request::Execute(command) => service
            .execute(command)
            .await
            .and_then(|v| serde_json::to_value(v).map_err(storage)),
        Request::Health => service
            .revision()
            .map(|revision| serde_json::json!({"revision":revision})),
        Request::InterfaceChunk {
            since,
            revision,
            offset,
        } => tokio::task::spawn_blocking(move || service.interface_chunk(since, revision, offset))
            .await
            .map_err(storage)?
            .and_then(|v| serde_json::to_value(v).map_err(storage)),
        Request::SnapshotChunk { revision, offset } => {
            tokio::task::spawn_blocking(move || service.snapshot_chunk(revision, offset))
                .await
                .map_err(storage)?
                .and_then(|s| serde_json::to_value(s).map_err(storage))
        }
        Request::Snapshot => tokio::task::spawn_blocking(move || service.snapshot())
            .await
            .map_err(storage)?
            .and_then(|s| serde_json::to_value(s).map_err(storage)),
    }
    .map(sanitize);
    write(&mut stream, &Response { result }).await
}
pub fn start(
    service: Arc<Service>,
    profile: PathBuf,
    isolated: bool,
) -> Result<tokio::task::JoinHandle<()>> {
    let profile = crate::canonical_profile(&profile)?;
    if service.db.path.parent() != Some(profile.as_path()) {
        return Err(Failure::Storage(
            "IPC owner and catalogue profile differ".into(),
        ));
    }
    let secret = Arc::new(credential(&profile, isolated, true)?);
    let path = endpoint(&profile)?;
    #[cfg(unix)]
    {
        if path.exists() {
            std::fs::remove_file(&path).map_err(storage)?;
        }
        let listener = tokio::net::UnixListener::bind(&path).map_err(storage)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(storage)?;
        Ok(tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let service = service.clone();
                let secret = secret.clone();
                tokio::spawn(async move {
                    let _ = connection(stream, service, secret).await;
                });
            }
        }))
    }
    #[cfg(windows)]
    {
        // Local named pipe, remote clients rejected; OS credential is available only to this user.
        use tokio::net::windows::named_pipe::ServerOptions;
        let mut first = Some(
            ServerOptions::new()
                .first_pipe_instance(true)
                .reject_remote_clients(true)
                .create(&path)
                .map_err(storage)?,
        );
        Ok(tokio::spawn(async move {
            loop {
                let server = match first.take() {
                    Some(s) => s,
                    None => match ServerOptions::new()
                        .reject_remote_clients(true)
                        .create(&path)
                    {
                        Ok(s) => s,
                        Err(_) => break,
                    },
                };
                if server.connect().await.is_err() {
                    break;
                }
                let service = service.clone();
                let secret = secret.clone();
                tokio::spawn(async move {
                    let _ = connection(server, service, secret).await;
                });
            }
        }))
    }
}
pub async fn call(profile: &Path, isolated: bool, request: Request) -> Result<serde_json::Value> {
    let profile = crate::canonical_profile(profile)?;
    let profile = profile.as_path();
    let token = credential(profile, isolated, false)?;
    #[cfg(unix)]
    let mut stream = tokio::net::UnixStream::connect(endpoint(profile)?)
        .await
        .map_err(|_| Failure::Connection)?;
    #[cfg(windows)]
    let mut stream = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(endpoint(profile)?)
        .map_err(|_| Failure::Connection)?;
    write(
        &mut stream,
        &Envelope {
            credential: token.to_string(),
            request,
        },
    )
    .await?;
    let response: Response =
        tokio::time::timeout(std::time::Duration::from_secs(660), read(&mut stream))
            .await
            .map_err(|_| Failure::Connection)??;
    response.result
}
/// Defensive boundary for extensible server documents; credentials and signed capabilities never leave MCP.
pub fn sanitize(mut value: serde_json::Value) -> serde_json::Value {
    match &mut value {
        serde_json::Value::Object(map) => {
            for (key, v) in map {
                let lower = key.to_ascii_lowercase();
                if [
                    "token",
                    "access_token",
                    "refresh_token",
                    "authorization",
                    "uploadurl",
                    "signedurl",
                    "password",
                ]
                .iter()
                .any(|s| lower == *s)
                {
                    *v = serde_json::Value::String("[redacted]".into());
                } else {
                    *v = sanitize(v.take());
                }
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                *v = sanitize(v.take());
            }
        }
        serde_json::Value::String(s) => {
            if let Ok(url) = url::Url::parse(s)
                && url.query_pairs().any(|(key, _)| {
                    let k = key.to_ascii_lowercase();
                    k.contains("signature") || k.contains("credential") || k == "token"
                })
            {
                *s = "[redacted signed URL]".into();
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
    value
}

pub(crate) fn chunk(revision: &str, bytes: &[u8], offset: usize) -> Result<SnapshotChunk> {
    if offset > bytes.len() {
        return Err(Failure::Validation(
            "Snapshot page offset is invalid".into(),
        ));
    }
    let end = offset.saturating_add(256_000).min(bytes.len());
    use base64::Engine;
    Ok(SnapshotChunk {
        revision: revision.into(),
        data: base64::engine::general_purpose::STANDARD.encode(&bytes[offset..end]),
        next: (end < bytes.len()).then_some(end),
    })
}
