use super::*;
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};

pub(super) unsafe fn client_cursor(window: HWND) -> Result<serde_json::Value, String> {
    let mut rect = RECT::default();
    let mut point = POINT::default();
    if GetClientRect(window, &mut rect) == 0
        || GetCursorPos(&mut point) == 0
        || ScreenToClient(window, &mut point) == 0
    {
        return Err("Cannot observe the receive-only client cursor.".into());
    }
    Ok(serde_json::json!({"x":point.x,"y":point.y,"width":rect.right,"height":rect.bottom,
        "inside":point.x>=0&&point.y>=0&&point.x<rect.right&&point.y<rect.bottom}))
}

pub(super) unsafe fn pointer_target(window: HWND) -> serde_json::Value {
    let mut point = POINT::default();
    let observed = GetCursorPos(&mut point) != 0;
    let target = if observed { WindowFromPoint(point) } else { ptr::null_mut() };
    let mut pid = 0;
    if !target.is_null() { GetWindowThreadProcessId(target, &mut pid); }
    let mut foreground = GUITHREADINFO { cbSize: size_of::<GUITHREADINFO>() as u32, ..Default::default() };
    let observed_capture = GetGUIThreadInfo(0, &mut foreground) != 0;
    serde_json::json!({"exactReceiver":observed && target == window,
        "window":(target as usize).to_string(),"pid":pid,
        "foregroundCapture":observed_capture.then(|| (foreground.hwndCapture as usize).to_string())})
}

pub(super) unsafe fn mouse_button_state(
    window: HWND,
    scope: &Scope,
    sequence: u64,
) -> serde_json::Value {
    let swap_buttons = GetSystemMetrics(SM_SWAPBUTTON) != 0;
    let eligible_before = eligible_scope(scope).unwrap_or(false);
    let before = GetForegroundWindow();
    let down = if eligible_before && before == window {
        Some(GetAsyncKeyState(VK_LBUTTON as i32) < 0)
    } else {
        None
    };
    let at_us = now_us();
    let eligible_after = eligible_scope(scope).unwrap_or(false);
    let after = GetForegroundWindow();
    let available = down.is_some()
        && eligible_after
        && after == window
        && (GetSystemMetrics(SM_SWAPBUTTON) != 0) == swap_buttons;
    serde_json::json!({"sequence":sequence.to_string(),"atUs":at_us.to_string(),
        "available":available,"leftButtonDown":if available {down} else {None},
        "swapButtons":swap_buttons,"foregroundBefore":(before as usize).to_string(),
        "foregroundAfter":(after as usize).to_string()})
}

pub(super) unsafe fn center_cursor(window: HWND) -> Result<(), String> {
    let mut rect = RECT::default();
    if GetClientRect(window, &mut rect) == 0 || rect.right <= 0 || rect.bottom <= 0 {
        return Err("The receive-only window has no client area.".into());
    }
    let mut point = POINT {
        x: rect.right / 2,
        y: rect.bottom / 2,
    };
    if ClientToScreen(window, &mut point) == 0 || SetCursorPos(point.x, point.y) == 0 {
        return Err("Cannot center the cursor in the receive-only client area.".into());
    }
    Ok(())
}

pub(super) unsafe fn position_after_focus(window: HWND, request: &str) -> Result<(FocusChange, Option<String>), String> {
    let focus = receiver_focus::receiver_foreground(window)?;
    let pointer_error = match request {
        "focus-and-center" => { center_cursor(window)?; None }
        "focus-and-position-pointer" => position_pointer(window).err(),
        _ => None,
    };
    Ok((focus, pointer_error))
}

unsafe fn position_pointer(window: HWND) -> Result<(), String> {
    const MARGIN: i32 = 32;
    // Raise only the fixture window before testing its visible client area.
    SetWindowPos(window, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    let mut rect = RECT::default();
    if GetClientRect(window, &mut rect) == 0 || rect.right <= 2 * MARGIN || rect.bottom <= 2 * MARGIN {
        return Err("The receive-only window has no room for bounded pointer movement.".into());
    }
    for x in [rect.right / 2, rect.right / 4, rect.right * 3 / 4] {
        for y in [rect.bottom / 2, rect.bottom / 4, rect.bottom * 3 / 4] {
            if x < MARGIN || y < MARGIN || x + MARGIN >= rect.right || y + MARGIN >= rect.bottom {
                continue;
            }
            let mut point = POINT { x, y };
            if ClientToScreen(window, &mut point) == 0 { continue; }
            let clear = [-MARGIN, 0, MARGIN].into_iter().all(|dx| {
                [-MARGIN, 0, MARGIN].into_iter().all(|dy| {
                    WindowFromPoint(POINT { x: point.x + dx, y: point.y + dy }) == window
                })
            });
            if clear && GetForegroundWindow() == window && SetCursorPos(point.x, point.y) != 0 {
                return Ok(());
            }
        }
    }
    Err("The foreground receiver has no unobstructed area for pointer movement.".into())
}
