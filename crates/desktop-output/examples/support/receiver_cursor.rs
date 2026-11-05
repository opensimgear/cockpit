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
