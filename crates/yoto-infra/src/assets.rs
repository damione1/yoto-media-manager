use crate::storage;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305,
    aead::stream::{DecryptorBE32, EncryptorBE32},
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use yoto_domain::*;
use zeroize::Zeroizing;
const CHUNK: usize = 64 * 1024;
const MAGIC: &[u8; 8] = b"YOTOAS01";
pub struct AssetStore {
    pub(crate) root: PathBuf,
    pub(crate) key: Zeroizing<[u8; 32]>,
    active: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
/// A bounded operation owns its immutable inputs and uncommitted outputs. GC
/// skips reclamation while such owners exist; it never blocks the operation.
pub(crate) struct AssetProtection(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl Drop for AssetProtection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}
pub struct Lease {
    file: tempfile::NamedTempFile,
}
impl Lease {
    pub fn path(&self) -> &Path {
        self.file.path()
    }
}
impl AssetStore {
    pub fn new(root: PathBuf, key: [u8; 32]) -> Result<Self> {
        for dir in [root.join("assets"), root.join("leases")] {
            fs::create_dir_all(&dir).map_err(storage)?;
            private(&dir)?;
        }
        for f in fs::read_dir(root.join("leases")).map_err(storage)? {
            let f = f.map_err(storage)?;
            if f.file_type().map_err(storage)?.is_file() {
                fs::remove_file(f.path()).map_err(storage)?;
            } else if f.file_type().map_err(storage)?.is_dir() {
                fs::remove_dir_all(f.path()).map_err(storage)?;
            }
        }
        Ok(Self {
            root,
            key: Zeroizing::new(key),
            active: Default::default(),
        })
    }
    pub(crate) fn protect(&self) -> AssetProtection {
        self.active
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        AssetProtection(self.active.clone())
    }
    pub(crate) fn has_active_owners(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::SeqCst) != 0
    }
    pub fn workspace(&self) -> Result<tempfile::TempDir> {
        let d = tempfile::tempdir_in(self.root.join("leases")).map_err(storage)?;
        private(d.path())?;
        Ok(d)
    }
    pub fn lease(&self, extension: &str) -> Result<Lease> {
        if !extension.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(Failure::Validation("Invalid extension".into()));
        }
        let file = tempfile::Builder::new()
            .suffix(&format!(".{extension}"))
            .tempfile_in(self.root.join("leases"))
            .map_err(storage)?;
        private(file.path())?;
        Ok(Lease { file })
    }
    fn path(&self, id: AssetId) -> PathBuf {
        self.root.join("assets").join(format!("{id}.yma"))
    }
    pub fn put(&self, path: &Path, extension: &str) -> Result<Asset> {
        let size = fs::metadata(path).map_err(storage)?.len();
        if size == 0 || size > 2_000_000_000 {
            return Err(Failure::Validation(
                "File must contain audio and be smaller than 2 GB.".into(),
            ));
        }
        let id = AssetId::new();
        let mut temp =
            tempfile::NamedTempFile::new_in(self.root.join("assets")).map_err(storage)?;
        private(temp.path())?;
        let mut nonce = [0u8; 19];
        rand::thread_rng().fill_bytes(&mut nonce);
        temp.write_all(MAGIC).map_err(storage)?;
        temp.write_all(&nonce).map_err(storage)?;
        temp.write_all(&size.to_be_bytes()).map_err(storage)?;
        let cipher = XChaCha20Poly1305::new((&*self.key).into());
        let mut enc = EncryptorBE32::from_aead(cipher, (&nonce).into());
        let mut input = File::open(path).map_err(storage)?;
        let mut digest = Sha256::new();
        let mut buf = vec![0; CHUNK];
        let mut left = size;
        while left > CHUNK as u64 {
            input.read_exact(&mut buf).map_err(storage)?;
            digest.update(&buf);
            let c = enc
                .encrypt_next(buf.as_slice())
                .map_err(|_| Failure::Storage("Asset encryption failed".into()))?;
            temp.write_all(&c).map_err(storage)?;
            left -= CHUNK as u64;
        }
        input
            .read_exact(&mut buf[..left as usize])
            .map_err(storage)?;
        digest.update(&buf[..left as usize]);
        let c = enc
            .encrypt_last(&buf[..left as usize])
            .map_err(|_| Failure::Storage("Asset encryption failed".into()))?;
        temp.write_all(&c).map_err(storage)?;
        temp.as_file().sync_all().map_err(storage)?;
        temp.persist(self.path(id)).map_err(storage)?;
        Ok(Asset {
            id,
            sha256: LocalSha256::new(hex::encode(digest.finalize()))?,
            bytes: size,
            extension: extension.into(),
        })
    }
    pub fn materialize(&self, asset: &Asset) -> Result<Lease> {
        let lease = self.lease(&asset.extension)?;
        let mut output = lease.file.reopen().map_err(storage)?;
        let mut input = File::open(self.path(asset.id)).map_err(storage)?;
        let mut magic = [0; 8];
        input.read_exact(&mut magic).map_err(storage)?;
        if &magic != MAGIC {
            return Err(Failure::Storage("Unsupported asset version".into()));
        }
        let mut nonce = [0; 19];
        input.read_exact(&mut nonce).map_err(storage)?;
        let mut size = [0; 8];
        input.read_exact(&mut size).map_err(storage)?;
        let mut left = u64::from_be_bytes(size);
        if left != asset.bytes {
            return Err(Failure::Storage("Asset length mismatch".into()));
        }
        let cipher = XChaCha20Poly1305::new((&*self.key).into());
        let mut dec = DecryptorBE32::from_aead(cipher, (&nonce).into());
        let mut digest = Sha256::new();
        let mut buf = vec![0; CHUNK + 16];
        while left > CHUNK as u64 {
            input.read_exact(&mut buf).map_err(storage)?;
            let p = dec
                .decrypt_next(buf.as_slice())
                .map_err(|_| Failure::Storage("Asset authentication failed".into()))?;
            digest.update(&p);
            output.write_all(&p).map_err(storage)?;
            left -= CHUNK as u64;
        }
        input
            .read_exact(&mut buf[..left as usize + 16])
            .map_err(storage)?;
        let p = dec
            .decrypt_last(&buf[..left as usize + 16])
            .map_err(|_| Failure::Storage("Asset authentication failed".into()))?;
        digest.update(&p);
        output.write_all(&p).map_err(storage)?;
        if input.read(&mut [0u8; 1]).map_err(storage)? != 0
            || hex::encode(digest.finalize()) != asset.sha256.as_str()
        {
            return Err(Failure::Storage("Asset integrity mismatch".into()));
        }
        output.flush().map_err(storage)?;
        Ok(lease)
    }
    pub fn export(&self, asset: &Asset, destination: &Path) -> Result<()> {
        let lease = self.materialize(asset)?;
        let mut dest = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(storage)?;
        std::io::copy(&mut File::open(lease.path()).map_err(storage)?, &mut dest)
            .map_err(storage)?;
        dest.sync_all().map_err(storage)
    }
    pub fn inventory(&self) -> Result<Vec<AssetId>> {
        std::fs::read_dir(self.root.join("assets"))
            .map_err(storage)?
            .filter_map(|entry| match entry {
                Ok(entry) => entry
                    .path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| {
                        serde_json::from_value::<AssetId>(serde_json::Value::String(s.into())).ok()
                    })
                    .map(Ok),
                Err(error) => Some(Err(storage(error))),
            })
            .collect()
    }
    pub fn delete(&self, id: AssetId) -> Result<()> {
        fs::remove_file(self.path(id)).map_err(storage)
    }
}
pub(crate) fn private(path: &Path) -> Result<()> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(storage)?;
    }
    Ok(())
}
pub fn library_key(profile: &Path) -> Result<[u8; 32]> {
    let canonical = crate::canonical_profile(profile)?;
    let profile = canonical.as_path();
    let namespace = crate::profile_namespace(profile);
    let entry = keyring::Entry::new("dev.yoto.desktop.library", &namespace).map_err(storage)?;
    match entry.get_secret() {
        Ok(value) => {
            let mut key = [0u8; 32];
            if value.len() != 32 {
                return Err(Failure::Storage("Invalid library key in OS vault".into()));
            }
            key.copy_from_slice(&value);
            Ok(key)
        }
        Err(keyring::Error::NoEntry) => {
            let mut key = [0; 32];
            rand::thread_rng().fill_bytes(&mut key);
            entry.set_secret(&key).map_err(storage)?;
            Ok(key)
        }
        Err(e) => Err(storage(e)),
    }
}

pub(crate) fn set_library_key(profile: &Path, key: &[u8; 32]) -> Result<()> {
    keyring::Entry::new(
        "dev.yoto.desktop.library",
        &crate::profile_namespace(profile),
    )
    .map_err(storage)?
    .set_secret(key)
    .map_err(storage)
}
