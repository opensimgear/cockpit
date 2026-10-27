use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
};
use tauri::{Manager, WebviewUrl, WebviewWindow};
mod geometry;
use geometry::{position_notification,native_notification_geometry,NativeNotificationGeometry};
#[cfg(test)]
use geometry::{notification_geometry,NotificationGeometry};

const LABEL: &str = "profile-activation";
const NOTIFICATION_WIDTH: f64 = 420.0;
const NOTIFICATION_HEIGHT: f64 = 96.0;
const NOTIFICATION_INSET: f64 = 16.0;

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
    delivery: ProgressDelivery,
    position: NotificationPosition,
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

fn notification_settings(app:&tauri::AppHandle)->ProgressSettings {
    app.try_state::<crate::NativeGateway>().and_then(|gateway|serde_json::from_value(gateway.0.ui_preferences.cached("progress")).ok()).unwrap_or_default()
}
/// Durable preference changes move an existing owned popup outside SQLite work.
pub(crate) fn preferences_changed(app:&tauri::AppHandle){
    let Some(child)=app.get_webview_window(LABEL) else{return};
    let Some(parent)=app.get_webview_window("main") else{return};
    let settings=notification_settings(app);let handle=app.clone();
    std::thread::spawn(move||{if let Err(error)=position_notification(&parent,&child,settings){
        use tauri::Emitter;let _=handle.emit("ui-preference-effect-error",serde_json::json!({"key":"progress","message":format!("Preferences saved, but the progress window could not move: {error}")}));
    }});
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
pub(crate) async fn profile_progress_settings_get(window:WebviewWindow,app:tauri::AppHandle)->Result<serde_json::Value,String>{
    require_window(&window,&["main"])?;
    let service=app.state::<crate::NativeGateway>().0.ui_preferences.clone();
    let row=tauri::async_runtime::spawn_blocking(move||service.read("progress")).await.map_err(|_|"The progress settings worker failed.")??;
    let mut value=row.value;value["settingsRevision"]=serde_json::json!(row.revision);Ok(value)
}
#[tauri::command]
pub(crate) async fn profile_progress_settings_save(window:WebviewWindow,app:tauri::AppHandle,settings:ProgressSettings,expected_revision:u64)->Result<serde_json::Value,String>{
    require_window(&window,&["main"])?;
    let service=app.state::<crate::NativeGateway>().0.ui_preferences.clone();let value=serde_json::to_value(settings).map_err(|_|"Invalid progress settings.")?;
    let activity=app.state::<crate::NativeGateway>().0.clone();
    let row=tauri::async_runtime::spawn_blocking(move||{
        let _activity_guard=activity.activity_mutation();
        let before=match service.read("progress") {
            Ok(entry)=>entry.value,
            Err(error)=>{
                activity.record_external_change("profile_progress_settings_save",None,None,&serde_json::Value::Null,&serde_json::Value::Null,Some(&error),"cockpit");
                return Err(error);
            }
        };
        let result=service.write("progress",expected_revision,value);
        let after=result.as_ref().map(|entry|entry.value.clone()).unwrap_or_else(|_|before.clone());
        activity.record_external_change("profile_progress_settings_save",None,None,&before,&after,result.as_ref().err().map(String::as_str),"cockpit");
        result
    }).await.map_err(|_|"The progress settings worker failed.")??;
    let mut value=row.value;value["settingsRevision"]=serde_json::json!(row.revision);Ok(value)
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
mod tests;
