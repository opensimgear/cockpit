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
pub const QUALIFIED: bool = super::capabilities::DEVELOPMENT_NATIVE_PROFILES;
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
// The uniquely owned kernel handle may move to the visibility worker. Calls are
// serialized through &mut self; no shared access or overlapping IO is introduced.
unsafe impl Send for Kernel {}
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
enum ArtifactError {
    Absent,
    Unsupported(&'static str),
    Io(std::io::Error),
}
impl ArtifactError {
    fn status(self) -> super::self_access::Status {
        match self {
            Self::Absent => super::self_access::Status::new("absent", "HidHide is not installed; no self-access setup is needed."),
            Self::Unsupported(message) => super::self_access::Status::new("unsupported", message),
            Self::Io(error) => super::self_access::Status::new("error", format!("Cannot inspect the installed HidHide driver: {error}")),
        }
    }
}
fn verify_artifact(path: &Path) -> Result<(), ArtifactError> {
    let file = std::fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound { ArtifactError::Absent } else { ArtifactError::Io(error) }
    })?;
    if file.metadata().map_err(ArtifactError::Io)?.len() > 4_194_304 {
        return Err(ArtifactError::Unsupported("Unsupported HidHide artifact size."));
    }
    let mut data = vec![];
    file.take(4_194_305)
        .read_to_end(&mut data)
        .map_err(ArtifactError::Io)?;
    if format!("{:x}", Sha256::digest(data)) != HASH {
        return Err(ArtifactError::Unsupported("Installed HidHide driver does not match the pinned 1.5.230 release artifact."));
    }
    Ok(())
}
fn artifact(path: &Path) -> Result<(), String> {
    verify_artifact(path).map_err(|error| error.status().message)
}
fn driver_path() -> Result<std::path::PathBuf, String> {
    let mut path = [0u16; 4096];
    let len = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) };
    if len == 0 || len as usize >= path.len() {
        return Err("Windows system directory is unavailable.".into());
    }
    Ok(std::path::PathBuf::from(
        String::from_utf16(&path[..len as usize]).map_err(|_| "Invalid system directory.")?,
    )
    .join("drivers/HidHide.sys"))
}
pub fn installed() -> Result<Driver<Kernel>, String> {
    artifact(&driver_path()?)?;
    Ok(Driver(Kernel(open(r"\\.\HidHide", GENERIC_READ)?)))
}

struct ReadIo<'a>(&'a mut Kernel);
impl Io for ReadIo<'_> {
    fn call(&mut self, function: u32, input: Option<&[u8]>) -> Result<Vec<u8>, String> {
        self.0.call(function, input)
    }
}
struct SelfAccessIo {
    kernel: Kernel,
    image: String,
}
impl Io for SelfAccessIo {
    fn call(&mut self, function: u32, input: Option<&[u8]>) -> Result<Vec<u8>, String> {
        if input.is_none() { return self.kernel.call(function, None); }
        if function != 2049 { return Err("Automatic HidHide access may only change its own application entry.".into()); }
        use super::journal::Backend;
        let before = Driver(ReadIo(&mut self.kernel)).read()?;
        super::self_access::validate_write(&before, &self.image, function, input)?;
        super::wire::exchange(function, input, |function, input, output| self.kernel.raw(function, input, output))
    }
}
/// The sole ungated capability is this process's exact self-access delta. It
/// cannot accept an executable or policy from IPC, or mutate hidden devices.
pub(super) fn startup_self_access(guard: impl FnOnce() -> Result<(), String>) -> super::self_access::Status {
    use super::self_access::Status;
    let path = match driver_path() { Ok(path) => path, Err(error) => return Status::new("error", error) };
    if let Err(error) = verify_artifact(&path) { return error.status(); }
    let image = match std::env::current_exe().map_err(|error| error.to_string()).and_then(|path| {
        executable(path.to_str().ok_or("The current executable path is not valid text.")?)
    }) { Ok(image) => image, Err(error) => return Status::new("error", error) };
    let kernel = match open(r"\\.\HidHide", GENERIC_READ) {
        Ok(handle) => Kernel(handle),
        Err(error) => {
            let mut status = Status::new("error", format!("Cannot open HidHide for Cockpit self-access: {error}. Close its configuration client or restore its current owner before restarting Cockpit."));
            status.executable = Some(image);
            return status;
        }
    };
    super::self_access::reconcile(&mut Driver(SelfAccessIo { kernel, image: image.clone() }), image, guard)
}
impl Io for Kernel {
    fn call(&mut self, function: u32, input: Option<&[u8]>) -> Result<Vec<u8>, String> {
        if input.is_some() && !QUALIFIED {
            return Err(
                "HidHide mutation is disabled until native visibility qualification is complete."
                    .into(),
            );
        }
        super::wire::exchange(function, input, |f, i, o| self.raw(f, i, o))
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
impl super::runtime::Adapter for Driver<Kernel> {
    fn executable(&mut self, path: &str) -> Result<String, String> {
        executable(path)
    }
    fn instance(&mut self, interface: &str) -> Result<String, String> {
        instance(interface)
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

/// Read-only executable selection. PID is display context; only the path is saved.
pub fn applications(offset: usize) -> Result<serde_json::Value, String> {
    use windows_sys::Win32::System::{Diagnostics::ToolHelp::*, Threading::*};
    if offset > 256 {
        return Err("Invalid process page.".into());
    }
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let snapshot = Handle(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    let mut applications = vec![];
    let mut visited = 0;
    while found != 0 && visited < 2048 && applications.len() < 256 {
        visited += 1;
        let process =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID) };
        if !process.is_null() {
            let process = Handle(process);
            let mut buffer = [0u16; 1024];
            let mut length = buffer.len() as u32;
            if unsafe { QueryFullProcessImageNameW(process.0, 0, buffer.as_mut_ptr(), &mut length) }
                != 0
                && length > 0
                && (length as usize) < buffer.len()
            {
                if let Ok(path) = String::from_utf16(&buffer[..length as usize]) {
                    if path.to_lowercase().ends_with(".exe") && Path::new(&path).is_absolute() {
                        applications
                            .push(serde_json::json!({"pid":entry.th32ProcessID,"path":path}));
                    }
                }
            }
        }
        found = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    Ok(
        serde_json::json!({"processes":applications.iter().skip(offset).take(16).collect::<Vec<_>>(),"next":if offset+16<applications.len(){Some(offset+16)}else{None},"total":applications.len()}),
    )
}
#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
