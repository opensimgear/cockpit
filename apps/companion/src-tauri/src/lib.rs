mod lifecycle;
use lifecycle::{Lifecycle, LifecycleSnapshot};
use osg_companion_service::client::ClientRuntime;
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};
use tauri::{Emitter, Manager};

type ClientInitialization = Result<Arc<ClientRuntime>, String>;
#[derive(Clone, Default)]
struct ClientBridge(Arc<OnceLock<ClientInitialization>>);

impl ClientBridge {
    fn client(&self) -> Result<Arc<ClientRuntime>, String> {
        self.0
            .get()
            .ok_or_else(|| "Native protected storage is initializing; please retry".to_string())?
            .as_ref()
            .cloned()
            .map_err(Clone::clone)
    }
}

#[tauri::command]
fn companion_lifecycle(state: tauri::State<'_, Lifecycle>) -> LifecycleSnapshot {
    state.snapshot()
}

#[tauri::command]
async fn companion_client_request(
    state: tauri::State<'_, ClientBridge>,
    window: tauri::Window,
    request: Value,
) -> Result<Value, String> {
    if state.0.get().is_none() && request["type"] == "status" {
        return Ok(json!({"phase":"initializing","busy":true,"epoch":0,
            "protectedStoreAvailable":false,"authenticationAvailable":false,
            "authenticationUnavailableReason":"Native protected storage is initializing",
            "assignments":[],"feedback":[]}));
    }
    let client = state.client()?;
    #[cfg(desktop)]
    if lifecycle::input_command(&request)
        && (!window.is_focused().unwrap_or(false)
            || !window.is_visible().unwrap_or(false)
            || !lifecycle::desktop_input_available())
    {
        client.invalidate(
            "Window is inactive or workstation is locked; Connect and Start explicitly",
        );
        return Err(
            "Input is disarmed while the window is inactive or the workstation is locked".into(),
        );
    }
    #[cfg(mobile)]
    let _ = window;
    tauri::async_runtime::spawn_blocking(move || client.command(request))
        .await
        .map_err(|error| error.to_string())?
}

fn invalidate(window: &tauri::Window, reason: &str) {
    if let Some(bridge) = window.try_state::<ClientBridge>() {
        if let Ok(client) = bridge.client() {
            if matches!(
                client.status().phase.as_str(),
                "armed" | "ready" | "connecting"
            ) {
                client.invalidate(reason);
            }
        }
    }
    let lifecycle = window.state::<Lifecycle>();
    let snapshot = lifecycle.invalidate();
    let _ = window.emit("companion-lifecycle-invalidated", snapshot);
}

fn local_document(url: &tauri::Url) -> bool {
    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https")
            && url.host_str() == Some("tauri.localhost")
            && url.port().is_none())
        || (cfg!(debug_assertions)
            && url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port() == Some(1431))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "android")]
    let builder = builder.plugin(tauri_plugin_companion_platform::init());
    builder
        .manage(Lifecycle::default())
        .plugin(
            tauri::plugin::Builder::<tauri::Wry, ()>::new("local-navigation")
                .on_navigation(|_, url| local_document(url))
                .build(),
        )
        .setup(|app| {
            let path = app
                .path()
                .app_data_dir()
                .map_err(|error| error.to_string())?
                .join("companion-client");
            let bridge = ClientBridge::default();
            app.manage(bridge.clone());
            #[cfg(target_os = "android")]
            let store = app
                .state::<tauri_plugin_companion_platform::PlatformStore<tauri::Wry>>()
                .inner()
                .clone();
            // Keychain/Secret Service can prompt. Android callbacks must not block
            // the UI thread that dispatches the mobile plugin.
            tauri::async_runtime::spawn_blocking(move || {
                #[cfg(target_os = "android")]
                let client = ClientRuntime::with_storage(path, Arc::new(store));
                #[cfg(not(target_os = "android"))]
                let client = ClientRuntime::open(path);
                let _ = bridge.0.set(client.map(Arc::new));
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            companion_lifecycle,
            companion_client_request
        ])
        .on_window_event(|window, event| {
            if let Some(reason) = lifecycle::reason(event) {
                invalidate(window, reason);
            }
        })
        .run(tauri::generate_context!())
        .expect("error running OpenSimGear Companion");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_never_gives_remote_documents_native_client_access() {
        for address in [
            "tauri://localhost/",
            "http://tauri.localhost/",
            "https://tauri.localhost/",
            "http://127.0.0.1:1431/",
        ] {
            assert!(local_document(&tauri::Url::parse(address).unwrap()));
        }
        for address in [
            "https://example.com/",
            "https://127.0.0.1:1431/",
            "http://127.0.0.1:1430/",
            "http://tauri.localhost:5000/",
            "file:///tmp/panel.html",
            "javascript:alert(1)",
        ] {
            assert!(!local_document(&tauri::Url::parse(address).unwrap()));
        }
    }
    #[test]
    fn asynchronous_initialization_never_exposes_a_half_ready_client() {
        let bridge = ClientBridge::default();
        assert!(bridge.client().is_err());
        bridge
            .0
            .set(Err("Protected store locked".into()))
            .unwrap_or_else(|_| panic!("already set"));
        assert_eq!(bridge.client().err().unwrap(), "Protected store locked");
    }
}
