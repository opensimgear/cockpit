mod lifecycle;
mod status_events;
use status_events::StatusEvents;
use lifecycle::{Lifecycle, LifecycleSnapshot};
use osg_companion_service::client::ClientRuntime;
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};
use tauri::{Emitter, Manager};
#[derive(Clone)]
struct CacheDirectory(std::path::PathBuf);

#[tauri::command]
async fn companion_cache_request(
    state:tauri::State<'_,CacheDirectory>,
    request:osg_companion_service::client_cache::CacheRequest,
)->Result<osg_companion_service::client_cache::Snapshot,String>{
    let directory=state.inner().0.clone();
    tauri::async_runtime::spawn_blocking(move||osg_companion_service::client_cache::ClientCache::open(&directory)?.execute(request))
        .await.map_err(|_|"Offline cache worker failed.".to_owned())?
}

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
    _app: tauri::AppHandle,
    state: tauri::State<'_, ClientBridge>,
    events: tauri::State<'_, StatusEvents>,
    request: Value,
) -> Result<Value, String> {
    if request["type"] == "status" { events.acknowledge(); }
    if state.0.get().is_none() && request["type"] == "status" {
        return Ok(json!({"phase":"initializing","busy":true,"epoch":0,
            "protectedStoreAvailable":false,"authenticationAvailable":false,
            "authenticationUnavailableReason":"Native protected storage is initializing",
            "assignments":[],"feedback":[]}));
    }
    let client = state.client()?;
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "android")]
        if request["type"].as_str().is_some_and(ClientRuntime::requires_network_access) {
            let platform = _app.state::<tauri_plugin_companion_platform::PlatformStore<tauri::Wry>>();
            let access = platform.request_local_network()
                .inspect_err(|_| client.set_network_access(false))?;
            client.set_network_access(access.allowed);
            if !access.allowed {
                return Err("Allow Local Network access for OpenSimGear Companion in Android Settings, then try again".into());
            }
            if access.prompted {
                return Err("Local Network access is granted. Select the action again to continue".into());
            }
            if request["type"] == "discover" {
                return platform.with_discovery(|| client.command(request));
            }
        }
        client.command(request)
    })
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

#[cfg(target_os = "android")]
fn refresh_android_network_access(window: &tauri::Window) {
    let app = window.app_handle().clone();
    let Ok(client) = app.state::<ClientBridge>().client() else { return };
    let epoch = app.state::<Lifecycle>().snapshot().invalidation_epoch;
    tauri::async_runtime::spawn_blocking(move || {
        let platform = app.state::<tauri_plugin_companion_platform::PlatformStore<tauri::Wry>>();
        let allowed = platform.local_network_allowed();
        if app.state::<Lifecycle>().snapshot().invalidation_epoch == epoch {
            client.set_network_access(allowed);
        }
    });
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
            app.manage(CacheDirectory(path.clone()));
            let bridge = ClientBridge::default();
            app.manage(bridge.clone());
            let events = StatusEvents::default();
            app.manage(events.clone());
            let app_handle = app.handle().clone();
            #[cfg(target_os = "android")]
            let store = app
                .state::<tauri_plugin_companion_platform::PlatformStore<tauri::Wry>>()
                .inner()
                .clone();
            // Keychain can prompt. Android callbacks must not block
            // the UI thread that dispatches the mobile plugin.
            tauri::async_runtime::spawn_blocking(move || {
                #[cfg(target_os = "android")]
                let client = {
                    let allowed = store.local_network_allowed();
                    let storage=osg_companion_service::persistence::SealedSqliteStore::with_sealer(path.join("companion-client.dpapi"),Arc::new(store));
                    ClientRuntime::with_storage_network_access(path, Arc::new(storage), allowed)
                };
                #[cfg(not(target_os = "android"))]
                let client = ClientRuntime::open(path);
                let _ = bridge.0.set(client.map(Arc::new));
                if let Ok(client) = bridge.client() {
                    events.start(app_handle, &client);
                } else {
                    let _ = app_handle.emit(status_events::CHANGED, ());
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            companion_lifecycle,
            companion_cache_request,
            companion_client_request
        ])
        .on_window_event(|window, event| {
            if let Some(reason) = lifecycle::reason(event) {
                invalidate(window, reason);
            }
            #[cfg(target_os = "android")]
            if matches!(event, tauri::WindowEvent::Resumed) {
                refresh_android_network_access(window);
            }
        })
        .build(tauri::generate_context!())
        .expect("error building OpenSimGear Companion")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                app.state::<StatusEvents>().stop();
            }
        });
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
