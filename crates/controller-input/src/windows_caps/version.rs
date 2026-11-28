use super::ApiVersion;
use windows_sys::Win32::{
    Foundation::{FARPROC, HMODULE},
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
};

struct Api {
    module: unsafe extern "system" fn(*const u16) -> HMODULE,
    symbol: unsafe extern "system" fn(HMODULE, *const u8) -> FARPROC,
}
type GetVersion = unsafe extern "system" fn(*mut u32) -> i32;

pub(super) fn native() -> ApiVersion {
    // hid.dll is already imported for the baseline HidP APIs. Borrow that
    // loaded module: no new loader search, static v2 import, or owned reference.
    detect(&Api {
        module: GetModuleHandleW,
        symbol: GetProcAddress,
    })
}
fn detect(api: &Api) -> ApiVersion {
    // Mirrors the official SDK hidpi.h header-only HidP_GetVersion routine:
    // optional HidP_GetVersionInternal export, absent export means version 1.
    const DLL: [u16; 8] = [104, 105, 100, 46, 100, 108, 108, 0];
    let module = unsafe { (api.module)(DLL.as_ptr()) };
    if module.is_null() {
        return ApiVersion {
            number: None,
            source: "hid-module-unavailable".into(),
            status: None,
        };
    }
    let Some(symbol) =
        (unsafe { (api.symbol)(module, c"HidP_GetVersionInternal".as_ptr().cast()) })
    else {
        return ApiVersion {
            number: Some(1),
            source: "hid-dll-without-version-export".into(),
            status: None,
        };
    };
    // Only the exact SDK-named export is interpreted as its documented ABI.
    let get_version: GetVersion = unsafe { std::mem::transmute(symbol) };
    let mut number = 1;
    let status = unsafe { get_version(&mut number) };
    ApiVersion {
        number: (status >= 0 && number >= 1).then_some(number),
        source: "hidp-get-version-internal".into(),
        status: Some(status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, ffi::CStr};
    thread_local! {
        static MODULE:Cell<bool>=const{Cell::new(true)};
        static EXPORT:Cell<bool>=const{Cell::new(true)};
        static NUMBER:Cell<u32>=const{Cell::new(2)};
        static STATUS:Cell<i32>=const{Cell::new(0)};
    }
    unsafe extern "system" fn module(_: *const u16) -> HMODULE {
        if MODULE.with(Cell::get) {
            1usize as HMODULE
        } else {
            std::ptr::null_mut()
        }
    }
    unsafe extern "system" fn symbol(_: HMODULE, name: *const u8) -> FARPROC {
        assert_eq!(
            CStr::from_ptr(name.cast()).to_bytes(),
            b"HidP_GetVersionInternal"
        );
        EXPORT.with(Cell::get).then(|| {
            std::mem::transmute::<GetVersion, unsafe extern "system" fn() -> isize>(get_version)
        })
    }
    unsafe extern "system" fn get_version(output: *mut u32) -> i32 {
        *output = NUMBER.with(Cell::get);
        STATUS.with(Cell::get)
    }
    #[test]
    fn runtime_library_version_lookup_requires_no_device_handle() {
        let version = native();
        assert!(
            version.number.is_some_and(|number| number >= 1),
            "{version:?}"
        );
        eprintln!("HidP runtime API version: {version:?}");
    }
    #[test]
    fn missing_export_is_version_one_and_v2_is_runtime_detected() {
        MODULE.with(|v| v.set(true));
        EXPORT.with(|v| v.set(false));
        assert_eq!(detect(&Api { module, symbol }).number, Some(1));
        EXPORT.with(|v| v.set(true));
        NUMBER.with(|v| v.set(2));
        STATUS.with(|v| v.set(0));
        let version = detect(&Api { module, symbol });
        assert_eq!(version.number, Some(2));
        assert_eq!(version.source, "hidp-get-version-internal");
        STATUS.with(|v| v.set(0x110000));
        assert_eq!(detect(&Api { module, symbol }).number, Some(2));
    }
    #[test]
    fn module_failure_export_failure_and_invalid_version_are_unavailable() {
        MODULE.with(|v| v.set(false));
        assert_eq!(detect(&Api { module, symbol }).number, None);
        MODULE.with(|v| v.set(true));
        EXPORT.with(|v| v.set(true));
        STATUS.with(|v| v.set(-1));
        NUMBER.with(|v| v.set(2));
        assert_eq!(detect(&Api { module, symbol }).number, None);
        STATUS.with(|v| v.set(0));
        NUMBER.with(|v| v.set(0));
        assert_eq!(detect(&Api { module, symbol }).number, None);
    }
}
