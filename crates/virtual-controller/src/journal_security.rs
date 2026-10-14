//! Protected machine journal keys. No security policy is repaired or broadened.
//! Existing keys must already match; callers invoke creation only after elevation.
use crate::registry::{wide, Key};
use std::{ffi::c_void, ptr};
use windows_sys::Win32::{
    Foundation::{LocalFree, ERROR_INSUFFICIENT_BUFFER},
    Security::{
        Authorization::*, DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, SECURITY_ATTRIBUTES,
    },
    System::Registry::*,
};
pub(crate) const POLICY: &str = "O:BAG:BAD:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)";
const INFO: u32 =
    OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
const LIMIT: u32 = 8192;
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn failure() -> String {
    format!(
        "Journal security check failed: {}.",
        std::io::Error::last_os_error()
    )
}
fn descriptor(sddl: &str) -> Result<Local, String> {
    let text = wide(sddl)?;
    let mut data = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut data,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(failure());
    }
    Ok(Local(data))
}
fn canonical(data: *mut c_void, information: u32) -> Result<String, String> {
    let mut text = ptr::null_mut();
    let mut length = 0;
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            data,
            1,
            information,
            &mut text,
            &mut length,
        )
    } == 0
    {
        return Err(failure());
    }
    let _allocated = Local(text.cast());
    if length == 0 || length > LIMIT {
        return Err("Journal security descriptor is too large.".into());
    }
    let bytes = unsafe { std::slice::from_raw_parts(text, length as usize) };
    let end = bytes
        .iter()
        .position(|value| *value == 0)
        .ok_or("Unterminated journal security descriptor.")?;
    let text =
        String::from_utf16(&bytes[..end]).map_err(|_| "Invalid journal security encoding.")?;
    // Automatic-inheritance metadata does not grant rights. Protection and every
    // ACE, owner and group must still match exactly.
    Ok(text.replace("D:PAI", "D:P"))
}
fn read(key: &Key, information: u32) -> Result<String, String> {
    let mut length = 0;
    let code = unsafe { RegGetKeySecurity(key.0, information, ptr::null_mut(), &mut length) };
    if code != ERROR_INSUFFICIENT_BUFFER || length == 0 || length > LIMIT {
        return Err(format!(
            "Cannot inspect journal security (Windows error {code})."
        ));
    }
    let mut data = vec![0u32; (length as usize).div_ceil(4)];
    let code =
        unsafe { RegGetKeySecurity(key.0, information, data.as_mut_ptr().cast(), &mut length) };
    if code != 0 {
        return Err(format!(
            "Cannot inspect journal security (Windows error {code})."
        ));
    }
    canonical(data.as_mut_ptr().cast(), information)
}
fn verify(key: &Key, sddl: &str, information: u32) -> Result<(), String> {
    let expected = descriptor(sddl)?;
    if read(key, information)? != canonical(expected.0, information)? {
        return Err("Journal owner or access policy differs; preserving the existing key.".into());
    }
    Ok(())
}
fn create(hive: HKEY, path: &str, sddl: &str, information: u32) -> Result<(Key, bool), String> {
    let security = descriptor(sddl)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let mut key = ptr::null_mut();
    let mut disposition = 0;
    let name = wide(path)?;
    let code = unsafe {
        RegCreateKeyExW(
            hive,
            name.as_ptr(),
            0,
            ptr::null(),
            0,
            KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
            &attributes,
            &mut key,
            &mut disposition,
        )
    };
    if code != 0 {
        return Err(format!(
            "Cannot prepare protected journal key (Windows error {code})."
        ));
    }
    let key = Key(key);
    verify(&key, sddl, information)?;
    Ok((key, disposition == REG_CREATED_NEW_KEY))
}
pub(crate) fn protected(hive: HKEY, path: &str) -> Result<(Key, bool), String> {
    create(hive, path, POLICY, INFO)
}
pub(crate) fn verify_protected(key: &Key) -> Result<(), String> {
    verify(key, POLICY, INFO)
}

#[cfg(test)]
#[path = "journal_security_tests.rs"]
mod tests;
