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
    enabled: bool,
    port: u16,
}

impl Server {
    pub fn native(manager: Arc<GatewayManager>) -> Arc<Self> {
        Self::open(
            manager,
            Some(crate::config_paths::config_path().with_file_name("mcp-v1.json")),
        )
    }
    fn open(manager: Arc<GatewayManager>, path: Option<PathBuf>) -> Arc<Self> {
        let store = Store::open(path);
        let initial = SettingsUpdate {
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
        if initial.enabled {
            let _ = server.configure(initial);
        }
        server
    }
    pub fn status(&self) -> Value {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let settings = &state.store.settings;
        json!({"enabled":settings.enabled,"port":settings.port,"endpoint":format!("http://127.0.0.1:{}/mcp",settings.port),"running":state.listener.is_some(),"error":state.error,"credentials":settings.credentials.iter().map(Credential::metadata).collect::<Vec<_>>(),"protocolVersions":VERSIONS,"sdk":"rmcp 3.6.0","readOnly":state.store.error.is_some()})
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
        state.store.save(next)?;
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
    pub fn create_credential(&self, name: String, scope: String) -> Result<Value, String> {
        let _control = self.controls.lock().unwrap_or_else(|p| p.into_inner());
        let name = name.trim();
        if name.is_empty() || name.len() > 80 || !matches!(scope.as_str(), "read" | "write") {
            return Err("Use a name of 1–80 bytes and read or write access.".into());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.store.settings.credentials.len() >= 32 {
            return Err("Remove an unused credential before creating another (maximum 32).".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let token = format!(
            "osg_mcp_{}.{}{}",
            id,
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let credential = Credential {
            id,
            name: name.into(),
            scope,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            hash: crate::mcp_credentials::digest(&token),
        };
        let metadata = credential.metadata();
        let mut next = state.store.settings.clone();
        next.credentials.push(credential);
        state.store.save(next)?;
        let enabled = state.store.settings.enabled && state.listener.is_some();
        drop(state);
        self.native_authorization(metadata["id"].as_str().unwrap(), enabled);
        Ok(json!({"credential":metadata,"token":token,"status":self.status()}))
    }
    pub fn revoke_credential(&self, id: &str) -> Result<Value, String> {
        let _control = self.controls.lock().unwrap_or_else(|p| p.into_inner());
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut next = state.store.settings.clone();
        next.credentials.retain(|c| c.id != id);
        if next.credentials.len() == state.store.settings.credentials.len() {
            return Err("That MCP credential no longer exists. Refresh the list.".into());
        }
        state.store.save(next)?;
        drop(state);
        self.native_authorization(id, false);
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
    fn native_authorization(&self, id: &str, authorized: bool) {
        self.manager
            .mcp_authorize_client(&mcp_tools::native_client(&format!("mcp-{id}")), authorized);
    }
    fn authenticate(&self, token: &str, generation: u64) -> Option<Principal> {
        if token.len() > 256 {
            return None;
        }
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed
            || !state.store.settings.enabled
            || !state
                .listener
                .as_ref()
                .is_some_and(|l| l.generation == generation)
        {
            return None;
        }
        state
            .store
            .settings
            .credentials
            .iter()
            .find(|c| crate::mcp_credentials::matches(token, &c.hash))
            .map(|c| Principal {
                id: c.id.clone(),
                scope: c.scope.clone(),
                generation,
            })
    }
    fn authorized(&self, principal: &Principal, mutate: bool) -> bool {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        !state.closed
            && state.store.settings.enabled
            && state
                .listener
                .as_ref()
                .is_some_and(|l| l.generation == principal.generation)
            && state.store.settings.credentials.iter().any(|c| {
                c.id == principal.id
                    && c.scope == principal.scope
                    && (!mutate || c.scope == "write")
            })
    }
    async fn listen(
        self: Arc<Self>,
        socket: std::net::TcpListener,
        port: u16,
        generation: u64,
        cancellation: CancellationToken,
    ) {
        let weak = Arc::downgrade(&self);
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_cancellation_token(cancellation.child_token())
            .with_max_request_body_bytes(BODY_LIMIT)
            .with_allowed_hosts([format!("127.0.0.1:{port}"), format!("localhost:{port}")])
            .with_allowed_origins([
                format!("http://127.0.0.1:{port}"),
                format!("http://localhost:{port}"),
            ])
            .enforce_origin_validation();
        let service = StreamableHttpService::new(
            move || {
                Ok(Handler {
                    server: weak.clone(),
                })
            },
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let app =
            Router::new()
                .nest_service("/mcp", service)
                .layer(middleware::from_fn_with_state(
                    AuthState {
                        server: self.clone(),
                        generation,
                    },
                    authenticate,
                ));
        let result = match tokio::net::TcpListener::from_std(socket) {
            Ok(listener) => axum::serve(listener, app)
                .with_graceful_shutdown(cancellation.cancelled_owned())
                .await
                .map_err(|e| e.to_string()),
            Err(error) => Err(error.to_string()),
        };
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state
            .listener
            .as_ref()
            .is_some_and(|l| l.generation == generation)
        {
            state.listener = None;
            if let Err(error) = result {
                state.error = Some(format!("MCP server stopped: {error}"));
            }
        }
    }
}

async fn authenticate(
    AxumState(auth): AxumState<AuthState>,
    mut request: Request,
    next: Next,
) -> Response {
    let principal = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .and_then(|token| auth.server.authenticate(token, auth.generation));
    match principal {
        Some(principal) => {
            request.extensions_mut().insert(principal);
            next.run(request).await
        }
        None => (
            StatusCode::UNAUTHORIZED,
            [(
                header::WWW_AUTHENTICATE,
                "Bearer realm=\"OpenSimGear Cockpit\"",
            )],
            "A valid local Cockpit MCP credential is required.",
        )
            .into_response(),
    }
}

#[derive(Clone)]
struct Handler {
    server: std::sync::Weak<Server>,
}
impl Handler {
    fn server(&self) -> Result<Arc<Server>, ErrorData> {
        self.server
            .upgrade()
            .ok_or_else(|| ErrorData::internal_error("Cockpit is shutting down.", None))
    }
    fn principal(
        &self,
        context: &RequestContext<RoleServer>,
        mutate: bool,
    ) -> Result<(Arc<Server>, Principal), ErrorData> {
        let server = self.server()?;
        let principal = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<Principal>())
            .cloned()
            .ok_or_else(|| {
                ErrorData::invalid_request("Missing authenticated local client.", None)
            })?;
        if !server.authorized(&principal, mutate) {
            return Err(ErrorData::invalid_request(
                "This credential is revoked, disabled, or does not permit changes.",
                None,
            ));
        }
        Ok((server, principal))
    }
}
fn bounded(value: Value) -> Value {
    if serde_json::to_vec(&value).map_or(true, |bytes| bytes.len() > RESPONSE_LIMIT / 2) {
        mcp_tools::failure(
            "response_limit",
            "Result exceeds 2 MiB. Use a smaller catalogue/inspection page.",
            false,
        )
    } else {
        value
    }
}
fn operation_error(code: &str, message: &str, operation_id: Option<Value>) -> Value {
    let mut value = mcp_tools::failure(code, message, false);
    if let Some(id) = operation_id.filter(Value::is_string) {
        value["operationId"] = id;
    }
    value
}
impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_server_info(Implementation::new("opensimgear-cockpit", env!("CARGO_PKG_VERSION")).with_title("OpenSimGear Cockpit"))
            .with_instructions("Configure the running OpenSimGear Cockpit. Discover current IDs and revisions, prepare and validate a draft, save it, then explicitly apply the exact saved revision and observed owner. Use an operationId for changes and reconcile uncertain results. Read access cannot mutate. Device writes retain native claims and generation checks.")
    }
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2026_07_28,
        ])
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let (_, principal) = self.principal(&context, false)?;
        if request.and_then(|r| r.cursor).is_some() {
            return Err(ErrorData::invalid_params(
                "The tool catalogue fits one page; omit cursor.",
                None,
            ));
        }
        let tools = mcp_tools::catalogue()
            .into_iter()
            .filter(|tool| {
                principal.scope == "write"
                    || !mcp_tools::is_mutation(tool["name"].as_str().unwrap_or(""))
            })
            .collect::<Vec<_>>();
        serde_json::from_value(json!({"tools":tools,"ttlMs":0,"cacheScope":"private"})).map_err(
            |e| ErrorData::internal_error(format!("Invalid native tool contract: {e}"), None),
        )
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        mcp_tools::catalogue()
            .into_iter()
            .find(|tool| tool["name"] == name)
            .and_then(|v| serde_json::from_value(v).ok())
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        let mutation = mcp_tools::is_mutation(&name);
        let (server, principal) = self.principal(&context, mutation)?;
        let urgent = matches!(
            name.as_str(),
            "profile_stop" | "activation_cancel" | "profile_recover"
        );
        let permit = (if urgent {
            server.urgent.clone()
        } else {
            server.calls.clone()
        })
        .try_acquire_owned();
        let Ok(permit) = permit else {
            return Ok(CallToolResult::structured_error(mcp_tools::failure(
                "busy",
                "Cockpit is serving the maximum concurrent requests. Retry this operationId later.",
                true,
            ))
            .into());
        };
        let args = Value::Object(request.arguments.unwrap_or_default());
        let operation_id = args.get("operationId").cloned();
        let tools = server.tools.clone();
        let worker_server = server.clone();
        let worker_principal = principal.clone();
        let work = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            if !worker_server.authorized(&worker_principal, mutation) {
                return mcp_tools::failure(
                    "access_denied",
                    "Credential revoked or integration disabled before dispatch.",
                    false,
                );
            }
            bounded(tools.call(&name, args, &format!("mcp-{}", worker_principal.id)))
        });
        let value=match tokio::time::timeout(Duration::from_secs(130),work).await {
            Ok(Ok(value))=>value,
            Ok(Err(_))=>operation_error("native_failure","Native task failed. Inspect current state before retrying.",operation_id),
            Err(_)=>operation_error("outcome_pending","Native work may still complete. Reconcile this operationId or activation status; do not repeat with a new operationId.",operation_id),
        };
        Ok(if value["ok"] == false {
            CallToolResult::structured_error(value)
        } else {
            CallToolResult::structured(value)
        }
        .into())
    }
    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        self.principal(&context, false)?;
        if request.and_then(|r| r.cursor).is_some() {
            return Err(ErrorData::invalid_params(
                "The context catalogue fits one page; omit cursor.",
                None,
            ));
        }
        serde_json::from_value(
            json!({"resources":mcp_tools::resources(),"ttlMs":0,"cacheScope":"private"}),
        )
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))
    }
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let (server, principal) = self.principal(&context, false)?;
        let tools = server.tools.clone();
        let uri = request.uri;
        let permit = server.calls.clone().try_acquire_owned().map_err(|_| {
            ErrorData::internal_error("Cockpit is busy; retry the context read later.", None)
        })?;
        let read_uri = uri.clone();
        let value = tokio::time::timeout(
            Duration::from_secs(30),
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                if !server.authorized(&principal, false) {
                    return json!({"ok":false,"error":{"code":"access_denied"}});
                }
                bounded(tools.read_resource(&read_uri, &format!("mcp-{}", principal.id)))
            }),
        )
        .await
        .map_err(|_| ErrorData::internal_error("Context read timed out.", None))?
        .map_err(|_| ErrorData::internal_error("Context read failed.", None))?;
        if value["ok"] == false {
            return Err(ErrorData::invalid_params(
                "Unknown or unavailable Cockpit context resource.",
                Some(value),
            ));
        }
        let result:ReadResourceResult=serde_json::from_value(json!({"contents":[{"uri":uri,"mimeType":"application/json","text":value.to_string()}],"ttlMs":0,"cacheScope":"private"})).map_err(|e|ErrorData::internal_error(e.to_string(),None))?;
        Ok(result.into())
    }
}

#[tauri::command]
pub fn mcp_settings_get(state: tauri::State<'_, Arc<Server>>) -> Value {
    state.status()
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
    state: tauri::State<'_, Arc<Server>>,
) -> Result<Value, String> {
    let server = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.create_credential(name, scope))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn mcp_credential_revoke(
    id: String,
    state: tauri::State<'_, Arc<Server>>,
) -> Result<Value, String> {
    let server = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || server.revoke_credential(&id))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(
        endpoint: &str,
        token: Option<&str>,
        payload: Value,
    ) -> Result<ureq::Response, ureq::Error> {
        let mut request = ureq::post(endpoint)
            .timeout(Duration::from_secs(10))
            .set("Accept", "application/json, text/event-stream")
            .set("MCP-Protocol-Version", "2025-11-25");
        if let Some(token) = token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        request.send_json(payload)
    }
    #[test]
    fn listener_authentication_scope_revocation_and_lifecycle() {
        let manager = Arc::new(GatewayManager::mcp_test_manager());
        let server = Server::open(manager, None);
        assert_eq!(server.status()["enabled"], false);
        let read = server
            .create_credential("Reader".into(), "read".into())
            .unwrap();
        let write = server
            .create_credential("Writer".into(), "write".into())
            .unwrap();
        let token = read["token"].as_str().unwrap();
        assert!(!server.status().to_string().contains(token));
        let socket = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        server
            .configure(SettingsUpdate {
                enabled: true,
                port,
            })
            .unwrap();
        let endpoint = format!("http://127.0.0.1:{port}/mcp");
        let payload = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
        assert!(matches!(
            request(&endpoint, None, payload.clone()),
            Err(ureq::Error::Status(401, _))
        ));
        let read_list: Value = request(&endpoint, Some(token), payload.clone())
            .unwrap()
            .into_json()
            .unwrap();
        assert!(
            read_list["result"]["tools"].as_array().is_some(),
            "{read_list}"
        );
        assert!(!read_list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "profile_create"));
        let write_list: Value = request(&endpoint, write["token"].as_str(), payload)
            .unwrap()
            .into_json()
            .unwrap();
        assert!(write_list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "profile_create"));
        let denied:Value=request(&endpoint,Some(token),json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"profile_create","arguments":{"name":"Forbidden","operationId":uuid::Uuid::new_v4().to_string()}}})).unwrap().into_json().unwrap();
        assert!(denied.get("error").is_some(), "{denied}");
        let invalid_origin = ureq::post(&endpoint)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Origin", "https://example.invalid")
            .set("Accept", "application/json, text/event-stream")
            .send_json(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}));
        assert!(matches!(invalid_origin, Err(ureq::Error::Status(403, _))));
        let invalid_host = ureq::post(&endpoint)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Host", "attacker.invalid")
            .set("Accept", "application/json, text/event-stream")
            .send_json(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}));
        assert!(matches!(invalid_host, Err(ureq::Error::Status(403, _))));
        server
            .revoke_credential(read["credential"]["id"].as_str().unwrap())
            .unwrap();
        assert!(matches!(
            request(
                &endpoint,
                Some(token),
                json!({"jsonrpc":"2.0","id":4,"method":"tools/list"})
            ),
            Err(ureq::Error::Status(401, _))
        ));
        server
            .configure(SettingsUpdate {
                enabled: false,
                port,
            })
            .unwrap();
        assert_eq!(server.status()["running"], false);
        server.shutdown();
    }
    #[test]
    fn occupied_port_does_not_destroy_existing_configuration() {
        let server = Server::open(Arc::new(GatewayManager::mcp_test_manager()), None);
        let occupied = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = occupied.local_addr().unwrap().port();
        assert!(server
            .configure(SettingsUpdate {
                enabled: true,
                port
            })
            .is_err());
        assert_eq!(server.status()["enabled"], false);
        assert_eq!(server.status()["running"], false);
        assert!(server.status()["error"].is_string());
        assert!(server
            .configure(SettingsUpdate {
                enabled: true,
                port: 80
            })
            .is_err());
        server.shutdown();
    }
}
