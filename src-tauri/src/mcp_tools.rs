//! Task-oriented MCP adapter for the running native manager. This boundary has
//! no general command, filesystem, shell or arbitrary action tool. Companion
//! firmware tools use only the approved native resolver and device guards.
use crate::gateways::GatewayManager;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

#[path = "mcp_tools/authoring.rs"]
mod authoring;
#[path = "mcp_tools/workspace.rs"]
mod workspace;
#[path="mcp_tools/host_settings.rs"]
mod host_settings;
#[path="mcp_tools/companion_controls.rs"]
mod companion_controls;
#[path="mcp_tools/integration_settings.rs"]
mod integration_settings;
#[path = "mcp_tools/catalogue.rs"]
mod definitions;
#[path = "mcp_tools/schema.rs"]
mod schema;
pub use definitions::{catalogue, is_mutation, resources};

const MAX_REQUEST: usize = osg_profiles::MAX_EDITOR_REQUEST_BYTES;
const MAX_REPLY: usize = 1_900_000;
const MAX_CLIENTS: usize = 16;
const MAX_OPERATIONS: usize = 128;
const MAX_OPERATION_REPLY: usize = 48 * 1024;
const MAX_DRAFTS: usize = 16;
const MAX_DRAFT_BYTES: usize = 8 * 1024 * 1024;
const DRAFT_LIFETIME: Duration = Duration::from_secs(30 * 60);

trait Backend: Send + Sync {
    fn request(&self, request: Value) -> Value;
    fn guarded(&self, request: Value, expected_owner: Value) -> Value;
}
impl Backend for GatewayManager {
    fn request(&self, request: Value) -> Value {
        self.mcp_request(request)
    }
    fn guarded(&self, request: Value, expected_owner: Value) -> Value {
        self.mcp_guarded_request(request, expected_owner)
    }
}

#[derive(Clone)]
struct Operation {
    signature: String,
    result: Option<Value>,
}
#[derive(Default)]
struct Ledger {
    clients: BTreeMap<String, BTreeMap<String, Operation>>,
}
enum Admission {
    Fresh,
    Reply(Value),
}
impl Ledger {
    fn begin(&mut self, client: &str, id: &str, signature: String) -> Result<Admission, Value> {
        if let Some(entry) = self.clients.get(client).and_then(|entries| entries.get(id)) {
            if entry.signature != signature {
                return Err(failure("operation_conflict", "This operationId was already used with different arguments. Inspect operation_status and submit a new operationId.", false));
            }
            return Ok(Admission::Reply(entry.result.clone().unwrap_or_else(|| json!({"ok":true,"data":{"operationId":id,"state":"pending","reconciliation":"Poll operation_status; the original request is still running."}}))));
        }
        // Never evict an operation to make room: a reconnect must not turn an
        // old create/save into a fresh mutation. A restart is an explicit ledger
        // boundary; clients must reconcile native state before retrying then.
        if !self.clients.contains_key(client) && self.clients.len() >= MAX_CLIENTS {
            return Err(failure("capacity", "The MCP operation client limit has been reached. Reconcile native state and restart MCP before admitting a new client.", false));
        }
        let entries = self.clients.entry(client.into()).or_default();
        if entries.len() >= MAX_OPERATIONS {
            return Err(failure("capacity", "The retained operation limit for this client has been reached. Reconcile native state before restarting MCP.", false));
        }
        entries.insert(
            id.into(),
            Operation {
                signature,
                result: None,
            },
        );
        Ok(Admission::Fresh)
    }
    fn finish(&mut self, client: &str, id: &str, result: &Value) {
        if let Some(entry) = self
            .clients
            .get_mut(client)
            .and_then(|entries| entries.get_mut(id))
        {
            entry.result = Some(compact_operation_result(result, id));
        }
    }
    fn status(&self, client: &str, id: &str) -> Value {
        match self.clients.get(client).and_then(|entries| entries.get(id)) {
            Some(entry) => json!({"ok":true,"data":{"operationId":id,"state":if entry.result.is_some(){"completed"}else{"pending"},"result":entry.result}}),
            None => failure("operation_unknown", "This operation is not retained for this client/server lifetime. Inspect native state before submitting a fresh operationId.", false),
        }
    }
}

#[derive(Clone)]
struct Draft {
    id: String,
    client: String,
    document: Value,
    assignments: Value,
    expected_revision: Value,
    created: Instant,
    bytes: usize,
}

pub struct Tools {
    backend: Arc<dyn Backend>,
    ledger: Mutex<Ledger>,
    drafts: Mutex<Vec<Draft>>,
    workspace_uploads: Mutex<Vec<workspace::Upload>>,
    integration:OnceLock<std::sync::Weak<crate::mcp::Server>>,
}
impl Tools {
    pub fn new(manager: Arc<GatewayManager>) -> Self {
        Self {
            backend: manager,
            ledger: Mutex::new(Ledger::default()),
            drafts: Mutex::new(vec![]),
            workspace_uploads: Mutex::new(vec![]),
            integration:OnceLock::new(),
        }
    }
    pub(crate) fn bind_integration(&self,server:std::sync::Weak<crate::mcp::Server>){let _=self.integration.set(server);}
    /// The caller authenticates the connection and provides a stable, native
    /// client identity. Session IDs are deliberately not a mutation authority.
    pub fn call(&self, name: &str, args: Value, client: &str) -> Value {
        if client.is_empty() || client.len() > 256 {
            return failure(
                "invalid_client",
                "A stable authenticated client identity is required.",
                false,
            );
        }
        let Some(schema) = catalogue().into_iter().find(|tool| tool["name"] == name) else {
            return failure(
                "unknown_tool",
                "Unknown Cockpit task tool. Discover tools/list again.",
                false,
            );
        };
        if args.to_string().len() > MAX_REQUEST {
            return failure(
                "request_too_large",
                "The task exceeds the 2 MiB request budget.",
                false,
            );
        }
        if let Err(error) = definitions::validate(&schema["inputSchema"], &args, "arguments") {
            return failure("invalid_arguments", &error, false);
        }
        let mutation = is_mutation(name);
        let operation = args["operationId"].as_str().unwrap_or("");
        if mutation {
            if !canonical_uuid(operation) {
                return failure(
                    "invalid_arguments",
                    "operationId must be a canonical lowercase UUID.",
                    false,
                );
            }
            let signature = digest(&json!([name, args]));
            match self
                .ledger
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .begin(client, operation, signature)
            {
                Ok(Admission::Fresh) => {}
                Ok(Admission::Reply(result)) | Err(result) => return result,
            }
        }
        let mut result = match self.dispatch(name, &args, client) {
            Ok(data) => json!({"ok":true,"data":data}),
            Err(error) => error,
        };
        if result.to_string().len() > MAX_REPLY {
            // Mutations must retain their known outcome. Their large documents
            // are inspectable separately and never cause an ambiguous error.
            result = if mutation {
                compact_operation_result(&result, operation)
            } else {
                failure(
                    "reply_too_large",
                    "The reply exceeds its budget. Request a smaller page or a specific section.",
                    false,
                )
            };
        }
        if mutation {
            result["operationId"] = json!(operation);
            self.ledger
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .finish(client, operation, &result);
        }
        result
    }
    pub fn read_resource(&self, uri: &str, client: &str) -> Value {
        let data = match uri {
            "cockpit://guide/configuration" => definitions::workflow(),
            "cockpit://schemas/profile" => definitions::profile_schema(),
            "cockpit://schemas/local-assignment" => definitions::assignment_schema(),
            "cockpit://schemas/task-results" => definitions::result_schema(),
            "cockpit://guide/widgets" => crate::widget_library::guide(),
            "cockpit://status" => return self.call("profile_status", json!({}), client),
            _ => return failure("resource_not_found", "Unknown Cockpit context resource. Use resources/list for the bounded context catalogue.", false),
        };
        json!({"ok":true,"data":data})
    }
    fn native(
        &self,
        command: &str,
        fields: Value,
        client: &str,
        section: Option<&str>,
    ) -> Result<Value, Value> {
        let mut request = fields;
        request["command"] = json!(command);
        request["clientId"] = json!(native_client(client));
        native_result(self.backend.request(request), section)
    }
    fn guarded(
        &self,
        command: &str,
        fields: Value,
        client: &str,
        owner: &Value,
        section: Option<&str>,
    ) -> Result<Value, Value> {
        let mut request = fields;
        request["command"] = json!(command);
        request["clientId"] = json!(native_client(client));
        native_result(self.backend.guarded(request, owner.clone()), section)
    }
    fn dispatch(&self, name: &str, args: &Value, client: &str) -> Result<Value, Value> {
        let selected = |keys: &[&str]| select(args, keys);
        let forward = |command: &str, keys: &[&str], section: Option<&str>| {
            self.native(command, selected(keys), client, section)
        };
        match name {
            "ui_preferences_get"=>forward("ui_preferences_get",&["key"],Some("uiPreferences")),
            "ui_preferences_save"=>forward("ui_preferences_save",&["key","expectedRevision","value"],Some("uiPreferences")),
            name if companion_controls::handles(name)=>self.companion_tool(name,args,client),
            "mcp_settings_get" | "mcp_settings_update" | "mcp_credential_create" | "mcp_credential_revoke" | "mcp_credential_rotate" => self.integration_tool(name,args),
            "companion_listener_get" | "companion_listener_save" => self.listener_tool(name,args,client),
            "designer_workspace_read" | "designer_workspace_begin" | "designer_workspace_chunk" | "designer_workspace_commit" | "designer_workspace_abort" => self.workspace_tool(name,args,client),
            "widgets_list" => forward("widgets_list", &["offset","limit"], Some("widgetLibrary")),
            "widget_get" => forward("widget_get", &["id","version"], Some("widgetLibrary")),
            "widget_save" => forward("widget_save", &["bundle","expectedVersion"], Some("widgetSaved")),
            "operation_status" => Ok(self
                .ledger
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .status(client, text(args, "operationId")?)["data"]
                .clone())
            .and_then(|data| {
                if data.is_null() {
                    Err(failure(
                        "operation_unknown",
                        "Operation not retained. Reconcile native state before retrying.",
                        false,
                    ))
                } else {
                    Ok(data)
                }
            }),
            "profiles_list" => {
                let mut state = forward("profile_list", &[], Some("profiles"))?;
                page_array(&mut state, "profiles", args)?;
                Ok(state)
            }
            "profile_get" => forward("profile_assignments_get", &["id"], Some("profiles")),
            "profile_export" => forward("profile_export", &["id"], Some("profiles")),
            "profile_status" => forward("profile_status", &[], Some("profiles")),
            "profile_create" => forward("profile_create", &["name"], Some("profiles")),
            "profile_duplicate" => forward(
                "profile_duplicate",
                &["id", "name", "expectedRevision"],
                Some("profiles"),
            ),
            "profile_rename" => forward(
                "profile_rename",
                &["id", "name", "expectedRevision"],
                Some("profiles"),
            ),
            "profile_delete" => forward(
                "profile_delete",
                &["id", "expectedRevision"],
                Some("profiles"),
            ),
            "profile_import" => {
                osg_profiles::ProfileDocument::parse(
                    &serde_json::to_vec(&args["document"]).unwrap(),
                )
                .map_err(|e| failure("invalid_document", &e, false))?;
                forward("profile_import", &["document", "name"], Some("profiles"))
            }
            "resources_list" => forward(
                "profile_resources_list",
                &["offset", "limit"],
                Some("profileResources"),
            )
            .map(|mut data| {
                next_offset(&mut data, "resources");
                data
            }),
            "resource_inspect" => forward(
                "profile_resources_inspect",
                &["resourceId", "offset", "limit"],
                Some("profileResources"),
            )
            .map(|mut data| {
                next_offset(&mut data, "ports");
                data
            }),
            "resource_search_ports" => self.search_ports(args, client),
            "processing_operations_list" => Ok(schema::operation_catalogue(args)),
            "games_list" => {
                let mut data = forward("game_get_state", &[], Some("games"))?;
                if let Some(games) = data["catalogue"].as_array_mut() {
                    for game in games {
                        if let Some(vehicles) = game["vehicleSupport"]
                            .as_object_mut()
                            .and_then(|support| support.remove("vehicles"))
                            .and_then(|value| value.as_array().map(Vec::len))
                        {
                            game["vehicleSupport"]["vehicleCount"] = json!(vehicles);
                            game["vehicleSupport"]["lookupTool"] = json!("game_vehicles_list");
                        }
                    }
                }
                page_array(&mut data, "catalogue", args)?;
                Ok(data)
            }
            "game_vehicles_list" => self.game_vehicles(args, client),
            "game_telemetry_inspect" => forward(
                "game_telemetry_inspect",
                &["gameId", "search", "group", "offset", "limit"],
                Some("gameTelemetry"),
            ),
            "game_aircraft_resource" => forward(
                "profile_dcs_vehicle_resource",
                &["vehicleId"],
                Some("profileResources"),
            ),
            "companions_list" => self.companions(args, client),
            "companion_panels_list" => {
                let mut data = forward(
                    "profile_companion_catalogue",
                    &[],
                    Some("profileCompanionAuthoring"),
                )?;
                page_array(&mut data, "panels", args)?;
                Ok(data)
            }
            "companion_inspect" => forward(
                "profile_companion_inspect",
                &["resourceId", "publicationHash", "offset", "limit"],
                Some("profileCompanionAuthoring"),
            ),
            "profile_prepare_changes" => self.prepare(args, client),
            "profile_save_prepared" => self.save_prepared(args, client),
            "compilation_inspect" => forward(
                "profile_compilation_inspect",
                &["compilationId", "section", "offset", "limit"],
                Some("profileCompilation"),
            ),
            "profile_apply" => {
                let mut fields =
                    selected(&["id", "expectedRevision", "expectedActiveOwner", "gameId"]);
                fields["activationId"] = args["operationId"].clone();
                self.native("profile_apply", fields, client, Some("profiles"))
            }
            "activation_status" => forward(
                "profile_activation_status",
                &["activationId"],
                Some("profileActivation"),
            ),
            "activation_current" => {
                forward("profile_activation_current", &[], Some("profileActivation"))
            }
            "activation_cancel" => forward(
                "profile_activation_cancel",
                &["activationId"],
                Some("profileActivation"),
            ),
            "profile_stop" | "profile_recover" => self.native(
                name,
                json!({"activationId":args["operationId"]}),
                client,
                Some("profiles"),
            ),
            "applied_ports_inspect" => forward(
                "profile_runtime_inspect",
                &["owner", "nodeId", "offset", "limit"],
                Some("profileInspection"),
            ),
            "applied_topology_inspect" => forward(
                "profile_runtime_topology",
                &["owner", "section", "offset", "limit"],
                Some("profileTopology"),
            ),
            "system_inventory" => forward(
                "system_inventory",
                &["offset", "limit", "observedAt"],
                Some("systemInventory"),
            ),
            "gateways_list" => {
                let mut state = forward("ports", &[], None)?;
                if state["ports"].is_array() {
                    page_array(&mut state, "ports", args)?;
                }
                Ok(state)
            }
            "gateway_inspect" => forward("status", &["gatewayId"], None),
            "gateway_session" => self.guarded(
                text(args, "action")?,
                selected(&["gatewayId"]),
                client,
                &args["expectedActiveOwner"],
                None,
            ),
            "gateway_config_read" => forward("config_read", &["gatewayId", "moduleId"], None),
            "gateway_config_change" => {
                let command = match text(args, "action")? {
                    "claim" => "claim",
                    "release" => "release",
                    "apply" => "config_apply",
                    "apply_save" => "config_apply_save",
                    "save" => "config_save",
                    "discard" => "config_discard",
                    "reset" => "config_reset",
                    _ => unreachable!(),
                };
                if matches!(command, "config_apply" | "config_apply_save")
                    && !args["config"].is_object()
                {
                    return Err(failure(
                        "invalid_arguments",
                        "Apply requires the complete typed module config from gateway_config_read.",
                        false,
                    ));
                }
                if !matches!(command, "config_apply" | "config_apply_save")
                    && args.get("config").is_some()
                {
                    return Err(failure(
                        "invalid_arguments",
                        "Only apply or apply_save accepts a module config.",
                        false,
                    ));
                }
                self.guarded(
                    command,
                    selected(&["gatewayId", "moduleId", "config"]),
                    client,
                    &args["expectedActiveOwner"],
                    None,
                )
            }
            "usb_modules_list" => {
                forward("module_usb_list", &["offset", "limit"], Some("moduleUsb"))
            }
            "usb_module_inspect" => forward("module_usb_status", &["key"], Some("moduleUsb")),
            "usb_module_session" => {
                let command = if args["action"] == "connect" {
                    "module_usb_connect"
                } else {
                    "module_usb_disconnect"
                };
                if command == "module_usb_connect" && !args["port"].is_string() {
                    return Err(failure(
                        "invalid_arguments",
                        "Connecting requires the currently discovered USB port.",
                        false,
                    ));
                }
                if command == "module_usb_disconnect" && args.get("port").is_some() {
                    return Err(failure(
                        "invalid_arguments",
                        "Disconnect accepts the exact module key, without a port selector.",
                        false,
                    ));
                }
                self.guarded(
                    command,
                    selected(&["key", "port"]),
                    client,
                    &args["expectedActiveOwner"],
                    Some("moduleUsb"),
                )
            }
            "usb_module_config_read" => forward(
                "module_usb_config_read",
                &["key", "route"],
                Some("moduleUsb"),
            ),
            "device_config_prepare" => self.device_prepare(args, client),
            "usb_module_config_change" => {
                let command = match text(args, "action")? {
                    "claim" => "module_usb_claim",
                    "release" => "module_usb_release",
                    "apply" | "apply_save" => "module_usb_config_apply",
                    "save" => "module_usb_config_save",
                    "discard" => "module_usb_config_discard",
                    "reset" => "module_usb_config_reset",
                    _ => unreachable!(),
                };
                let mut fields = selected(&["key", "route", "config"]);
                if command == "module_usb_config_apply" {
                    if !args["config"].is_object() {
                        return Err(failure(
                            "invalid_arguments",
                            "Apply requires the complete typed module configuration.",
                            false,
                        ));
                    }
                    fields["save"] = json!(args["action"] == "apply_save");
                } else if args.get("config").is_some() {
                    return Err(failure(
                        "invalid_arguments",
                        "Only apply or apply_save accepts a module config.",
                        false,
                    ));
                }
                self.guarded(
                    command,
                    fields,
                    client,
                    &args["expectedActiveOwner"],
                    Some("moduleUsb"),
                )
            }
            "local_settings_get" => forward(
                "profile_local_settings_get",
                &["scope", "gameId"],
                Some("localSettings"),
            ),
            "local_settings_save" => forward(
                "profile_local_settings_save",
                &[
                    "scope",
                    "gameId",
                    "expectedSettingsRevision",
                    "expectedActiveOwner",
                    "settings",
                ],
                Some("localSettings"),
            ),
            _ => Err(failure("unknown_tool", "Unknown Cockpit task tool.", false)),
        }
    }
    fn search_ports(&self, args: &Value, client: &str) -> Result<Value, Value> {
        let search = text(args, "search")?.trim().to_lowercase();
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(32) as usize;
        // Native resource descriptors are bounded to 262144 ports. Scan one
        // exact resource only, avoiding whole-catalogue cloning for AI search.
        let mut items = vec![];
        let mut matched = 0;
        let mut native_offset = 0;
        let mut revision = Value::Null;
        let mut resource = Value::Null;
        loop {
            let page = self.native(
                "profile_resources_inspect",
                json!({"resourceId":args["resourceId"],"offset":native_offset,"limit":128}),
                client,
                Some("profileResources"),
            )?;
            let stamp = json!([page["session"], page["revision"], page["resource"]]);
            if revision.is_null() {
                revision = stamp;
                resource = page["resource"].clone();
            } else if revision != stamp {
                return Err(failure(
                    "resource_changed",
                    "Resource capabilities changed while searching. Repeat the query.",
                    true,
                ));
            }
            let ports = page["ports"].as_array().ok_or_else(|| {
                failure(
                    "invalid_native_result",
                    "The native resource has no port page.",
                    false,
                )
            })?;
            for port in ports {
                if port.to_string().to_lowercase().contains(&search) {
                    if matched >= offset && items.len() < limit {
                        items.push(port.clone());
                    }
                    matched += 1;
                }
            }
            native_offset += ports.len();
            let total = page["total"]
                .as_u64()
                .or_else(|| page["totalPorts"].as_u64())
                .unwrap_or(native_offset as u64) as usize;
            if ports.is_empty() || native_offset >= total {
                break;
            }
            if native_offset >= 262_144 {
                return Err(failure(
                    "capacity",
                    "Resource search exceeds the native port budget.",
                    false,
                ));
            }
        }
        let total = matched;
        let next = offset + items.len();
        Ok(
            json!({"resource":resource,"search":search,"offset":offset,"total":total,"ports":items,"nextOffset":if next<total{Some(next)}else{None},"capabilityStamp":revision}),
        )
    }
    fn companions(&self, args: &Value, client: &str) -> Result<Value, Value> {
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(32) as usize;
        let mut native_offset = 0;
        let mut total = 0;
        let mut items = vec![];
        let mut stamp = Value::Null;
        loop {
            let page = self.native(
                "profile_resources_list",
                json!({"offset":native_offset,"limit":128}),
                client,
                Some("profileResources"),
            )?;
            let observed = json!([page["session"], page["revision"]]);
            if stamp.is_null() {
                stamp = observed;
            } else if stamp != observed {
                return Err(failure(
                    "resource_changed",
                    "The native resource inventory changed while listing companions.",
                    true,
                ));
            }
            let resources = page["resources"].as_array().ok_or_else(|| {
                failure(
                    "invalid_native_result",
                    "Native inventory has no resource page.",
                    false,
                )
            })?;
            for resource in resources {
                if resource["resourceType"] == "companion" {
                    if total >= offset && items.len() < limit {
                        items.push(resource.clone());
                    }
                    total += 1;
                }
            }
            native_offset += resources.len();
            if resources.is_empty() || native_offset >= page["total"].as_u64().unwrap_or(0) as usize
            {
                break;
            }
            if native_offset >= 262_144 {
                return Err(failure(
                    "capacity",
                    "Native resource inventory exceeds its bounded scan.",
                    false,
                ));
            }
        }
        let next = offset + items.len();
        Ok(
            json!({"companions":items,"total":total,"offset":offset,"nextOffset":if next<total{Some(next)}else{None},"inventoryStamp":stamp}),
        )
    }
    fn game_vehicles(&self, args: &Value, client: &str) -> Result<Value, Value> {
        let games = self.native("game_get_state", json!({}), client, Some("games"))?;
        let id = text(args, "gameId")?;
        let game = games["catalogue"]
            .as_array()
            .and_then(|games| games.iter().find(|game| game["id"] == id))
            .ok_or_else(|| {
                failure(
                    "not_found",
                    "Choose a supported native game ID from games_list.",
                    false,
                )
            })?;
        let search = args["search"].as_str().unwrap_or("").trim().to_lowercase();
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(32) as usize;
        let vehicles = game["vehicleSupport"]["vehicles"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let filtered = vehicles
            .iter()
            .filter(|vehicle| {
                search.is_empty() || vehicle.to_string().to_lowercase().contains(&search)
            })
            .collect::<Vec<_>>();
        let total = filtered.len();
        let items = filtered
            .into_iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next = offset + items.len();
        Ok(
            json!({"game":select(game,&["id","name","installed","provider"]),"support":select(&game["vehicleSupport"],&["supported","kind","message"]),"setup":game["integrationSetup"],"currentVehicle":game["currentVehicle"],"vehicleStatus":game["vehicleStatus"],"catalogueStamp":digest(&json!([game["id"],game["installed"],game["executable"],game["integrationSetup"],game["vehicleSupport"]])),"offset":offset,"total":total,"vehicles":items,"nextOffset":if next<total{Some(next)}else{None}}),
        )
    }
}

fn select(value: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .filter_map(|key| {
                value
                    .get(*key)
                    .map(|value| ((*key).to_owned(), value.clone()))
            })
            .collect(),
    )
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, Value> {
    value[key].as_str().ok_or_else(|| {
        failure(
            "invalid_arguments",
            &format!("{key} must be a string."),
            false,
        )
    })
}
fn digest(value: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("JSON value"))
    )
}
pub(crate) fn native_client(client: &str) -> String {
    format!("cockpit-mcp-{}", &digest(&json!(client))[..48])
}
fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
pub fn failure(code: &str, message: &str, retryable: bool) -> Value {
    json!({"ok":false,"error":{"code":code,"message":message.chars().take(2048).collect::<String>(),"retryable":retryable,"guidance":match code {
        "revision_conflict"=>"Reload the saved profile/settings and prepare the changes again.",
        "ownership_conflict"=>"Inspect profile_status and current device ownership; do not reuse an expired owner or route.",
        "resource_changed"=>"Refresh the resource descriptor and compile the current draft again.",
        _=>"Inspect the error and current native state before changing the request.",
    }}})
}
fn native_result(response: Value, section: Option<&str>) -> Result<Value, Value> {
    if response["ok"] != true {
        let message = response["error"]
            .as_str()
            .unwrap_or("Native operation failed.");
        let lower = message.to_lowercase();
        let code = if lower.contains("authorization") {
            "access_denied"
        } else if lower.contains("companion panel references") {
            "resource_changed"
        } else if lower.contains("revision") || lower.contains("changed") {
            "revision_conflict"
        } else if lower.contains("owner") || lower.contains("owns") || lower.contains("claimed") {
            "ownership_conflict"
        } else if lower.contains("expired") || lower.contains("no longer") {
            "expired"
        } else if lower.contains("read-only") {
            "read_only"
        } else if lower.contains("cancel") {
            "cancelled"
        } else if lower.contains("not found") {
            "not_found"
        } else {
            "native_rejected"
        };
        return Err(failure(code, message, false));
    }
    let state = response.get("state").unwrap_or(&response);
    Ok(section
        .and_then(|section| state.get(section))
        .unwrap_or(state)
        .clone())
}
fn page_array(data: &mut Value, key: &str, args: &Value) -> Result<(), Value> {
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let limit = args["limit"].as_u64().unwrap_or(32) as usize;
    let items = data[key].as_array().ok_or_else(|| {
        failure(
            "invalid_native_result",
            "The native result has no requested collection.",
            false,
        )
    })?;
    let total = items.len();
    let page = items
        .iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    let next = offset + page.len();
    data[key] = json!(page);
    data["offset"] = json!(offset);
    data["total"] = json!(total);
    data["nextOffset"] = json!(if next < total { Some(next) } else { None });
    Ok(())
}
fn next_offset(data: &mut Value, key: &str) {
    let offset = data["offset"].as_u64().unwrap_or(0);
    let count = data[key].as_array().map_or(0, |items| items.len() as u64);
    let next = offset + count;
    data["nextOffset"] = json!(if count > 0 && next < data["total"].as_u64().unwrap_or(0) {
        Some(next)
    } else {
        None
    });
}
fn compact_operation_result(result: &Value, id: &str) -> Value {
    if result.to_string().len() <= MAX_OPERATION_REPLY {
        return result.clone();
    }
    if result["ok"] != true {
        return result.clone();
    }
    let data = &result["data"];
    json!({"ok":true,"operationId":id,"data":{"state":"completed","document":select(&data["document"],&["id","name","revision"]),"session":data["session"],"profileSession":data["profileSession"],"saveApplication":data["saveApplication"],"activationId":data["activationId"],"revision":data["revision"],"reconciliation":"Operation completed. Read profile_get/profile_status or the device/settings inspection tool for the full current state."}})
}

#[cfg(test)]
#[path = "mcp_tools/tests.rs"]
mod tests;

#[cfg(test)]
#[path="mcp_tools/workspace_tests.rs"]
mod workspace_tests;
#[cfg(test)]
#[path="mcp_tools/ui_preferences_tests.rs"]
mod ui_preferences_tests;
