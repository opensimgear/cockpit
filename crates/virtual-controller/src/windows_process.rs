//! Read-only process identity for the elevated helper and recovery guardian.
//! Hold the original kernel handle; a PID alone never establishes ownership.
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    },
    Security::{GetTokenInformation, IsValidSid, TokenUser, TOKEN_QUERY, TOKEN_USER},
    System::Threading::*,
};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub pid: u32,
    pub created: String,
}
impl Identity {
    pub fn validate(&self) -> Result<u64, String> {
        let created = self
            .created
            .parse::<u64>()
            .map_err(|_| "Invalid process creation time.")?;
        if self.pid == 0 || created == 0 || created.to_string() != self.created {
            return Err("Invalid process identity.".into());
        }
        Ok(created)
    }
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub struct Process {
    handle: Handle,
}
fn failure() -> String {
    format!(
        "Unable to verify process identity: {}.",
        std::io::Error::last_os_error()
    )
}
fn open(pid: u32) -> Result<Option<Handle>, String> {
    if pid == 0 {
        return Err("Invalid process ID.".into());
    }
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
            return Ok(None);
        }
        return Err(failure());
    }
    Ok(Some(Handle(handle)))
}
fn created(handle: HANDLE) -> Result<u64, String> {
    let mut start = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(handle, &mut start, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(failure());
    }
    Ok(((start.dwHighDateTime as u64) << 32) | start.dwLowDateTime as u64)
}
fn image(handle: HANDLE) -> Result<PathBuf, String> {
    let mut text = vec![0u16; 4096];
    let mut length = text.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle, 0, text.as_mut_ptr(), &mut length) } == 0 {
        return Err(failure());
    }
    if length == 0 || length as usize >= text.len() {
        return Err("Process executable path exceeds its bounds.".into());
    }
    let path = PathBuf::from(
        String::from_utf16(&text[..length as usize])
            .map_err(|_| "Invalid executable path encoding.")?,
    );
    if !path.is_absolute() {
        return Err("Process executable must be absolute.".into());
    }
    Ok(path)
}
fn user_sid(process: HANDLE) -> Result<Vec<u8>, String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(failure());
    }
    let token = Handle(token);
    let mut length = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut length);
    }
    if length < std::mem::size_of::<TOKEN_USER>() as u32 || length > 4096 {
        return Err("Process user token exceeds its bounds.".into());
    }
    let mut bytes = vec![0u64; (length as usize).div_ceil(8)];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            bytes.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(failure());
    }
    let record = unsafe { &*bytes.as_ptr().cast::<TOKEN_USER>() };
    let first = bytes.as_ptr() as usize;
    let last = first + length as usize;
    let sid = record.User.Sid as usize;
    if sid < first || sid.checked_add(8).is_none_or(|end| end > last) {
        return Err("Invalid process user SID.".into());
    }
    let size = 8 + 4 * unsafe { *record.User.Sid.cast::<u8>().add(1) } as usize;
    if !(8..=68).contains(&size) || sid.checked_add(size).is_none_or(|end| end > last) {
        return Err("Process user SID exceeds its bounds.".into());
    }
    if unsafe { IsValidSid(record.User.Sid) } == 0 {
        return Err("Invalid process user SID.".into());
    }
    Ok(unsafe { std::slice::from_raw_parts(record.User.Sid.cast::<u8>(), size) }.to_vec())
}
fn same_image(actual: &Path, expected: &Path) -> Result<bool, String> {
    let actual = actual.canonicalize().map_err(|e| e.to_string())?;
    let expected = expected.canonicalize().map_err(|e| e.to_string())?;
    Ok(actual == expected)
}
impl Process {
    /// A dead or reused PID is not the original process. Access errors remain
    /// explicit instead of being treated as successful proof of termination.
    pub fn track(identity: &Identity, expected_executable: &Path) -> Result<Option<Self>, String> {
        let expected = identity.validate()?;
        let Some(handle) = open(identity.pid)? else {
            return Ok(None);
        };
        if created(handle.0)? != expected {
            return Ok(None);
        }
        let process = Self { handle };
        if !process.alive()? {
            return Ok(None);
        }
        if !same_image(&image(process.handle.0)?, expected_executable)? {
            return Err("Process executable does not match this helper.".into());
        }
        if user_sid(process.handle.0)? != user_sid(unsafe { GetCurrentProcess() })? {
            return Err("Provisioning requires the same Windows user as Cockpit.".into());
        }
        Ok(Some(process))
    }
    pub fn alive(&self) -> Result<bool, String> {
        match unsafe { WaitForSingleObject(self.handle.0, 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(failure()),
        }
    }
}
pub fn identity(pid: u32) -> Result<Identity, String> {
    let process = open(pid)?.ok_or("Process no longer exists.")?;
    let identity = Identity {
        pid,
        created: created(process.0)?.to_string(),
    };
    identity.validate()?;
    Ok(identity)
}
pub fn current() -> Result<Identity, String> {
    identity(std::process::id())
}

#[cfg(test)]
#[path = "windows_process_tests.rs"]
mod tests;
