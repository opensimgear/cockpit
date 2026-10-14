mod acr_windows;
mod actions;
pub mod dcs_bios;
mod dcs_profile;
mod dcs_profile_runtime;
mod dcs_runtime;
mod game_discovery;
mod game_telemetry;
mod games;
pub mod gateways;
mod hid_names;
mod integration_profile;
mod integration_runtime;
mod local_gateway;
mod output_leases;
mod panel_events;
pub mod pico;
mod protocol;
mod runtime;
pub mod stm32;
mod transport;

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
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri::plugin::Builder::<tauri::Wry, ()>::new("local-navigation")
                .on_navigation(|_, url| local_document(url))
                .build(),
        )
        .setup(|app| {
            app.manage(NativeGateway(Arc::new(gateways::GatewayManager::new())));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![gateway_request])
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                window.state::<NativeGateway>().0.shutdown();
                window.app_handle().exit(0);
            }
        })
        .build(tauri::generate_context!())
        .expect("could not start OpenSimGear Cockpit");
    app.run(|handle, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            handle.state::<NativeGateway>().0.shutdown();
        }
    });
}
