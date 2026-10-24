//! Local MCP transport. All tools share the desktop's existing native manager.
use crate::{
    gateways::GatewayManager,
    mcp_credentials::{Credential, Store},
    mcp_tools::{self, Tools},
};
use axum::{
    extract::{Request, State as AxumState},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use rmcp::{
    model::*,
    service::RequestContext,
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    ErrorData, RoleServer, ServerHandler,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

const BODY_LIMIT: usize = 2_097_152;
// SDK structured results also include a text representation for older clients.
const RESPONSE_LIMIT: usize = 4_194_304;
pub const VERSIONS: [&str; 2] = ["2025-11-25", "2026-07-28"];

#[derive(Clone)]
struct Principal {
    credential_hash:String,
    id: String,
    scope: String,
    generation: u64,
}
#[derive(Clone)]
struct AuthState {
    server: Arc<Server>,
    generation: u64,
}
struct Listener {
    cancellation: CancellationToken,
    port: u16,
    generation: u64,
}
struct State {
    store: Store,
    listener: Option<Listener>,
    generation: u64,
    error: Option<String>,
    closed: bool,
}
pub struct Server {
    state: Mutex<State>,
    controls: Mutex<()>,
    manager: Arc<GatewayManager>,
    tools: Arc<Tools>,
    calls: Arc<Semaphore>,
    urgent: Arc<Semaphore>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsUpdate {
    pub(crate) expected_settings_revision:u64,
    pub(crate) enabled: bool,
    pub(crate) port: u16,
}

impl Server {
    pub fn native(manager: Arc<GatewayManager>) -> Arc<Self> {
        Self::open(
            manager,
            Some(crate::config_paths::config_path().with_file_name("mcp-v1.json")),
        )
    }
    pub(crate) fn open(manager: Arc<GatewayManager>, path: Option<PathBuf>) -> Arc<Self> {
        let store = Store::open(path);
        let initial = SettingsUpdate {
            expected_settings_revision:store.revision,
            enabled: store.settings.enabled,
            port: store.settings.port,
        };
        let server = Arc::new(Self {
            state: Mutex::new(State {
                error: store.error.clone(),
                store,
                listener: None,
                generation: 0,
                closed: false,
            }),
            controls: Mutex::new(()),
            tools: Arc::new(Tools::new(manager.clone())),
            manager,
            calls: Arc::new(Semaphore::new(12)),
            urgent: Arc::new(Semaphore::new(4)),
        });
        server.tools.bind_integration(Arc::downgrade(&server));
        if initial.enabled {
            let _ = server.configure(initial);
        }
        server
    }
    pub fn status(&self) -> Value {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let _=self.refresh_credentials(&mut state);
        let settings = &state.store.settings;
        json!({"settingsRevision":state.store.revision,"enabled":settings.enabled,"port":settings.port,"endpoint":format!("http://127.0.0.1:{}/mcp",settings.port),"running":state.listener.is_some(),"error":state.error,"credentials":settings.credentials.iter().map(Credential::metadata).collect::<Vec<_>>(),"protocolVersions":VERSIONS,"sdk":"rmcp 3.6.0","readOnly":state.store.error.is_some()})
    }
    pub fn configure(self: &Arc<Self>, update: SettingsUpdate) -> Result<Value, String> {
        let _control = self.controls.lock().unwrap_or_else(|p| p.into_inner());
        if update.port < 1024 {
            return Err("Choose a TCP port between 1024 and 65535.".into());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed {
            return Err("Cockpit is shutting down.".into());
        }
        self.refresh_credentials(&mut state)?;
        state.store.check_revision(update.expected_settings_revision)?;
        let unchanged = update.enabled
            && state
                .listener
                .as_ref()
                .is_some_and(|l| l.port == update.port);
        let socket = if update.enabled && !unchanged {
            match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, update.port)) {
                Ok(socket) => {
                    socket.set_nonblocking(true).map_err(|e| e.to_string())?;
                    Some(socket)
                }
                Err(error) => {
                    let message = format!(
                        "Cannot start the local MCP server on port {}: {error}",
                        update.port
                    );
                    state.error = Some(message.clone());
                    return Err(message);
                }
            }
        } else {
            None
        };
        let mut next = state.store.settings.clone();
        next.enabled = update.enabled;
        next.port = update.port;
        if next!=state.store.settings {state.store.save(next)?;}
        if !unchanged && state.listener.is_some() {
            for credential in &state.store.settings.credentials {
                self.native_authorization(&credential.id, false);
            }
        }
        for credential in &state.store.settings.credentials {
            self.native_authorization(&credential.id, update.enabled);
        }
        state.error = None;
        if !unchanged {
            if let Some(old) = state.listener.take() {
                old.cancellation.cancel();
            }
            state.generation = state.generation.wrapping_add(1);
            if let Some(socket) = socket {
                let generation = state.generation;
                let cancellation = CancellationToken::new();
                state.listener = Some(Listener {
                    cancellation: cancellation.clone(),
                    port: update.port,
                    generation,
                });
                let server = self.clone();
                tauri::async_runtime::spawn(async move {
                    server
                        .listen(socket, update.port, generation, cancellation)
                        .await;
                });
            }
        }
        drop(state);
        Ok(self.status())
    }
    pub fn shutdown(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.closed = true;
        for credential in &state.store.settings.credentials {
            self.native_authorization(&credential.id, false);
        }
        if let Some(listener) = state.listener.take() {
            listener.cancellation.cancel();
        }
    }

}

#[tauri::command]
pub async fn mcp_settings_get(state: tauri::State<'_, Arc<Server>>) -> Result<Value,String> {
    let server=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||server.status()).await.map_err(|_|"MCP settings worker failed.".into())
}
#[tauri::command]
pub async fn mcp_settings_update(
    settings: SettingsUpdate,
    state: tauri::State<'_, Arc<Server>>,
) -> Result<Value, String> {
    let server = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.configure(settings))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn mcp_credential_create(
    name: String,
    scope: String,
    expected_settings_revision:u64,
    state: tauri::State<'_, Arc<Server>>,
) -> Result<Value, String> {
    let server = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.create_credential(name, scope,expected_settings_revision))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn mcp_credential_revoke(
    id: String,
    expected_settings_revision:u64,
    state: tauri::State<'_, Arc<Server>>,
) -> Result<Value, String> {
    let server = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.revoke_credential(&id,expected_settings_revision))
        .await
        .map_err(|e| e.to_string())?
}


#[tauri::command]
pub async fn mcp_credential_rotate(id:String,expected_settings_revision:u64,state:tauri::State<'_,Arc<Server>>)->Result<Value,String>{
    let server=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||server.rotate_credential(&id,expected_settings_revision)).await.map_err(|_|"MCP credential worker failed.".to_owned())?
}

mod auth;
mod credentials;
mod transport;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ipc_tests;
#[cfg(test)]
mod transport_tests;
