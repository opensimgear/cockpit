//! Unelevated SendInput, physical scan codes, session exclusion and foreground
//! process creation identity. No GetAsyncKeyState ownership inference is used.
use crate::{keys::scan_code, protocol::*};
use osg_mapping::desktop::MouseButton;
use std::{mem::size_of, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, FILETIME, HANDLE},
    Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, IsValidSid,
        TokenIntegrityLevel, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    },
    System::{StationsAndDesktops::*, SystemInformation::GetTickCount64, Threading::*},
    UI::{
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId, WHEEL_DELTA, XBUTTON1, XBUTTON2,
        },
    },
};
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn failure(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}
pub fn now_us() -> u64 {
    unsafe { GetTickCount64().saturating_mul(1000) }
}
fn open(pid: u32) -> Result<Handle, String> {
    let h = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if h.is_null() {
        Err(failure("Cannot inspect process"))
    } else {
        Ok(Handle(h))
    }
}
fn identity(handle: HANDLE, pid: u32) -> Result<ProcessIdentity, String> {
    let (mut created, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    if unsafe { GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(failure("Cannot read process creation identity"));
    }
    let mut image = vec![0u16; 32768];
    let mut size = image.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle, 0, image.as_mut_ptr(), &mut size) } == 0
        || size == 0
    {
        return Err(failure("Cannot read process executable"));
    }
    let executable =
        String::from_utf16(&image[..size as usize]).map_err(|_| "Invalid executable encoding.")?;
    let created =
        (((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64).to_string();
    Ok(ProcessIdentity {
        executable,
        pid,
        created,
    })
}
pub fn process_identity(pid: u32) -> Result<ProcessIdentity, String> {
    let handle = open(pid)?;
    identity(handle.0, pid)
}
pub fn foreground_identity() -> Result<ProcessIdentity, String> {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return Err("No foreground application can be identified.".into());
    }
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(foreground, &mut pid);
    }
    if pid == 0 {
        return Err("Foreground application identity is unknown.".into());
    }
    process_identity(pid)
}
fn same_identity(a: &ProcessIdentity, b: &ProcessIdentity) -> bool {
    a.pid == b.pid && a.created == b.created && a.executable.eq_ignore_ascii_case(&b.executable)
}
pub fn process_alive(expected: &ProcessIdentity) -> Result<bool, String> {
    let h = open(expected.pid)?;
    Ok(unsafe { WaitForSingleObject(h.0, 0) } == 258
        && same_identity(&identity(h.0, expected.pid)?, expected))
}
fn integrity(process: HANDLE) -> Result<u32, String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(failure("Cannot inspect process integrity"));
    }
    let token = Handle(token);
    let mut length = 0;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            ptr::null_mut(),
            0,
            &mut length,
        );
    }
    if length < size_of::<TOKEN_MANDATORY_LABEL>() as u32 || length > 4096 {
        return Err("Invalid process integrity token bounds.".into());
    }
    let mut data = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            data.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(failure("Cannot inspect process integrity"));
    }
    let label = unsafe { &*data.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    let start = data.as_ptr() as usize;
    let end = start + length as usize;
    let sid = label.Label.Sid as usize;
    if sid < start || sid.checked_add(8).is_none_or(|v| v > end) {
        return Err("Invalid integrity SID bounds.".into());
    }
    let count = unsafe { *GetSidSubAuthorityCount(label.Label.Sid) };
    if count == 0
        || sid
            .checked_add(8 + 4 * count as usize)
            .is_none_or(|v| v > end)
        || unsafe { IsValidSid(label.Label.Sid) } == 0
    {
        return Err("Invalid integrity SID.".into());
    }
    Ok(unsafe { *GetSidSubAuthority(label.Label.Sid, u32::from(count) - 1) })
}
fn interactive_desktop() -> Result<bool, String> {
    let desktop = unsafe { OpenInputDesktop(0, 0, DESKTOP_READOBJECTS) };
    if desktop.is_null() {
        return Ok(false);
    }
    let mut text = [0u16; 128];
    let mut needed = 0;
    let result = unsafe {
        GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            text.as_mut_ptr().cast(),
            (text.len() * 2) as u32,
            &mut needed,
        )
    };
    unsafe {
        CloseDesktop(desktop);
    }
    if result == 0 {
        return Ok(false);
    }
    let len = text.iter().position(|c| *c == 0).unwrap_or(text.len());
    Ok(String::from_utf16_lossy(&text[..len]).eq_ignore_ascii_case("Default"))
}
pub fn eligible_scope(scope: &Scope) -> Result<bool, String> {
    if !interactive_desktop()? {
        return Ok(false);
    }
    let current = foreground_identity()?;
    let handle = open(current.pid)?;
    if integrity(handle.0)? > integrity(unsafe { GetCurrentProcess() })? {
        return Err(
            "Foreground application has higher integrity; run the target unelevated.".into(),
        );
    }
    Ok(match scope {
        Scope::DesktopWide => true,
        Scope::Application { identity } => same_identity(identity, &current),
    })
}
/// Local namespace is Windows-session scoped and spans desktop/browser services.
/// An event is used as lifetime exclusion, avoiding thread-affine mutex ownership.
pub struct SessionLock {
    _handle: Handle,
}
impl SessionLock {
    pub fn acquire() -> Result<Self, String> {
        let name: Vec<u16> = "Local\\OpenSimGear.DesktopInput.v1"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let h = unsafe { CreateEventW(ptr::null(), 1, 0, name.as_ptr()) };
        if h.is_null() {
            return Err(failure("Cannot acquire desktop coordinator lock"));
        }
        let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let guard = Self { _handle: Handle(h) };
        if exists {
            Err(
                "Another Cockpit process owns keyboard and mouse output in this Windows session."
                    .into(),
            )
        } else {
            Ok(guard)
        }
    }
}
unsafe impl Send for SessionLock {}
pub struct Native {
    _exclusive: SessionLock,
}
impl Native {
    pub fn open() -> Result<Self, String> {
        if !interactive_desktop()? {
            return Err("Interactive desktop is locked or unavailable.".into());
        }
        Ok(Self {
            _exclusive: SessionLock::acquire()?,
        })
    }
}
fn keyboard(usage: u16, down: bool) -> Result<INPUT, String> {
    let scan = scan_code(usage)?;
    Ok(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: scan.code,
                dwFlags: KEYEVENTF_SCANCODE
                    | if scan.extended {
                        KEYEVENTF_EXTENDEDKEY
                    } else {
                        0
                    }
                    | if down { 0 } else { KEYEVENTF_KEYUP },
                time: 0,
                dwExtraInfo: 0x4f534744,
            },
        },
    })
}
fn mouse(dx: i32, dy: i32, data: u32, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0x4f534744,
            },
        },
    }
}
pub fn records(events: &[NativeEvent]) -> Result<Vec<INPUT>, String> {
    let mut inputs = Vec::new();
    for event in events {
        match event {
            NativeEvent::Key { usage, down, .. } => inputs.push(keyboard(*usage, *down)?),
            NativeEvent::MouseButton { button, down } => {
                let (flags, data) = match button {
                    MouseButton::Left => (
                        if *down {
                            MOUSEEVENTF_LEFTDOWN
                        } else {
                            MOUSEEVENTF_LEFTUP
                        },
                        0,
                    ),
                    MouseButton::Right => (
                        if *down {
                            MOUSEEVENTF_RIGHTDOWN
                        } else {
                            MOUSEEVENTF_RIGHTUP
                        },
                        0,
                    ),
                    MouseButton::Middle => (
                        if *down {
                            MOUSEEVENTF_MIDDLEDOWN
                        } else {
                            MOUSEEVENTF_MIDDLEUP
                        },
                        0,
                    ),
                    MouseButton::X1 => (
                        if *down {
                            MOUSEEVENTF_XDOWN
                        } else {
                            MOUSEEVENTF_XUP
                        },
                        XBUTTON1 as u32,
                    ),
                    MouseButton::X2 => (
                        if *down {
                            MOUSEEVENTF_XDOWN
                        } else {
                            MOUSEEVENTF_XUP
                        },
                        XBUTTON2 as u32,
                    ),
                };
                inputs.push(mouse(0, 0, data, flags));
            }
            NativeEvent::Pointer { dx, dy } => {
                if dx.unsigned_abs() > 32767 || dy.unsigned_abs() > 32767 {
                    return Err("Relative pointer step exceeds its bound.".into());
                }
                inputs.push(mouse(*dx, *dy, 0, MOUSEEVENTF_MOVE));
            }
            NativeEvent::Wheel {
                horizontal,
                vertical,
            } => {
                for (amount, flags) in [
                    (*horizontal, MOUSEEVENTF_HWHEEL),
                    (*vertical, MOUSEEVENTF_WHEEL),
                ] {
                    if amount != 0 {
                        let data = amount
                            .checked_mul(WHEEL_DELTA as i32)
                            .ok_or("Wheel step overflow.")?;
                        inputs.push(mouse(0, 0, data as u32, flags));
                    }
                }
            }
        }
    }
    if inputs.len() > MAX_BATCH {
        return Err("SendInput batch exceeds 64 records.".into());
    }
    Ok(inputs)
}
impl Adapter for Native {
    fn eligible(&mut self, scope: &Scope) -> Result<bool, String> {
        eligible_scope(scope)
    }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        let inputs = records(events)?;
        if inputs.is_empty() {
            return Ok(0);
        }
        let confirmed = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                size_of::<INPUT>() as i32,
            )
        } as usize;
        // Capture the end boundary immediately after the actual native call;
        // formatting, queueing and ACK handling occur after this timestamp.
        if let Some(returned) = crate::audit::stamp() {
            let context = crate::worker::submission_context();
            if context.is_none() {
                crate::audit::fault(crate::audit::Fault::Context);
            }
            crate::audit::record_at(returned, crate::audit::Kind::SubmitReturned, &[
                crate::audit::channel().unwrap_or(0),
                context.map_or(0, |value| value.request),
                context.map_or(0, |value| value.operation),
                context.map_or(0, |value| value.category),
                inputs.len() as u64,
                confirmed as u64,
            ]);
        }
        Ok(confirmed)
    }
}
