use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct TrayPreferences {
    close_to_tray: bool,
    minimize_to_tray: bool,
}

pub struct DesktopState {
    preferences: Mutex<TrayPreferences>,
    path: PathBuf,
    tray_available: bool,
    tray_error: Option<String>,
    settings_error: Option<String>,
    minimized_handled: AtomicBool,
}

fn read_preferences(path: &std::path::Path) -> Result<TrayPreferences, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TrayPreferences::default()),
        Err(e) => Err(e.to_string()),
    }
}

fn write_preferences(path: &std::path::Path, preferences: TrayPreferences) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid desktop preferences path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(&preferences).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&temporary, path).map_err(|e| e.to_string())
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
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("cockpit")
        .tooltip("OpenSimGear Cockpit")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open-cockpit" => show_main(app),
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
    let path = app
        .path()
        .app_config_dir()?
        .join("desktop-preferences.json");
    let loaded = read_preferences(&path);
    let settings_error = loaded.as_ref().err().cloned();
    let tray_error = create_tray(app).err().map(|e| e.to_string());
    app.manage(DesktopState {
        preferences: Mutex::new(loaded.unwrap_or_default()),
        path,
        tray_available: tray_error.is_none(),
        tray_error,
        settings_error,
        minimized_handled: AtomicBool::new(false),
    });
    Ok(())
}

#[tauri::command]
pub fn desktop_preferences(
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> Result<Value, String> {
    let preferences = *state.preferences.lock().map_err(|e| e.to_string())?;
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
        "settingsError": state.settings_error,
        "autostartSupported": cfg!(any(target_os = "windows", target_os = "macos", target_os = "linux")),
        "autostartEnabled": autostart.as_ref().copied().unwrap_or(false),
        "autostartError": autostart.err(),
    }))
}

#[tauri::command]
pub fn set_tray_preferences(
    preferences: TrayPreferences,
    state: tauri::State<'_, DesktopState>,
) -> Result<(), String> {
    if !state.tray_available && (preferences.close_to_tray || preferences.minimize_to_tray) {
        return Err("The system tray is unavailable on this desktop.".into());
    }
    let mut current = state.preferences.lock().map_err(|e| e.to_string())?;
    write_preferences(&state.path, preferences)?;
    *current = preferences;
    state.minimized_handled.store(false, Ordering::Relaxed);
    Ok(())
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
    if !state.tray_available {
        return;
    }
    let Ok(preferences) = state.preferences.lock().map(|p| *p) else {
        return;
    };
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } if preferences.close_to_tray => {
            if window.hide().is_ok() {
                api.prevent_close();
            }
        }
        _ => {}
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
    let enabled = state
        .preferences
        .lock()
        .map(|p| p.minimize_to_tray)
        .unwrap_or(false);
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

    #[test]
    fn preferences_survive_restart_and_replacement() {
        let directory =
            std::env::temp_dir().join(format!("cockpit-desktop-{}", uuid::Uuid::new_v4()));
        let path = directory.join("settings.json");
        assert_eq!(read_preferences(&path).unwrap(), TrayPreferences::default());
        let preferences = TrayPreferences {
            close_to_tray: true,
            minimize_to_tray: true,
        };
        write_preferences(&path, preferences).unwrap();
        assert_eq!(read_preferences(&path).unwrap(), preferences);
        write_preferences(&path, TrayPreferences::default()).unwrap();
        assert_eq!(read_preferences(&path).unwrap(), TrayPreferences::default());
        fs::write(&path, "invalid").unwrap();
        assert!(read_preferences(&path).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
