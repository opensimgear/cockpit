use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering}, Mutex,
    },
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase",deny_unknown_fields)]
pub struct TrayPreferences {
    close_to_tray: bool,
    minimize_to_tray: bool,
}

pub struct DesktopState {
    preferences: crate::persistence::ui::Preferences,
    tray_available: bool,
    tray_error: Option<String>,
    minimized_handled: AtomicBool,
    window_save: crate::window_persistence::SaveWorker,
    geometry: crate::persistence::ui::window_writer::WindowWriter,
    window_error: Mutex<Option<String>>,
}

pub fn show_main(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<DesktopState>() {
        state.minimized_handled.store(false, Ordering::Relaxed);
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn create_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open-cockpit", "Open Cockpit", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit-cockpit", "Quit", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop-desktop-output", "Stop keyboard and mouse output", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &stop, &quit])?;
    let mut tray = TrayIconBuilder::with_id("cockpit")
        .tooltip("OpenSimGear Cockpit")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open-cockpit" => show_main(app),
            "stop-desktop-output" => {
                if let Some(gateway) = app.try_state::<crate::NativeGateway>() {
                    let gateway = gateway.0.clone();
                    std::thread::spawn(move || { let _ = gateway.request(json!({"command":"profile_desktop_stop","clientId":"cockpit-native-tray"})); });
                }
            }
            "quit-cockpit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    app.handle().plugin(tauri_plugin_autostart::init(
        tauri_plugin_autostart::MacosLauncher::LaunchAgent,
        None,
    ))?;
    let preferences=app.state::<crate::NativeGateway>().0.ui_preferences.clone();
    let _=preferences.read("tray");let _=preferences.read("progress");
    let tray_error = create_tray(app).err().map(|e|e.to_string());
    preferences.set_tray_available(tray_error.is_none());
    let window_error=crate::desktop_geometry::restore(app.handle()).err();
    let geometry=crate::persistence::ui::window_writer::WindowWriter::new(preferences.clone());
    let handle=app.handle().clone();
    let window_save=crate::window_persistence::SaveWorker::new(std::time::Duration::from_millis(300),move || {
        let save_handle=handle.clone();
        if let Err(error)=handle.run_on_main_thread(move || save_window_state(&save_handle)) {
            eprintln!("Could not schedule window state save: {error}");
        }
    });
    app.manage(DesktopState {
        preferences,
        tray_available: tray_error.is_none(),
        tray_error,
        minimized_handled: AtomicBool::new(false),
        window_save, geometry, window_error:Mutex::new(window_error),
    });
    Ok(())
}

#[tauri::command]
pub async fn desktop_preferences(
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> Result<Value, String> {
    let service=state.preferences.clone();
    let loaded=tauri::async_runtime::spawn_blocking(move||service.read("tray")).await.map_err(|_|"The tray settings worker failed.")?;
    let settings_error=loaded.as_ref().err().cloned();
    let settings_revision=loaded.as_ref().map(|row|row.revision).unwrap_or(0);
    let preferences:TrayPreferences=loaded.ok().and_then(|row|serde_json::from_value(row.value).ok()).unwrap_or_default();
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    let autostart = {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().is_enabled().map_err(|e| e.to_string())
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let autostart: Result<bool, String> = Ok(false);
    Ok(json!({
        "closeToTray": preferences.close_to_tray,
        "minimizeToTray": preferences.minimize_to_tray,
        "trayAvailable": state.tray_available,
        "trayError": state.tray_error,
        "settingsError": settings_error,
        "settingsRevision":settings_revision,
        "windowStateError":state.geometry.error().or_else(||state.window_error.lock().unwrap_or_else(|p|p.into_inner()).clone()),
        "autostartSupported": cfg!(any(target_os = "windows", target_os = "macos", target_os = "linux")),
        "autostartEnabled": autostart.as_ref().copied().unwrap_or(false),
        "autostartError": autostart.err(),
    }))
}

#[tauri::command]
pub async fn set_tray_preferences(
    preferences: TrayPreferences, expected_revision:u64,
    state: tauri::State<'_, DesktopState>,
) -> Result<Value, String> {
    let service=state.preferences.clone();
    let value=serde_json::to_value(preferences).map_err(|_|"Invalid tray preferences.")?;
    let entry=tauri::async_runtime::spawn_blocking(move||service.write("tray",expected_revision,value)).await.map_err(|_|"The tray settings worker failed.")??;
    state.minimized_handled.store(false,Ordering::Relaxed);
    serde_json::to_value(entry).map_err(|_|"Cannot encode tray settings.".into())
}

#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        use tauri_plugin_autostart::ManagerExt;
        let manager = app.autolaunch();
        if enabled {
            manager.enable()
        } else {
            manager.disable()
        }
        .map_err(|e| e.to_string())?;
        manager.is_enabled().map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    Err("Auto-start is not supported on this platform.".into())
}

pub fn window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != "main" {
        return;
    }
    let Some(state) = window.try_state::<DesktopState>() else {
        return;
    };
    match event {
        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => state.window_save.changed(),
        // Flush before destruction (or hiding to tray), while the live window
        // can still provide its final size and maximized state to the plugin.
        tauri::WindowEvent::CloseRequested { .. } => { save_window_state(window.app_handle()); if let Err(error)=state.geometry.flush(){*state.window_error.lock().unwrap_or_else(|p|p.into_inner())=Some(error.clone());eprintln!("Could not flush window geometry: {error}");} },
        _ => {},
    }
    if !state.tray_available {
        return;
    }
    let preferences:TrayPreferences=serde_json::from_value(state.preferences.cached("tray")).unwrap_or_default();
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } if preferences.close_to_tray => {
            if window.hide().is_ok() {
                api.prevent_close();
            }
        }
        _ => {}
    }
}

fn save_window_state(app:&tauri::AppHandle){
    let Some(state)=app.try_state::<DesktopState>()else{return};
    let result=crate::desktop_geometry::capture(app).and_then(|value|if value.is_null(){Ok(())}else{state.geometry.queue(value)});
    if let Err(error)=result{*state.window_error.lock().unwrap_or_else(|p|p.into_inner())=Some(error.clone());eprintln!("Could not save window geometry: {error}");}
}
pub(crate) fn shutdown(app:&tauri::AppHandle){
    if let Some(state)=app.try_state::<DesktopState>(){
        state.window_save.stop();save_window_state(app);
        if let Err(error)=state.geometry.flush(){eprintln!("Could not flush window geometry: {error}");}
        state.geometry.stop();
    }
}

// Some platforms do not emit a resize event when minimizing. Check after native
// events have been processed so the window's minimized state is up to date.
pub fn hide_if_minimized(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<DesktopState>() else {
        return;
    };
    if !state.tray_available {
        return;
    }
    let enabled=state.preferences.cached("tray")["minimizeToTray"]==true;
    if !enabled {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        if window.is_minimized().unwrap_or(false) {
            if !state.minimized_handled.swap(true, Ordering::Relaxed) {
                let _ = window.hide();
            }
        } else {
            state.minimized_handled.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_default_to_normal_window_behavior() {
        let preferences: TrayPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(preferences, TrayPreferences::default());
        assert!(!preferences.close_to_tray && !preferences.minimize_to_tray);
        let partial: TrayPreferences = serde_json::from_str(r#"{"closeToTray":true}"#).unwrap();
        assert!(partial.close_to_tray && !partial.minimize_to_tray);
    }

}
