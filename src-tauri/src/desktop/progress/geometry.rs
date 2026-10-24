use super::*;
#[derive(Debug, PartialEq, Eq)]
pub(super) struct NotificationGeometry {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: u32,
    pub(super) height: u32,
}

pub(super) fn notification_geometry(
    position: NotificationPosition,
    origin: (i32, i32),
    area: (u32, u32),
    scale: f64,
) -> Result<NotificationGeometry, String> {
    if area.0 == 0 || area.1 == 0 || !scale.is_finite() || scale <= 0.0 {
        return Err("The notification monitor has an invalid work area or scale.".into());
    }
    let pixels = |logical: f64| (logical * scale).round().clamp(1.0, u32::MAX as f64) as u32;
    let width = pixels(NOTIFICATION_WIDTH).min(area.0);
    let height = pixels(NOTIFICATION_HEIGHT).min(area.1);
    let inset = pixels(NOTIFICATION_INSET);
    // An oversized notification occupies the available dimension. Insets shrink
    // when necessary, keeping every edge within even a tiny work area.
    let horizontal_inset = inset.min((area.0 - width) / 2);
    let vertical_inset = inset.min((area.1 - height) / 2);
    let left = i64::from(origin.0) + i64::from(horizontal_inset);
    let right = i64::from(origin.0) + i64::from(area.0 - width - horizontal_inset);
    let top = i64::from(origin.1) + i64::from(vertical_inset);
    let bottom = i64::from(origin.1) + i64::from(area.1 - height - vertical_inset);
    let x = if matches!(position, NotificationPosition::BottomRight | NotificationPosition::TopRight) { right } else { left };
    let y = if matches!(position, NotificationPosition::BottomRight | NotificationPosition::BottomLeft) { bottom } else { top };
    Ok(NotificationGeometry {
        x: i32::try_from(x).map_err(|_| "The notification monitor position is out of range.")?,
        y: i32::try_from(y).map_err(|_| "The notification monitor position is out of range.")?,
        width,
        height,
    })
}

pub(super) fn position_notification(main: &WebviewWindow, child: &WebviewWindow, settings: ProgressSettings) -> Result<(), String> {
    let monitor = match main.current_monitor().map_err(|error| error.to_string())? {
        Some(monitor) => Some(monitor),
        None => main.primary_monitor().map_err(|error| error.to_string())?,
    }
        .ok_or("No monitor is available for profile progress.")?;
    let area = monitor.work_area();
    let geometry = notification_geometry(settings.position, (area.position.x, area.position.y), (area.size.width, area.size.height), monitor.scale_factor())?;
    apply_notification_geometry(child, geometry, (area.position.x, area.position.y, area.size.width, area.size.height))
}

#[cfg(target_os = "windows")]
fn apply_notification_geometry(child: &WebviewWindow, geometry: NotificationGeometry, work_area: (i32, i32, u32, u32)) -> Result<(), String> {
    use windows::Win32::{
        Foundation::{HWND, RECT},
        UI::WindowsAndMessaging::{GetClientRect, GetWindowRect, SetWindowPos, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER},
    };
    let handle = child.hwnd().map_err(|error| error.to_string())?.0 as usize;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    child.run_on_main_thread(move || {
        let result = (|| {
            let handle = HWND(handle as *mut core::ffi::c_void);
            let flags = SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED;
            // Force nonclient recalculation after the monitor/DPI move. Tao's
            // undecorated WM_NCCALCSIZE path needs wParam!=0; its normal inner
            // sizing can otherwise retain an invisible caption's dimensions.
            unsafe { SetWindowPos(handle, None, geometry.x, geometry.y, 0, 0, flags | SWP_NOSIZE) }
                .map_err(|error| format!("Cannot position the profile notification: {error}"))?;
            for _ in 0..3 {
                let mut client = RECT::default();
                let mut outer = RECT::default();
                unsafe { GetClientRect(handle, &mut client) }.map_err(|error| error.to_string())?;
                unsafe { GetWindowRect(handle, &mut outer) }.map_err(|error| error.to_string())?;
                let width = i64::from(outer.right) - i64::from(outer.left)
                    + i64::from(geometry.width) - (i64::from(client.right) - i64::from(client.left));
                let height = i64::from(outer.bottom) - i64::from(outer.top)
                    + i64::from(geometry.height) - (i64::from(client.bottom) - i64::from(client.top));
                let width = i32::try_from(width).ok().filter(|width| *width > 0).ok_or("Invalid native profile notification width.")?;
                let height = i32::try_from(height).ok().filter(|height| *height > 0).ok_or("Invalid native profile notification height.")?;
                unsafe { SetWindowPos(handle, None, geometry.x, geometry.y, width, height, flags) }
                    .map_err(|error| format!("Cannot size the profile notification: {error}"))?;
                unsafe { GetClientRect(handle, &mut client) }.map_err(|error| error.to_string())?;
                unsafe { GetWindowRect(handle, &mut outer) }.map_err(|error| error.to_string())?;
                if i64::from(client.right) - i64::from(client.left) == i64::from(geometry.width)
                    && i64::from(client.bottom) - i64::from(client.top) == i64::from(geometry.height)
                    && outer.left == geometry.x && outer.top == geometry.y
                {
                    if outer.left < work_area.0 || outer.top < work_area.1
                        || i64::from(outer.right) > i64::from(work_area.0) + i64::from(work_area.2)
                        || i64::from(outer.bottom) > i64::from(work_area.1) + i64::from(work_area.3)
                    {
                        return Err("Windows could not fit the profile notification frame inside the monitor work area.".into());
                    }
                    return Ok(());
                }
            }
            Err("Windows could not apply the requested profile notification client size and position.".into())
        })();
        let _ = sender.send(result);
    }).map_err(|error| error.to_string())?;
    receiver.recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|error| format!("The profile notification could not be positioned on its window thread: {error}"))?
}

#[cfg(not(target_os = "windows"))]
fn apply_notification_geometry(child: &WebviewWindow, geometry: NotificationGeometry, _work_area: (i32, i32, u32, u32)) -> Result<(), String> {
    // Moving across monitors can trigger a native DPI resize. Move first, then
    // set the target monitor's physical size and reapply the clamped position.
    child.set_position(tauri::PhysicalPosition::new(geometry.x, geometry.y)).map_err(|error| error.to_string())?;
    child.set_size(tauri::PhysicalSize::new(geometry.width, geometry.height)).map_err(|error| error.to_string())?;
    child.set_position(tauri::PhysicalPosition::new(geometry.x, geometry.y)).map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeNotificationGeometry {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) scale: f64,
    work_area: NativeWorkArea,
}

#[derive(Clone, Debug, Serialize)]
struct NativeWorkArea {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: u32,
    pub(super) height: u32,
}

pub(super) fn native_notification_geometry(child: &WebviewWindow) -> Option<NativeNotificationGeometry> {
    let origin = child.outer_position().ok()?;
    let size = child.inner_size().ok()?;
    let scale = child.scale_factor().ok()?;
    if !scale.is_finite() || scale <= 0.0 { return None; }
    let monitor = match child.current_monitor().ok()? {
        Some(monitor) => monitor,
        None => child.primary_monitor().ok()??,
    };
    let area = monitor.work_area();
    Some(NativeNotificationGeometry {
        x: origin.x, y: origin.y, width: size.width, height: size.height, scale,
        work_area: NativeWorkArea { x: area.position.x, y: area.position.y, width: area.size.width, height: area.size.height },
    })
}

