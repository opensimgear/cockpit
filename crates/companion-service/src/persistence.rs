use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::Zeroize;

pub const STATE_BYTES: usize = 8 * 1024 * 1024;
pub trait StateStorage: Send + Sync {
    fn read(&self) -> Result<Option<Vec<u8>>, String>;
    fn write(&self, bytes: &[u8]) -> Result<(), String>;
}

#[derive(Clone)]
pub struct SealedFileStore {
    path: PathBuf,
}
impl SealedFileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}
impl StateStorage for SealedFileStore {
    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Cannot inspect protected companion state: {error}")),
        };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > STATE_BYTES as u64
        {
            return Err("Invalid protected companion state file.".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(&self.path)
            .map_err(|e| e.to_string())?
            .take((STATE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > STATE_BYTES {
            return Err("Protected companion state exceeds budget.".into());
        }
        unseal(&bytes).map(Some)
    }
    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        atomic_write(&self.path, &seal(bytes)?)
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
pub fn seal(bytes: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        protect(bytes, false)
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err("Protected companion storage is not implemented on this platform; no plaintext fallback is permitted.".into())
    }
}
pub fn unseal(bytes: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        protect(bytes, true)
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err("Protected companion storage is not implemented on this platform; no plaintext fallback is permitted.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let store = SealedFileStore::new(root.path().join("missing"));
        assert!(store.read().unwrap().is_none());
        assert!(SealedFileStore::new(root.path()).read().is_err());
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
        let store = SealedFileStore::new(&path);
        store.write(data).unwrap();
        assert_eq!(store.read().unwrap().unwrap(), data);
        assert_ne!(fs::read(path).unwrap(), data);
    }
    #[cfg(not(windows))]
    #[test]
    fn unsupported_platform_fails_closed() {
        assert!(seal(b"secret").is_err());
        assert!(unseal(b"secret").is_err());
    }
}
