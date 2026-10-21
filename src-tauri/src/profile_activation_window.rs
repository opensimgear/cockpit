use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{Manager, WebviewUrl, WebviewWindow};

const LABEL: &str = "profile-activation";
const NOTIFICATION_WIDTH: f64 = 420.0;
const NOTIFICATION_HEIGHT: f64 = 96.0;
const NOTIFICATION_INSET: f64 = 16.0;
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ProgressDelivery {
    #[default]
    System,
    Window,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum NotificationPosition {
    #[default]
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProgressSettings {
    #[serde(default)]
    delivery: ProgressDelivery,
    position: NotificationPosition,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SavedProgressSettings {
    schema_version: u32,
    #[serde(default)]
    delivery: ProgressDelivery,
    position: NotificationPosition,
}

fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir()
        .map(|directory| directory.join("profile-progress-preferences.json"))
        .map_err(|error| format!("Cannot locate profile progress settings: {error}"))
}

fn read_settings(path: &Path) -> Result<ProgressSettings, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(ProgressSettings::default()),
        Err(error) => return Err(format!("Cannot read profile progress settings: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read profile progress settings: {error}"))?;
    if bytes.len() > 4096 {
        return Err("Saved profile progress settings exceed the size limit.".into());
    }
    let saved: SavedProgressSettings = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Saved profile progress settings are invalid: {error}"))?;
    if saved.schema_version != 1 {
        return Err("Saved profile progress settings use an unsupported version.".into());
    }
    Ok(ProgressSettings { delivery: saved.delivery, position: saved.position })
}

fn write_settings(path: &Path, settings: ProgressSettings) -> Result<(), String> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty())
        .ok_or("Invalid profile progress settings path.")?;
    fs::create_dir_all(parent).map_err(|error| format!("Cannot save profile progress settings: {error}"))?;
    let bytes = serde_json::to_vec_pretty(&SavedProgressSettings { schema_version: 1, delivery: settings.delivery, position: settings.position })
        .map_err(|error| format!("Cannot encode profile progress settings: {error}"))?;
    // The temporary file lives beside its destination. Persist replaces the old
    // file atomically, including on Windows, after all bytes have been synced.
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("Cannot save profile progress settings: {error}"))?;
    temporary.write_all(&bytes).and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| format!("Cannot save profile progress settings: {error}"))?;
    temporary.persist(path).map_err(|error| format!("Cannot save profile progress settings: {}", error.error))?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct NotificationGeometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn notification_geometry(
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

fn position_notification(main: &WebviewWindow, child: &WebviewWindow, settings: ProgressSettings) -> Result<(), String> {
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ActivationProgress {
    operation_id: String,
    profile_name: String,
    title: String,
    phase: ActivationPhase,
    message: String,
    details: Vec<String>,
    can_cancel: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ActivationPhase {
    Preparing,
    Activating,
    Cancelling,
    Launching,
    Deactivating,
    Recovering,
    Switching,
}

impl ActivationPhase {
    fn can_cancel(&self) -> bool {
        matches!(self, Self::Preparing | Self::Activating)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActivationWindowSnapshot {
    progress: ActivationProgress,
    cancel_requested: bool,
    delivery: ProgressDelivery,
    // WebView document focus can differ from OS window activation. Populate
    // this from the native window API when serving a snapshot to the renderer.
    window_focused: Option<bool>,
    native_geometry: Option<NativeNotificationGeometry>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeNotificationGeometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f64,
    work_area: NativeWorkArea,
}

#[derive(Clone, Debug, Serialize)]
struct NativeWorkArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn native_notification_geometry(child: &WebviewWindow) -> Option<NativeNotificationGeometry> {
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

struct ActiveSystemNotification {
    operation_id: String,
    notification: crate::profile_system_notification::SystemNotification,
}

#[derive(Default)]
pub(crate) struct ActivationWindowState(Mutex<Option<ActivationWindowSnapshot>>, Mutex<Option<ActiveSystemNotification>>);

impl ActivationProgress {
    fn validate(&self) -> Result<(), String> {
        if self.operation_id.is_empty()
            || self.operation_id.len() > 128
            || !self.operation_id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || self.profile_name.len() > 256
            || self.title.len() > 160
            || self.message.len() > 4096
            || self.details.len() > 32
            || self.details.iter().any(|line| line.len() > 4096)
        {
            return Err("Invalid profile activation progress.".into());
        }
        Ok(())
    }
}

impl ActivationWindowState {
    pub(crate) fn shutdown(&self) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).take();
        close_system_notification(self, None);
    }

    fn start(&self, progress: ActivationProgress) {
        *self.0.lock().unwrap() = Some(ActivationWindowSnapshot {
            progress,
            cancel_requested: false,
            delivery: ProgressDelivery::Window,
            window_focused: None,
            native_geometry: None,
        });
    }

    fn snapshot(&self) -> Option<ActivationWindowSnapshot> {
        self.0.lock().unwrap().clone()
    }

    fn delivery(&self, operation_id: &str, delivery: ProgressDelivery) -> bool {
        let mut state = self.0.lock().unwrap();
        let Some(current) = state.as_mut().filter(|s| s.progress.operation_id == operation_id) else { return false; };
        current.delivery = delivery;
        true
    }

    fn update(&self, progress: ActivationProgress, reset_cancel_request: bool) -> bool {
        let mut state = self.0.lock().unwrap();
        let Some(current) = state.as_mut().filter(|s| s.progress.operation_id == progress.operation_id) else {
            return false;
        };
        current.progress = progress;
        if reset_cancel_request {
            current.cancel_requested = false;
        }
        true
    }

    fn request_cancel(&self, operation_id: &str) -> bool {
        let mut state = self.0.lock().unwrap();
        let Some(current) = state.as_mut().filter(|s| s.progress.operation_id == operation_id && s.progress.can_cancel && s.progress.phase.can_cancel()) else {
            return false;
        };
        current.cancel_requested = true;
        true
    }

    fn finish(&self, operation_id: &str) -> bool {
        let mut state = self.0.lock().unwrap();
        if state.as_ref().is_some_and(|s| s.progress.operation_id == operation_id) {
            *state = None;
            return true;
        }
        false
    }
}

fn notification_settings(app: &tauri::AppHandle) -> ProgressSettings {
    (|| {
        let _guard = SETTINGS_LOCK.lock().map_err(|error| error.to_string())?;
        read_settings(&settings_path(app)?)
    })().unwrap_or_else(|error: String| {
        eprintln!("Profile progress is using its default preferences: {error}");
        ProgressSettings::default()
    })
}

fn close_system_notification(state: &ActivationWindowState, operation_id: Option<&str>) {
    let mut active = state.1.lock().unwrap_or_else(|p| p.into_inner());
    if active.as_ref().is_some_and(|current| operation_id.is_none_or(|id| current.operation_id == id)) {
        if let Some(mut current) = active.take() { current.notification.close(); }
    }
}

#[cfg(target_os = "windows")]
fn show_without_activation(child: &WebviewWindow) -> Result<(), String> {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{IsWindowVisible, ShowWindow, SW_SHOWNOACTIVATE},
    };
    // Tao's focused(false) marker only affects construction; later `.show()`
    // uses SW_SHOW, which activates the window. This notification has a single
    // native show path and never calls Tauri hide/show visibility toggles.
    // WS_EX_NOACTIVATE from focusable(false) additionally preserves simulator
    // focus when its mouse-operated Cancel button is clicked.
    let handle = child.hwnd().map_err(|error| error.to_string())?.0 as usize;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    child.run_on_main_thread(move || {
        let handle = HWND(handle as *mut core::ffi::c_void);
        unsafe { let _ = ShowWindow(handle, SW_SHOWNOACTIVATE); }
        let result = if unsafe { IsWindowVisible(handle) }.as_bool() {
            Ok(())
        } else {
            Err("Windows could not show the profile progress notification.".to_owned())
        };
        let _ = sender.send(result);
    }).map_err(|error| error.to_string())?;
    receiver.recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|error| format!("The profile notification could not be shown on its window thread: {error}"))?
}

#[cfg(not(target_os = "windows"))]
fn show_without_activation(child: &WebviewWindow) -> Result<(), String> {
    child.show().map_err(|error| error.to_string())
}

fn show_progress_window(window: &WebviewWindow, app: &tauri::AppHandle) -> Result<(), String> {
    let settings = notification_settings(app);
    if let Some(existing) = app.get_webview_window(LABEL) {
        position_notification(window, &existing, settings)?;
        show_without_activation(&existing)?;
        return Ok(());
    }
    // Independent of main-window minimization and never takes simulator focus.
    let child = tauri::WebviewWindowBuilder::new(
        app, LABEL, WebviewUrl::App("index.html?profile-activation-window".into()),
    )
    .title("Profile progress · OpenSimGear")
    .inner_size(NOTIFICATION_WIDTH, NOTIFICATION_HEIGHT)
    // Explicitly override Windows' default minimum tracking size; otherwise its
    // WM_GETMINMAXINFO default can enlarge a short notification at high DPI.
    .min_inner_size(1.0, 1.0)
    // Avoid Tao's shadow-specific nonclient compensation. Native placement
    // below explicitly verifies the requested client size after recalculation.
    .decorations(false).shadow(false).resizable(false).maximizable(false).minimizable(false)
    .skip_taskbar(true).always_on_top(true).focused(false).focusable(false).visible(false)
    .background_color(tauri::window::Color(20, 25, 31, 255))
    .build().map_err(|error| error.to_string())?;
    if let Err(error) = position_notification(window, &child, settings).and_then(|()| show_without_activation(&child)) {
        let _ = child.destroy();
        return Err(error);
    }
    let handle = app.clone();
    child.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            if handle.state::<ActivationWindowState>().snapshot().is_some() {
                api.prevent_close();
                if let Some(child) = handle.get_webview_window(LABEL) {
                    let _ = child.eval("window.dispatchEvent(new Event('profile-activation-close'))");
                }
            }
        }
    });
    Ok(())
}

fn require_window(window: &WebviewWindow, expected: &[&str]) -> Result<(), String> {
    if expected.contains(&window.label()) && super::local_document(&window.url().map_err(|e| e.to_string())?, window.app_handle().config().build.dev_url.as_ref()) {
        Ok(())
    } else {
        Err("This window cannot control profile activation progress.".into())
    }
}

#[tauri::command]
pub(crate) fn profile_progress_settings_get(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<ProgressSettings, String> {
    require_window(&window, &["main"])?;
    let _guard = SETTINGS_LOCK.lock().map_err(|error| error.to_string())?;
    read_settings(&settings_path(&app)?)
}

#[tauri::command]
pub(crate) async fn profile_progress_settings_save(
    window: WebviewWindow,
    app: tauri::AppHandle,
    settings: ProgressSettings,
) -> Result<ProgressSettings, String> {
    require_window(&window, &["main"])?;
    let _guard = SETTINGS_LOCK.lock().map_err(|error| error.to_string())?;
    write_settings(&settings_path(&app)?, settings)?;
    if let Some(child) = app.get_webview_window(LABEL) {
        position_notification(&window, &child, settings)
            .map_err(|error| format!("Profile progress settings were saved, but the notification could not be moved: {error}"))?;
    }
    Ok(settings)
}

#[tauri::command]
pub(crate) async fn profile_activation_window_open(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, ActivationWindowState>,
    progress: ActivationProgress,
) -> Result<(), String> {
    require_window(&window, &["main"])?;
    progress.validate()?;
    let operation_id = progress.operation_id.clone();
    state.start(progress.clone());
    let notification_app = app.clone();
    let shown = tauri::async_runtime::spawn_blocking(move || {
        let state = notification_app.state::<ActivationWindowState>();
        let retired = {
            let snapshot = state.0.lock().unwrap_or_else(|p| p.into_inner());
            if snapshot.as_ref().is_none_or(|current| current.progress.operation_id != progress.operation_id) { return false; }
            state.1.lock().unwrap_or_else(|p| p.into_inner()).take()
        };
        if let Some(mut retired) = retired { retired.notification.close(); }
        if notification_settings(&notification_app).delivery != ProgressDelivery::System { return false; }
        let callback_app = notification_app.clone();
        let callback_id = progress.operation_id.clone();
        let on_cancel = Arc::new(move || { callback_app.state::<ActivationWindowState>().request_cancel(&callback_id); });
        match crate::profile_system_notification::SystemNotification::show(
            &notification_app.config().identifier, &progress.operation_id,
            &progress.title, &progress.profile_name, &progress.message,
            progress.can_cancel && progress.phase.can_cancel(), on_cancel,
        ) {
            Ok(notification) => {
                let mut notification = Some(notification);
                let installed = {
                    let mut snapshot = state.0.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(current) = snapshot.as_mut().filter(|current| current.progress.operation_id == progress.operation_id) {
                        *state.1.lock().unwrap_or_else(|p| p.into_inner()) = Some(ActiveSystemNotification { operation_id: progress.operation_id, notification: notification.take().unwrap() });
                        current.delivery = ProgressDelivery::System;
                        true
                    } else { false }
                };
                if let Some(mut notification) = notification { notification.close(); }
                installed
            }
            Err(error) => { eprintln!("System profile notification unavailable; using Cockpit window: {error}"); false }
        }
    }).await.unwrap_or(false);
    if shown {
        if state.snapshot().is_some_and(|snapshot| snapshot.progress.operation_id == operation_id && snapshot.delivery == ProgressDelivery::System) {
            if let Some(child) = app.get_webview_window(LABEL) { let _ = child.destroy(); }
        }
        return Ok(());
    }
    if state.snapshot().is_none_or(|snapshot| snapshot.progress.operation_id != operation_id) { return Ok(()); }
    state.delivery(&operation_id, ProgressDelivery::Window);
    let result = show_progress_window(&window, &app);
    if result.is_err() {
        state.finish(&operation_id);
    }
    result
}

#[tauri::command]
pub(crate) async fn profile_activation_window_update(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, ActivationWindowState>,
    progress: ActivationProgress,
    reset_cancel_request: Option<bool>,
) -> Result<bool, String> {
    require_window(&window, &["main"])?;
    progress.validate()?;
    if !state.update(progress.clone(), reset_cancel_request.unwrap_or(false)) { return Ok(false); }
    let notification_app = app.clone();
    let operation_id = progress.operation_id.clone();
    let system_failed = tauri::async_runtime::spawn_blocking(move || {
        let state = notification_app.state::<ActivationWindowState>();
        let mut active = state.1.lock().unwrap_or_else(|p| p.into_inner());
        let Some(current) = active.as_mut().filter(|current| current.operation_id == progress.operation_id) else { return false; };
        match current.notification.update(&progress.title, &progress.profile_name, &progress.message,
            progress.can_cancel && progress.phase.can_cancel()) {
            Ok(()) => false,
            Err(error) => {
                eprintln!("System profile notification update failed; using Cockpit window: {error}");
                if let Some(mut current) = active.take() { current.notification.close(); }
                true
            }
        }
    }).await.unwrap_or(true);
    if system_failed && state.delivery(&operation_id, ProgressDelivery::Window) {
        show_progress_window(&window, &app)?;
    }
    Ok(true)
}

#[tauri::command]
pub(crate) fn profile_activation_window_state(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, ActivationWindowState>,
) -> Result<Option<ActivationWindowSnapshot>, String> {
    require_window(&window, &["main", LABEL])?;
    let mut snapshot = state.snapshot();
    if let Some(snapshot) = snapshot.as_mut() {
        if let Some(child) = app.get_webview_window(LABEL) {
            snapshot.window_focused = child.is_focused().ok();
            snapshot.native_geometry = native_notification_geometry(&child);
        }
    }
    Ok(snapshot)
}

#[tauri::command]
pub(crate) fn profile_activation_window_cancel(
    window: WebviewWindow,
    state: tauri::State<'_, ActivationWindowState>,
    operation_id: String,
) -> Result<bool, String> {
    require_window(&window, &[LABEL])?;
    Ok(state.request_cancel(&operation_id))
}

#[tauri::command]
pub(crate) async fn profile_activation_window_close(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, ActivationWindowState>,
    operation_id: String,
) -> Result<bool, String> {
    require_window(&window, &["main"])?;
    if !state.finish(&operation_id) {
        return Ok(false);
    }
    let notification_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        close_system_notification(&notification_app.state::<ActivationWindowState>(), Some(&operation_id));
    }).await.map_err(|error| error.to_string())?;
    // An OS cleanup await may overlap another native Open. Never destroy the
    // next operation's fallback window when this exact operation has retired.
    if state.snapshot().is_none() {
        if let Some(child) = app.get_webview_window(LABEL) {
            child.destroy().map_err(|e| e.to_string())?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(id: &str) -> ActivationProgress {
        ActivationProgress { operation_id: id.into(), profile_name: "Flight rig".into(), title: "Activating profile".into(), phase: ActivationPhase::Preparing, message: "Preparing companions".into(), details: vec![], can_cancel: true }
    }

    #[test]
    fn stale_window_actions_cannot_change_or_cancel_the_next_activation() {
        let state = ActivationWindowState::default();
        state.start(progress("old-operation"));
        assert!(state.request_cancel("old-operation"));
        state.start(progress("new-operation"));
        assert!(!state.update(progress("old-operation"), false));
        assert!(!state.request_cancel("old-operation"));
        assert!(!state.finish("old-operation"));
        assert!(!state.snapshot().unwrap().cancel_requested);
        assert!(state.request_cancel("new-operation"));
        assert!(state.snapshot().unwrap().cancel_requested);
        assert!(state.finish("new-operation"));
        assert!(state.snapshot().is_none());
    }

    #[test]
    fn progress_updates_preserve_cancel_intent_and_disable_repeated_cancel() {
        let state = ActivationWindowState::default();
        state.start(progress("operation"));
        assert!(state.request_cancel("operation"));
        let mut next = progress("operation");
        next.phase = ActivationPhase::Cancelling;
        next.can_cancel = false;
        assert!(state.update(next, false));
        assert!(state.snapshot().unwrap().cancel_requested);
        assert!(!state.request_cancel("operation"));
    }

    #[test]
    fn a_failed_cancel_request_can_be_retried_without_restarting_activation() {
        let state = ActivationWindowState::default();
        state.start(progress("operation"));
        assert!(state.request_cancel("operation"));
        assert!(!state.update(progress("old-operation"), true));
        assert!(state.snapshot().unwrap().cancel_requested);
        assert!(state.update(progress("operation"), true));
        assert!(!state.snapshot().unwrap().cancel_requested);
        assert!(state.request_cancel("operation"));
    }

    #[test]
    fn bounds_reject_untrusted_or_unbounded_progress() {
        assert!(progress("a-valid-operation_1").validate().is_ok());
        assert!(progress("../other?route=1").validate().is_err());
        let mut value = progress("operation");
        value.details = vec![String::new(); 33];
        assert!(value.validate().is_err());
    }

    #[test]
    fn game_launch_status_keeps_the_same_window_without_allowing_cancel() {
        let state = ActivationWindowState::default();
        state.start(progress("operation"));
        let mut launching = progress("operation");
        launching.phase = ActivationPhase::Launching;
        launching.message = "Launching Assetto Corsa Rally".into();
        launching.can_cancel = false;
        assert!(launching.validate().is_ok());
        assert!(state.update(launching, false));
        assert!(!state.request_cancel("operation"));
        let snapshot = serde_json::to_value(state.snapshot().unwrap()).unwrap();
        assert_eq!(snapshot["progress"]["phase"], "launching");
        assert_eq!(snapshot["progress"]["message"], "Launching Assetto Corsa Rally");
        assert!(state.finish("operation"));
    }

    #[test]
    fn cleanup_phases_retain_native_status_and_refuse_cancel_even_with_a_stale_flag() {
        for (phase, serialized) in [(ActivationPhase::Deactivating, "deactivating"), (ActivationPhase::Recovering, "recovering")] {
            let state = ActivationWindowState::default();
            state.start(progress("operation"));
            assert!(state.request_cancel("operation"));
            let mut cleanup = progress("operation");
            cleanup.phase = phase;
            cleanup.message = "Unhiding peripherals".into();
            // The phase remains authoritative if a stale renderer carries the
            // prior cancellable activation flag across the cleanup boundary.
            cleanup.can_cancel = true;
            assert!(cleanup.validate().is_ok());
            assert!(state.update(cleanup, true));
            assert!(!state.request_cancel("operation"));
            let snapshot = serde_json::to_value(state.snapshot().unwrap()).unwrap();
            assert_eq!(snapshot["progress"]["phase"], serialized);
            assert_eq!(snapshot["progress"]["message"], "Unhiding peripherals");
            assert_eq!(snapshot["cancelRequested"], false);
            assert!(state.finish("operation"));
        }
    }

    #[test]
    fn vehicle_switching_progress_is_never_cancellable() {
        let state = ActivationWindowState::default();
        let mut switching = progress("vehicle-switch-1");
        switching.phase = ActivationPhase::Switching;
        // A renderer cannot enable cancellation of an automatic switch by
        // carrying the manual activation's previous flag into its next update.
        switching.can_cancel = true;
        assert!(switching.validate().is_ok());
        state.start(switching);
        assert!(!state.request_cancel("vehicle-switch-1"));
        assert_eq!(serde_json::to_value(state.snapshot().unwrap()).unwrap()["progress"]["phase"], "switching");
    }

    #[test]
    fn notification_corners_respect_the_work_area_and_taskbar() {
        let cases = [
            (NotificationPosition::BottomRight, 1484, 928),
            (NotificationPosition::BottomLeft, 16, 928),
            (NotificationPosition::TopRight, 1484, 16),
            (NotificationPosition::TopLeft, 16, 16),
        ];
        for (position, x, y) in cases {
            assert_eq!(notification_geometry(position, (0, 0), (1920, 1040), 1.0).unwrap(), NotificationGeometry { x, y, width: 420, height: 96 });
        }
    }

    #[test]
    fn mixed_dpi_and_negative_monitor_origins_use_physical_coordinates() {
        assert_eq!(notification_geometry(NotificationPosition::BottomRight, (-2560, -120), (2560, 1400), 1.5).unwrap(), NotificationGeometry { x: -654, y: 1112, width: 630, height: 144 });
        assert_eq!(notification_geometry(NotificationPosition::TopLeft, (-1920, 80), (1920, 1000), 1.25).unwrap(), NotificationGeometry { x: -1900, y: 100, width: 525, height: 120 });
    }

    #[test]
    fn tiny_work_areas_keep_the_whole_notification_visible() {
        for position in [NotificationPosition::BottomRight, NotificationPosition::BottomLeft, NotificationPosition::TopRight, NotificationPosition::TopLeft] {
            assert_eq!(notification_geometry(position, (-320, -40), (300, 80), 2.0).unwrap(), NotificationGeometry { x: -320, y: -40, width: 300, height: 80 });
            let geometry = notification_geometry(position, (-430, 0), (430, 100), 1.0).unwrap();
            assert_eq!(geometry.width, 420);
            assert_eq!(geometry.height, 96);
            assert!(geometry.x >= -430 && i64::from(geometry.x) + i64::from(geometry.width) <= 0);
            assert!(geometry.y >= 0 && geometry.y + geometry.height as i32 <= 100);
        }
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(notification_geometry(NotificationPosition::BottomRight, (0, 0), (1920, 1080), scale).is_err());
        }
        assert!(notification_geometry(NotificationPosition::BottomRight, (0, 0), (0, 1080), 1.0).is_err());
    }

    #[test]
    fn progress_settings_default_and_replace_durably_without_extra_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences/progress.json");
        assert_eq!(read_settings(&path).unwrap(), ProgressSettings::default());
        for position in [NotificationPosition::TopLeft, NotificationPosition::BottomRight] {
            let settings = ProgressSettings { position, ..Default::default() };
            write_settings(&path, settings).unwrap();
            assert_eq!(read_settings(&path).unwrap(), settings);
            let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(saved["schemaVersion"], 1);
            assert_eq!(saved["position"], serde_json::to_value(position).unwrap());
            assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        }
    }

    #[test]
    fn invalid_preferences_are_reported_without_silent_repair() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("progress.json");
        for contents in [
            "not json".to_owned(),
            r#"{"schemaVersion":2,"position":"bottom-right"}"#.into(),
            r#"{"schemaVersion":1,"position":"center"}"#.into(),
            r#"{"schemaVersion":1,"delivery":"off","position":"bottom-right"}"#.into(),
            r#"{"schemaVersion":1,"position":"top-right","unexpected":true}"#.into(),
            "x".repeat(4097),
        ] {
            fs::write(&path, contents.as_bytes()).unwrap();
            assert!(read_settings(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        }
        for value in [r#"{"position":"center"}"#, r#"{"position":"top-left","url":"https://example.com"}"#] {
            assert!(serde_json::from_str::<ProgressSettings>(value).is_err());
        }
    }

    #[test]
    fn legacy_position_defaults_to_system_and_delivery_choice_persists() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("progress.json");
        fs::write(&path, br#"{"schemaVersion":1,"position":"top-left"}"#).unwrap();
        assert_eq!(read_settings(&path).unwrap(), ProgressSettings { delivery: ProgressDelivery::System, position: NotificationPosition::TopLeft });
        let selected = ProgressSettings { delivery: ProgressDelivery::Window, position: NotificationPosition::BottomLeft };
        write_settings(&path, selected).unwrap();
        assert_eq!(read_settings(&path).unwrap(), selected);
    }

    #[test]
    fn stale_delivery_results_cannot_replace_the_current_operation() {
        let state = ActivationWindowState::default();
        state.start(progress("old"));
        assert!(state.delivery("old", ProgressDelivery::System));
        state.start(progress("current"));
        assert!(!state.delivery("old", ProgressDelivery::System));
        assert_eq!(state.snapshot().unwrap().delivery, ProgressDelivery::Window);
        assert!(state.delivery("current", ProgressDelivery::System));
        assert!(!state.finish("old"));
        assert_eq!(state.snapshot().unwrap().delivery, ProgressDelivery::System);
    }

    #[test]
    fn failed_settings_replace_preserves_the_existing_target_and_cleans_temporary_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("progress.json");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("retain.txt"), b"preserved").unwrap();
        assert!(write_settings(&path, ProgressSettings::default()).is_err());
        assert_eq!(fs::read(path.join("retain.txt")).unwrap(), b"preserved");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
