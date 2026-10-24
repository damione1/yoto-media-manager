//! DPAPI-protected files, as used by MSAL and Git Credential Manager's DPAPI store.
//! The Windows user protects the data; no application-managed encryption key is needed.
//! References:
//! <https://learn.microsoft.com/en-us/entra/identity-platform/msal-node-extensions>
//! <https://github.com/git-ecosystem/git-credential-manager/blob/main/src/Core/Interop/Windows/DpapiCredentialStore.cs>
use crate::storage;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};
use yoto_domain::{AccountId, Failure, Result};
use zeroize::{Zeroize, Zeroizing};

pub(super) struct WindowsVault {
    root: PathBuf,
    namespace: String,
}

impl WindowsVault {
    pub(super) fn new(profile: &Path, namespace: String) -> Self {
        Self {
            root: profile.join("oauth"),
            namespace,
        }
    }

    fn path(&self, id: &AccountId) -> PathBuf {
        self.root.join(format!(
            "{}.dpapi",
            hex::encode(Sha256::digest(id.as_str()))
        ))
    }

    fn entropy(&self, id: &AccountId) -> Vec<u8> {
        // Bind the ciphertext to this app, canonical profile and account so copied
        // files cannot silently substitute another profile's or account's grant.
        format!("dev.yoto.desktop.oauth:{}:{id}", self.namespace).into_bytes()
    }

    pub(super) fn load(&self, id: &AccountId) -> Result<Option<Zeroizing<Vec<u8>>>> {
        let bytes = match fs::read(self.path(id)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(storage(e)),
        };
        // A corrupt file is an error, never a reason to resurrect a legacy grant.
        crypt(&bytes, &self.entropy(id), false).map(Some)
    }

    pub(super) fn save(&self, id: &AccountId, bytes: &[u8]) -> Result<()> {
        let encrypted = crypt(bytes, &self.entropy(id), true)?;
        fs::create_dir_all(&self.root).map_err(storage)?;
        // Encrypt before opening any file. Replace on the same filesystem only
        // after flushing, so rotation cannot leave a truncated or plaintext grant.
        let mut temp = tempfile::NamedTempFile::new_in(&self.root).map_err(storage)?;
        temp.write_all(&encrypted).map_err(storage)?;
        temp.as_file().sync_all().map_err(storage)?;
        temp.persist(self.path(id)).map_err(storage)?;
        Ok(())
    }

    pub(super) fn delete(&self, id: &AccountId) -> Result<()> {
        match fs::remove_file(self.path(id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(storage(e)),
        }
    }
}

fn crypt(bytes: &[u8], entropy: &[u8], protect: bool) -> Result<Zeroizing<Vec<u8>>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(storage)?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(entropy.len()).map_err(storage)?,
        pbData: entropy.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: Both input buffers remain alive for the call. DPAPI reads them and
    // initializes output on success. Null optional parameters request no UI or
    // description. Omitting LOCAL_MACHINE binds encryption to the current user.
    let success = unsafe {
        if protect {
            CryptProtectData(
                &input,
                ptr::null(),
                &entropy,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                &entropy,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if success == 0 {
        return Err(if protect {
            storage(std::io::Error::last_os_error())
        } else {
            Failure::Authentication
        });
    }
    // SAFETY: Successful DPAPI calls return cbData initialized bytes allocated
    // with LocalAlloc. Copy into zeroizing memory, wipe the native buffer, and
    // release it with LocalFree as required by DPAPI.
    let result = unsafe {
        let buffer = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let result = Zeroizing::new(buffer.to_vec());
        buffer.zeroize();
        LocalFree(output.pbData.cast());
        result
    };
    Ok(result)
}
