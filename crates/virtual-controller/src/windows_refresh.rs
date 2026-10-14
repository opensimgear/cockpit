//! Exact-device, bounded refresh used only by the explicit elevated helper.
//! No device discovery in this module changes configuration.
use crate::{
    registry::wide,
    vjoy::{Slot, Status},
};
use std::os::windows::process::CommandExt;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Devices::DeviceAndDriverInstallation::*,
    System::{Registry::REG_SZ, SystemInformation::GetSystemDirectoryW},
};
const LIMIT: u32 = 4096;
fn checked(code: u32) -> Result<(), String> {
    if code == CR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "vJoy device lookup failed (Configuration Manager {code})."
        ))
    }
}
fn multi_string(buffer: &[u16]) -> Result<Vec<String>, String> {
    if buffer.len() < 2 || buffer.len() > LIMIT as usize || !buffer.ends_with(&[0, 0]) {
        return Err("Invalid bounded device inventory.".into());
    }
    let mut result = Vec::new();
    let mut position = 0;
    while position < buffer.len() && buffer[position] != 0 {
        let count = buffer[position..]
            .iter()
            .position(|v| *v == 0)
            .ok_or("Unterminated device identity.")?;
        let item = String::from_utf16(&buffer[position..position + count])
            .map_err(|_| "Invalid device identity encoding.")?;
        if item.len() > 256 || result.len() >= 16 {
            return Err("Device inventory exceeds its bounds.".into());
        }
        result.push(item);
        position += count + 1;
    }
    if buffer[position..].iter().any(|v| *v != 0) {
        return Err("Invalid device inventory tail.".into());
    }
    Ok(result)
}
fn device_identity(id: &str) -> bool {
    let id = id.to_ascii_uppercase();
    id.strip_prefix("ROOT\\HIDCLASS\\")
        .is_some_and(|tail| tail.len() == 4 && tail.bytes().all(|v| v.is_ascii_hexdigit()))
}
fn native_ids() -> Result<Vec<String>, String> {
    let service = wide("vjoy")?;
    let flags = CM_GETIDLIST_FILTER_SERVICE | CM_GETIDLIST_FILTER_PRESENT;
    let mut count = 0;
    checked(unsafe { CM_Get_Device_ID_List_SizeW(&mut count, service.as_ptr(), flags) })?;
    if count == 0 || count > LIMIT {
        return Err("vJoy inventory exceeds its bounds.".into());
    }
    // Windows uses one NUL for an empty list. Normalize only that documented case.
    if count == 1 {
        return Ok(vec![]);
    }
    let mut data = vec![0u16; count as usize];
    checked(unsafe { CM_Get_Device_ID_ListW(service.as_ptr(), data.as_mut_ptr(), count, flags) })?;
    multi_string(&data)
}
fn service_text(kind: u32, data: &[u16], bytes: u32) -> Result<String, String> {
    if kind != REG_SZ || bytes < 2 || bytes % 2 != 0 || bytes as usize > data.len() * 2 {
        return Err("Invalid vJoy service property.".into());
    }
    let data = &data[..bytes as usize / 2];
    if data.last() != Some(&0) || data[..data.len() - 1].contains(&0) {
        return Err("Invalid service terminator.".into());
    }
    String::from_utf16(&data[..data.len() - 1]).map_err(|_| "Invalid service encoding.".into())
}
fn native_service(id: &str) -> Result<(String, bool), String> {
    let id = wide(id)?;
    let mut node = 0;
    checked(unsafe { CM_Locate_DevNodeW(&mut node, id.as_ptr(), 0) })?;
    let mut kind = 0;
    let mut data = [0u16; 128];
    let mut bytes = (data.len() * 2) as u32;
    checked(unsafe {
        CM_Get_DevNode_Registry_PropertyW(
            node,
            CM_DRP_SERVICE,
            &mut kind,
            data.as_mut_ptr().cast(),
            &mut bytes,
            0,
        )
    })?;
    let service = service_text(kind, &data, bytes)?;
    let mut status = 0;
    let mut problem = 0;
    checked(unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, node, 0) })?;
    Ok((service, problem == 0 && status & DN_STARTED != 0))
}
fn select(
    ids: Vec<String>,
    mut inspect: impl FnMut(&str) -> Result<(String, bool), String>,
) -> Result<String, String> {
    if ids.len() != 1 || !device_identity(&ids[0]) {
        return Err("Expected exactly one qualified root vJoy device.".into());
    }
    let id = ids.into_iter().next().unwrap();
    let (service, started) = inspect(&id)?;
    if !service.eq_ignore_ascii_case("vjoy") || !started {
        return Err("Selected root device is not a started vJoy service.".into());
    }
    Ok(id)
}
/// Read-only identity check; it never restarts or acquires a device.
pub fn device() -> Result<String, String> {
    select(native_ids()?, native_service)
}
fn system_program() -> Result<PathBuf, String> {
    let mut buffer = [0u16; 512];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 || length >= buffer.len() as u32 {
        return Err("Windows system directory is unavailable.".into());
    }
    let path = String::from_utf16(&buffer[..length as usize])
        .map_err(|_| "Invalid system directory encoding.")?;
    let program = PathBuf::from(path).join("pnputil.exe");
    if !program.is_absolute() || !program.is_file() {
        return Err("Windows PnP utility is unavailable.".into());
    }
    Ok(program)
}
/// Whether the exact spawned child is proven to have stopped. An unconfirmed
/// child must leave the durable refresh guard set, including after helper death.
#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub quiescent: bool,
}
impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self {
            message,
            quiescent: true,
        }
    }
}
impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
trait ChildControl {
    fn status(&mut self) -> Result<Option<bool>, String>;
    fn terminate(&mut self) -> Result<(), String>;
}
impl ChildControl for std::process::Child {
    fn status(&mut self) -> Result<Option<bool>, String> {
        self.try_wait()
            .map(|s| s.map(|s| s.success()))
            .map_err(|e| e.to_string())
    }
    fn terminate(&mut self) -> Result<(), String> {
        self.kill().map_err(|e| e.to_string())
    }
}
fn await_child(
    child: &mut impl ChildControl,
    timeout: Duration,
    reap_timeout: Duration,
) -> Result<(), Failure> {
    let started = Instant::now();
    let reason = loop {
        match child.status() {
            Ok(Some(true)) => return Ok(()),
            Ok(Some(false)) => {
                return Err("Driver refresh exited unsuccessfully; verify the driver state.".into())
            }
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(5)),
            Err(error) => break format!("Driver refresh wait failed: {error}"),
            _ => break "Driver refresh deadline expired.".to_owned(),
        }
    };
    let termination = child.terminate().err();
    let reap = Instant::now();
    loop {
        if child.status().ok().flatten().is_some() {
            return Err(Failure {
                message: reason,
                quiescent: true,
            });
        }
        if reap.elapsed() >= reap_timeout {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    Err(Failure {
        message: format!(
            "{reason} Child exit is unconfirmed; preserve configuration for recovery.{}",
            termination
                .map(|e| format!(" Termination failed: {e}"))
                .unwrap_or_default()
        ),
        quiescent: false,
    })
}
fn bounded(program: &Path, args: &[String], timeout: Duration) -> Result<(), Failure> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| Failure::from(format!("Unable to launch driver refresh: {e}")))?;
    await_child(&mut child, timeout, Duration::from_millis(500))
}
fn refresh_with(
    slots: Vec<Slot>,
    resolve: impl FnOnce() -> Result<String, String>,
    run: impl FnOnce(&str) -> Result<(), Failure>,
) -> Result<(), Failure> {
    if slots.is_empty() || slots.len() > 16 || slots.iter().any(|s| s.status != Status::Free) {
        return Err("Close all vJoy feeders before refreshing the driver.".into());
    }
    let id = resolve()?;
    if !device_identity(&id) {
        return Err("Unqualified refresh target.".into());
    }
    run(&id)
}
/// Requires explicit elevation/operation ownership; layout readback follows in
/// the provisioning transaction. A successful utility exit alone proves nothing.
pub fn refresh() -> Result<(), Failure> {
    refresh_with(crate::inspection::current()?, device, |id| {
        bounded(
            &system_program()?,
            &["/restart-device".into(), id.into()],
            Duration::from_secs(20),
        )
    })
}
#[cfg(test)]
#[path = "windows_refresh_tests.rs"]
mod tests;
