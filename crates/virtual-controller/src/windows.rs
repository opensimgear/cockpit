//! Load only the hash-pinned signed x64 vJoy release, with an explicit DLL search policy.
use crate::vjoy::{Axis, Driver, Position, Slot, Status, AXIS_USAGES, VERSION};
use sha2::{Digest, Sha256};
use std::{
    ffi::{c_void, CString},
    fs::OpenOptions,
    io::Read,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::Path,
};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    },
};
pub const DLL_HASH: &str = "199316aaefac95118e869c07e7f332ce492dd9a8b6dac5a3ca20b1d5bb5058a1";
pub const DRIVER_HASH: &str = "540ac46bc529001798068bd402bb4c36891d4c0e63b9dac9761b14bd38667d7d";
type Query = unsafe extern "C" fn(u32) -> i32;
type AxisQuery = unsafe extern "C" fn(u32, u32) -> i32;
type AxisRange = unsafe extern "C" fn(u32, u32, *mut i32) -> i32;
struct Api {
    module: HMODULE,
    enabled: unsafe extern "C" fn() -> i32,
    version: unsafe extern "C" fn() -> u16,
    matches: unsafe extern "C" fn(*mut u16, *mut u16) -> i32,
    status: Query,
    owner: Query,
    buttons: Query,
    discrete: Query,
    continuous: Query,
    ffb: Query,
    axis: AxisQuery,
    minimum: AxisRange,
    maximum: AxisRange,
    acquire: Query,
    submit: unsafe extern "C" fn(u32, *mut Position) -> i32,
    release: unsafe extern "C" fn(u32),
}
impl Drop for Api {
    fn drop(&mut self) {
        if !self.module.is_null() {
            unsafe {
                FreeLibrary(self.module);
            }
        }
    }
}
fn pinned_file(path: &Path, expected: &str) -> Result<std::fs::File, String> {
    // Deny replacement/write between hashing and loading the inspected artifact.
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
    if bytes.len() > 16_777_216 || format!("{:x}", Sha256::digest(&bytes)) != expected {
        return Err("vJoy artifact is not the qualified signed 2.2.2.0 x64 release.".into());
    }
    Ok(file)
}
unsafe fn load_api(path: &Path) -> Result<Api, String> {
    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
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
    // A guard releases the module even when a required symbol is absent.
    struct Guard(HMODULE);
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                FreeLibrary(self.0);
            }
        }
    }
    let guard = Guard(module);
    let symbol = |name: &str| -> Result<*const c_void, String> {
        let name = CString::new(name).unwrap();
        GetProcAddress(module, name.as_ptr().cast())
            .map(|f| f as *const c_void)
            .ok_or_else(|| format!("Qualified DLL is missing {}.", name.to_string_lossy()))
    };
    macro_rules! get {
        ($name:literal,$ty:ty) => {
            std::mem::transmute::<*const c_void, $ty>(symbol($name)?)
        };
    }
    let api = Api {
        module,
        enabled: get!("vJoyEnabled", unsafe extern "C" fn() -> i32),
        version: get!("GetvJoyVersion", unsafe extern "C" fn() -> u16),
        matches: get!(
            "DriverMatch",
            unsafe extern "C" fn(*mut u16, *mut u16) -> i32
        ),
        status: get!("GetVJDStatus", Query),
        owner: get!("GetOwnerPid", Query),
        buttons: get!("GetVJDButtonNumber", Query),
        discrete: get!("GetVJDDiscPovNumber", Query),
        continuous: get!("GetVJDContPovNumber", Query),
        ffb: get!("IsDeviceFfb", Query),
        axis: get!("GetVJDAxisExist", AxisQuery),
        minimum: get!("GetVJDAxisMin", AxisRange),
        maximum: get!("GetVJDAxisMax", AxisRange),
        acquire: get!("AcquireVJD", Query),
        submit: get!("UpdateVJD", unsafe extern "C" fn(u32, *mut Position) -> i32),
        release: get!("RelinquishVJD", unsafe extern "C" fn(u32)),
    };
    std::mem::forget(guard);
    Ok(api)
}
pub struct Native {
    api: Api,
}
impl Native {
    pub fn open() -> Result<Self, String> {
        if !cfg!(target_arch = "x86_64") {
            return Err("Only the qualified Windows x64 vJoy artifact is supported.".into());
        }
        let dll = Path::new(r"C:\Program Files\vJoy\x64\vJoyInterface.dll");
        let driver = Path::new(r"C:\Windows\System32\drivers\vjoy.sys");
        let _dll_guard = pinned_file(dll, DLL_HASH)?;
        let _driver_guard = pinned_file(driver, DRIVER_HASH)?;
        let mut native = Self {
            api: unsafe { load_api(dll)? },
        };
        native.check()?;
        Ok(native)
    }
}
impl Driver for Native {
    fn check(&mut self) -> Result<(), String> {
        let (mut dll, mut driver) = (0u16, 0u16);
        unsafe {
            if (self.api.enabled)() != 1 {
                return Err("vJoy is missing, disabled, or requires restart.".into());
            }
            if (self.api.version)() != VERSION
                || (self.api.matches)(&mut dll, &mut driver) != 1
                || dll != VERSION
                || driver != VERSION
            {
                return Err(
                    "vJoy DLL and active driver do not match the qualified 2.2.2 release.".into(),
                );
            }
        }
        Ok(())
    }
    fn inspect(&mut self, id: u8) -> Result<Slot, String> {
        if !(1..=16).contains(&id) {
            return Err("vJoy slot must be 1–16.".into());
        }
        let slot = u32::from(id);
        unsafe {
            let status = match (self.api.status)(slot) {
                0 => Status::Owned,
                1 => Status::Free,
                2 => Status::Busy,
                3 => Status::Missing,
                _ => Status::Unknown,
            };
            let mut result = Slot {
                id,
                status,
                owner_pid: None,
                buttons: 0,
                axes: vec![],
                discrete_hats: 0,
                continuous_hats: 0,
                force_feedback: false,
            };
            if matches!(status, Status::Missing | Status::Unknown) {
                return Ok(result);
            }
            let owner = (self.api.owner)(slot);
            if matches!(status, Status::Owned | Status::Busy) {
                result.owner_pid = u32::try_from(owner).ok().filter(|p| *p > 0);
            }
            let bounded = |value: i32, max: i32| -> Result<u16, String> {
                if !(0..=max).contains(&value) {
                    Err("vJoy capability query failed or exceeds the supported bound.".into())
                } else {
                    Ok(value as u16)
                }
            };
            result.buttons = bounded((self.api.buttons)(slot), 128)?;
            result.discrete_hats = bounded((self.api.discrete)(slot), 4)? as u8;
            result.continuous_hats = bounded((self.api.continuous)(slot), 4)? as u8;
            let ffb = (self.api.ffb)(slot);
            if ![0, 1].contains(&ffb) {
                return Err("Cannot inspect vJoy FFB capability.".into());
            }
            result.force_feedback = ffb == 1;
            for usage in AXIS_USAGES {
                match (self.api.axis)(slot, usage) {
                    1 => {
                        let (mut min, mut max) = (0i32, 0i32);
                        if (self.api.minimum)(slot, usage, &mut min) != 1
                            || (self.api.maximum)(slot, usage, &mut max) != 1
                            || min >= max
                        {
                            return Err("Cannot inspect vJoy axis range.".into());
                        }
                        result.axes.push(Axis { usage, min, max });
                    }
                    0 | -10 => {} // The selected SDK returns NO_SUCH_AXIS, not just FALSE.
                    error => return Err(format!("vJoy axis query failed with code {error}.")),
                }
            }
            Ok(result)
        }
    }
    fn acquire(&mut self, slot: u8) -> Result<(), String> {
        if self.inspect(slot)?.status != Status::Free {
            return Err("vJoy target is not FREE.".into());
        }
        if unsafe { (self.api.acquire)(u32::from(slot)) } != 1 {
            return Err("vJoy acquisition failed; another feeder may own the slot.".into());
        }
        Ok(())
    }
    fn submit(&mut self, slot: u8, position: &Position) -> Result<(), String> {
        if !(1..=16).contains(&slot) || slot != position.device {
            return Err("Invalid report slot.".into());
        }
        if unsafe { (self.api.status)(u32::from(slot)) } != 0 {
            return Err("Feeder no longer owns the vJoy target.".into());
        }
        let mut position = position.clone();
        if unsafe { (self.api.submit)(u32::from(slot), &mut position) } != 1 {
            return Err("Full vJoy report submission failed.".into());
        }
        Ok(())
    }
    fn release(&mut self, slot: u8) {
        if (1..=16).contains(&slot) {
            unsafe {
                (self.api.release)(u32::from(slot));
            }
        }
    }
}
#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
