use crate::game_discovery::{normalized_path, ProcessIdentity};
use std::{ffi::c_void, ptr};

pub(super) type RawHandle = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> RawHandle;
    fn GetProcessTimes(
        h: RawHandle,
        c: *mut FileTime,
        e: *mut FileTime,
        k: *mut FileTime,
        u: *mut FileTime,
    ) -> i32;
    fn QueryFullProcessImageNameW(
        h: RawHandle,
        flags: u32,
        image: *mut u16,
        length: *mut u32,
    ) -> i32;
    fn CreateEventW(
        attributes: *const c_void,
        manual: i32,
        initial: i32,
        name: *const u16,
    ) -> RawHandle;
    fn SetEvent(h: RawHandle) -> i32;
    fn WaitForMultipleObjects(count: u32, handles: *const RawHandle, all: i32, timeout: u32)
        -> u32;
    fn WaitForSingleObject(h: RawHandle, timeout: u32) -> u32;
    fn CloseHandle(h: RawHandle) -> i32;
}
pub(super) struct Handle(RawHandle);
// Kernel handles may be waited/signalled from any thread. Ownership stays unique.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn last_error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}
impl Handle {
    pub(super) unsafe fn from_raw(raw: RawHandle) -> Self {
        Self(raw)
    }
    pub(super) fn raw(&self) -> RawHandle {
        self.0
    }
    pub(super) fn open(pid: u32) -> Result<Self, String> {
        let raw = unsafe { OpenProcess(0x1000 | 0x00100000, 0, pid) };
        if raw.is_null() {
            Err(last_error("Cannot retain native process"))
        } else {
            Ok(Self(raw))
        }
    }
    pub(super) fn event() -> Result<Self, String> {
        let raw = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
        if raw.is_null() {
            Err(last_error("Cannot create X-Plane process event"))
        } else {
            Ok(Self(raw))
        }
    }
    pub(super) fn signal(&self) {
        unsafe {
            SetEvent(self.0);
        }
    }
    pub(super) fn process(expected: &ProcessIdentity) -> Result<Self, String> {
        let raw = unsafe { OpenProcess(0x1000 | 0x00100000, 0, expected.pid) };
        if raw.is_null() {
            return Err(last_error("Cannot retain selected X-Plane process"));
        }
        let handle = Self(raw);
        let actual = handle.identity(expected.pid)?;
        if actual.epoch != expected.epoch
            || normalized_path(&actual.path) != normalized_path(&expected.path)
        {
            return Err("The selected X-Plane process identity changed.".into());
        }
        Ok(handle)
    }
    pub(super) fn identity(&self, pid: u32) -> Result<ProcessIdentity, String> {
        self.running()?;
        let (mut c, mut e, mut k, mut u) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        if unsafe { GetProcessTimes(self.0, &mut c, &mut e, &mut k, &mut u) } == 0 {
            return Err(last_error("Cannot validate X-Plane process creation time"));
        }
        let mut image = vec![0u16; 32768];
        let mut length = image.len() as u32;
        if unsafe { QueryFullProcessImageNameW(self.0, 0, image.as_mut_ptr(), &mut length) } == 0 {
            return Err(last_error("Cannot validate X-Plane process executable"));
        }
        let path = String::from_utf16(&image[..length as usize])
            .map_err(|_| "Invalid X-Plane process path.")?;
        self.running()?;
        Ok(ProcessIdentity {
            pid,
            path,
            epoch: (u64::from(c.high) << 32) | u64::from(c.low),
        })
    }
    pub(super) fn running(&self) -> Result<(), String> {
        match unsafe { WaitForSingleObject(self.0, 0) } {
            258 => Ok(()),
            0 => Err("The selected X-Plane process exited.".into()),
            _ => Err(last_error(
                "Cannot inspect selected X-Plane process lifetime",
            )),
        }
    }
    #[cfg(test)]
    pub(super) fn current_identity() -> ProcessIdentity {
        Self::identity_for_pid(std::process::id())
    }
    #[cfg(test)]
    pub(super) fn identity_for_pid(pid: u32) -> ProcessIdentity {
        let handle = Self(unsafe { OpenProcess(0x1000 | 0x00100000, 0, pid) });
        handle.identity(pid).unwrap()
    }
}
pub(super) fn wait(stop: &Handle, wake: &Handle, process: &Handle) -> Result<u32, String> {
    let handles = [stop.0, wake.0, process.0];
    match unsafe { WaitForMultipleObjects(3, handles.as_ptr(), 0, u32::MAX) } {
        index @ 0..=2 => Ok(index),
        _ => Err(last_error("Cannot wait for X-Plane process events")),
    }
}
