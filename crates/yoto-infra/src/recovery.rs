//! Complete encrypted catalogue backup. Manifest authentication binds database and asset hashes.
use crate::{assets::AssetStore, db::Database, storage};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305,
    aead::{Aead, Payload},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, path::Path};
use yoto_domain::*;
use zeroize::Zeroizing;
const MAGIC: &[u8] = b"YOTOBK01";
#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    schema: u32,
    key: String,
    files: BTreeMap<String, String>,
}
fn recovery_key(text: &str) -> Result<Zeroizing<[u8; 32]>> {
    let decoded = Zeroizing::new(
        hex::decode(text).map_err(|_| Failure::Validation(messages::RECOVERY_KEY_FORMAT.into()))?,
    );
    if decoded.len() != 32 {
        return Err(Failure::Validation(messages::RECOVERY_KEY_LENGTH.into()));
    }
    let mut key = Zeroizing::new([0; 32]);
    key.copy_from_slice(&decoded);
    Ok(key)
}
fn checksum(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).map_err(storage)?;
    let mut digest = Sha256::new();
    let mut bytes = [0; 65536];
    loop {
        let n = file.read(&mut bytes).map_err(storage)?;
        if n == 0 {
            break;
        }
        digest.update(&bytes[..n]);
    }
    Ok(hex::encode(digest.finalize()))
}
fn finish(
    root: &Path,
    assets: &Path,
    key: &[u8; 32],
    recovery: &[u8; 32],
    schema: u32,
) -> Result<()> {
    std::fs::create_dir(root.join("assets")).map_err(storage)?;
    let mut files = BTreeMap::new();
    files.insert("catalogue.db".into(), checksum(&root.join("catalogue.db"))?);
    if assets.exists() {
        for entry in std::fs::read_dir(assets).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            if !entry.file_type().map_err(storage)?.is_file()
                || entry.path().extension().is_none_or(|s| s != "yma")
            {
                continue;
            }
            let name = format!("assets/{}", entry.file_name().to_string_lossy());
            let target = root.join(&name);
            std::fs::copy(entry.path(), &target).map_err(storage)?;
            files.insert(name, checksum(&target)?);
        }
    }
    let mut manifest = Manifest {
        version: 1,
        schema,
        key: hex::encode(key),
        files,
    };
    let data = Zeroizing::new(serde_json::to_vec(&manifest).map_err(storage)?);
    use zeroize::Zeroize;
    manifest.key.zeroize();
    let mut nonce = [0; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let cipher = XChaCha20Poly1305::new(recovery.into());
    let encrypted = cipher
        .encrypt(
            (&nonce).into(),
            Payload {
                msg: &data,
                aad: MAGIC,
            },
        )
        .map_err(|_| Failure::Storage("Backup manifest encryption failed".into()))?;
    let mut bytes = MAGIC.to_vec();
    bytes.extend(nonce);
    bytes.extend(encrypted);
    std::fs::write(root.join("manifest.ybk"), bytes).map_err(storage)?;
    crate::assets::private(root)?;
    crate::assets::private(&root.join("assets"))?;
    verify(root, recovery)?;
    let db = rusqlite::Connection::open(root.join("catalogue.db")).map_err(storage)?;
    db.pragma_update(None, "key", format!("x'{}'", hex::encode(key)))
        .map_err(storage)?;
    let integrity: String = db
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(storage)?;
    if integrity != "ok" {
        return Err(Failure::Storage(messages::BACKUP_DATABASE_INVALID.into()));
    }
    Ok(())
}
fn verify(source: &Path, recovery: &[u8; 32]) -> Result<Zeroizing<[u8; 32]>> {
    let path = source.join("manifest.ybk");
    if std::fs::metadata(&path).map_err(storage)?.len() > 16_000_000 {
        return Err(Failure::Validation("Backup manifest too large".into()));
    }
    let bytes = std::fs::read(path).map_err(storage)?;
    if bytes.len() < 48 || &bytes[..8] != MAGIC {
        return Err(Failure::Compatibility(messages::BACKUP_UNSUPPORTED.into()));
    }
    let cipher = XChaCha20Poly1305::new(recovery.into());
    let data = Zeroizing::new(
        cipher
            .decrypt(
                (&bytes[8..32]).into(),
                Payload {
                    msg: &bytes[32..],
                    aad: MAGIC,
                },
            )
            .map_err(|_| Failure::Validation(messages::BACKUP_AUTH_FAILED.into()))?,
    );
    let mut manifest: Manifest = serde_json::from_slice(&data).map_err(storage)?;
    if manifest.version != 1 || manifest.schema > 2 {
        return Err(Failure::Compatibility(messages::BACKUP_NEWER.into()));
    }
    let key = recovery_key(&manifest.key)?;
    use zeroize::Zeroize;
    manifest.key.zeroize();
    let root = source.canonicalize().map_err(storage)?;
    for (name, digest) in &manifest.files {
        let relative = Path::new(name);
        if !relative
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
            || (name != "catalogue.db" && !name.starts_with("assets/"))
        {
            return Err(Failure::Validation(messages::BACKUP_PATH_INVALID.into()));
        }
        let path = root.join(name);
        if !path.canonicalize().map_err(storage)?.starts_with(&root)
            || std::fs::symlink_metadata(&path)
                .map_err(storage)?
                .file_type()
                .is_symlink()
            || checksum(&path)? != *digest
        {
            return Err(Failure::Storage(messages::BACKUP_INTEGRITY.into()));
        }
    }
    for entry in std::fs::read_dir(root.join("assets")).map_err(storage)? {
        let entry = entry.map_err(storage)?;
        let name = format!("assets/{}", entry.file_name().to_string_lossy());
        if !entry.file_type().map_err(storage)?.is_file() || !manifest.files.contains_key(&name) {
            return Err(Failure::Storage("Unlisted backup asset".into()));
        }
    }
    if !manifest.files.contains_key("catalogue.db") {
        return Err(Failure::Validation(messages::BACKUP_DATABASE_ABSENT.into()));
    }
    Ok(key)
}
pub fn backup(db: &Database, store: &AssetStore, destination: &Path, recovery: &str) -> Result<()> {
    if destination.exists() {
        return Err(Failure::Validation(
            messages::BACKUP_DESTINATION_EXISTS.into(),
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| Failure::Validation(messages::BACKUP_NEEDS_PARENT.into()))?;
    std::fs::create_dir_all(parent).map_err(storage)?;
    let temp = tempfile::tempdir_in(parent).map_err(storage)?;
    crate::assets::private(temp.path())?;
    db.backup(&temp.path().join("catalogue.db"), &store.key)?;
    finish(
        temp.path(),
        &store.root.join("assets"),
        &store.key,
        &*recovery_key(recovery)?,
        2,
    )?;
    std::fs::rename(temp.path(), destination).map_err(storage)?;
    Ok(())
}
pub fn migration_backup(
    connection: &rusqlite::Connection,
    path: &Path,
    key: &[u8; 32],
    schema: u32,
) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Failure::Storage("Catalogue has no directory".into()))?;
    let backups = parent.join("backups");
    std::fs::create_dir_all(&backups).map_err(storage)?;
    crate::assets::private(&backups)?;
    let temp = tempfile::tempdir_in(&backups).map_err(storage)?;
    let mut out = rusqlite::Connection::open(temp.path().join("catalogue.db")).map_err(storage)?;
    out.pragma_update(None, "key", format!("x'{}'", hex::encode(key)))
        .map_err(storage)?;
    rusqlite::backup::Backup::new(connection, &mut out)
        .map_err(storage)?
        .run_to_completion(128, std::time::Duration::from_millis(10), None)
        .map_err(storage)?;
    drop(out);
    finish(temp.path(), &parent.join("assets"), key, key, schema)?;
    std::fs::rename(
        temp.path(),
        backups.join(format!("schema-{schema}-{}", uuid::Uuid::new_v4())),
    )
    .map_err(storage)?;
    Ok(())
}
pub fn restore(
    source: &Path,
    destination: &Path,
    recovery: &str,
    install_vault: bool,
) -> Result<()> {
    if destination.exists() {
        return Err(Failure::Validation(
            messages::RESTORE_DESTINATION_EXISTS.into(),
        ));
    }
    let recovery = recovery_key(recovery)?;
    let key = verify(source, &recovery)?;
    let parent = destination
        .parent()
        .ok_or_else(|| Failure::Validation(messages::RESTORE_NEEDS_PARENT.into()))?;
    std::fs::create_dir_all(parent).map_err(storage)?;
    let temp = tempfile::tempdir_in(parent).map_err(storage)?;
    crate::assets::private(temp.path())?;
    std::fs::create_dir(temp.path().join("assets")).map_err(storage)?;
    std::fs::copy(
        source.join("catalogue.db"),
        temp.path().join("catalogue.db"),
    )
    .map_err(storage)?;
    for entry in std::fs::read_dir(source.join("assets")).map_err(storage)? {
        let entry = entry.map_err(storage)?;
        if entry.file_type().map_err(storage)?.is_file() {
            std::fs::copy(
                entry.path(),
                temp.path().join("assets").join(entry.file_name()),
            )
            .map_err(storage)?;
        }
    }
    let db = Database::open(&temp.path().join("catalogue.db"), &key)?;
    db.verify()?;
    let store = AssetStore::new(temp.path().to_owned(), *key)?;
    for asset in db.list::<Asset>(crate::db::Table::Asset)? {
        drop(store.materialize(&asset)?);
    }
    drop(db);
    drop(store);
    std::fs::rename(temp.path(), destination).map_err(storage)?;
    if install_vault
        && let Err(error) =
            crate::assets::set_library_key(&crate::canonical_profile(destination)?, &key)
    {
        std::fs::remove_dir_all(destination).map_err(storage)?;
        return Err(error);
    }
    Ok(())
}
