use super::*;
use std::ffi::c_void;
use windows_sys::{
    core::GUID,
    Win32::{Security::SECURITY_ATTRIBUTES, Storage::FileSystem::*},
};

pub(super) struct Api {
    pub guid: unsafe extern "system" fn(*mut GUID),
    pub classes: unsafe extern "system" fn(*const GUID, *const u16, HWND, u32) -> isize,
    pub enumerate: unsafe extern "system" fn(
        isize,
        *const SP_DEVINFO_DATA,
        *const GUID,
        u32,
        *mut SP_DEVICE_INTERFACE_DATA,
    ) -> i32,
    pub detail: unsafe extern "system" fn(
        isize,
        *const SP_DEVICE_INTERFACE_DATA,
        *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W,
        u32,
        *mut u32,
        *mut SP_DEVINFO_DATA,
    ) -> i32,
    pub destroy: unsafe extern "system" fn(isize) -> i32,
    pub open: unsafe extern "system" fn(
        *const u16,
        u32,
        u32,
        *const SECURITY_ATTRIBUTES,
        u32,
        u32,
        HANDLE,
    ) -> HANDLE,
    pub close: unsafe extern "system" fn(HANDLE) -> i32,
    pub attributes: unsafe extern "system" fn(HANDLE, *mut HIDD_ATTRIBUTES) -> bool,
    pub manufacturer: unsafe extern "system" fn(HANDLE, *mut c_void, u32) -> bool,
    pub product: unsafe extern "system" fn(HANDLE, *mut c_void, u32) -> bool,
    pub serial: unsafe extern "system" fn(HANDLE, *mut c_void, u32) -> bool,
    pub preparsed: unsafe extern "system" fn(HANDLE, *mut isize) -> bool,
    pub free_preparsed: unsafe extern "system" fn(isize) -> bool,
    pub snapshot: unsafe fn(isize) -> windows_caps::Snapshot,
    pub last_error: unsafe extern "system" fn() -> u32,
}
impl Api {
    pub fn native() -> Self {
        Self {
            guid: HidD_GetHidGuid,
            classes: SetupDiGetClassDevsW,
            enumerate: SetupDiEnumDeviceInterfaces,
            detail: SetupDiGetDeviceInterfaceDetailW,
            destroy: SetupDiDestroyDeviceInfoList,
            open: CreateFileW,
            close: CloseHandle,
            attributes: HidD_GetAttributes,
            manufacturer: HidD_GetManufacturerString,
            product: HidD_GetProductString,
            serial: HidD_GetSerialNumberString,
            preparsed: HidD_GetPreparsedData,
            free_preparsed: HidD_FreePreparsedData,
            snapshot: windows_caps::snapshot,
            last_error: GetLastError,
        }
    }
}

pub(super) struct OwnedSet<'a> {
    api: &'a Api,
    value: isize,
    owned: bool,
}
impl<'a> OwnedSet<'a> {
    pub fn new(api: &'a Api, value: isize) -> Self {
        Self {
            api,
            value,
            owned: true,
        }
    }
    pub unsafe fn release(&mut self) -> bool {
        self.owned = false;
        (self.api.destroy)(self.value) != 0
    }
}
impl Drop for OwnedSet<'_> {
    fn drop(&mut self) {
        if self.owned {
            unsafe {
                (self.api.destroy)(self.value);
            }
        }
    }
}

pub(super) struct OwnedHandle<'a> {
    api: &'a Api,
    pub value: HANDLE,
    owned: bool,
}
impl<'a> OwnedHandle<'a> {
    pub fn new(api: &'a Api, value: HANDLE) -> Self {
        Self {
            api,
            value,
            owned: true,
        }
    }
    pub unsafe fn release(&mut self) -> bool {
        self.owned = false;
        (self.api.close)(self.value) != 0
    }
}
impl Drop for OwnedHandle<'_> {
    fn drop(&mut self) {
        if self.owned {
            unsafe {
                (self.api.close)(self.value);
            }
        }
    }
}

pub(super) struct OwnedPreparsed<'a> {
    api: &'a Api,
    value: isize,
    owned: bool,
}
impl<'a> OwnedPreparsed<'a> {
    pub fn new(api: &'a Api, value: isize) -> Self {
        Self {
            api,
            value,
            owned: true,
        }
    }
    pub unsafe fn release(&mut self) -> bool {
        self.owned = false;
        (self.api.free_preparsed)(self.value)
    }
}
impl Drop for OwnedPreparsed<'_> {
    fn drop(&mut self) {
        if self.owned {
            unsafe {
                (self.api.free_preparsed)(self.value);
            }
        }
    }
}
