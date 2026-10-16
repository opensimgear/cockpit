use osg_companion_service::client::ClientRuntime;
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// Lifecycle epoch invalidates local preview if JavaScript missed an event while suspended.
/// Active remote sessions are invalidated natively before the suspended UI can resume.
#[derive(Default)]
struct Lifecycle(AtomicU64);

impl Lifecycle {
    fn invalidate(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleSnapshot {
    invalidation_epoch: u64,
}

#[tauri::command]
fn companion_lifecycle(state: tauri::State<'_, Lifecycle>) -> LifecycleSnapshot {
    LifecycleSnapshot {
        invalidation_epoch: state.0.load(Ordering::SeqCst),
    }
}

struct ClientBridge {
    client: Result<Arc<ClientRuntime>, String>,
}
#[tauri::command]
async fn companion_client_request(
    state: tauri::State<'_, ClientBridge>,
    request: Value,
) -> Result<Value, String> {
    let client = state.client.as_ref().map_err(Clone::clone)?.clone();
    tauri::async_runtime::spawn_blocking(move || client.command(request))
        .await
        .map_err(|error| error.to_string())?
}

fn invalidate(window: &tauri::Window) {
    if let Some(bridge) = window.try_state::<ClientBridge>() {
        if let Ok(client) = &bridge.client {
            if matches!(
                client.status().phase.as_str(),
                "armed" | "ready" | "connecting"
            ) {
                client.invalidate("Native lifecycle changed; Connect and Start explicitly");
            }
        }
    }
    let lifecycle = window.state::<Lifecycle>();
    let invalidation_epoch = lifecycle.invalidate();
    let _ = window.emit(
        "companion-lifecycle-invalidated",
        LifecycleSnapshot { invalidation_epoch },
    );
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
    tauri::Builder::default()
        .manage(Lifecycle::default())
        .plugin(
            tauri::plugin::Builder::<tauri::Wry, ()>::new("local-navigation")
                .on_navigation(|_, url| local_document(url))
                .build(),
        )
        .setup(|app| {
            let client = app
                .path()
                .app_data_dir()
                .map_err(|error| error.to_string())
                .and_then(|path| ClientRuntime::open(path.join("companion-client")))
                .map(Arc::new);
            app.manage(ClientBridge { client });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            companion_lifecycle,
            companion_client_request
        ])
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Focused(false) | tauri::WindowEvent::Destroyed => {
                invalidate(window)
            }
            #[cfg(mobile)]
            tauri::WindowEvent::Suspended | tauri::WindowEvent::Resumed => invalidate(window),
            _ => {}
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
    fn every_native_invalidation_advances_epoch_without_rearming_anything() {
        let lifecycle = Lifecycle::default();
        assert_eq!(lifecycle.0.load(Ordering::SeqCst), 0);
        assert_eq!(lifecycle.invalidate(), 1);
        assert_eq!(lifecycle.invalidate(), 2);
        assert_eq!(lifecycle.0.load(Ordering::SeqCst), 2);
    }
}
