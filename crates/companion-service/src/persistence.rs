use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::Zeroize;

mod sqlite;
pub use sqlite::{NativeSealer,SealedSqliteStore};

pub const STATE_BYTES: usize = 8 * 1024 * 1024;
pub trait StateStorage: Send + Sync {
    fn read(&self) -> Result<Option<Vec<u8>>, String>;
    fn write(&self, bytes: &[u8]) -> Result<(), String>;
    /// Explicitly supplied adapters are trusted native boundaries, never web storage.
    fn available(&self) -> Result<(), String> {
        Ok(())
    }
    fn delete(&self) -> Result<(), String> {
        Err("This protected store does not support deletion.".into())
    }
}

/// Caller provides a fixed application-owned leaf path. Each failed replacement preserves
/// the old committed file. No recursive operation or plaintext secret file is used.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > STATE_BYTES {
        return Err("Protected companion state exceeds budget.".into());
    }
    let parent = path
        .parent()
        .ok_or("Companion state needs an application directory.")?;
    fs::create_dir_all(parent).map_err(|e| format!("Cannot prepare companion state: {e}"))?;
    if let Ok(meta) = fs::symlink_metadata(path) {
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("Refusing to replace an indirect companion state path.".into());
        }
    }
    let temporary = parent.join(format!(".companion-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|e| format!("Cannot commit companion state: {e}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn protect(bytes: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    if bytes.is_empty() || bytes.len() > STATE_BYTES {
        return Err("Invalid protected companion payload size.".into());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        if decrypt {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err(format!(
            "Windows could not protect/unprotect companion state: {}",
            std::io::Error::last_os_error()
        ));
    }
    let value =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize).zeroize();
        LocalFree(output.pbData as _);
    }
    Ok(value)
}
/// AES-GCM envelope version, authenticated as associated data. Keys never enter files.
#[cfg(any(test, target_os = "linux", target_os = "macos", target_os = "ios"))]
const KEYRING_MAGIC: &[u8; 8] = b"OSGSEC01";
#[cfg(any(test, target_os = "linux", target_os = "macos", target_os = "ios"))]
const OVERHEAD: usize = 8 + 12 + 16;
#[cfg(any(test, target_os = "linux", target_os = "macos", target_os = "ios"))]
fn encrypt_with_key(bytes: &[u8], key: &[u8], nonce: [u8; 12]) -> Result<Vec<u8>, String> {
    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
    if bytes.is_empty() || bytes.len() > STATE_BYTES - OVERHEAD {
        return Err("Invalid protected companion payload size.".into());
    }
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, key).map_err(|_| "Invalid native encryption key.")?,
    );
    let mut encrypted = zeroize::Zeroizing::new(bytes.to_vec());
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(KEYRING_MAGIC),
        &mut *encrypted,
    )
    .map_err(|_| "Native state encryption failed.")?;
    let mut envelope = Vec::with_capacity(OVERHEAD + bytes.len());
    envelope.extend_from_slice(KEYRING_MAGIC);
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&encrypted);
    encrypted.zeroize();
    Ok(envelope)
}
#[cfg(any(test, target_os = "linux", target_os = "macos", target_os = "ios"))]
fn decrypt_with_key(bytes: &[u8], key: &[u8]) -> Result<Vec<u8>, String> {
    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
    if bytes.len() <= OVERHEAD || bytes.len() > STATE_BYTES || &bytes[..8] != KEYRING_MAGIC {
        return Err("Invalid protected companion envelope.".into());
    }
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, key).map_err(|_| "Invalid native encryption key.")?,
    );
    let nonce: [u8; 12] = bytes[8..20].try_into().unwrap();
    let mut encrypted = zeroize::Zeroizing::new(bytes[20..].to_vec());
    match key.open_in_place(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(KEYRING_MAGIC),
        &mut *encrypted,
    ) {
        Ok(plain) => {
            let result = plain.to_vec();
            encrypted.zeroize();
            Ok(result)
        }
        Err(_) => {
            encrypted.zeroize();
            Err("Native protected state authentication failed; no reset or plaintext fallback was attempted.".into())
        }
    }
}
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
fn native_key(create: bool) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
    // Serialize both in-process RPC access and first-key creation. Desktop processes
    // also take an OS lock so concurrent first runs cannot overwrite each other's key.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _serial = SERIAL
        .lock()
        .map_err(|_| "Native protected-store lock poisoned.")?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let _process = key_creation_lock()?;
    let entry = keyring::Entry::new("org.opensimgear.companion.native-state", "aes256-gcm-v1")
        .map_err(|_| "Cannot initialize native Keychain/Secret Service entry.")?;
    let key = match entry.get_secret() {
        Ok(key) => key,
        Err(keyring::Error::NoEntry) if create => {
            let mut key = zeroize::Zeroizing::new(vec![0u8;32]);
            getrandom::fill(&mut key).map_err(|_| "Secure native randomness is unavailable.")?;
            entry.set_secret(&key).map_err(|_| "Native Keychain/Secret Service is locked or unavailable; no plaintext fallback is permitted.")?;
            let verified = entry.get_secret().map_err(|_| "Cannot verify the stored native encryption key.")?;
            if verified.as_slice() != key.as_slice() { return Err("Native encryption key changed during creation.".into()); }
            verified
        },
        Err(keyring::Error::NoEntry) => return Err("The native encryption key is missing; restore the original Keychain/Secret Service or explicitly forget and pair again.".into()),
        Err(_) => return Err("Native Keychain/Secret Service is locked or unavailable; no plaintext fallback is permitted.".into()),
    };
    let key = zeroize::Zeroizing::new(key);
    if key.len() != 32 {
        return Err("Native encryption key is invalid; it was not overwritten.".into());
    }
    Ok(key)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn key_creation_lock() -> Result<fs::File, String> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("Native protected storage needs the current user's home directory.")?;
    #[cfg(target_os = "linux")]
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    #[cfg(target_os = "macos")]
    let root = home.join("Library/Application Support");
    let directory = root.join("org.opensimgear.companion-native-state");
    fs::create_dir_all(&directory)
        .map_err(|_| "Cannot prepare the per-user protected-store lock directory.")?;
    let metadata = fs::symlink_metadata(&directory)
        .map_err(|_| "Cannot inspect the protected-store lock directory.")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Refusing an indirect protected-store lock directory.".into());
    }
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|_| "Cannot restrict the protected-store lock directory.")?;
    let path = directory.join("key-creation.lock");
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("Refusing an indirect protected-store lock file.".into());
        }
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .open(&path)
        .map_err(|_| "Cannot open the native protected-store lock.")?;
    fs2::FileExt::lock_exclusive(&file)
        .map_err(|_| "Cannot lock native protected-store key creation.")?;
    Ok(file)
}
/// Probes the real native service, never a compile-time platform claim.
pub fn available() -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut plain = protect(b"opensimgear-protected-store-probe", false)
            .and_then(|cipher| protect(&cipher, true))?;
        plain.zeroize();
        Ok(())
    }
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
    {
        native_key(true).map(|_| ())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "ios")))]
    {
        Err(
            "No native protected-store adapter is installed; no plaintext fallback is permitted."
                .into(),
        )
    }
}
pub fn seal(bytes: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        protect(bytes, false)
    }
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
    {
        let key = native_key(true)?;
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|_| "Secure native randomness is unavailable.")?;
        encrypt_with_key(bytes, &key, nonce)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "ios")))]
    {
        let _ = bytes;
        Err(
            "No native protected-store adapter is installed; no plaintext fallback is permitted."
                .into(),
        )
    }
}
pub fn unseal(bytes: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        protect(bytes, true)
    }
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
    {
        let key = native_key(false)?;
        decrypt_with_key(bytes, &key)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "ios")))]
    {
        let _ = bytes;
        Err(
            "No native protected-store adapter is installed; no plaintext fallback is permitted."
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_envelopes_bind_key_header_nonce_and_ciphertext() {
        let key = [7u8; 32];
        let data = b"protected identity credential and trust";
        let sealed = encrypt_with_key(data, &key, [1u8; 12]).unwrap();
        assert!(!sealed.windows(data.len()).any(|bytes| bytes == data));
        assert_eq!(decrypt_with_key(&sealed, &key).unwrap(), data);
        assert_ne!(sealed, encrypt_with_key(data, &key, [2u8; 12]).unwrap());
        assert!(decrypt_with_key(&sealed, &[8u8; 32]).is_err());
        for at in [0, 8, 20, sealed.len() - 1] {
            let mut corrupt = sealed.clone();
            corrupt[at] ^= 1;
            assert!(decrypt_with_key(&corrupt, &key).is_err());
        }
        assert!(encrypt_with_key(data, &[0; 31], [0; 12]).is_err());
        assert!(decrypt_with_key(&sealed, &[0; 31]).is_err());
        assert!(decrypt_with_key(b"plaintext", &key).is_err());
        assert!(decrypt_with_key(&sealed[..OVERHEAD], &key).is_err());
    }
    #[test]
    fn envelope_capacity_and_native_store_deletion_fail_closed() {
        let key = [4u8; 32];
        assert!(encrypt_with_key(&[], &key, [0; 12]).is_err());
        let maximum = vec![2u8; STATE_BYTES - OVERHEAD];
        let sealed = encrypt_with_key(&maximum, &key, [3; 12]).unwrap();
        assert_eq!(sealed.len(), STATE_BYTES);
        assert_eq!(decrypt_with_key(&sealed, &key).unwrap(), maximum);
        assert!(encrypt_with_key(&vec![2; STATE_BYTES - OVERHEAD + 1], &key, [3; 12]).is_err());
        assert!(decrypt_with_key(&vec![0; STATE_BYTES + 1], &key).is_err());
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sealed");
        let store = SealedSqliteStore::new(&path);
        store.delete().unwrap();
        fs::write(&path, b"encrypted").unwrap();
        store.delete().unwrap();
        assert_eq!(fs::read(&path).unwrap(),b"encrypted","Retired files are never deleted or imported.");
        assert!(SealedSqliteStore::new(root.path()).delete().is_err());
    }
    #[test]
    fn atomic_commit_replaces_only_valid_leafs_and_bounds_data() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("private/state");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert!(atomic_write(&path, &vec![0; STATE_BYTES + 1]).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert!(atomic_write(root.path(), b"no").is_err());
        let store = SealedSqliteStore::new(root.path().join("missing"));
        assert!(store.read().unwrap().is_none());
        assert!(SealedSqliteStore::new(root.path()).read().is_err());
    }
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
    #[test]
    #[ignore = "Requires an explicitly supplied unlocked native Keychain/Secret Service session"]
    fn real_native_store_retains_the_key_across_reopen_and_rejects_tampered_ciphertext() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native-state.sealed");
        let store = SealedSqliteStore::new(&path);
        store
            .available()
            .expect("native OS store must be unlocked; this check cannot pass by skipping");
        let private = b"native companion authority and host trust";
        store.write(private).unwrap();
        let database=osg_persistence::Database::for_namespace(&path).unwrap();
        let key=path.file_name().unwrap().to_str().unwrap();
        let sealed = database.read("secrets",key).unwrap().unwrap().payload;
        assert!(!sealed.windows(private.len()).any(|value| value == private));
        assert_eq!(
            SealedSqliteStore::new(&path).read().unwrap().unwrap(),
            private
        );
        let mut corrupt = sealed.clone();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        database.commit(&[osg_persistence::Change{domain:"secrets",key,expected_revision:database.revision("secrets",key).unwrap(),payload:Some(&corrupt)}]).unwrap();
        assert!(store.read().is_err());
        assert_eq!(database.read("secrets",key).unwrap().unwrap().payload, corrupt);
        database.commit(&[osg_persistence::Change{domain:"secrets",key,expected_revision:database.revision("secrets",key).unwrap(),payload:Some(&sealed)}]).unwrap();
        assert_eq!(store.read().unwrap().unwrap(), private);
        store.delete().unwrap();
        assert!(store.read().unwrap().is_none());
    }
    #[cfg(windows)]
    #[test]
    fn dpapi_user_roundtrip_never_writes_plaintext_and_tampering_fails() {
        let data = b"native private identity and installation credential";
        let sealed = seal(data).unwrap();
        assert_ne!(sealed, data);
        assert_eq!(unseal(&sealed).unwrap(), data);
        assert!(seal(&[]).is_err());
        assert!(unseal(b"invalid").is_err());
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("secret.dpapi");
        let store = SealedSqliteStore::new(&path);
        store.write(data).unwrap();
        assert_eq!(store.read().unwrap().unwrap(), data);
        assert_ne!(osg_persistence::Database::for_namespace(&path).unwrap().read("secrets","secret.dpapi").unwrap().unwrap().payload,data);
        assert!(!path.exists());
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "ios")))]
    #[test]
    fn unsupported_platform_fails_closed() {
        assert!(seal(b"secret").is_err());
        assert!(unseal(b"secret").is_err());
    }
}
