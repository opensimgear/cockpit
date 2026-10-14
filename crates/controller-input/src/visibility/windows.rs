//! Native read-only readiness and the qualification-gated persistent ABI.
use super::{
    policy,
    wire::{Driver, Io},
};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path, ptr};
use windows_sys::Win32::{
    Devices::{DeviceAndDriverInstallation::*, Properties::*},
    Foundation::*,
    Storage::FileSystem::*,
    System::{SystemInformation::GetSystemDirectoryW, IO::DeviceIoControl},
};
pub const QUALIFIED: bool = false;
const HASH: &str = "e3bb8b6a4e4ff90290b34e710edbef88cf71cec6ba0cadcb8e1685d5dd65ddb2";
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub struct Kernel(Handle);
fn open(path: &str, access: u32) -> Result<Handle, String> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err("Invalid Windows path.".into());
    }
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(Handle(handle))
}
fn artifact(path: &Path) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|_| {
        "Install the supported HidHide release and complete its restart requirements."
    })?;
    if file.metadata().map_err(|e| e.to_string())?.len() > 4_194_304 {
        return Err("Unsupported HidHide artifact size.".into());
    }
    let mut data = vec![];
    file.take(4_194_305)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if format!("{:x}", Sha256::digest(data)) != HASH {
        return Err(
            "Installed HidHide driver does not match the pinned 1.5.230 release artifact.".into(),
        );
    }
    Ok(())
}
pub fn installed() -> Result<Driver<Kernel>, String> {
    let mut path = [0u16; 4096];
    let len = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) };
    if len == 0 || len as usize >= path.len() {
        return Err("Windows system directory is unavailable.".into());
    }
    let path = std::path::PathBuf::from(
        String::from_utf16(&path[..len as usize]).map_err(|_| "Invalid system directory.")?,
    )
    .join("drivers/HidHide.sys");
    artifact(&path)?;
    Ok(Driver(Kernel(open(r"\\.\HidHide", GENERIC_READ)?)))
}
impl Io for Kernel {
    fn call(&mut self, function: u32, input: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let reading = matches!(function, 2048 | 2050 | 2052 | 2054);
        if !(reading || matches!(function, 2049 | 2051 | 2053 | 2055)) || reading != input.is_none()
        {
            return Err("Invalid HidHide operation.".into());
        }
        if !reading && !QUALIFIED {
            return Err(
                "HidHide mutation is disabled until native visibility qualification is complete."
                    .into(),
            );
        }
        if input.is_some_and(|v| v.len() > policy::LIMIT) {
            return Err("Oversized HidHide operation.".into());
        }
        let length = if matches!(function, 2048 | 2050) {
            let size = self.raw(function, None, &mut [])?;
            if size < 4 || size > policy::LIMIT || size % 2 != 0 {
                return Err("Invalid HidHide list size.".into());
            }
            size
        } else if reading {
            1
        } else {
            0
        };
        let mut output = vec![0; length];
        if self.raw(function, input, &mut output)? != length {
            return Err("HidHide response size changed during inspection.".into());
        }
        Ok(output)
    }
}
impl Kernel {
    fn raw(
        &mut self,
        function: u32,
        input: Option<&[u8]>,
        output: &mut [u8],
    ) -> Result<usize, String> {
        let mut returned = 0;
        let code = (32769u32 << 16) | (1 << 14) | (function << 2);
        let ok = unsafe {
            DeviceIoControl(
                self.0 .0,
                code,
                input.map_or(ptr::null(), |v| v.as_ptr()).cast(),
                input.map_or(0, |v| v.len() as u32),
                if output.is_empty() {
                    ptr::null_mut()
                } else {
                    output.as_mut_ptr().cast()
                },
                output.len() as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(returned as usize)
    }
}
/// Existing local executable, resolved by an OS handle (not string substitution).
pub fn executable(path: &str) -> Result<String, String> {
    if !Path::new(path).is_absolute()
        || !path.to_lowercase().ends_with(".exe")
        || !Path::new(path).is_file()
    {
        return Err("Choose an existing absolute executable path.".into());
    }
    let handle = open(path, 0)?;
    let mut buffer = [0u16; 4096];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            handle.0,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            VOLUME_NAME_NT,
        )
    };
    if length == 0 || length as usize >= buffer.len() {
        return Err("Cannot resolve the executable image path.".into());
    }
    let result = String::from_utf16(&buffer[..length as usize])
        .map_err(|_| "Invalid executable path encoding.")?;
    policy::executable(&result)?;
    Ok(result)
}
/// Resolve the exact current HID interface to its PnP instance; never match VID/PID.
pub fn instance(interface: &str) -> Result<String, String> {
    if !interface.starts_with(r"\\?\") || interface.len() > 2048 || interface.contains('\0') {
        return Err("Invalid observed HID interface.".into());
    }
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    let mut kind = 0;
    let code = unsafe {
        CM_Get_Device_Interface_PropertyW(
            wide(interface).as_ptr(),
            &DEVPKEY_Device_InstanceId,
            &mut kind,
            buffer.as_mut_ptr().cast(),
            &mut size,
            0,
        )
    };
    if code != CR_SUCCESS
        || kind != DEVPROP_TYPE_STRING
        || size < 2
        || size as usize > buffer.len() * 2
        || size % 2 != 0
    {
        return Err("Cannot resolve the current HID device instance.".into());
    }
    let used = size as usize / 2;
    if buffer[used - 1] != 0 {
        return Err("Unterminated HID instance property.".into());
    }
    let result =
        String::from_utf16(&buffer[..used - 1]).map_err(|_| "Invalid HID instance encoding.")?;
    policy::instance(&result)?;
    Ok(result)
}
#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
