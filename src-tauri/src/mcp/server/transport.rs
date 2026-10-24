use super::*;

impl Server {
    pub(super) async fn listen(
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
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .filter(|token|token.len()<=256)
        .map(str::to_owned);
    let principal=if let Some(token)=token {
        tauri::async_runtime::spawn_blocking(move||auth.server.authenticate(&token,auth.generation)).await.ok().flatten()
    }else{None};
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
pub(super) fn bounded(value: Value) -> Value {
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
pub(super) fn operation_error(code: &str, message: &str, operation_id: Option<Value>) -> Value {
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
