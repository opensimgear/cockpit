//! Called only by the explicit user-requested enable operation.
use super::{
    super::native::{Handle, RawHandle},
    protocol::Request,
};
use std::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

#[repr(C)]
struct ShellInfo {
    size: u32,
    mask: u32,
    window: RawHandle,
    verb: *const u16,
    file: *const u16,
    parameters: *const u16,
    directory: *const u16,
    show: i32,
    instance: RawHandle,
    id_list: *mut c_void,
    class: *const u16,
    key: RawHandle,
    hot_key: u32,
    icon: RawHandle,
    process: RawHandle,
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteExW(info: *mut ShellInfo) -> i32;
}
static OS_LAUNCH_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
struct LaunchPermit;
impl LaunchPermit {
    fn acquire() -> Result<Self, String> {
        OS_LAUNCH_IN_FLIGHT.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "A Windows process-events consent request is still pending. Complete or dismiss that Windows prompt before enabling again.".to_string())?;
        Ok(Self)
    }
}
impl Drop for LaunchPermit {
    fn drop(&mut self) {
        OS_LAUNCH_IN_FLIGHT.store(false, Ordering::Release);
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
pub(super) fn elevated(request: &Request) -> Result<Handle, String> {
    with_shell(request, |info| unsafe { ShellExecuteExW(info) })
}
fn with_shell<F>(request: &Request, shell: F) -> Result<Handle, String>
where
    F: FnOnce(&mut ShellInfo) -> i32,
{
    // Validate the structured request before COM initialization or any OS launch.
    // Only canonical hexadecimal/numeric arguments can reach ShellExecute.
    Request::parse(&[
        super::protocol::ARGUMENT.into(),
        request.nonce.clone(),
        request.parent_pid.to_string(),
        request.parent_epoch.to_string(),
    ])?;
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }
        .map_err(|e| format!("Cannot initialize explicit process-events elevation: {e}"))?;
    let _apartment = Apartment;
    let program = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !program.is_absolute() || !program.is_file() {
        return Err("The Cockpit process-events helper executable is unavailable.".into());
    }
    let program = wide(
        program
            .to_str()
            .ok_or("The Cockpit executable path is not valid text.")?,
    );
    let parameters = wide(&request.arguments());
    let verb = wide("runas");
    let mut info: ShellInfo = unsafe { std::mem::zeroed() };
    info.size = std::mem::size_of::<ShellInfo>() as u32;
    info.mask = 0x40 | 0x100 | 0x400; // NOCLOSEPROCESS | NOASYNC | FLAG_NO_UI.
    info.verb = verb.as_ptr();
    info.file = program.as_ptr();
    info.parameters = parameters.as_ptr();
    info.directory = ptr::null();
    info.show = 0; // SW_HIDE; Windows still presents its own UAC consent prompt.
    let succeeded = {
        // A timed-out caller cannot dismiss Windows consent. Keep this permit
        // until the actual OS call returns so retries cannot accumulate prompts.
        let _permit = LaunchPermit::acquire()?;
        shell(&mut info)
    };
    // Take ownership immediately, including an unusual failed call returning a
    // handle, so every supplied native handle has exactly one closing owner.
    let process = if info.process.is_null() {
        None
    } else {
        Some(unsafe { Handle::from_raw(info.process) })
    };
    if succeeded == 0 {
        return Err(format!(
            "Windows process-events elevation was declined or could not start: {}",
            std::io::Error::last_os_error()
        ));
    }
    process.ok_or_else(|| "Windows did not return the exact process-events helper handle.".into())
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
