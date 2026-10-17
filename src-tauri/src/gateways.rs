//! Fleet routing. Discovery never opens endpoints; each gateway owns its CDC worker.
use crate::{
    pico::PicoService,
    stm32::Stm32Service,
    transport::{Factory, NativeFactory, PortInfo},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
};

/// Complete profile graphs have a separate budget from 64 KiB gateway commands.
pub const PROFILE_REQUEST_LIMIT: usize = osg_profiles::MAX_PROFILE_BYTES + 65_536;

trait Adapter: Send + Sync {
    fn request(&self, request: Value) -> Value;
    fn cached_state(&self) -> Value { empty_state() }
    fn configuration_claims(&self) -> Vec<crate::module_configuration_routes::WriterPin> { vec![] }
    fn shutdown(&self);
}
impl Adapter for PicoService {
    fn configuration_claims(&self) -> Vec<crate::module_configuration_routes::WriterPin> { self.configuration_claims() }
    fn cached_state(&self) -> Value { self.cached_state() }
    fn request(&self, request: Value) -> Value {
        self.request(request)
    }
    fn shutdown(&self) {
        self.shutdown();
    }
}
impl Adapter for Stm32Service {
    fn configuration_claims(&self) -> Vec<crate::module_configuration_routes::WriterPin> { self.configuration_claims() }
    fn cached_state(&self) -> Value { self.cached_state() }
    fn request(&self, request: Value) -> Value {
        self.request(request)
    }
    fn shutdown(&self) {
        self.shutdown();
    }
}
trait Provider: Send + Sync {
    fn ports(&self) -> Result<Vec<PortInfo>, String>;
    fn adapter(&self, backend: &str) -> Arc<dyn Adapter>;
}
struct NativeProvider;
impl Provider for NativeProvider {
    fn ports(&self) -> Result<Vec<PortInfo>, String> {
        NativeFactory.ports()
    }
    fn adapter(&self, backend: &str) -> Arc<dyn Adapter> {
        match backend {
            "pico" => Arc::new(PicoService::new()),
            _ => Arc::new(Stm32Service::new()),
        }
    }
}

pub(crate) fn canonical_id(backend: &str, serial: &str) -> Option<String> {
    match backend {
        "pico"
            if serial
                .strip_prefix("OSG-PICO-")
                .is_some_and(|s| crate::protocol::identifier(s, 4, 64)) =>
        {
            Some(format!("pico:{serial}"))
        }
        "stm32"
            if serial.len() == 24
                && serial.bytes().all(|b| b.is_ascii_hexdigit())
                && serial.bytes().any(|b| b != b'0')
                && serial.bytes().any(|b| b != b'f' && b != b'F') =>
        {
            Some(format!("stm32:{}", serial.to_ascii_lowercase()))
        }
        _ => None,
    }
}
fn candidate(port: &PortInfo) -> Option<(&'static str, String)> {
    if !crate::transport::allowed_path(&port.path) {
        return None;
    }
    let serial = port.serial_number.as_deref()?;
    let hex = |s: &Option<String>, expected: &str| {
        s.as_deref()
            .is_some_and(|v| v.trim_start_matches("0x").eq_ignore_ascii_case(expected))
    };
    if hex(&port.vendor_id, "cafe") && hex(&port.product_id, "4010") {
        if let Some(id) = canonical_id("pico", serial) {
            return Some(("pico", id));
        }
    }
    if crate::stm32::candidate(port) {
        return canonical_id("stm32", serial).map(|id| ("stm32", id));
    }
    None
}
fn empty_state() -> Value {
    json!({"connected":false,"running":false,"needsReconnect":false,"reconnecting":false,"modules":[],"port":null,"ports":[]})
}

/// One process can keep any number of independently addressed gateways connected.
/// The session map is held only while selecting an adapter, never during CDC I/O.
pub struct GatewayManager {
    profiles: Mutex<osg_profiles::service::Service>,
    profile_resources: crate::profile_resources::Runtime,
    module_usb: Arc<crate::module_usb_fleet::Fleet>,
    provider: Arc<dyn Provider>,
    sessions: Arc<Mutex<BTreeMap<String, Arc<dyn Adapter>>>>,
    inventory: Mutex<Vec<Value>>,
    configuration_gate: Mutex<()>,
    closed: AtomicBool,
    integration_gate: Arc<Mutex<()>>,
    dcs: Arc<crate::dcs_runtime::DcsRuntime>,
    integration: Arc<crate::integration_runtime::IntegrationRuntime>,
    games: Arc<crate::games::GamesRuntime>,
    game_admission: RwLock<()>,
    controllers: Arc<osg_controller_input::ControllerService>,
    mapping_host: Arc<osg_mapping_host::service::Service>,
    mapping_documents: osg_mapping::documents::Documents,
    companion_host: Arc<crate::companion_host::Runtime>,
    companion_feedback: Arc<crate::companion_host::feedback::Runtime>,
}
impl Default for GatewayManager {
    fn default() -> Self {
        Self::new()
    }
}
impl GatewayManager {
    fn configuration_claims(&self) -> Vec<(String, crate::module_configuration_routes::WriterPin)> {
        let sessions = self.sessions.lock().unwrap_or_else(|p|p.into_inner()).iter()
            .map(|(id,adapter)|(id.clone(),adapter.clone())).collect::<Vec<_>>();
        sessions.into_iter().flat_map(|(id,adapter)|adapter.configuration_claims().into_iter().map(move |pin|(id.clone(),pin))).collect()
    }
    fn cached_gateways(&self) -> Vec<(String, Value)> {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).iter()
            .map(|(id, adapter)| (id.clone(), adapter.clone())).collect::<Vec<_>>();
        sessions.into_iter().map(|(id, adapter)| (id, adapter.cached_state())).collect()
    }

    pub fn new() -> Self {
        Self::with_options(Arc::new(NativeProvider), true)
    }
    fn with_provider(provider: Arc<dyn Provider>) -> Self {
        Self::with_options(provider, false)
    }
    fn with_options(provider: Arc<dyn Provider>, native: bool) -> Self {
        let controllers = Arc::new(osg_controller_input::ControllerService::new(
            native.then(|| crate::dcs_runtime::config_path().with_file_name("controllers.json")),
            native,
        ));
        let root = native.then(|| crate::dcs_runtime::config_path().with_file_name("mappings"));
        let mapping_documents = osg_mapping::documents::Documents::new(root.clone());
        let mapping_host = Arc::new(osg_mapping_host::service::Service::new(
            controllers.clone(),
            root,
            Arc::new(osg_virtual_controller::inspection::current),
        ));
        let companion_clock = controllers.clone();
        let companion_provider = Arc::new(osg_mapping_host::companions::CompanionProvider::new(
            Arc::new(move || companion_clock.stream_page("", 0).captured_us),
        ));
        mapping_host.set_companion_provider(companion_provider.clone());
        let companion_host = Arc::new(crate::companion_host::Runtime::new(
            native.then(|| crate::dcs_runtime::config_path().with_file_name("companions")),
            companion_provider.clone(),
        ));
        if native { companion_host.start_detection(); }
        let companion_blueprints = companion_host.clone();
        mapping_host.set_companion_blueprint_provider(Arc::new(move |instance_id| {
            companion_blueprints.assigned_publication(instance_id)
        }));
        let sessions: Arc<Mutex<BTreeMap<String, Arc<dyn Adapter>>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        let targets = sessions.clone();
        let panel = Arc::new(move |request: Value| {
            let adapter = targets
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(request["gatewayId"].as_str().unwrap_or(""))
                .cloned();
            adapter.map_or_else(
                || json!({"ok":false,"error":"Connect the bound gateway first."}),
                |adapter| adapter.request(request),
            )
        });
        let integration_gate = Arc::new(Mutex::new(()));
        let dcs = Arc::new(crate::dcs_runtime::DcsRuntime::new(
            panel.clone(),
            crate::dcs_runtime::config_path(),
        ));
        let integration = Arc::new(crate::integration_runtime::IntegrationRuntime::new(
            panel.clone(),
            crate::dcs_runtime::config_path().with_file_name("integration.json"),
        ));
        let feedback_host = companion_host.clone();
        let feedback_source = integration.clone();
        let feedback_output = mapping_host.clone();
        let companion_feedback = Arc::new(crate::companion_host::feedback::Runtime::new(
            native.then(|| {
                crate::dcs_runtime::config_path()
                    .with_file_name("companions")
                    .join("feedback-routes.json")
            }),
            Arc::new(move || feedback_host.hub()),
            Arc::new(move || feedback_source.feedback_snapshot()),
            Arc::new(move |id| feedback_output.companion_submission(id)),
        ));
        let game_companions = companion_host.clone();
        let game_feedback = companion_feedback.clone();
        let game_dcs = dcs.clone();
        let game_integration = integration.clone();
        let game_gate = integration_gate.clone();
        let game_mapping = mapping_host.clone();
        let game_panel = Arc::new(move |request: Value| {
            let command = request["command"].as_str().unwrap_or("");
            if command.starts_with("_game_mapping_") {
                let journal = &request["mapping"];
                let result = match command {
                    "_game_mapping_snapshot" => game_mapping.game_snapshot(&request["capture"]),
                    "_game_mapping_take" => game_mapping.game_take(journal).map(|_| json!({})),
                    "_game_mapping_arm" => game_mapping.game_arm(journal).map(|_| json!({})),
                    "_game_mapping_stop" => game_mapping.game_stop(journal).map(|_| json!({})),
                    "_game_mapping_abandon" => {
                        game_mapping.game_abandon(journal);
                        Ok(json!({}))
                    }
                    "_game_mapping_resume" => game_mapping
                        .game_resume(journal, request["recovery"] == true)
                        .map(|_| json!({})),
                    _ => Err("Unknown game mapping operation.".into()),
                };
                match result {
                    Ok(state) => json!({"ok":true,"state":state}),
                    Err(error) => json!({"ok":false,"error":error}),
                }
            } else if command.starts_with("_game_companion_") {
                let result = (|| {
                    if command == "_game_companion_snapshot" || command == "_game_companion_take" {
                        let desired = if command == "_game_companion_snapshot" {
                            &request["capture"]
                        } else {
                            &request["companions"]["desired"]
                        };
                        game_feedback.preflight_assignments(desired)?;
                        let capture = crate::game_mapping::companions::Capture::parse(desired)?;
                        for a in capture.assignments {
                            game_mapping.companion_assignment_available(&a.instance_id)?;
                        }
                    }
                    game_companions.game_command(command, &request)
                })();
                match result {
                    Ok(state) => json!({"ok":true,"state":state}),
                    Err(error) => json!({"ok":false,"error":error}),
                }
            } else if command.starts_with("integration_") || command.starts_with("dcs_") {
                let _guard = game_gate.lock().unwrap_or_else(|p| p.into_inner());
                let result = if command.starts_with("integration_") {
                    if command == "integration_start" && game_dcs.status()["running"] == true {
                        Err("Stop DCS-BIOS before starting another integration.".into())
                    } else {
                        (|| {
                            if command == "integration_preflight" {
                                let config = crate::integration_profile::validate(
                                    &request["integrationConfig"],
                                )?;
                                game_feedback.preflight_integration(
                                    &request["gameCompanions"],
                                    &crate::integration_runtime::feedback_revision(&config),
                                )?;
                            }
                            game_integration.request(&request)
                        })()
                    }
                } else if command == "dcs_start" && game_integration.running() {
                    Err("Stop the active integration before starting DCS-BIOS.".into())
                } else {
                    game_dcs.request(&request)
                };
                let mut response = json!({"ok":result.is_ok(),"state":{
                    "integration":game_integration.status(),"dcsBios":game_dcs.status()}});
                if let Err(error) = result {
                    response["error"] = json!(error);
                }
                response
            } else {
                panel(request)
            }
        });
        let games = Arc::new(crate::games::GamesRuntime::new(
            game_panel,
            std::env::var_os("OSG_GAMES_CONFIG")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| crate::dcs_runtime::config_path().with_file_name("games.json")),
            Arc::new(crate::game_discovery::NativePlatform),
        ));
        let resource_sessions = sessions.clone();
        let resource_requests = sessions.clone();
        let resource_ports = provider.clone();
        let module_usb = Arc::new(crate::module_usb_fleet::Fleet::new(Arc::new(move || resource_ports.ports()), native));
        let collect = crate::profile_resource_sources::collector(
            controllers.clone(), companion_provider, companion_host.clone(), games.clone(), integration.clone(),
            Arc::new(move || resource_sessions.lock().unwrap_or_else(|p| p.into_inner()).iter().map(|(id, adapter)| (id.clone(), adapter.cached_state())).collect()),
            Arc::new(move |request| {
                let adapter = resource_requests.lock().unwrap_or_else(|p| p.into_inner()).get(request["gatewayId"].as_str().unwrap_or("")).cloned();
                adapter.map_or_else(|| json!({"ok":false,"error":"The inventory gateway is disconnected."}), |adapter| adapter.request(request))
            }),
            module_usb.clone(), native,
        );
        let profile_resources = crate::profile_resources::Runtime::new(native.then(|| crate::dcs_runtime::config_path().with_file_name("profile-resources-v1")), collect, native);
        let manager = Self {
            module_usb,
            profile_resources,
            profiles: Mutex::new(osg_profiles::service::Service::new(native.then(|| {
                crate::dcs_runtime::config_path().with_file_name("profiles-v1")
            }))),
            provider,
            sessions,
            inventory: Mutex::new(Vec::new()),
            configuration_gate: Mutex::new(()),
            closed: AtomicBool::new(false),
            integration_gate,
            dcs,
            integration,
            games,
            game_admission: RwLock::new(()),
            controllers,
            mapping_host,
            mapping_documents,
            companion_host,
            companion_feedback,
        };
        manager.attach_panel_capture();
        manager
    }
    fn attach_panel_capture(&self) {
        let sessions = self.sessions.clone();
        let inventory = sessions.clone();
        self.mapping_host
            .set_panel_provider(Arc::new(crate::mapping_panels::Provider {
                panel: Arc::new(move |request: Value| {
                    let adapter = sessions
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .get(request["gatewayId"].as_str().unwrap_or(""))
                        .cloned();
                    adapter.map_or_else(
                        || json!({"ok":false,"error":"Connect the assigned gateway first."}),
                        |adapter| adapter.request(request),
                    )
                }),
                gateways: Arc::new(move || {
                    inventory
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .keys()
                        .cloned()
                        .collect()
                }),
                clock: self.controllers.clone(),
            }));
    }
    fn discover(&self) -> Result<Vec<Value>, String> {
        let mut ports = BTreeMap::<String, Value>::new();
        for port in self.provider.ports()? {
            let Some((backend, id)) = candidate(&port) else {
                continue;
            };
            let mut value = port.value();
            value["backend"] = json!(backend);
            value["gatewayId"] = json!(id);
            // macOS call-in/call-out aliases refer to the same interface.
            let path = port
                .path
                .replace("/dev/tty.usbmodem", "/dev/usbmodem")
                .replace("/dev/cu.usbmodem", "/dev/usbmodem");
            if !ports.contains_key(&path) || port.path.starts_with("/dev/cu.") {
                ports.insert(path, value);
            }
        }
        let ports: Vec<_> = ports.into_values().collect();
        *self.inventory.lock().unwrap_or_else(|p| p.into_inner()) = ports.clone();
        Ok(ports)
    }
    fn decorate(&self, mut response: Value, id: Option<&str>) -> Value {
        if !response["state"].is_object() {
            response["state"] = empty_state();
        }
        response["state"]["ports"] = json!(self
            .inventory
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone());
        if let Some(id) = id {
            response["state"]["gatewayId"] = json!(id);
            response["state"]["backend"] = json!(id.split_once(':').unwrap().0);
        }
        response["state"]["dcsBios"] = self.dcs.status();
        response["state"]["integration"] = self.integration.status();
        response
    }
    fn error(&self, message: &str, id: Option<&str>) -> Value {
        self.decorate(json!({"ok":false,"error":message}), id)
    }
    pub fn request(&self, request: Value) -> Value {
        // Starting a game waits for manual requests admitted before it. Normal
        // requests share this lock, preserving independent gateway workers.
        // Coordinator callbacks address adapters directly after admission.
        if request["command"]
            .as_str()
            .is_some_and(|c| c.starts_with("game_") && c != "game_get_state")
        {
            let _admission = self
                .game_admission
                .write()
                .unwrap_or_else(|p| p.into_inner());
            self.request_inner(request)
        } else {
            let _admission = self
                .game_admission
                .read()
                .unwrap_or_else(|p| p.into_inner());
            self.request_inner(request)
        }
    }
    fn request_inner(&self, mut request: Value) -> Value {
        if self.closed.load(Ordering::Acquire) {
            return self.error("Cockpit gateway manager has closed.", None);
        }
        let profile_request = request["command"]
            .as_str()
            .is_some_and(|command| command.starts_with("profile_"));
        let request_limit = if profile_request {
            PROFILE_REQUEST_LIMIT
        } else {
            65_536
        };
        if !request.is_object() || request.to_string().len() > request_limit {
            return self.error("Invalid gateway request.", None);
        }
        if !request["clientId"]
            .as_str()
            .is_some_and(|s| crate::protocol::identifier(s, 16, 80))
        {
            return self.error(
                "Missing Cockpit client identity; reload the application.",
                None,
            );
        }
        let Some(command) = request["command"].as_str().map(str::to_owned) else {
            return self.error("Missing gateway command.", None);
        };
        // Hold the ownership check and its actual worker dispatch together.
        // Numeric firmware owner IDs may match on USB/CAN for one UI client;
        // that does not authorize transferring its existing draft or route.
        let _configuration = matches!(command.as_str(),
            "claim" | "release" | "connect" | "disconnect" | "config_apply" | "config_apply_save" | "config_save" | "config_discard" | "config_reset"
            | "module_usb_claim" | "module_usb_release" | "module_usb_connect" | "module_usb_disconnect" | "module_usb_config_apply" | "module_usb_config_save" | "module_usb_config_discard" | "module_usb_config_reset")
            .then(|| self.configuration_gate.lock().unwrap_or_else(|p|p.into_inner()));
        if self.closed.load(Ordering::Acquire) {
            return self.error("Cockpit gateway manager has closed.", None);
        }
        // Profile documents are independent of gateway inventory and legacy game
        // ownership. Their service never starts an output or restores old stores.
        if command.starts_with("module_usb_") {
            if matches!(command.as_str(), "module_usb_claim" | "module_usb_config_apply" | "module_usb_config_save" | "module_usb_config_reset" | "module_usb_actions_invoke") {
                let usb = self.module_usb.snapshots();
                if let Some(module) = usb.iter().find(|slot| request["key"] == slot.key).and_then(|slot| slot.snapshot.identity.as_ref()) {
                    if let Err(error) = crate::module_configuration_routes::evaluate(&usb, &self.cached_gateways(), &module.module_id) {
                        return json!({"ok":false,"error":error});
                    }
                    if command == "module_usb_claim" {
                        if let Err(error) = crate::module_configuration_routes::guard_usb_writer(&module.module_id, &self.configuration_claims()) {
                            return json!({"ok":false,"error":error});
                        }
                    }
                }
            }
            return match self.module_usb.request(&request) {
                Ok(module_usb) => json!({"ok":true,"state":{"moduleUsb":module_usb}}),
                Err(error) => json!({"ok":false,"error":error.chars().take(256).collect::<String>()}),
            };
        }
        if command.starts_with("profile_resources_") {
            request.as_object_mut().unwrap().remove("clientId");
            return match self.profile_resources.request(&request) {
                Ok(resources) => json!({"ok":true,"state":{"profileResources":resources}}),
                Err(error) => json!({"ok":false,"error":error.chars().take(256).collect::<String>()}),
            };
        }
        if profile_request {
            request.as_object_mut().unwrap().remove("clientId");
            let result = self
                .profiles
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .request(&request);
            return match result {
                Ok(profiles) => json!({"ok":true,"state":{"profiles":profiles}}),
                Err(error) => json!({"ok":false,"error":error.chars().take(256).collect::<String>()}),
            };
        }
        if command.starts_with("companion_") {
            if let Err(error) = self.games.guard_request(&request) {
                return json!({"ok":false,"error":error});
            }
            let result = if command == "companion_feedback_routes" {
                self.companion_feedback.request(&request["companion"])
            } else {
                self.companion_host.request(&request)
            };
            return match result {
                Ok(state) => json!({"ok":true,"state":{"companions":state}}),
                Err(error) => {
                    json!({"ok":false,"error":error.chars().take(256).collect::<String>()})
                }
            };
        }
        if command.starts_with("game_") {
            if matches!(command.as_str(), "game_activate" | "game_launch")
                && (self.mapping_host.provisioning_active() || self.controllers.visibility_active())
            {
                return self.error("Restore standalone visibility and finish virtual-controller provisioning before activating a captured game profile.", None);
            }
            let mut response = match self.games.request(&request) {
                Ok(()) => self.decorate(json!({"ok":true,"state":empty_state()}), None),
                Err(error) => self.error(&error, None),
            };
            response["state"]["games"] = self.games.status_with_telemetry(
                &response["state"]["integration"],
                &response["state"]["dcsBios"],
            );
            return response;
        }
        if let Err(error) = self.games.guard_request(&request) {
            if !matches!(
                command.as_str(),
                "mapping_provision_cancel"
                    | "controller_visibility_status"
                    | "controller_visibility_inspect"
                    | "controller_visibility_processes"
                    | "controller_visibility_restore"
            ) {
                return self.error(&error, None);
            }
        }
        if command.starts_with("mapping_") {
            // Explicit companion discovery restores saved sources on demand;
            // ordinary gateway/mapping startup never opens OS credentials.
            if command == "mapping_companion_sources" {
                if let Err(error) = self.companion_host.hub() {
                    return self.error(&error, None);
                }
            }
            // Document responses can use the full profile limit. Keep unrelated
            // gateway inventory out of this separately bounded response.
            let result = if osg_mapping_host::service::Service::handles(&command) {
                self.mapping_host.request(&request)
            } else {
                self.mapping_documents.request(&request)
            };
            return match result {
                Ok(state) => json!({"ok":true,"state":{"mappings":state}}),
                Err(error) => {
                    json!({"ok":false,"error":error.chars().take(256).collect::<String>()})
                }
            };
        }
        if command.starts_with("controller_") {
            return match self.controllers.request(&request) {
                Ok(state) => {
                    let mut response =
                        self.decorate(json!({"ok":true,"state":empty_state()}), None);
                    response["state"]["controllers"] = state;
                    response
                }
                Err(error) => self.error(&error, None),
            };
        }
        let _integration_guard =
            if command.starts_with("integration_") || command.starts_with("dcs_") {
                Some(
                    self.integration_gate
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()),
                )
            } else {
                None
            };
        if command.starts_with("integration_") {
            if command == "integration_start" && self.dcs.status()["running"] == true {
                return self.error("Stop DCS-BIOS before starting another integration.", None);
            }
            return match self.integration.request(&request) {
                Ok(()) => self.decorate(json!({"ok":true,"state":empty_state()}), None),
                Err(error) => self.error(&error, None),
            };
        }
        if command.starts_with("dcs_") {
            if command == "dcs_start" && self.integration.running() {
                return self.error(
                    "Stop the active integration before starting DCS-BIOS.",
                    None,
                );
            }
            return match self.dcs.request(&request) {
                Ok(()) => self.decorate(json!({"ok":true,"state":empty_state()}), None),
                Err(error) => self.error(&error, None),
            };
        }
        let backend = match request.get("backend") {
            Some(Value::String(s)) if s == "pico" || s == "stm32" => Some(s.clone()),
            Some(_) => return self.error("Unknown gateway backend.", None),
            None => None,
        };
        let mut id = match request.get("gatewayId") {
            Some(Value::String(s)) => match s
                .split_once(':')
                .and_then(|(b, serial)| canonical_id(b, serial))
            {
                Some(id) => Some(id),
                None => return self.error("Invalid gateway identity.", None),
            },
            Some(_) => return self.error("Invalid gateway identity.", None),
            None => None,
        };
        if id
            .as_ref()
            .zip(backend.as_ref())
            .is_some_and(|(id, b)| id.split_once(':').unwrap().0 != b)
        {
            return self.error("Gateway identity and backend do not match.", id.as_deref());
        }
        if command == "ports" {
            return match self.discover() {
                Ok(_) => self.decorate(json!({"ok":true,"state":empty_state()}), None),
                Err(e) => self.error(&e, None),
            };
        }
        if command == "connect" {
            let ports = match self.discover() {
                Ok(p) => p,
                Err(e) => return self.error(&e, id.as_deref()),
            };
            let selected: Vec<_> = ports
                .iter()
                .filter(|p| {
                    id.as_ref().is_none_or(|id| p["gatewayId"] == *id)
                        && backend.as_ref().is_none_or(|b| p["backend"] == *b)
                        && request.get("port").is_none_or(|path| p["path"] == *path)
                })
                .collect();
            if selected.len() != 1 {
                return self.error(
                    "Select one available gateway whose identity matches its port.",
                    id.as_deref(),
                );
            }
            id = selected[0]["gatewayId"].as_str().map(str::to_owned);
            request["port"] = selected[0]["path"].clone();
        }
        // Only the manager computes transport preference. A caller cannot use a
        // private dispatch flag to bypass a ready direct-USB route.
        request.as_object_mut().unwrap().remove("_directUsbPreferred");
        if command == "claim" {
            if let Some(module) = request["moduleId"].as_str() {
                if let Err(error) = crate::module_configuration_routes::guard_can_writer(module, &self.module_usb.configuration_claims()) {
                    return self.error(&error, id.as_deref());
                }
            }
        }
        if matches!(command.as_str(), "claim" | "config_apply" | "config_save" | "config_reset" | "action_invoke" | "action_claim" | "action_renew" | "action_invoke_owned") {
            if let Some(module) = request["moduleId"].as_str() {
                match crate::module_configuration_routes::evaluate(&self.module_usb.snapshots(), &self.cached_gateways(), module) {
                    Ok(preference) => if command == "claim" { request["_directUsbPreferred"] = json!(preference.direct_usb_preferred); },
                    Err(error) => return self.error(&error, id.as_deref()),
                }
            }
        }
        let (id, adapter) = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            if self.closed.load(Ordering::Acquire) {
                drop(sessions);
                return self.error("Cockpit gateway manager has closed.", id.as_deref());
            }
            let id = match id {
                Some(id) => id,
                None if sessions.len() == 1 => sessions.keys().next().unwrap().clone(),
                None if sessions.is_empty() && command == "status" => {
                    drop(sessions);
                    return match self.discover() {
                        Ok(_) => self.decorate(json!({"ok":true,"state":empty_state()}), None),
                        Err(e) => self.error(&e, None),
                    };
                }
                None => {
                    drop(sessions);
                    return self.error(
                        "Specify gatewayId when more than one gateway session exists.",
                        None,
                    );
                }
            };
            let gateway_backend = id.split_once(':').unwrap().0;
            if backend.as_ref().is_some_and(|b| b != gateway_backend) {
                drop(sessions);
                return self.error("Gateway identity and backend do not match.", Some(&id));
            }
            let adapter = if command == "connect" {
                sessions
                    .entry(id.clone())
                    .or_insert_with(|| self.provider.adapter(gateway_backend))
                    .clone()
            } else {
                let Some(adapter) = sessions.get(&id) else {
                    drop(sessions);
                    return self.error(
                        "Unknown gateway session; connect this gateway first.",
                        Some(&id),
                    );
                };
                adapter.clone()
            };
            (id, adapter)
        };
        request["gatewayId"] = json!(id);
        request.as_object_mut().unwrap().remove("backend");
        let bound_led = command == "action_invoke" && self.dcs.bound_led(&request);
        let mut response = adapter.request(if bound_led {
            json!({"command":"status","clientId":request["clientId"],"gatewayId":id})
        } else {
            request
        });
        if bound_led {
            response["ok"] = json!(false);
            response["error"] =
                json!("Stop DCS-BIOS before manually controlling its bound output.");
        }
        if response["state"]["connected"] == true {
            let backend = id.split_once(':').unwrap().0;
            if response["state"]["gatewaySerial"]
                .as_str()
                .and_then(|s| canonical_id(backend, s))
                .as_deref()
                != Some(&id)
            {
                adapter.shutdown();
                let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
                if sessions.get(&id).is_some_and(|a| Arc::ptr_eq(a, &adapter)) {
                    sessions.remove(&id);
                }
                drop(sessions);
                return self.error(
                    "Gateway handshake did not match the requested identity.",
                    Some(&id),
                );
            }
        }
        self.decorate(response, Some(&id))
    }
    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::Release);
        self.profile_resources.shutdown();
        self.module_usb.shutdown();
        self.companion_feedback.stop();
        self.companion_host.shutdown();
        self.games.shutdown();
        self.mapping_host.shutdown();
        self.controllers.shutdown();
        self.mapping_documents.clear();
        self.integration.stop(true);
        self.dcs.shutdown();
        let sessions =
            std::mem::take(&mut *self.sessions.lock().unwrap_or_else(|p| p.into_inner()));
        // Cancellation/release happens concurrently, bounded by each adapter's own deadlines.
        std::thread::scope(|scope| {
            for adapter in sessions.values() {
                scope.spawn(move || adapter.shutdown());
            }
        });
    }
}
impl Drop for GatewayManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, sync::mpsc, time::Duration};
    const CLIENT: &str = "cockpit-fleet-test-client";
    const FIRST: &str = "stm32:00112233445566778899aabb";
    const SECOND: &str = "stm32:112233445566778899aabbcc";
    const PICO: &str = "pico:OSG-PICO-0123456789ABCDEF";
    fn port(id: &str, path: &str) -> PortInfo {
        let (backend, serial) = id.split_once(':').unwrap();
        PortInfo {
            path: path.into(),
            serial_number: Some(serial.into()),
            vendor_id: Some(if backend == "pico" { "cafe" } else { "1209" }.into()),
            product_id: Some("4010".into()),
            manufacturer: Some("OpenSimGear".into()),
            friendly_name: Some("OpenSimGear Gateway".into()),
        }
    }
    #[derive(Default)]
    struct MockAdapter {
        state: Mutex<Value>,
        calls: Mutex<Vec<Value>>,
        closed: AtomicBool,
        entered: AtomicBool,
        blocked: AtomicBool,
        wrong_identity: AtomicBool,
        configuration_claims: Mutex<Vec<crate::module_configuration_routes::WriterPin>>,
    }
    impl Adapter for MockAdapter {
        fn cached_state(&self) -> Value { self.state.lock().unwrap().clone() }
        fn configuration_claims(&self) -> Vec<crate::module_configuration_routes::WriterPin> {
            if self.closed.load(Ordering::Acquire) { vec![] } else { self.configuration_claims.lock().unwrap().clone() }
        }
        fn request(&self, request: Value) -> Value {
            self.calls.lock().unwrap().push(request.clone());
            if request["command"] == "wait" {
                self.entered.store(true, Ordering::Release);
                while self.blocked.load(Ordering::Acquire) && !self.closed.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            let mut state = self.state.lock().unwrap();
            if request["command"] == "connect" {
                let id = request["gatewayId"].as_str().unwrap();
                let (backend, serial) = id.split_once(':').unwrap();
                *state = json!({"connected":true,"backend":backend,"gatewaySerial":serial,"modules":[],"claimedModules":[]});
                if self.wrong_identity.load(Ordering::Acquire) {
                    state["gatewaySerial"] = json!("abcdefabcdefabcdefabcdef");
                }
            }
            if request["command"] == "disconnect" {
                state["connected"] = json!(false);
                self.configuration_claims.lock().unwrap().clear();
            }
            if request["command"] == "claim" {
                state["claimedModules"] = json!([request["moduleId"]]);
                self.configuration_claims.lock().unwrap().push(crate::module_configuration_routes::WriterPin {
                    module_id:request["moduleId"].as_str().unwrap().into(),client_id:request["clientId"].as_str().unwrap().into(),boot:10,
                });
            }
            if request["command"] == "release" {
                self.configuration_claims.lock().unwrap().retain(|pin|request["moduleId"] != pin.module_id || request["clientId"] != pin.client_id);
                state["claimedModules"] = json!([]);
            }
            if request["command"] == "config_stage" {
                state["moduleConfigs"][request["moduleId"].as_str().unwrap()] =
                    request["config"].clone();
            }
            json!({"ok":request["command"] != "fail","state":state.clone()})
        }
        fn shutdown(&self) {
            self.closed.store(true, Ordering::Release);
        }
    }
    struct MockProvider {
        ports: Mutex<Vec<PortInfo>>,
        adapters: Mutex<Vec<Arc<MockAdapter>>>,
        wrong_identity: AtomicBool,
    }
    impl Provider for MockProvider {
        fn ports(&self) -> Result<Vec<PortInfo>, String> {
            Ok(self.ports.lock().unwrap().clone())
        }
        fn adapter(&self, _: &str) -> Arc<dyn Adapter> {
            let adapter = Arc::new(MockAdapter::default());
            adapter.wrong_identity.store(
                self.wrong_identity.load(Ordering::Acquire),
                Ordering::Release,
            );
            self.adapters.lock().unwrap().push(adapter.clone());
            adapter
        }
    }
    fn setup() -> (Arc<GatewayManager>, Arc<MockProvider>) {
        let provider = Arc::new(MockProvider {
            ports: Mutex::new(vec![
                port(FIRST, "COM1"),
                port(SECOND, "COM2"),
                port(PICO, "COM3"),
            ]),
            adapters: Mutex::new(vec![]),
            wrong_identity: AtomicBool::new(false),
        });
        (
            Arc::new(GatewayManager::with_provider(provider.clone())),
            provider,
        )
    }
    fn request(manager: &GatewayManager, command: &str, id: Option<&str>, fields: Value) -> Value {
        let mut request = json!({"command":command,"clientId":CLIENT});
        if let Some(id) = id {
            request["gatewayId"] = json!(id);
        }
        for (key, value) in fields.as_object().unwrap() {
            request[key] = value.clone();
        }
        manager.request(request)
    }
    #[test]
    fn unified_profile_documents_do_not_discover_gateways_or_activate_outputs() {
        let (manager, provider) = setup();
        let listed = request(&manager, "profile_list", None, json!({}));
        assert_eq!(listed["ok"], true);
        assert_eq!(listed["state"]["profiles"]["profiles"].as_array().unwrap().len(), 1);
        assert!(listed["state"].get("modules").is_none());
        let created = request(&manager, "profile_create", None, json!({"name":"Flight"}));
        assert_eq!(created["ok"], true);
        let document = &created["state"]["profiles"]["document"];
        let opened = request(&manager, "profile_get", None, json!({"id":document["id"]}));
        assert_eq!(opened["state"]["profiles"]["document"], *document);
        let status = request(&manager, "profile_status", None, json!({}));
        assert_eq!(status["state"]["profiles"]["session"]["selectedProfileId"], osg_profiles::DEFAULT_PROFILE_ID);
        assert_eq!(status["state"]["profiles"]["session"]["state"], "stopped");
        assert!(status["state"]["profiles"]["session"]["appliedSnapshot"].is_null());
        assert_eq!(request(&manager, "profile_apply", None, json!({"id":document["id"]}))["ok"], false);
        assert!(provider.adapters.lock().unwrap().is_empty());
    }
    #[test]
    fn profile_request_budget_does_not_expand_gateway_command_budget() {
        let (manager, _) = setup();
        let oversized_legacy = request(&manager, "status", None, json!({"payload":"x".repeat(70_000)}));
        assert_eq!(oversized_legacy["error"], "Invalid gateway request.");
        let profile = request(&manager, "profile_apply", None, json!({"payload":"x".repeat(70_000)}));
        assert_eq!(profile["ok"], false);
        assert_ne!(profile["error"], "Invalid gateway request.");
        let oversized_profile = request(&manager, "profile_import", None, json!({"payload":"x".repeat(PROFILE_REQUEST_LIMIT)}));
        assert_eq!(oversized_profile["error"], "Invalid gateway request.");
    }
    #[test]
    fn controller_status_is_independent_of_gateway_inventory_and_rejects_invalid_edits() {
        let (manager, provider) = setup();
        let response = request(&manager, "controller_list", None, json!({}));
        assert_eq!(response["ok"], true);
        assert_eq!(
            response["state"]["controllers"]["backendStatus"],
            "unavailable"
        );
        assert_eq!(response["state"]["controllers"]["total"], 0);
        assert!(provider.adapters.lock().unwrap().is_empty());
        let bad = request(
            &manager,
            "controller_inspect",
            None,
            json!({"sourceId":"missing"}),
        );
        assert_eq!(bad["ok"], false);
        assert!(bad["error"]
            .as_str()
            .unwrap()
            .contains("Unknown controller"));
        manager.shutdown();
        assert_eq!(
            request(&manager, "controller_list", None, json!({}))["ok"],
            false
        );
    }
    #[test]
    fn discovery_is_complete_deduplicated_and_never_opens_or_disconnects() {
        let (manager, provider) = setup();
        let mut aliases = provider.ports.lock().unwrap();
        aliases.push(port(FIRST, "/dev/tty.usbmodem123"));
        aliases.push(port(FIRST, "/dev/cu.usbmodem123"));
        let mut unknown = port(FIRST, "COM4");
        unknown.manufacturer = Some("Unrelated".into());
        aliases.push(unknown);
        drop(aliases);
        let result = request(&manager, "ports", None, json!({}));
        assert_eq!(result["state"]["ports"].as_array().unwrap().len(), 4);
        assert_eq!(result["state"]["connected"], false);
        assert!(provider.adapters.lock().unwrap().is_empty());
        assert_eq!(
            request(&manager, "connect", Some(FIRST), json!({"port":"COM1"}))["ok"],
            true
        );
        request(&manager, "ports", None, json!({}));
        assert_eq!(
            request(&manager, "status", Some(FIRST), json!({}))["state"]["connected"],
            true
        );
        assert_eq!(provider.adapters.lock().unwrap().len(), 1);
    }
    #[test]
    fn same_backend_and_cross_backend_sessions_keep_state_and_ownership_separate() {
        const SHARED_MODULE: &str = "00112233445566778899aabbccddeeff";
        let (manager, provider) = setup();
        for id in [FIRST, SECOND, PICO] {
            assert_eq!(
                request(&manager, "connect", Some(id), json!({}))["ok"],
                true
            );
        }
        for (index, id) in [FIRST, SECOND, PICO].iter().enumerate() {
            request(
                &manager,
                "claim",
                Some(id),
                json!({"moduleId":SHARED_MODULE}),
            );
            request(
                &manager,
                "config_stage",
                Some(id),
                json!({"moduleId":SHARED_MODULE,"config":{"name":format!("Gateway {index}")}}),
            );
        }
        for (index, id) in [FIRST, SECOND, PICO].iter().enumerate() {
            let state = request(&manager, "status", Some(id), json!({}))["state"].clone();
            assert_eq!(state["connected"], true);
            assert_eq!(state["gatewayId"], *id);
            assert_eq!(
                state["moduleConfigs"][SHARED_MODULE]["name"],
                format!("Gateway {index}")
            );
            assert_eq!(state["claimedModules"], json!([SHARED_MODULE]));
            assert_eq!(state["ports"].as_array().unwrap().len(), 3);
        }
        request(&manager, "disconnect", Some(SECOND), json!({}));
        assert_eq!(
            request(&manager, "status", Some(FIRST), json!({}))["state"]["connected"],
            true
        );
        assert_eq!(
            request(&manager, "status", Some(PICO), json!({}))["state"]["connected"],
            true
        );
        manager.shutdown();
        assert!(provider
            .adapters
            .lock()
            .unwrap()
            .iter()
            .all(|a| a.closed.load(Ordering::Acquire)));
        assert_eq!(
            request(&manager, "status", Some(FIRST), json!({}))["ok"],
            false
        );
    }
    #[test]
    fn ambiguous_unknown_and_mismatched_targets_never_reach_an_adapter() {
        let (manager, provider) = setup();
        assert_eq!(
            request(&manager, "connect", Some(FIRST), json!({"port":"COM2"}))["ok"],
            false
        );
        assert_eq!(
            request(&manager, "connect", Some(FIRST), json!({"backend":"pico"}))["ok"],
            false
        );
        assert_eq!(
            request(&manager, "status", Some(FIRST), json!({}))["ok"],
            false
        );
        assert!(provider.adapters.lock().unwrap().is_empty());
        request(&manager, "connect", None, json!({"port":"COM1"}));
        assert_eq!(
            request(&manager, "status", None, json!({}))["state"]["gatewayId"],
            FIRST
        );
        request(&manager, "connect", Some(SECOND), json!({}));
        let count: usize = provider
            .adapters
            .lock()
            .unwrap()
            .iter()
            .map(|a| a.calls.lock().unwrap().len())
            .sum();
        for command in ["status", "disconnect", "claim", "config_stage"] {
            assert_eq!(request(&manager, command, None, json!({}))["ok"], false);
        }
        assert_eq!(
            provider
                .adapters
                .lock()
                .unwrap()
                .iter()
                .map(|a| a.calls.lock().unwrap().len())
                .sum::<usize>(),
            count
        );
    }
    fn configuration_setup() -> (Arc<GatewayManager>, Arc<MockProvider>, Value) {
        let (mut manager,provider) = setup();
        Arc::get_mut(&mut manager).unwrap().module_usb = Arc::new(crate::module_usb_fleet::tests::manager_fixture());
        assert_eq!(request(&manager,"connect",Some(FIRST),json!({}))["ok"],true);
        let module = manager.module_usb.snapshots()[0].value();
        provider.adapters.lock().unwrap()[0].state.lock().unwrap()["modules"] = json!([{"id":module["snapshot"]["identity"]["moduleId"],"online":true,"boot":10}]);
        (manager,provider,module)
    }
    #[test]
    fn configuration_writers_require_explicit_release_before_either_transport_transfer() {
        let (manager,provider,usb) = configuration_setup();
        let module = usb["snapshot"]["identity"]["moduleId"].clone();
        let fields = json!({"key":usb["key"],"route":usb["snapshot"]["route"]});
        assert_eq!(request(&manager,"claim",Some(FIRST),json!({"moduleId":module}))["ok"],true);
        // The private adapter pin is authoritative even when a public response
        // intentionally omits caller-specific claimedModules.
        provider.adapters.lock().unwrap()[0].state.lock().unwrap()["claimedModules"] = json!([]);
        let blocked = request(&manager,"module_usb_claim",None,fields.clone());
        assert_eq!(blocked["ok"],false,"{blocked}");
        assert!(blocked["error"].as_str().unwrap().contains("release the current CAN"));
        assert!(manager.module_usb.snapshots()[0].snapshot.claimed_by.is_none());
        assert_eq!(request(&manager,"claim",Some(FIRST),json!({"moduleId":module}))["ok"],true,"current CAN owner may renew");
        assert_eq!(request(&manager,"release",Some(FIRST),json!({"moduleId":module}))["ok"],true);
        assert_eq!(request(&manager,"module_usb_claim",None,fields.clone())["ok"],true);
        assert_eq!(manager.module_usb.configuration_claims().len(),1);
        let before = provider.adapters.lock().unwrap()[0].calls.lock().unwrap().len();
        let blocked = request(&manager,"claim",Some(FIRST),json!({"moduleId":module,"_directUsbPreferred":false}));
        assert_eq!(blocked["ok"],false,"{blocked}");
        assert!(blocked["error"].as_str().unwrap().contains("release the current Direct USB"));
        assert_eq!(provider.adapters.lock().unwrap()[0].calls.lock().unwrap().len(),before,"opposite claim never reached the worker");
        assert_eq!(request(&manager,"module_usb_claim",None,fields.clone())["ok"],true,"current USB owner may renew");
        assert_eq!(request(&manager,"module_usb_release",None,fields)["ok"],true);
        assert!(manager.module_usb.configuration_claims().is_empty());
        assert_eq!(request(&manager,"claim",Some(FIRST),json!({"moduleId":module}))["ok"],true);
    }
    #[test]
    fn simultaneous_same_client_configuration_claims_choose_only_one_transport() {
        let (manager,_,usb) = configuration_setup();
        let start = Arc::new(std::sync::Barrier::new(3));
        let can_manager = manager.clone();
        let can_start = start.clone();
        let module = usb["snapshot"]["identity"]["moduleId"].clone();
        let can = std::thread::spawn(move || { can_start.wait(); request(&can_manager,"claim",Some(FIRST),json!({"moduleId":module})) });
        let usb_manager = manager.clone();
        let usb_start = start.clone();
        let direct = std::thread::spawn(move || { usb_start.wait(); request(&usb_manager,"module_usb_claim",None,json!({"key":usb["key"],"route":usb["snapshot"]["route"]})) });
        start.wait();
        let can = can.join().unwrap();
        let direct = direct.join().unwrap();
        assert_ne!(can["ok"],direct["ok"],"exactly one same-client transport may acquire a writer: CAN={can} USB={direct}");
        assert_eq!(manager.configuration_claims().len()+manager.module_usb.configuration_claims().len(),1);
    }
    #[test]
    fn failed_handshake_and_command_do_not_close_other_gateways() {
        let (manager, provider) = setup();
        request(&manager, "connect", Some(FIRST), json!({}));
        provider.wrong_identity.store(true, Ordering::Release);
        assert_eq!(
            request(&manager, "connect", Some(SECOND), json!({}))["ok"],
            false
        );
        let adapters = provider.adapters.lock().unwrap();
        assert!(!adapters[0].closed.load(Ordering::Acquire));
        assert!(adapters[1].closed.load(Ordering::Acquire));
        drop(adapters);
        assert_eq!(
            request(&manager, "fail", Some(FIRST), json!({}))["ok"],
            false
        );
        assert_eq!(
            request(&manager, "status", Some(FIRST), json!({}))["state"]["connected"],
            true
        );
    }
    #[test]
    fn a_slow_gateway_does_not_hold_the_fleet_lock_and_shutdown_interrupts_all() {
        let (manager, provider) = setup();
        request(&manager, "connect", Some(FIRST), json!({}));
        request(&manager, "connect", Some(SECOND), json!({}));
        let first = provider.adapters.lock().unwrap()[0].clone();
        first.blocked.store(true, Ordering::Release);
        let worker_manager = manager.clone();
        let slow =
            std::thread::spawn(move || request(&worker_manager, "wait", Some(FIRST), json!({})));
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !first.entered.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(first.entered.load(Ordering::Acquire));
        let (sender, receiver) = mpsc::channel();
        let worker_manager = manager.clone();
        std::thread::spawn(move || {
            sender
                .send(request(&worker_manager, "status", Some(SECOND), json!({})))
                .unwrap();
        });
        assert_eq!(
            receiver.recv_timeout(Duration::from_millis(250)).unwrap()["ok"],
            true
        );
        manager.shutdown();
        slow.join().unwrap();
    }

    #[test]
    fn rejected_gateway_selection_does_not_deadlock_a_starting_dcs_profile() {
        let (mut manager, _) = setup();
        let sessions = manager.sessions.clone();
        let entered = Arc::new(AtomicBool::new(false));
        let released = Arc::new(AtomicBool::new(false));
        let (worker_entered, worker_released) = (entered.clone(), released.clone());
        let panel = Arc::new(move |request: Value| {
            // Starting DCS holds its settings lock while preparing the panel.
            // Pause before its fleet lookup so a rejected request can decorate
            // its response concurrently with this lookup.
            worker_entered.store(true, Ordering::Release);
            while !worker_released.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            let adapter = sessions.lock().unwrap().get(FIRST).cloned().unwrap();
            adapter.request(request)
        });
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.research/milestone2/gateway-lock-tests")
            .join(uuid::Uuid::new_v4().to_string())
            .join("profile.json");
        Arc::get_mut(&mut manager).unwrap().dcs =
            Arc::new(crate::dcs_runtime::DcsRuntime::new(panel, path.clone()));
        for id in [FIRST, SECOND] {
            assert_eq!(
                request(&manager, "connect", Some(id), json!({}))["ok"],
                true
            );
        }
        let config = json!({"schemaVersion":1,"cataloguePath":PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios"),"gatewayId":FIRST,"moduleId":"00112233445566778899aabbccddeeff","button":0,"ledActionId":"led.boolean"});
        assert_eq!(
            request(&manager, "dcs_configure", None, json!({"dcsConfig":config}))["ok"],
            true
        );
        let (sender, receiver) = mpsc::channel();
        let pending_manager = manager.clone();
        let started = sender.clone();
        let start = std::thread::spawn(move || {
            started
                .send(request(&pending_manager, "dcs_start", None, json!({})))
                .unwrap();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !entered.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(entered.load(Ordering::Acquire));
        let pending_manager = manager.clone();
        let rejected = std::thread::spawn(move || {
            sender
                .send(request(&pending_manager, "status", None, json!({})))
                .unwrap();
        });
        // The rejected request must wait for DCS status, but must not retain the
        // fleet lock that DCS preparation needs. Both finish once it resumes.
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        released.store(true, Ordering::Release);
        for _ in 0..2 {
            assert_eq!(
                receiver.recv_timeout(Duration::from_secs(1)).unwrap()["ok"],
                false
            );
        }
        start.join().unwrap();
        rejected.join().unwrap();
        manager.shutdown();
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn full_size_mapping_document_uses_bounded_upload_and_no_device_adapter() {
        let (mut manager, provider) = setup();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.research/mapping-transport-tests")
            .join(uuid::Uuid::new_v4().to_string());
        Arc::get_mut(&mut manager).unwrap().mapping_documents =
            osg_mapping::documents::Documents::new(Some(path.clone()));
        let mut bytes =
            include_bytes!("../../crates/mapping/fixtures/direct-profile.json").to_vec();
        bytes.resize(65_536, b' ');
        let begin = request(
            &manager,
            "mapping_import_begin",
            None,
            json!({"size":bytes.len()}),
        );
        assert_eq!(begin["ok"], true);
        let id = begin["state"]["mappings"]["uploadId"].clone();
        for (index, chunk) in bytes.chunks(8192).enumerate() {
            let payload = json!({"uploadId":id,"offset":index * 8192,"bytes":chunk});
            assert!(payload.to_string().len() < 65_000);
            assert_eq!(
                request(&manager, "mapping_import_chunk", None, payload)["ok"],
                true
            );
        }
        let committed = request(
            &manager,
            "mapping_import_commit",
            None,
            json!({"uploadId":id}),
        );
        assert_eq!(committed["ok"], true, "{committed}");
        assert!(committed.to_string().len() < 66_560);
        let document = &committed["state"]["mappings"]["document"];
        assert!(document.is_object());
        let exported = request(
            &manager,
            "mapping_export",
            None,
            json!({"profileId":document["id"]}),
        );
        assert_eq!(&exported["state"]["mappings"]["document"], document);
        assert_eq!(
            request(
                &manager,
                "mapping_import_begin",
                None,
                json!({"size":65_537})
            )["ok"],
            false
        );
        assert!(provider.adapters.lock().unwrap().is_empty());
        assert!(manager.sessions.lock().unwrap().is_empty());
        manager.shutdown();
        assert_eq!(
            request(&manager, "mapping_profiles", None, json!({"offset":0}))["ok"],
            false
        );
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn shutdown_during_discovery_cannot_create_a_late_adapter() {
        struct SuspendedDiscovery {
            inner: Arc<MockProvider>,
            entered: AtomicBool,
            released: AtomicBool,
        }
        impl Provider for SuspendedDiscovery {
            fn ports(&self) -> Result<Vec<PortInfo>, String> {
                self.entered.store(true, Ordering::Release);
                while !self.released.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                self.inner.ports()
            }
            fn adapter(&self, backend: &str) -> Arc<dyn Adapter> {
                self.inner.adapter(backend)
            }
        }
        let (_, inner) = setup();
        let provider = Arc::new(SuspendedDiscovery {
            inner: inner.clone(),
            entered: AtomicBool::new(false),
            released: AtomicBool::new(false),
        });
        let manager = Arc::new(GatewayManager::with_provider(provider.clone()));
        let worker_manager = manager.clone();
        let pending =
            std::thread::spawn(move || request(&worker_manager, "connect", Some(FIRST), json!({})));
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !provider.entered.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(provider.entered.load(Ordering::Acquire));
        manager.shutdown();
        provider.released.store(true, Ordering::Release);
        assert_eq!(pending.join().unwrap()["ok"], false);
        assert!(inner.adapters.lock().unwrap().is_empty());
    }
}
