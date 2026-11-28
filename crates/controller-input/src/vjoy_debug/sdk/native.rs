//! Minimal read/receive-only ABI for the qualified Brunner vJoy 2.2.2.0 x64 DLL.
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    fs::OpenOptions,
    io::Read,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::Path,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
};

pub(super) const DLL_HASH: &str =
    "199316aaefac95118e869c07e7f332ce492dd9a8b6dac5a3ca20b1d5bb5058a1";
pub(super) const DRIVER_HASH: &str =
    "540ac46bc529001798068bd402bb4c36891d4c0e63b9dac9761b14bd38667d7d";
pub(super) type Query = unsafe extern "C" fn(u32) -> i32;
pub(super) type Axis = unsafe extern "C" fn(u32, u32) -> i32;
pub(super) type Range = unsafe extern "C" fn(u32, u32, *mut i32) -> i32;
pub(super) type Out = unsafe extern "C" fn(*mut i32) -> i32;
pub(super) type Callback = unsafe extern "system" fn(*mut c_void, *mut c_void);
pub(super) struct Api {
    // Never unload the SDK: its dummy window and FFB worker have no join API.
    // Only a short-lived helper opens it; process exit reclaims these resources.
    pub(super) _files: Vec<std::fs::File>,
    pub enabled: unsafe extern "C" fn() -> i32,
    pub version: unsafe extern "C" fn() -> u16,
    pub matches: unsafe extern "C" fn(*mut u16, *mut u16) -> i32,
    pub driver_ffb: Out,
    pub maximum_devices: Out,
    pub existing_devices: Out,
    pub status: Query,
    pub owner: Query,
    pub exists: Query,
    pub buttons: Query,
    pub discrete: Query,
    pub continuous: Query,
    pub ffb: Query,
    pub effect: Axis,
    pub axis: Axis,
    pub minimum: Range,
    pub maximum: Range,
    pub acquire: Query,
    pub release: unsafe extern "C" fn(u32),
    pub register: unsafe extern "C" fn(Option<Callback>, *mut c_void),
}

fn pinned(path: &Path, hash: &str) -> Result<std::fs::File, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .map_err(|e| {
            format!(
                "Required vJoy artifact {} is unavailable: {e}",
                path.display()
            )
        })?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(16_777_217)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16_777_216 || format!("{:x}", Sha256::digest(&bytes)) != hash {
        return Err("vJoy artifact is not the qualified signed 2.2.2.0 x64 release.".into());
    }
    Ok(file)
}

impl Api {
    pub fn open() -> Result<Self, String> {
        if !cfg!(target_arch = "x86_64") {
            return Err("vJoy inspection requires the qualified Windows x64 SDK.".into());
        }
        let path = Path::new(r"C:\Program Files\vJoy\x64\vJoyInterface.dll");
        let files = vec![
            pinned(path, DLL_HASH)?,
            pinned(
                Path::new(r"C:\Windows\System32\drivers\vjoy.sys"),
                DRIVER_HASH,
            )?,
        ];
        let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let module = LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            );
            if module.is_null() {
                return Err(format!(
                    "Cannot load qualified vJoy DLL: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let symbol = |name: &[u8]| {
                GetProcAddress(module, name.as_ptr())
                    .map(|f| f as *const c_void)
                    .ok_or_else(|| {
                        format!(
                            "Qualified vJoy SDK is missing {}.",
                            String::from_utf8_lossy(&name[..name.len() - 1])
                        )
                    })
            };
            let api = Self::from_symbols(symbol, files)?;
            api.check()?;
            Ok(api)
        }
    }

    /// The resolver must return the pinned header's exact function ABI and
    /// keep its module loaded for the lifetime of this function table.
    pub(super) unsafe fn from_symbols(
        symbol: impl Fn(&[u8]) -> Result<*const c_void, String>,
        files: Vec<std::fs::File>,
    ) -> Result<Self, String> {
        macro_rules! get {
            ($name:literal, $ty:ty) => {
                std::mem::transmute::<*const c_void, $ty>(symbol(concat!($name, "\0").as_bytes())?)
            };
        }
        // Do not free a successfully loaded SDK even on a symbol error: DLL
        // initialization may already have started its window thread.
        Ok(Self {
            _files: files,
            enabled: get!("vJoyEnabled", unsafe extern "C" fn() -> i32),
            version: get!("GetvJoyVersion", unsafe extern "C" fn() -> u16),
            matches: get!(
                "DriverMatch",
                unsafe extern "C" fn(*mut u16, *mut u16) -> i32
            ),
            driver_ffb: get!("vJoyFfbCap", Out),
            maximum_devices: get!("GetvJoyMaxDevices", Out),
            existing_devices: get!("GetNumberExistingVJD", Out),
            status: get!("GetVJDStatus", Query),
            owner: get!("GetOwnerPid", Query),
            exists: get!("isVJDExists", Query),
            buttons: get!("GetVJDButtonNumber", Query),
            discrete: get!("GetVJDDiscPovNumber", Query),
            continuous: get!("GetVJDContPovNumber", Query),
            ffb: get!("IsDeviceFfb", Query),
            effect: get!("IsDeviceFfbEffect", Axis),
            axis: get!("GetVJDAxisExist", Axis),
            minimum: get!("GetVJDAxisMin", Range),
            maximum: get!("GetVJDAxisMax", Range),
            acquire: get!("AcquireVJD", Query),
            release: get!("RelinquishVJD", unsafe extern "C" fn(u32)),
            register: get!(
                "FfbRegisterGenCB",
                unsafe extern "C" fn(Option<Callback>, *mut c_void)
            ),
        })
    }
    pub fn check(&self) -> Result<(), String> {
        let (mut dll, mut driver) = (0, 0);
        unsafe {
            if (self.enabled)() != 1 {
                return Err("vJoy is disabled, unavailable, or requires restart.".into());
            }
            if (self.version)() != 0x222
                || (self.matches)(&mut dll, &mut driver) != 1
                || dll != 0x222
                || driver != 0x222
            {
                return Err(
                    "vJoy DLL and active driver do not match the qualified 2.2.2.0 release.".into(),
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artifact_pins_reject_missing_modified_and_replaced_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vJoyInterface.dll");
        assert!(pinned(&path, DLL_HASH).is_err());
        std::fs::write(&path, b"pinned SDK fixture").unwrap();
        assert!(pinned(&path, DLL_HASH).is_err());
        let hash = format!("{:x}", Sha256::digest(b"pinned SDK fixture"));
        let guard = pinned(&path, &hash).unwrap();
        assert!(std::fs::write(&path, b"replacement").is_err());
        assert!(std::fs::remove_file(&path).is_err());
        drop(guard);
        std::fs::write(&path, b"replacement").unwrap();
        assert!(pinned(&path, &hash).is_err());
    }
    #[test]
    fn missing_required_export_fails_closed() {
        let error = unsafe { Api::from_symbols(|_| Err("missing export".into()), vec![]) }
            .err()
            .unwrap();
        assert_eq!(error, "missing export");
    }
}
