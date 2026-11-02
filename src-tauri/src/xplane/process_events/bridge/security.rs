use super::{
    super::native::{Handle, RawHandle},
    protocol::Request,
};
use crate::game_discovery::{normalized_path, ProcessIdentity};
use std::{ffi::c_void, os::windows::io::AsRawHandle, ptr};
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions};

#[repr(C)]
struct SidAndAttributes {
    sid: *mut c_void,
    attributes: u32,
}
#[repr(C)]
struct SecurityAttributes {
    length: u32,
    descriptor: *mut c_void,
    inherit: i32,
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: RawHandle, access: u32, token: *mut RawHandle) -> i32;
    fn GetTokenInformation(
        token: RawHandle,
        class: u32,
        data: *mut c_void,
        length: u32,
        needed: *mut u32,
    ) -> i32;
    fn IsValidSid(sid: *const c_void) -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> RawHandle;
    fn GetProcessId(process: RawHandle) -> u32;
    fn GetNamedPipeClientProcessId(pipe: RawHandle, pid: *mut u32) -> i32;
    fn GetNamedPipeServerProcessId(pipe: RawHandle, pid: *mut u32) -> i32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}
#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(algorithm: RawHandle, data: *mut u8, length: u32, flags: u32) -> i32;
}
fn error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn token(process: RawHandle) -> Result<Handle, String> {
    let mut raw = ptr::null_mut();
    if unsafe { OpenProcessToken(process, 8, &mut raw) } == 0 {
        return Err(error("Cannot inspect native process user"));
    }
    Ok(unsafe { Handle::from_raw(raw) })
}
fn sid(process: RawHandle) -> Result<Vec<u8>, String> {
    let token = token(process)?;
    let mut needed = 0;
    unsafe {
        GetTokenInformation(token.raw(), 1, ptr::null_mut(), 0, &mut needed);
    }
    if !(std::mem::size_of::<SidAndAttributes>() as u32..=4096).contains(&needed) {
        return Err("Invalid process token size.".into());
    }
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    let capacity = needed;
    if unsafe {
        GetTokenInformation(
            token.raw(),
            1,
            buffer.as_mut_ptr().cast(),
            capacity,
            &mut needed,
        )
    } == 0
        || needed > capacity
    {
        return Err(error("Cannot read native process user"));
    }
    let record = unsafe { &*buffer.as_ptr().cast::<SidAndAttributes>() };
    let begin = buffer.as_ptr() as usize;
    let end = begin + needed as usize;
    let address = record.sid as usize;
    if address < begin || address.checked_add(8).is_none_or(|value| value > end) {
        return Err("Invalid process user SID.".into());
    }
    let length = 8 + 4 * unsafe { *record.sid.cast::<u8>().add(1) } as usize;
    if !(8..=68).contains(&length)
        || address.checked_add(length).is_none_or(|value| value > end)
        || unsafe { IsValidSid(record.sid) } == 0
    {
        return Err("Invalid process user SID bounds.".into());
    }
    Ok(unsafe { std::slice::from_raw_parts(record.sid.cast::<u8>(), length) }.to_vec())
}
pub(super) fn same_user(process: &Handle) -> Result<(), String> {
    if sid(process.raw())? != sid(unsafe { GetCurrentProcess() })? {
        return Err("Process-event monitoring requires the same Windows user as Cockpit.".into());
    }
    Ok(())
}
pub(super) fn elevated() -> Result<bool, String> {
    let token = token(unsafe { GetCurrentProcess() })?;
    let mut elevation = 0u32;
    let mut needed = 0;
    if unsafe {
        GetTokenInformation(
            token.raw(),
            20,
            (&mut elevation as *mut u32).cast(),
            4,
            &mut needed,
        )
    } == 0
        || needed != 4
    {
        return Err(error("Cannot verify process-events helper elevation"));
    }
    Ok(elevation != 0)
}
pub(super) fn current() -> Result<ProcessIdentity, String> {
    let pid = std::process::id();
    Handle::open(pid)?.identity(pid)
}
pub(super) fn authenticate_parent(
    request: &Request,
    pipe: &NamedPipeClient,
) -> Result<Handle, String> {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
        return Err(error("Cannot authenticate process-events pipe server"));
    }
    if pid != request.parent_pid {
        return Err("Process-events pipe server PID does not match Cockpit.".into());
    }
    let process = Handle::open(pid)?;
    let actual = process.identity(pid)?;
    if actual.epoch != request.parent_epoch {
        return Err("Process-events parent creation identity changed.".into());
    }
    let own = current()?;
    if normalized_path(&actual.path) != normalized_path(&own.path) {
        return Err("Process-events parent is not this Cockpit executable.".into());
    }
    same_user(&process)?;
    process.running()?;
    Ok(process)
}
pub(super) fn authenticate_child(pipe: &NamedPipeServer, child: &Handle) -> Result<(), String> {
    let mut pid = 0;
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
        return Err(error("Cannot authenticate process-events pipe client"));
    }
    let launched_pid = unsafe { GetProcessId(child.raw()) };
    if launched_pid == 0 || pid != launched_pid {
        return Err("Process-events pipe client is not the launched helper.".into());
    }
    let actual = child.identity(launched_pid)?;
    let own = current()?;
    if normalized_path(&actual.path) != normalized_path(&own.path) {
        return Err("Process-events pipe client is not this Cockpit executable.".into());
    }
    same_user(child)?;
    child.running()
}
pub(super) fn nonce() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    let status =
        unsafe { BCryptGenRandom(ptr::null_mut(), bytes.as_mut_ptr(), bytes.len() as u32, 2) };
    if status < 0 {
        return Err(format!(
            "Cannot generate process-events request nonce ({status:#x})."
        ));
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn sid_text(bytes: &[u8]) -> String {
    let authority = bytes[2..8]
        .iter()
        .fold(0u64, |value, byte| (value << 8) | u64::from(*byte));
    let mut text = format!("S-{}-{authority}", bytes[0]);
    for chunk in bytes[8..].chunks_exact(4) {
        text.push_str(&format!(
            "-{}",
            u32::from_le_bytes(chunk.try_into().unwrap())
        ));
    }
    text
}
struct Descriptor(*mut c_void);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
pub(super) fn server(request: &Request) -> Result<NamedPipeServer, String> {
    let user = sid_text(&sid(unsafe { GetCurrentProcess() })?);
    // A protected DACL grants only this user and SYSTEM. Never inherit ambient ACLs.
    let sddl = wide(&format!("D:P(A;;GA;;;{user})(A;;GA;;;SY)"));
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(error("Cannot protect process-events pipe"));
    }
    let descriptor = Descriptor(descriptor);
    let mut attributes = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        descriptor: descriptor.0,
        inherit: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .max_instances(1)
        .in_buffer_size(128 * 1024)
        .out_buffer_size(128 * 1024);
    unsafe {
        options.create_with_security_attributes_raw(
            request.pipe(),
            (&mut attributes as *mut SecurityAttributes).cast(),
        )
    }
    .map_err(|e| format!("Cannot create private process-events pipe: {e}"))
}
