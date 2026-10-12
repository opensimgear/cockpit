mod corehid;
mod firmware;
mod hid_names;
pub mod pico;
mod protocol;
mod transport;

use serde_json::Value;
use std::sync::Arc;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

fn local_document(url: &tauri::Url) -> bool {
    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost"))
        || (cfg!(debug_assertions)
            && url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port() == Some(1420))
}

struct NativeGateways {
    pico: Arc<pico::PicoService>,
    corehid: Arc<corehid::CoreHidService>,
}

impl NativeGateways {
    fn shutdown(&self) {
        self.pico.shutdown();
        self.corehid.shutdown();
    }
}

#[tauri::command]
async fn pico_gateway(
    request: Value,
    state: tauri::State<'_, NativeGateways>,
) -> Result<Value, String> {
    let service = state.pico.clone();
    tauri::async_runtime::spawn_blocking(move || service.request(request))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn corehid_gateway(
    request: Value,
    state: tauri::State<'_, NativeGateways>,
) -> Result<Value, String> {
    let service = state.corehid.clone();
    tauri::async_runtime::spawn_blocking(move || service.request(request))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn export_pico_firmware(app: tauri::AppHandle) -> Result<String, String> {
    let resources = app
        .path()
        .resource_dir()
        .map_err(|error| error.to_string())?;
    let downloads = app
        .path()
        .download_dir()
        .map_err(|error| error.to_string())?;
    firmware::export(&resources, &downloads).map(|path| path.display().to_string())
}

#[tauri::command]
fn open_pico_flashing_guide(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("pico-guide") {
        return window.set_focus().map_err(|error| error.to_string());
    }
    let guide_app = app.clone();
    WebviewWindowBuilder::new(
        &app,
        "pico-guide",
        WebviewUrl::App("pico-flashing.html".into()),
    )
    .title("Flash the Pico W · OpenSimGear")
    .inner_size(900.0, 820.0)
    .on_navigation(move |url| {
        if local_document(url) && url.path() == "/" {
            if let Some(window) = guide_app.get_webview_window("main") {
                let _ = window.set_focus();
            }
            return false;
        }
        if url.as_str() == "https://datasheets.raspberrypi.com/picow/pico-w-datasheet.pdf" {
            let _ = guide_app.opener().open_url(url.as_str(), None::<&str>);
            return false;
        }
        local_document(url)
            && (url.path() == "/pico-flashing.html" || url.path().starts_with("/firmware/"))
    })
    .build()
    .map(|_| ())
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
            let resources = app.path().resource_dir()?;
            app.manage(NativeGateways {
                pico: Arc::new(pico::PicoService::new()),
                corehid: Arc::new(corehid::CoreHidService::new(resources)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pico_gateway,
            corehid_gateway,
            open_pico_flashing_guide,
            export_pico_firmware
        ])
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                window.state::<NativeGateways>().shutdown();
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
            handle.state::<NativeGateways>().shutdown();
        }
    });
}
