use super::*;
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};

pub(super) const RECEIVER_CLASS: &str = "OpenSimGearDesktopReceiver";
pub(super) const ALLOW_RECEIVER_FOREGROUND: u32 = WM_APP + 97;

pub(super) struct FocusChange {
    pub(super) method: &'static str,
    pub(super) before: usize,
    pub(super) after: usize,
    pub(super) capture_before: usize,
    pub(super) capture_after: usize,
}

pub(crate) unsafe fn foreground(window: HWND) {
    let thread = GetCurrentThreadId();
    let other = GetWindowThreadProcessId(GetForegroundWindow(), ptr::null_mut());
    let attached = other != 0 && other != thread && AttachThreadInput(thread, other, 1) != 0;
    ShowWindow(window, SW_RESTORE);
    SetWindowPos(
        window,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
    );
    SetForegroundWindow(window);
    SetActiveWindow(window);
    SetFocus(window);
    if attached {
        AttachThreadInput(thread, other, 0);
    }
}

unsafe fn receiver_identity(window: HWND) -> Result<ProcessIdentity, String> {
    if window.is_null() || IsWindow(window) == 0 || GetAncestor(window, GA_ROOT) != window {
        return Err("Cooperative focus requires an existing top-level receiver window.".into());
    }
    let mut class = [0u16; 64];
    let length = GetClassNameW(window, class.as_mut_ptr(), class.len() as i32);
    if length <= 0 || String::from_utf16_lossy(&class[..length as usize]) != RECEIVER_CLASS {
        return Err("Cooperative focus requires the exact receiver window class.".into());
    }
    let mut pid = 0;
    if GetWindowThreadProcessId(window, &mut pid) == 0 || pid == 0 {
        return Err("Cannot identify the cooperative receiver process.".into());
    }
    let identity = process_identity(pid)?;
    let mut current_pid = 0;
    if !process_alive(&identity)?
        || GetWindowThreadProcessId(window, &mut current_pid) == 0
        || current_pid != pid
    {
        return Err("The cooperative receiver process changed or exited.".into());
    }
    Ok(identity)
}

pub(super) unsafe fn allow_receiver_foreground(window: HWND, target: HWND, pid: u32) -> bool {
    if !RECEIVE_ONLY.load(Ordering::Relaxed)
        || GetForegroundWindow() != window
        || target == window
        || pid == 0
        || pid == ASFW_ANY
    {
        return false;
    }
    let (Ok(source), Ok(target)) = (receiver_identity(window), receiver_identity(target)) else {
        return false;
    };
    source.pid == GetCurrentProcessId()
        && target.pid == pid
        && source.pid != target.pid
        && source.executable.eq_ignore_ascii_case(&target.executable)
        && AllowSetForegroundWindow(target.pid) != 0
}

pub(super) unsafe fn receiver_foreground(window: HWND) -> Result<FocusChange, String> {
    let before = GetForegroundWindow();
    let capture_before = GetCapture() as usize;
    let target = receiver_identity(window)?;
    if target.pid != GetCurrentProcessId() {
        return Err("The focus target must belong to this receiver process.".into());
    }
    let method = if before == window {
        "already-foreground"
    } else if let Ok(source) = receiver_identity(before) {
        if source.pid == target.pid
            || !source.executable.eq_ignore_ascii_case(&target.executable)
        {
            return Err("Cooperative focus requires a separate receiver from the same executable.".into());
        }
        let mut granted = 0usize;
        if SendMessageTimeoutW(
            before,
            ALLOW_RECEIVER_FOREGROUND,
            window as WPARAM,
            target.pid as LPARAM,
            SMTO_BLOCK | SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
            500,
            &mut granted,
        ) == 0
            || granted != 1
        {
            return Err("The foreground receiver did not grant the bounded focus handoff.".into());
        }
        if GetForegroundWindow() != before
            || receiver_identity(before)? != source
            || receiver_identity(window)? != target
        {
            return Err("Receiver identity or foreground changed during the focus handoff.".into());
        }
        "cooperative-receiver"
    } else {
        "direct-no-attach"
    };
    if before != window {
        SetForegroundWindow(window);
    }
    if GetForegroundWindow() != window {
        return Err("The receiver did not acquire the actual foreground window.".into());
    }
    SetActiveWindow(window);
    SetFocus(window);
    let after = GetForegroundWindow();
    if after != window {
        return Err("The receiver lost foreground during activation.".into());
    }
    Ok(FocusChange {
        method,
        before: before as usize,
        after: after as usize,
        capture_before,
        capture_after: GetCapture() as usize,
    })
}
