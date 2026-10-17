mod acr_setup;
mod acr_windows;
mod companion_host;
mod companion_panels;
mod actions;
pub mod dcs_bios;
mod dcs_profile;
mod dcs_profile_runtime;
mod dcs_runtime;
mod dcs_setup;
mod desktop;
mod game_discovery;
mod game_context;
mod game_integration_setup;
mod game_telemetry;
mod games;
pub mod gateways;
mod hid_names;
mod integration_profile;
mod integration_runtime;
mod local_gateway;
mod optional_dependencies;
mod module_console;
mod module_usb;
mod module_usb_fleet;
mod module_configuration_routes;
mod profile_resources;
mod profile_compilation;
mod profile_session;
mod profile_session_games;
mod profile_resource_sources;
mod profile_resource_ports;
mod output_leases;
mod panel_events;
mod mapping_panels;
mod game_mapping;
pub mod pico;
mod protocol;
mod runtime;
pub mod stm32;
mod transport;
mod xplane;
mod xplane_installation;

use serde_json::Value;
use std::sync::Arc;
use tauri::Manager;

fn local_document(url: &tauri::Url) -> bool {
    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost"))
        || (cfg!(debug_assertions)
            && url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port() == Some(1420))
}

struct NativeGateway(Arc<gateways::GatewayManager>);

#[tauri::command]
async fn pick_game_executable(
    app: tauri::AppHandle,
    game_id: String,
    current_path: Option<String>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (title, executable) = game_discovery::executable_picker(&game_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut picker = app
            .dialog()
            .file()
            .set_title(title)
            .add_filter(executable, &["exe"])
            .set_file_name(executable);
        if let Some(parent) = current_path
            .as_deref()
            .map(std::path::Path::new)
            .and_then(|p| p.parent())
            .filter(|p| p.is_absolute() && p.is_dir())
        {
            picker = picker.set_directory(parent);
        }
        picker
            .blocking_pick_file()
            .map(|file| {
                let path = file.into_path().map_err(|e| e.to_string())?;
                path.to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "The executable path is not valid text.".to_owned())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn optional_dependencies() -> Value {
    optional_dependencies::list()
}

#[tauri::command]
async fn optional_dependency_install(app: tauri::AppHandle, id: String) -> Result<String, String> {
    let resources = app.path().resource_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        optional_dependencies::install(&resources, &id).map(str::to_owned)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_companion_browser(app: tauri::AppHandle, state: tauri::State<'_, NativeGateway>, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let service=state.0.clone();
    let expected=tauri::async_runtime::spawn_blocking(move || service.request(serde_json::json!({"command":"companion_status","clientId":"browser-launch-native"}))).await.map_err(|e|e.to_string())?;
    let base=expected["state"]["companions"]["browserUrl"].as_str().ok_or("Enable the companion listener first")?;
    if url != base && !url.strip_prefix(&format!("{base}#pair=")).is_some_and(|s|s.len()==43 && s.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'_' || b==b'-')) {
        return Err("Only the current Cockpit browser companion URL can be opened".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e|e.to_string())
}
#[tauri::command]
async fn export_companion_certificate(app: tauri::AppHandle, state: tauri::State<'_, NativeGateway>) -> Result<bool,String> {
    use tauri_plugin_dialog::DialogExt;
    let service=state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result=service.request(serde_json::json!({"command":"companion_browser_certificate","clientId":"browser-certificate"}));
        let pem=result["state"]["companions"]["certificatePem"].as_str().ok_or("Cannot read Cockpit's public certificate")?;
        let Some(file)=app.dialog().file().set_title("Export Cockpit public certificate").set_file_name("opensimgear-cockpit.crt").add_filter("Certificate",&["crt"]).blocking_save_file() else {return Ok(false)};
        let path=file.into_path().map_err(|e|e.to_string())?;
        std::fs::write(path,pem).map_err(|e|e.to_string())?;
        Ok(true)
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
async fn gateway_request(
    request: Value,
    state: tauri::State<'_, NativeGateway>,
) -> Result<Value, String> {
    let service = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || service.request(request))
        .await
        .map_err(|error| error.to_string())
}

pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        desktop::show_main(app);
    }));
    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri::plugin::Builder::<tauri::Wry, ()>::new("local-navigation")
                .on_navigation(|_, url| local_document(url))
                .build(),
        )
        .setup(|app| {
            app.manage(NativeGateway(Arc::new(gateways::GatewayManager::new())));
            desktop::setup(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            gateway_request,
            open_companion_browser,
            export_companion_certificate,
            optional_dependencies,
            optional_dependency_install,
            pick_game_executable,
            desktop::desktop_preferences,
            desktop::set_tray_preferences,
            desktop::set_autostart
        ])
        .on_window_event(|window, event| {
            desktop::window_event(window, event);
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                window.state::<NativeGateway>().0.shutdown();
                window.app_handle().exit(0);
            }
        })
        .build(tauri::generate_context!())
        .expect("could not start OpenSimGear Cockpit");
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::MainEventsCleared) {
            desktop::hide_if_minimized(handle);
        }
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            handle.state::<NativeGateway>().0.shutdown();
        }
    });
}
