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
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, RwLock,
    },
};

/// Complete profile graphs have a separate budget from 64 KiB gateway commands.
pub const PROFILE_REQUEST_LIMIT: usize = osg_profiles::MAX_PROFILE_BYTES + 65_536;
/// One editor transaction includes the portable graph and exact machine-local pins.
pub const PROFILE_EDITOR_REQUEST_LIMIT: usize = osg_profiles::MAX_EDITOR_REQUEST_BYTES;
type ProfilePrepare = Arc<dyn Fn(&crate::profile_session::Candidate) -> Result<Arc<crate::profile_session::Stage>, String> + Send + Sync>;

#[path = "profile_session_panels_io.rs"]
mod profile_panels_io;
pub(crate) use profile_panels_io::ProfilePanelIo;
#[path = "profile_automatic.rs"]
mod automatic;
#[path = "profile_companion_authoring_gateway.rs"]
mod companion_authoring;

trait Adapter: Send + Sync {
    fn output_activity(&self)->bool{false}
    fn request(&self, request: Value) -> Value;
    fn cached_state(&self) -> Value { empty_state() }
    fn configuration_claims(&self) -> Vec<crate::module_configuration_routes::WriterPin> { vec![] }
    fn shutdown(&self);
}
impl Adapter for PicoService {
    fn output_activity(&self)->bool{self.output_activity()}
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
    fn output_activity(&self)->bool{self.output_activity()}
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
fn profile_guard_operation(command: &str) -> Option<&str> {
    if matches!(command,
        "mapping_profiles" | "mapping_get" | "mapping_export" | "mapping_status" | "mapping_targets_get" | "mapping_assignments_get" | "mapping_trace_export" | "mapping_script_validate"
        | "mapping_panel_sources" | "mapping_panel_inspect" | "mapping_companion_sources" | "mapping_companion_inspect" | "mapping_virtual_inspect"
        | "companion_status" | "companion_usb_ports" | "companion_usb_status" | "companion_browser_certificate"
        | "controller_list" | "controller_inspect" | "controller_visibility_status" | "controller_visibility_inspect" | "controller_visibility_processes" | "controller_visibility_restore"
        | "integration_get_state" | "integration_catalogue" | "dcs_status" | "dcs_catalogue" | "game_get_state" | "game_integration_settings_save"
        | "module_usb_ports" | "module_usb_list" | "module_usb_status" | "module_usb_config_read" | "module_usb_actions_describe" | "module_usb_actions_state" | "module_usb_events_state" | "module_usb_events_read") { return None; }
    if command.starts_with("module_usb_") || command.starts_with("controller_") || command.starts_with("config_") || command.starts_with("action_") || matches!(command,"connect"|"disconnect"|"release"|"profile_reset") { return Some("claim"); }
    Some(command)
}
/// Only explicit writes/control changes interrupt a capture. Ordinary status,
/// catalogue and editor reads must not keep cancelling the native learner.
fn learning_interrupt(request:&Value)->bool{
    let command=request["command"].as_str().unwrap_or("");
    if command.starts_with("profile_"){return matches!(command,"profile_apply"|"profile_game_launch"|"profile_pause"|"profile_stop"|"profile_recover"|"profile_reset"|"profile_select"|"profile_selection_mode"|"profile_game_rule_save"|"profile_game_rule_remove");}
    if command=="mapping_companion_route"{return !matches!(request["companion"]["action"].as_str(),Some("inspect"|"recording"));}
    if command=="companion_feedback_routes"{return request["companion"]["action"]!="inspect";}
    if command.starts_with("mapping_")||command.starts_with("module_usb_")||command.starts_with("controller_")||command.starts_with("integration_")||command.starts_with("dcs_")||command.starts_with("game_")||command.starts_with("companion_"){
        return !matches!(command,
            "mapping_profiles"|"mapping_get"|"mapping_export"|"mapping_status"|"mapping_targets_get"|"mapping_assignments_get"|"mapping_trace_export"|"mapping_script_validate"|"mapping_panel_sources"|"mapping_panel_inspect"|"mapping_companion_sources"|"mapping_companion_inspect"|"mapping_virtual_inspect"|"mapping_recording"
            |"module_usb_ports"|"module_usb_list"|"module_usb_status"|"module_usb_config_read"|"module_usb_actions_describe"|"module_usb_actions_state"|"module_usb_events_state"|"module_usb_events_read"
            |"controller_list"|"controller_inspect"|"controller_visibility_status"|"controller_visibility_inspect"|"controller_visibility_processes"
            |"integration_get_state"|"integration_catalogue"|"dcs_status"|"dcs_catalogue"|"game_get_state"|"companion_status"|"companion_usb_ports"|"companion_usb_status"|"companion_browser_certificate");
    }
    matches!(command,"start"|"stop"|"apply"|"save"|"register"|"unregister"|"enable"|"disable"|"forget"|"update-profile"|"configure"|"claim"|"release"|"connect"|"disconnect"|"reconnect"|"remove"|"config_apply"|"config_apply_save"|"config_save"|"config_discard"|"config_reset"|"action_claim"|"action_claim_current"|"action_renew"|"action_release"|"action_invoke"|"action_invoke_owned"|"debug_invoke")
}
struct PendingProfileApply<'a>(&'a AtomicUsize);
impl Drop for PendingProfileApply<'_>{fn drop(&mut self){self.0.fetch_sub(1,Ordering::AcqRel);}}

/// One process can keep any number of independently addressed gateways connected.
/// The session map is held only while selecting an adapter, never during CDC I/O.
pub struct GatewayManager {
    profiles: Mutex<osg_profiles::service::Service>,
    profile_resources: Arc<crate::profile_resources::Runtime>,
    profile_compilation: crate::profile_compilation::Runtime,
    profile_session: Arc<Mutex<crate::profile_session::Coordinator<crate::profile_session_host::NativeProfileHost>>>,
    profile_admission: RwLock<()>,
    profile_prepare: ProfilePrepare,
    profile_native: bool,
    profile_admission_epoch: Arc<AtomicU64>,
    profile_pending: AtomicUsize,
    profile_learning: crate::profile_input_learning::Runtime,
    retired_output_until:Mutex<Option<std::time::Instant>>,
    profile_closed: Arc<AtomicBool>,
    profile_worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    profile_automatic: automatic::Runtime,
    profile_automatic_worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    module_usb: Arc<crate::module_usb_fleet::Fleet>,
    provider: Arc<dyn Provider>,
    sessions: Arc<Mutex<BTreeMap<String, Arc<dyn Adapter>>>>,
    inventory: Mutex<Vec<Value>>,
    configuration_gate: Arc<Mutex<()>>,
    profile_panels: Arc<ProfilePanelIo>,
    closed: Arc<AtomicBool>,
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
                if matches!(command, "integration_start" | "dcs_start") {
                    return json!({"ok":false,"error":"A retired game integration cannot restart independently. Apply a unified profile with its matching verified game context.","state":{"integration":game_integration.status(),"dcsBios":game_dcs.status()}});
                }
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
        let configuration_gate = Arc::new(Mutex::new(()));
        let closed = Arc::new(AtomicBool::new(false));
        let profile_panels = Arc::new(ProfilePanelIo::new(sessions.clone(), module_usb.clone(), configuration_gate.clone(), Arc::new(AtomicBool::new(false))));
        let collect = crate::profile_resource_sources::collector(
            controllers.clone(), companion_provider, companion_host.clone(), games.clone(),
            Arc::new(move || resource_sessions.lock().unwrap_or_else(|p| p.into_inner()).iter().map(|(id, adapter)| (id.clone(), adapter.cached_state())).collect()),
            Arc::new(move |request| {
                let adapter = resource_requests.lock().unwrap_or_else(|p| p.into_inner()).get(request["gatewayId"].as_str().unwrap_or("")).cloned();
                adapter.map_or_else(|| json!({"ok":false,"error":"The inventory gateway is disconnected."}), |adapter| adapter.request(request))
            }),
            module_usb.clone(), native,
        );
        let profile_resources = Arc::new(crate::profile_resources::Runtime::new(native.then(|| crate::dcs_runtime::config_path().with_file_name("profile-resources-v1")), collect, native));
        let profile_admission_epoch = Arc::new(AtomicU64::new(0));
        let profile_learning=crate::profile_input_learning::Runtime::new(profile_resources.clone(),controllers.clone(),profile_admission_epoch.clone());
        let profile_session = Arc::new(Mutex::new(crate::profile_session::Coordinator::new(
            crate::profile_session_host::NativeProfileHost::new(profile_resources.clone(), controllers.clone(), games.clone(), companion_host.clone(), profile_panels.clone(), native, profile_admission_epoch.clone()),
            native.then(|| crate::dcs_runtime::config_path().with_file_name("profile-session-v1.json")),
        )));
        let prepare_resources = profile_resources.clone();
        let prepare_games = games.clone();
        let prepare_companions = companion_host.clone();
        let prepare_controllers = controllers.clone();
        let prepare_session = profile_session.clone();
        let profile_prepare: ProfilePrepare = Arc::new(move |candidate| {
            let outgoing = prepare_session.lock().unwrap_or_else(|p| p.into_inner()).visibility_handoff_source()?;
            crate::profile_session_host::NativeProfileHost::stage_with_handoff(&prepare_resources, &prepare_games, &prepare_companions, prepare_controllers.clone(), native, candidate, outgoing)
        });
        let profile_closed = Arc::new(AtomicBool::new(false));
        let profile_worker = native.then(|| {
            let runtime = profile_session.clone();
            let closed = profile_closed.clone();
            std::thread::spawn(move || {
                while !closed.load(Ordering::Acquire) {
                    let active = {
                        let mut runtime = runtime.lock().unwrap_or_else(|p| p.into_inner());
                        let _ = runtime.poll();
                        runtime.status().owner.is_some()
                    };
                    std::thread::sleep(std::time::Duration::from_millis(if active { 5 } else { 100 }));
                }
            })
        });
        let manager = Self {
            module_usb,
            profile_resources,
            profile_compilation: crate::profile_compilation::Runtime::default(),
            profile_session,
            profile_admission: RwLock::new(()),
            profile_prepare,
            profile_native: native,
            profile_admission_epoch,
            profile_pending:AtomicUsize::new(0),
            retired_output_until:Mutex::new(None),
            profile_learning,
            profile_closed,
            profile_worker: Mutex::new(profile_worker),
            profile_automatic: automatic::Runtime::new(native.then(||crate::dcs_runtime::config_path().with_file_name("profiles-activation-v1.json"))),
            profile_automatic_worker: Mutex::new(None),
            profiles: Mutex::new(osg_profiles::service::Service::new(native.then(|| {
                crate::dcs_runtime::config_path().with_file_name("profiles-v2")
            }))),
            provider,
            sessions,
            inventory: Mutex::new(Vec::new()),
            configuration_gate,
            profile_panels,
            closed,
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
        manager.restore_automatic_selection();
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
    fn decorate_profile_session(&self, response: &mut Value) {
        let status = serde_json::to_value(self.profile_session.lock().unwrap_or_else(|p| p.into_inner()).status()).unwrap();
        if let Some(session) = response.get_mut("session").and_then(Value::as_object_mut) {
            for key in ["state", "paused", "appliedSnapshot", "fault"] { session.insert(key.into(), status[key].clone()); }
        }
        response["profileSession"] = status;
        response["automatic"] = self.profile_automatic.view();
    }
    fn profile_session_request(&self, request: &Value, native_arrival_epoch: Option<u64>) -> Result<Value, String> {
        let command = request["command"].as_str().ok_or("Choose a profile session operation.")?;
        let fields: &[&str] = match command {
            "profile_apply" => &["id", "expectedRevision", "gameId"],
            "profile_game_launch" => &["gameId"],
            "profile_pause" => &["paused"],
            "profile_stop" | "profile_recover" => &[],
            _ => return Err("Unknown profile session operation.".into()),
        };
        if request.as_object().ok_or("A profile session request must be an object.")?.keys().any(|key| key != "command" && !fields.contains(&key.as_str())) {
            return Err("Unknown field in profile session request.".into());
        }
        let staged_epoch = || native_arrival_epoch.filter(|epoch| self.profile_admission_epoch.load(Ordering::Acquire) == *epoch).ok_or("This staged profile request was cancelled by a newer native control. Submit it again explicitly.");
        match command {
            "profile_game_launch" => return self.launch_profile_game(request["gameId"].as_str().filter(|id| matches!(*id,"dcs"|"x-plane-11"|"x-plane-12"|"assetto-corsa-rally")).ok_or("Choose a supported native game installation to launch.")?, staged_epoch()?),
            "profile_apply" => {
                let admission_epoch = staged_epoch()?;
                let id = request["id"].as_str().ok_or("Choose a saved profile to apply.")?;
                let expected = request["expectedRevision"].as_u64().filter(|revision| *revision > 0 && *revision <= osg_profiles::MAX_REVISION).ok_or("Apply requires the current saved expectedRevision.")?;
                let (document, assignments, read_only) = self.profiles.lock().unwrap_or_else(|p| p.into_inner()).compile_source(id)?;
                if document.revision != expected { return Err("The saved profile changed. Reload its revision before applying.".into()); }
                let game_id = request.get("gameId").map(|value| value.as_str().filter(|id| matches!(*id,"dcs"|"x-plane-11"|"x-plane-12"|"assetto-corsa-rally")).map(str::to_owned).ok_or("Choose a supported game context or omit gameId for ordinary hardware.")).transpose()?;
                if game_id.as_ref().is_some_and(|game| !document.eligibility.game_ids.is_empty() && !document.eligibility.game_ids.contains(game)) { return Err("This profile is not eligible for the selected game context.".into()); }
                self.pin_staged_manual(id,admission_epoch)?;
                crate::profile_session::apply_staged_admitted(
                    &self.profile_session,
                    crate::profile_session::Candidate { document, assignments, read_only, game_id, admission_epoch: Some(admission_epoch), stage: None },
                    |candidate| self.prepare_profile_at(candidate,admission_epoch),
                    |candidate| {
                        let admission = self.profile_admission.write().unwrap_or_else(|p| p.into_inner());
                        let game_admission = self.game_admission.read().unwrap_or_else(|p| p.into_inner());
                        if self.closed.load(Ordering::Acquire)
                            || self.profile_admission_epoch.load(Ordering::Acquire) != admission_epoch {
                            return Err("Profile Apply was cancelled by Stop, Pause, recovery or a newer activation. Apply again explicitly.".into());
                        }
                        let mut profiles = self.profiles.lock().unwrap_or_else(|p| p.into_inner());
                        let (document, assignments, read_only) = profiles.compile_source(id)?;
                        let saved = crate::profile_session::Candidate {
                            document, assignments, read_only, game_id: candidate.game_id.clone(), admission_epoch: candidate.admission_epoch, stage: None,
                        };
                        candidate.stage.as_ref().ok_or("Fresh native preparation is missing.")?.validate(&saved)?;
                        profiles.request(&json!({"command":"profile_select","id":id,"selectionSource":"manual"}))?;
                        Ok((admission, game_admission))
                    },
                    || { self.profile_admission_epoch.fetch_add(1, Ordering::AcqRel); },
                )?;
            },
            "profile_pause" => {let result=self.profile_session.lock().unwrap_or_else(|p| p.into_inner()).pause(request["paused"].as_bool().ok_or("paused must be a boolean.")?);self.profile_automatic.retry_event();result?;},
            "profile_stop" => {let stopped=self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).stop();let pinned=self.automatic_stop_pin();stopped.and(pinned)?;},
            "profile_recover" => {let result=self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).recover();self.profile_automatic.retry_event();result?;},
            _ => unreachable!(),
        }
        let mut response = self.profiles.lock().unwrap_or_else(|p| p.into_inner()).request(&json!({"command":"profile_status"}))?;
        self.decorate_profile_session(&mut response);
        Ok(response)
    }
    fn pin_staged_manual(&self,id:&str,arrival_epoch:u64)->Result<(),String>{
        let _admission=self.profile_admission.write().unwrap_or_else(|p|p.into_inner());
        if self.closed.load(Ordering::Acquire)||self.profile_admission_epoch.load(Ordering::Acquire)!=arrival_epoch{return Err("This staged Manual selection was cancelled by a newer native control.".into());}
        self.automatic_manual_pin(id)?;
        self.profiles.lock().unwrap_or_else(|p|p.into_inner()).request(&json!({"command":"profile_selection_mode","mode":"manual"}))?;
        Ok(())
    }
    fn prepare_profile_at(&self,candidate:&crate::profile_session::Candidate,arrival_epoch:u64)->Result<Arc<crate::profile_session::Stage>,String>{
        let current=||self.profile_admission_epoch.load(Ordering::Acquire)==arrival_epoch;
        if !current(){return Err("Profile preparation was cancelled by a newer native control.".into());}
        let prepared=(self.profile_prepare)(candidate);
        if !current(){return Err("Profile preparation was cancelled by a newer native control.".into());}
        prepared
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
    fn invalidate_learning(&self)->u64{let epoch=self.profile_admission_epoch.fetch_add(1,Ordering::AcqRel).wrapping_add(1);self.profile_learning.invalidate("A native control/output request cancelled input learning. Restart explicitly when outputs are idle.");epoch}
    fn guard_learning(&self)->Result<(),String>{
        if self.profile_pending.load(Ordering::Acquire)!=0||self.profile_automatic.pending(){return Err("Wait for pending profile Apply or selection control to finish before input learning.".into());}
        self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).guard_request("mapping_preview_start")?;
        self.mapping_host.guard_capture_only()?;
        let adapters=self.sessions.lock().unwrap_or_else(|p|p.into_inner()).values().cloned().collect::<Vec<_>>();
        let retired=self.retired_output_until.lock().unwrap_or_else(|p|p.into_inner()).is_some_and(|until|until>std::time::Instant::now());
        if retired||adapters.iter().any(|adapter|adapter.output_activity())||self.module_usb.output_activity(){return Err("Wait for independent device output leases and uncertain fallback recovery to finish before input learning.".into());}
        if self.games.active()||self.integration.running()||self.integration.status()["running"]==true||self.dcs.status()["running"]==true{return Err("Stop active or pending game/integration output before input learning.".into());}
        let visibility=self.controllers.request(&json!({"command":"controller_visibility_status"}))?;
        if self.controllers.visibility_active()||visibility["visibility"]["recovery"]["required"]!=false{return Err("Restore active/pending device visibility and its recovery before input learning.".into());}
        if !self.configuration_claims().is_empty()||!self.module_usb.configuration_claims().is_empty(){return Err("Release independent device configuration ownership before input learning.".into());}
        Ok(())
    }
    fn learning_request(&self,request:&Value)->Result<Value,String>{
        let command=request["command"].as_str().ok_or("Choose an input-learning operation.")?;
        let fields:&[&str]=match command{"profile_input_learn_start"=>&["resourceId","generation","capabilityRevision"],"profile_input_learn_status"|"profile_input_learn_cancel"=>&["learnId"],_=>return Err("Unknown input-learning operation.".into())};
        if request.as_object().ok_or("Input learning requires an object.")?.keys().any(|key|key!="command"&&key!="clientId"&&!fields.contains(&key.as_str())){return Err("Unknown field in input-learning request.".into());}
        let text=|key:&str|request[key].as_str().ok_or_else(||format!("Input learning requires {key}."));
        match command{
            "profile_input_learn_start"=>{
                let epoch={let _admission=self.profile_admission.write().unwrap_or_else(|p|p.into_inner());if self.closed.load(Ordering::Acquire){return Err("Cockpit has closed.".into());}self.guard_learning()?;self.profile_admission_epoch.load(Ordering::Acquire)};
                self.profile_learning.start_at(text("resourceId")?,text("generation")?,text("capabilityRevision")?,epoch)
            },
            "profile_input_learn_status"=>self.profile_learning.status(text("learnId")?),
            _=>self.profile_learning.cancel(text("learnId")?),
        }
    }
    pub fn request(&self, request: Value) -> Value {
        // An admitted independent write and a profile takeover cannot overlap.
        // The native host addresses its existing owners directly after this
        // boundary; it never re-enters Manager while holding the writer lock.
        let profile_mutation = matches!(request["command"].as_str(), Some("profile_apply" | "profile_game_launch" | "profile_stop" | "profile_pause" | "profile_recover" | "profile_select" | "profile_selection_mode" | "profile_game_rule_save" | "profile_game_rule_remove" | "mapping_stop" | "integration_stop" | "dcs_stop" | "controller_visibility_restore"));
        let _policy=self.profile_automatic.begin_control(request["command"].as_str().unwrap_or(""));
        self.profile_automatic.control(request["command"].as_str().unwrap_or(""));
        let staging = matches!(request["command"].as_str(), Some("profile_apply" | "profile_game_launch"));
        let learning_start=request["command"]=="profile_input_learn_start";
        let learning_passive=matches!(request["command"].as_str(),Some("profile_input_learn_status"|"profile_input_learn_cancel"));
        let _pending=if staging{self.profile_pending.fetch_add(1,Ordering::AcqRel);Some(PendingProfileApply(&self.profile_pending))}else{None};
        let interrupt=learning_interrupt(&request);
        let arrival_epoch=interrupt.then(||self.invalidate_learning());
        if staging{let _admission=self.profile_admission.write().unwrap_or_else(|p|p.into_inner());if arrival_epoch!=Some(self.profile_admission_epoch.load(Ordering::Acquire)){return self.error("This staged profile request was cancelled by a newer native control. Submit it again explicitly.",None);}}
        if learning_start||learning_passive{return self.request_inner(request);}
        let _profile_write = (profile_mutation && !staging).then(|| self.profile_admission.write().unwrap_or_else(|p| p.into_inner()));
        let _profile_read = (!profile_mutation).then(|| self.profile_admission.read().unwrap_or_else(|p| p.into_inner()));
        if interrupt&&!staging{self.invalidate_learning();}
        // Slow read-only staging holds neither admission lock. Final takeover
        // acquires profile then game admission together in the established order.
        if staging { return self.request_inner_at(request,arrival_epoch); }
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
    fn request_inner(&self, request: Value) -> Value {self.request_inner_at(request,None)}
    fn request_inner_at(&self, mut request: Value, native_arrival_epoch: Option<u64>) -> Value {
        if self.closed.load(Ordering::Acquire) {
            return self.error("Cockpit gateway manager has closed.", None);
        }
        let profile_request = request["command"]
            .as_str()
            .is_some_and(|command| command.starts_with("profile_"));
        let request_limit = if matches!(request["command"].as_str(), Some("profile_editor_save" | "profile_compile")) {
            PROFILE_EDITOR_REQUEST_LIMIT
        } else if profile_request {
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
        if command.starts_with("profile_input_learn_"){return match self.learning_request(&request){Ok(learning)=>json!({"ok":true,"state":{"profileLearning":learning}}),Err(error)=>json!({"ok":false,"error":error.chars().take(512).collect::<String>()})};}
        if let Some(operation) = profile_guard_operation(&command) {
            if let Err(error) = self.profile_session.lock().unwrap_or_else(|p| p.into_inner()).guard_request(operation) {
                return self.error(&error, None);
            }
        }
        if matches!(command.as_str(), "mapping_stop" | "integration_stop" | "dcs_stop" | "controller_visibility_restore") {
            if let Err(error) = self.profile_session.lock().unwrap_or_else(|p| p.into_inner()).stop() { return self.error(&error, None); }
        }
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
        if matches!(command.as_str(), "profile_compile" | "profile_compilation_inspect") {
            request.as_object_mut().unwrap().remove("clientId");
            return match self.profile_compilation.request(&request, |document, draft_assignments| {
                if let Some(assignments) = draft_assignments {
                    // New/unsaved drafts have no saved identity requirement.
                    // This is a cached preview, never a save or Apply authority.
                    let read_only = self.profiles.lock().unwrap_or_else(|p| p.into_inner())
                        .compile_source(osg_profiles::DEFAULT_PROFILE_ID)?.2;
                    let mut snapshot = self.profile_resources.snapshot_for_compile(assignments)?;
                    snapshot.read_only |= read_only;
                    return Ok(snapshot);
                }
                let (stored, mut assignments, read_only) = self.profiles.lock().unwrap_or_else(|p| p.into_inner()).compile_source(&document.id)?;
                if stored.revision != document.revision {
                    return Err("The profile revision changed; reload its saved revision before compiling this draft.".into());
                }
                // Draft removal is reflected in this preview without changing
                // the stored graph or its machine-local assignments.
                assignments.retain(|assignment| document.resources.iter().any(|resource| resource.id == assignment.resource_id));
                for assignment in &mut assignments {
                    let resource = document.resources.iter().find(|resource| resource.id == assignment.resource_id).unwrap();
                    assignment.ports.retain(|port| resource.ports.iter().any(|logical| logical.id == port.profile_port_id));
                }
                let mut snapshot = self.profile_resources.snapshot_for_compile(assignments)?;
                snapshot.read_only |= read_only;
                if read_only { snapshot.inventory_diagnostics.push("The profile library is read-only; repair its storage before activation.".into()); }
                Ok(snapshot)
            }) {
                Ok(compilation) => json!({"ok":true,"state":{"profileCompilation":compilation}}),
                Err(error) => json!({"ok":false,"error":error.chars().take(256).collect::<String>()}),
            };
        }
        if command == "profile_runtime_inspect" {
            request.as_object_mut().unwrap().remove("clientId");
            let result = crate::profile_session::inspection::Request::parse(&request)
                .and_then(|request| self.profile_session.lock().unwrap_or_else(|p| p.into_inner()).inspect(&request));
            return match result {
                Ok(page) => json!({"ok":true,"state":{"profileInspection":page}}),
                Err(error) => json!({"ok":false,"error":error.chars().take(512).collect::<String>()}),
            };
        }
        if command == "profile_runtime_topology" {
            request.as_object_mut().unwrap().remove("clientId");
            let result = crate::profile_session::topology::Request::parse(&request)
                .and_then(|request| self.profile_session.lock().unwrap_or_else(|p|p.into_inner()).inspect_topology(&request));
            return match result { Ok(page)=>json!({"ok":true,"state":{"profileTopology":page}}), Err(error)=>json!({"ok":false,"error":error.chars().take(512).collect::<String>()}) };
        }
        if profile_request {
            request.as_object_mut().unwrap().remove("clientId");
            if matches!(command.as_str(),"profile_companion_catalogue"|"profile_companion_inspect"){
                return match self.companion_authoring_request(&request){Ok(page)=>json!({"ok":true,"state":{"profileCompanionAuthoring":page}}),Err(error)=>json!({"ok":false,"error":error.chars().take(512).collect::<String>()})};
            }
            if matches!(command.as_str(),"profile_automatic_status"|"profile_game_rule_save"|"profile_game_rule_remove"|"profile_selection_mode"){
                return match self.automatic_request(&request){Ok(profiles)=>json!({"ok":true,"state":{"profiles":profiles}}),Err(error)=>json!({"ok":false,"error":error.chars().take(512).collect::<String>()})};
            }
            if matches!(command.as_str(), "profile_apply" | "profile_game_launch" | "profile_stop" | "profile_pause" | "profile_recover") {
                return match self.profile_session_request(&request,native_arrival_epoch) {
                    Ok(profiles) => json!({"ok":true,"state":{"profiles":profiles}}),
                    Err(error) => json!({"ok":false,"error":error.chars().take(512).collect::<String>()}),
                };
            }
            let result = self
                .profiles
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .request(&request);
            return match result {
                Ok(mut profiles) => {
                    if command=="profile_select"&&request["selectionSource"]=="manual"{
                        if let Err(error)=self.automatic_manual_pin(request["id"].as_str().unwrap_or("")){return json!({"ok":false,"error":error});}
                    }
                    self.decorate_profile_session(&mut profiles); json!({"ok":true,"state":{"profiles":profiles}})
                },
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
            if let Err(error) = self.games.guard_unified_integration_request(&request) {
                return self.error(&error, None);
            }
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
        if let Err(error) = crate::game_context::guard_public_provider_request(&command) {
            return self.error(&error, None);
        }
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
        if matches!(command.as_str(), "claim" | "config_apply" | "config_save" | "config_reset" | "action_invoke" | "action_claim" | "action_claim_current" | "action_renew" | "action_invoke_owned") {
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
                if adapter.output_activity(){let until=std::time::Instant::now()+std::time::Duration::from_millis(crate::output_leases::TTL);let mut retired=self.retired_output_until.lock().unwrap_or_else(|p|p.into_inner());*retired=Some(retired.map_or(until,|old|old.max(until)));}
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
        self.profile_admission_epoch.fetch_add(1,Ordering::AcqRel);
        self.profile_closed.store(true,Ordering::Release);
        if let Some(worker)=self.profile_automatic_worker.lock().unwrap_or_else(|p|p.into_inner()).take(){if worker.thread().id()!=std::thread::current().id(){let _=worker.join();}}
        self.profile_learning.shutdown();
        self.profile_closed.store(true, Ordering::Release);
        if let Some(worker) = self.profile_worker.lock().unwrap_or_else(|p| p.into_inner()).take() { let _ = worker.join(); }
        let _ = self.profile_session.lock().unwrap_or_else(|p| p.into_inner()).stop();
        self.profile_panels.close();
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
        endpoint_activity:crate::output_leases::LeaseActivity,
    }
    impl Adapter for MockAdapter {
        fn output_activity(&self)->bool{self.endpoint_activity.active()}
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
            let mut reply=state.clone();
            if request["command"] == "events_read" {
                let id=request["moduleId"].as_str().unwrap();
                let events=&mut reply["moduleEvents"][id];
                let after=request["after"].as_u64().unwrap();
                events["events"]=json!(events["events"].as_array().unwrap().iter().filter(|event|event["sequence"].as_u64().unwrap()>after).cloned().collect::<Vec<_>>());
                events["after"]=json!(after);events["next"]=events["head"].clone();events["more"]=json!(false);
            }
            json!({"ok":request["command"] != "fail","state":reply})
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
    fn slow_profile_preflight_keeps_status_and_stop_responsive_without_late_rearm() {
        for operation in ["profile_stop", "profile_pause", "profile_recover"] {
            let (mut manager, _) = setup();
            let fields = json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1});
            assert_eq!(request(&manager,"profile_apply",None,fields.clone())["ok"],true);
            let prepare = manager.profile_prepare.clone();
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let release = Mutex::new(release_rx);
            Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
                entered_tx.send(()).unwrap();
                release.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
                prepare(candidate)
            });
            let pending = manager.clone();
            let apply = std::thread::spawn(move || request(&pending,"profile_apply",None,fields));
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let (completed_tx, completed_rx) = mpsc::channel();
            let control = manager.clone();
            let control = std::thread::spawn(move || {
                let status = request(&control,"profile_status",None,json!({}));
                // Legacy game-profile writes take the real game writer before
                // the active unified owner rejects their independent mutation.
                let settings = request(&control,"game_profile_save",None,json!({}));
                assert_eq!(settings["ok"],false);
                let fields = if operation == "profile_pause" { json!({"paused":true}) } else { json!({}) };
                let stopped = request(&control,operation,None,fields);
                completed_tx.send((status,stopped)).unwrap();
            });
            let completed = completed_rx.recv_timeout(Duration::from_secs(2));
            release_tx.send(()).unwrap();
            let result = apply.join().unwrap();
            control.join().unwrap();
            let (status,stopped) = completed.expect("Status and emergency control must complete before slow staging is released.");
            assert_eq!(status["state"]["profiles"]["profileSession"]["state"],"running");
            assert_eq!(stopped["ok"],true,"{stopped}");
            assert_eq!(result["ok"],false,"{operation}: {result}");
            assert!(result["error"].as_str().unwrap().contains("cancelled"),"{operation}: {result}");
            let status = request(&manager,"profile_status",None,json!({}));
            assert!(status["state"]["profiles"]["profileSession"]["owner"].is_null());
            if operation == "profile_pause" { assert_eq!(status["state"]["profiles"]["profileSession"]["paused"],true); }
        }
    }
    #[test]
    fn successful_competing_apply_or_saved_edit_invalidates_the_older_native_stage() {
        for saved_edit in [false,true] {
            let (mut manager, _) = setup();
            let fields = json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1});
            let initial = request(&manager,"profile_apply",None,fields.clone());
            assert_eq!(initial["ok"],true);
            let initial_owner = initial["state"]["profiles"]["profileSession"]["owner"].clone();
            let prepare = manager.profile_prepare.clone();
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let release = Mutex::new(release_rx);
            let first = AtomicBool::new(true);
            Arc::get_mut(&mut manager).unwrap().profile_prepare = Arc::new(move |candidate| {
                if first.swap(false,Ordering::AcqRel) {
                    entered_tx.send(()).unwrap();
                    release.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
                }
                prepare(candidate)
            });
            let pending = manager.clone();
            let pending_fields = fields.clone();
            let apply = std::thread::spawn(move || request(&pending,"profile_apply",None,pending_fields));
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let mut retained_owner = initial_owner;
            if saved_edit {
                let mut document = request(&manager,"profile_get",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID}))["state"]["profiles"]["document"].clone();
                document["eligibility"]["gameIds"] = json!(["dcs"]);
                let saved = request(&manager,"profile_save",None,json!({"document":document,"expectedRevision":1}));
                assert_eq!(saved["ok"],true,"{saved}");
            } else {
                let newer = request(&manager,"profile_apply",None,fields);
                assert_eq!(newer["ok"],true,"{newer}");
                retained_owner = newer["state"]["profiles"]["profileSession"]["owner"].clone();
            }
            release_tx.send(()).unwrap();
            let result = apply.join().unwrap();
            assert_eq!(result["ok"],false,"{result}");
            let status = request(&manager,"profile_status",None,json!({}));
            assert_eq!(status["state"]["profiles"]["profileSession"]["owner"],retained_owner);
            assert_eq!(status["state"]["profiles"]["profileSession"]["state"],"running");
        }
    }
    #[test]
    fn cancellation_between_admission_and_native_prepare_uses_the_captured_epoch() {
        use crate::profile_session::{Host, OwnedRun, RunOwner, APPLY_PHASES};
        let (manager, _) = setup();
        let applied = request(&manager,"profile_apply",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));
        assert_eq!(applied["ok"],true);
        let retained_owner = applied["state"]["profiles"]["profileSession"]["owner"].clone();
        let captured = manager.profile_admission_epoch.load(Ordering::Acquire);
        let (document, assignments, read_only) = manager.profiles.lock().unwrap().compile_source(osg_profiles::DEFAULT_PROFILE_ID).unwrap();
        let mut candidate = crate::profile_session::Candidate {
            document, assignments, read_only, game_id: None, admission_epoch: Some(captured), stage: None,
        };
        candidate.stage = Some((manager.profile_prepare)(&candidate).unwrap());
        let panels = manager.profile_panels.clone();
        let mut host = crate::profile_session_host::NativeProfileHost::new(
            manager.profile_resources.clone(),manager.controllers.clone(),manager.games.clone(),
            manager.companion_host.clone(),panels,
            false,manager.profile_admission_epoch.clone(),
        );
        let owner = RunOwner {
            session:uuid::Uuid::new_v4().to_string(),generation:1,profile_id:candidate.document.id.clone(),
            profile_revision:1,game_id:None,
        };
        // This is the narrow handoff window after Manager validated admission,
        // before Host::prepare installs NativeRun. A late epoch load would miss it.
        manager.profile_admission_epoch.fetch_add(1,Ordering::AcqRel);
        assert!(host.prepare(&candidate,&owner).unwrap_err().contains("cancelled"));
        candidate.admission_epoch = Some(manager.profile_admission_epoch.load(Ordering::Acquire));
        candidate.stage = Some((manager.profile_prepare)(&candidate).unwrap());
        let prepared = host.prepare(&candidate,&owner).unwrap();
        let run = OwnedRun {
            owner:owner.clone(),snapshot:osg_profiles::session::AppliedSnapshot::capture(&candidate.document).unwrap().summary().clone(),
            program_hash:prepared.program_hash.clone(),capture_fingerprint:prepared.capture_fingerprint.clone(),
            receipts:vec![],attempted:vec![],
        };
        manager.profile_admission_epoch.fetch_add(1,Ordering::AcqRel);
        assert!(host.validate_prepared(&prepared,&owner).is_err());
        for phase in APPLY_PHASES {
            assert!(host.enter(phase,&prepared,&run).unwrap_err().contains("cancelled"),"{phase:?}");
        }
        host.restore(&run,false).unwrap();
        // Successful Apply itself advances this epoch. Active health must use
        // actual owner/generation fences, rather than this transition-only fence.
        manager.profile_session.lock().unwrap().poll().unwrap();
        assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"],retained_owner);
    }
    #[test]
    fn profile_runtime_requires_saved_revision_and_keeps_navigation_separate() {
        let (manager, _) = setup();
        let default = osg_profiles::DEFAULT_PROFILE_ID;
        let applied = request(&manager,"profile_apply",None,json!({"id":default,"expectedRevision":1}));
        assert_eq!(applied["ok"],true,"{applied}");
        assert_eq!(applied["state"]["profiles"]["session"]["state"],"running");
        let owner=applied["state"]["profiles"]["profileSession"]["owner"].clone();
        let created=request(&manager,"profile_create",None,json!({"name":"Draft only"}));
        let document=&created["state"]["profiles"]["document"];
        assert_eq!(request(&manager,"profile_get",None,json!({"id":document["id"]}))["ok"],true);
        assert_eq!(request(&manager,"profile_select",None,json!({"id":document["id"],"selectionSource":"manual"}))["ok"],true);
        assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"],owner);
        assert_eq!(request(&manager,"profile_apply",None,json!({"id":document["id"],"expectedRevision":2}))["ok"],false);
        assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"],owner);
        for command in ["module_usb_claim","module_usb_config_apply","module_usb_actions_invoke","action_invoke_owned","config_stage","controller_update","mapping_virtual_start","companion_assign","profile_reset"] {
            let blocked=request(&manager,command,None,json!({}));
            assert_eq!(blocked["ok"],false,"{command}");
            assert!(blocked["error"].as_str().unwrap().contains("unified profile session"),"{command}: {blocked}");
        }
        assert_eq!(request(&manager,"controller_list",None,json!({}))["ok"],true);
        assert_eq!(request(&manager,"mapping_status",None,json!({}))["ok"],true);
        assert_eq!(request(&manager,"module_usb_list",None,json!({}))["ok"],true);
        assert_eq!(request(&manager,"profile_stop",None,json!({}))["ok"],true);
    }
    fn learning_manager(gate:Option<Arc<std::sync::Barrier>>)->(GatewayManager,Value){let (manager,_)=setup();let mut manager=Arc::try_unwrap(manager).ok().unwrap();let(runtime,source)=crate::profile_input_learning::manager_fixture(manager.profile_admission_epoch.clone(),gate);manager.profile_learning=runtime;(manager,source)}
    #[test]
    fn input_learning_manager_start_is_strict_passive_and_controls_invalidate_before_effects(){
        let(manager,source)=learning_manager(None);let started=request(&manager,"profile_input_learn_start",None,source.clone());assert_eq!(started["ok"],true,"{started}");let id=started["state"]["profileLearning"]["learnId"].as_str().unwrap();
        for command in ["profile_status","controller_list","profile_list"]{assert_eq!(request(&manager,command,None,json!({}))["ok"],true);assert_eq!(request(&manager,"profile_input_learn_status",None,json!({"learnId":id}))["state"]["profileLearning"]["phase"],"listening");}
        let mut forged=source.clone();for key in ["owner","sample","token","resume"]{forged[key]=json!(true);assert_eq!(request(&manager,"profile_input_learn_start",None,forged.clone())["ok"],false);forged.as_object_mut().unwrap().remove(key);}
        let applied=request(&manager,"profile_apply",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));assert_eq!(applied["ok"],true,"{applied}");let owner=applied["state"]["profiles"]["profileSession"]["owner"].clone();assert_eq!(request(&manager,"profile_input_learn_status",None,json!({"learnId":id}))["state"]["profileLearning"]["phase"],"stopped");
        let denied=request(&manager,"profile_input_learn_start",None,source.clone());assert_eq!(denied["ok"],false);assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"],owner);
        request(&manager,"profile_stop",None,json!({}));let newer=request(&manager,"profile_input_learn_start",None,source);assert_eq!(newer["ok"],true,"{newer}");let newer_id=&newer["state"]["profileLearning"]["learnId"];assert_eq!(request(&manager,"profile_input_learn_cancel",None,json!({"learnId":id}))["ok"],false);assert_eq!(request(&manager,"profile_input_learn_status",None,json!({"learnId":newer_id}))["state"]["profileLearning"]["phase"],"listening");request(&manager,"action_invoke",None,json!({}));assert_eq!(request(&manager,"profile_input_learn_status",None,json!({"learnId":newer_id}))["state"]["profileLearning"]["phase"],"stopped");
    }
    #[test]
    fn input_learning_pending_apply_and_late_baseline_cannot_overlap_new_output(){
        let (manager,source)=learning_manager(None);manager.profile_pending.store(1,Ordering::Release);let blocked=request(&manager,"profile_input_learn_start",None,source);assert_eq!(blocked["ok"],false);assert!(blocked["error"].as_str().unwrap().contains("pending profile Apply"));manager.profile_pending.store(0,Ordering::Release);
        let gate=Arc::new(std::sync::Barrier::new(2));let(manager,source)=learning_manager(Some(gate.clone()));let manager=Arc::new(manager);let learning=manager.clone();let pending=std::thread::spawn(move||request(&learning,"profile_input_learn_start",None,source));gate.wait();let applied=request(&manager,"profile_apply",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));assert_eq!(applied["ok"],true,"{applied}");gate.wait();let late=pending.join().unwrap();assert_eq!(late["ok"],false,"{late}");assert!(late["error"].as_str().unwrap().contains("cancelled"));assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["state"],"running");
    }
    #[test]
    fn input_learning_rejects_existing_endpoint_fallback_work_until_exact_release(){
        let(manager,source)=learning_manager(None);let adapter=Arc::new(MockAdapter::default());manager.sessions.lock().unwrap().insert(FIRST.into(),adapter.clone());
        adapter.endpoint_activity.begin().finish("physical:100","action_claim",None,Some(&json!({"leaseId":7,"remainingMs":5000})));
        let denied=request(&manager,"profile_input_learn_start",None,source.clone());assert_eq!(denied["ok"],false,"{denied}");assert!(denied["error"].as_str().unwrap().contains("output leases"));assert!(adapter.calls.lock().unwrap().is_empty(),"admission must only read cached activity");
        adapter.endpoint_activity.begin().finish("physical:100","action_release",Some(7),Some(&json!({"leaseId":7,"released":true})));
        let started=request(&manager,"profile_input_learn_start",None,source);assert_eq!(started["ok"],true,"{started}");
    }
    #[test]
    fn input_learning_keeps_bounded_fallback_guard_when_bad_handshake_retires_the_adapter(){
        let(manager,source)=learning_manager(None);let adapter=Arc::new(MockAdapter::default());adapter.wrong_identity.store(true,Ordering::Release);adapter.endpoint_activity.begin().finish("physical:100","action_claim",None,Some(&json!({"leaseId":7,"remainingMs":5000})));manager.sessions.lock().unwrap().insert(FIRST.into(),adapter);
        assert_eq!(request(&manager,"connect",Some(FIRST),json!({}))["ok"],false);assert!(manager.sessions.lock().unwrap().is_empty());
        let blocked=request(&manager,"profile_input_learn_start",None,source.clone());assert_eq!(blocked["ok"],false);assert!(blocked["error"].as_str().unwrap().contains("output leases"));
        *manager.retired_output_until.lock().unwrap()=Some(std::time::Instant::now()-std::time::Duration::from_millis(1));assert_eq!(request(&manager,"profile_input_learn_start",None,source)["ok"],true);
    }

    #[test]
    fn applied_port_inspection_reads_only_the_exact_native_run_without_loading_a_newer_draft() {
        use osg_profiles::{Port,Resource,ResourceKind,ProcessingNode,Direction,PortContract,PortValue,Binding,Endpoint};
        let (manager,_) = setup();
        let mut document=osg_profiles::default_document();
        let contract=PortContract::State {schema:osg_profiles::resources_ports::ScalarSchema::Boolean{},freshness_ms:1000,fallback:None};
        document.resources.push(Resource{id:"display".into(),name:"Optional display".into(),kind:ResourceKind::Companion,required:false,ports:vec![Port{id:"indicator".into(),name:"Indicator".into(),direction:Direction::Input,contract:contract.clone()}]});
        document.processing_nodes.push(ProcessingNode{id:"constant".into(),name:"Constant".into(),operation:osg_mapping::program::Operation::Constant{value:PortValue::Boolean(true)},inputs:vec![],outputs:vec![Port{id:"value".into(),name:"Value".into(),direction:Direction::Output,contract}]});
        document.bindings.push(Binding{id:"to-display".into(),from:Endpoint{node_id:"constant".into(),port_id:"value".into()},to:Endpoint{node_id:"display".into(),port_id:"indicator".into()}});
        let saved=request(&manager,"profile_editor_save",None,json!({"document":document,"expectedRevision":1,"assignments":[]}));
        assert_eq!(saved["ok"],true,"{saved}");
        let applied=request(&manager,"profile_apply",None,json!({"id":document.id,"expectedRevision":2}));
        assert_eq!(applied["ok"],true,"{applied}");
        let owner=applied["state"]["profiles"]["profileSession"]["owner"].clone();
        let page=request(&manager,"profile_runtime_inspect",None,json!({"owner":owner}));assert_eq!(page["ok"],true,"{page}");
        let page=&page["state"]["profileInspection"];assert_eq!(page["total"],2);assert_eq!(page["owner"],owner);assert_eq!(page["appliedSnapshot"]["revision"],2);
        assert_eq!(page["ports"][0]["sample"],json!({"status":"known","value":{"kind":"scalar","value":true}}));
        assert_eq!(page["ports"][0]["observedUs"],"0");
        let mut edited=saved["state"]["profiles"]["document"].clone();edited["processingNodes"][0]["operation"]["value"]=json!(false);
        assert_eq!(request(&manager,"profile_editor_save",None,json!({"document":edited,"expectedRevision":2,"assignments":[]}))["ok"],true);
        let old=request(&manager,"profile_runtime_inspect",None,json!({"owner":owner,"nodeId":"display","limit":1}));assert_eq!(old["ok"],true,"{old}");
        assert_eq!(old["state"]["profileInspection"]["appliedSnapshot"]["revision"],2);
        assert_eq!(old["state"]["profileInspection"]["ports"][0]["sample"]["value"]["value"],true);
        for fields in [json!({"owner":owner,"limit":65}),json!({"owner":owner,"nodeId":"missing"}),json!({"owner":owner,"drive":true})] {assert_eq!(request(&manager,"profile_runtime_inspect",None,fields)["ok"],false);}
        let mut wrong=owner.clone();wrong["generation"]=json!(999);assert_eq!(request(&manager,"profile_runtime_inspect",None,json!({"owner":wrong}))["ok"],false);
        assert_eq!(request(&manager,"profile_stop",None,json!({}))["ok"],true);
        assert_eq!(request(&manager,"profile_runtime_inspect",None,json!({"owner":owner}))["ok"],false);
    }

    #[test]
    fn native_profile_acquires_can_inputs_and_disarms_changed_routes_without_output_writes() {
        use osg_profiles::{Port,Resource,ResourceKind,ProcessingNode,Direction,Binding,Endpoint};
        let(manager,provider)=setup();
        assert_eq!(request(&manager,"connect",Some(FIRST),json!({"port":"COM1"}))["ok"],true);
        let adapter=provider.adapters.lock().unwrap()[0].clone();
        let route=json!([1,5,3,2]);
        let events=json!({"ok":true,"schemaVersion":1,"stream":7,"head":0,"oldest":1,"overflow":0,"buttonCount":1,"held":[false],"contactButtonCount":1,"maintainedButtons":[0],"encoderButtons":[],"selectorButtons":[],"axisCount":0,"axes":[],"axisNeutral":[],"axisUsage":[],"axisAvailable":[],"sampledAtMs":10,"route":route,"configurationGeneration":4,"events":[]});
        {
            let mut state=adapter.state.lock().unwrap();
            state["epoch"]=json!(1);state["canProtocol"]=json!(3);
            state["modules"]=json!([{"id":"module","name":"Panel","online":true,"node":5,"lease":3,"boot":2,"configurationGeneration":4,"canConfigure":false}]);
            state["moduleEvents"]=json!({"module":events});
            state["moduleActions"]=json!({"module":{"status":"unsupported","route":route,"configurationGeneration":4}});
        }
        manager.profile_resources.refresh_native().unwrap();
        let listed=manager.profile_resources.request(&json!({"command":"profile_resources_list","offset":0,"limit":128})).unwrap();
        let resource=listed["resources"].as_array().unwrap().iter().find(|r|r["resourceType"]=="osg-module").expect("actual CAN resource");
        let details=manager.profile_resources.request(&json!({"command":"profile_resources_inspect","resourceId":resource["id"],"offset":0,"limit":128})).unwrap();
        let observed=details["ports"].as_array().unwrap().iter().find(|p|p["descriptor"]["id"]=="button:0").unwrap();
        let contract:osg_profiles::PortContract=serde_json::from_value(observed["descriptor"]["contract"].clone()).unwrap();
        let mut document=osg_profiles::default_document();
        document.resources.push(Resource{id:"panel".into(),name:"Panel".into(),kind:ResourceKind::Device,required:true,ports:vec![Port{id:"pressed".into(),name:"Pressed".into(),direction:Direction::Output,contract:contract.clone()}]});
        document.processing_nodes.push(ProcessingNode{id:"invert".into(),name:"Readout".into(),operation:osg_mapping::program::Operation::Not{},inputs:vec![Port{id:"input".into(),name:"Input".into(),direction:Direction::Input,contract:contract.clone()}],outputs:vec![Port{id:"output".into(),name:"Output".into(),direction:Direction::Output,contract}]});
        document.bindings.push(Binding{id:"to-readout".into(),from:Endpoint{node_id:"panel".into(),port_id:"pressed".into()},to:Endpoint{node_id:"invert".into(),port_id:"input".into()}});
        let assignments=json!([{"resourceId":"panel","sourceId":resource["id"],"ports":[{"profilePortId":"pressed","observedPortId":"button:0","expectedContractHash":observed["contractHash"]}]}]);
        assert_eq!(request(&manager,"profile_editor_save",None,json!({"document":document,"expectedRevision":1,"assignments":assignments}))["ok"],true);
        let applied=request(&manager,"profile_apply",None,json!({"id":document.id,"expectedRevision":2}));assert_eq!(applied["ok"],true,"{applied}");
        let owner=applied["state"]["profiles"]["profileSession"]["owner"].clone();
        {
            let mut state=adapter.state.lock().unwrap();let events=&mut state["moduleEvents"]["module"];
            events["head"]=json!(1);events["held"]=json!([true]);events["sampledAtMs"]=json!(11);events["events"]=json!([{"sequence":1,"control":0,"pressed":true}]);
        }
        let deadline=std::time::Instant::now()+Duration::from_secs(2);
        loop {
            manager.profile_session.lock().unwrap().poll().unwrap();
            let inspected=request(&manager,"profile_runtime_inspect",None,json!({"owner":owner,"nodeId":"panel"}));assert_eq!(inspected["ok"],true,"{inspected}");
            if inspected["state"]["profileInspection"]["ports"][0]["sample"]["value"]["value"]==true {break;}
            assert!(std::time::Instant::now()<deadline,"{inspected}");std::thread::sleep(Duration::from_millis(10));
        }
        adapter.state.lock().unwrap()["modules"][0]["boot"]=json!(99);
        let deadline=std::time::Instant::now()+Duration::from_secs(2);
        loop {
            manager.profile_session.lock().unwrap().poll().unwrap();
            let status=request(&manager,"profile_status",None,json!({}));
            if status["state"]["profiles"]["profileSession"]["state"]=="degraded" {break;}
            assert!(std::time::Instant::now()<deadline,"{status}");std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(request(&manager,"profile_stop",None,json!({}))["ok"],true);
        assert!(adapter.calls.lock().unwrap().iter().all(|r|matches!(r["command"].as_str(),Some("connect"|"events_state"|"events_read"))));
    }

    #[test]
    fn profile_pause_latches_and_legacy_stop_stops_the_profile() {
        let (manager, _) = setup();
        let fields=json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1});
        assert_eq!(request(&manager,"profile_apply",None,fields.clone())["ok"],true);
        assert_eq!(request(&manager,"profile_pause",None,json!({"paused":true}))["ok"],true);
        assert_eq!(request(&manager,"profile_selection_mode",None,json!({"mode":"automatic"}))["ok"],true);
        assert_eq!(request(&manager,"profile_select",None,json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"selectionSource":"automatic"}))["ok"],true);
        assert_eq!(request(&manager,"profile_apply",None,fields.clone())["ok"],false);
        assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["session"]["state"],"paused");
        assert_eq!(request(&manager,"profile_pause",None,json!({"paused":false}))["ok"],true);
        assert_eq!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["session"]["state"],"stopped");
        assert_eq!(request(&manager,"profile_apply",None,fields)["ok"],true);
        let _=request(&manager,"mapping_stop",None,json!({}));
        let status=request(&manager,"profile_status",None,json!({}));
        assert_eq!(status["state"]["profiles"]["session"]["state"],"stopped");
        assert!(status["state"]["profiles"]["profileSession"]["owner"].is_null());
    }

    #[test]
    fn profile_runtime_rejects_forged_authority_and_replay_flags() {
        let (manager, _) = setup();
        for (command,fields) in [
            ("profile_apply",json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"owner":{"generation":5}})),
            ("profile_apply",json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"gameId":"dcs","processIdentity":{"pid":1}})),
            ("profile_game_launch",json!({"gameId":"dcs","profileId":osg_profiles::DEFAULT_PROFILE_ID})),
            ("profile_game_launch",json!({"gameId":"dcs","processIdentity":{"pid":1}})),
            ("profile_game_launch",json!({"gameId":"unknown"})),
            ("profile_game_launch",json!({})),
            ("profile_stop",json!({"replay":true})),
            ("profile_recover",json!({"rehide":true})),
            ("profile_pause",json!({"paused":"false"})),
        ] {assert_eq!(request(&manager,command,None,fields)["ok"],false,"{command}");}
        assert!(request(&manager,"profile_status",None,json!({}))["state"]["profiles"]["profileSession"]["owner"].is_null());
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
    include!("../../tests/fixtures/profiles/editor-boundary.rs");
    #[test]
    fn atomic_editor_save_accepts_legal_combined_payload_without_device_discovery() {
        let (manager, provider) = setup();
        let mut payload = editor_boundary_request();
        payload["clientId"] = json!(CLIENT);
        assert!(payload.to_string().len() > PROFILE_REQUEST_LIMIT);
        assert!(payload.to_string().len() < PROFILE_EDITOR_REQUEST_LIMIT);
        let result = manager.request(payload.clone());
        assert_eq!(result["ok"], true, "{}", result["error"]);
        assert_eq!(result["state"]["profiles"]["assignments"].as_array().unwrap().len(), 16);
        assert_eq!(result["state"]["profiles"]["document"]["revision"], 1);
        assert!(provider.adapters.lock().unwrap().is_empty());
        payload["command"] = json!("profile_import");
        assert_eq!(manager.request(payload)["error"], "Invalid gateway request.");
        assert_eq!(request(&manager, "profile_editor_save", None, json!({"payload":"x".repeat(PROFILE_EDITOR_REQUEST_LIMIT)}))["error"], "Invalid gateway request.");
    }
    #[test]
    fn graph_compilation_and_paged_inspection_never_select_save_or_open_a_device() {
        let (manager, provider) = setup();
        let created = request(&manager, "profile_create", None, json!({"name":"Flight"}));
        let mut document = created["state"]["profiles"]["document"].clone();
        document["name"] = json!("Unsaved draft name");
        let compiled = request(&manager, "profile_compile", None, json!({"document":document}));
        assert_eq!(compiled["ok"], true, "{compiled}");
        let preview = &compiled["state"]["profileCompilation"];
        assert_eq!(preview["profileId"], document["id"]);
        assert_eq!(preview["profileRevision"], 1);
        let inspected = request(&manager, "profile_compilation_inspect", None,
            json!({"compilationId":preview["compilationId"],"section":"sources","offset":0,"limit":32}));
        assert_eq!(inspected["ok"], true);
        assert_eq!(inspected["state"]["profileCompilation"]["items"], json!([]));
        let saved = request(&manager, "profile_get", None, json!({"id":document["id"]}));
        assert_eq!(saved["state"]["profiles"]["document"]["name"], "Flight");
        let mut invalid_draft = document.clone();
        invalid_draft["bindings"] = json!([{"id":"missing-binding","from":{"nodeId":"missing-source","portId":"value"},"to":{"nodeId":"missing-sink","portId":"value"}}]);
        let invalid_preview = request(&manager, "profile_compile", None, json!({"document":invalid_draft}));
        assert_eq!(invalid_preview["ok"], true, "{invalid_preview}");
        assert_eq!(invalid_preview["state"]["profileCompilation"]["readiness"], "invalid");
        assert!(invalid_preview["state"]["profileCompilation"]["programHash"].is_null());
        let renamed = request(&manager, "profile_rename", None, json!({"id":document["id"],"expectedRevision":1,"name":"Saved name"}));
        assert_eq!(renamed["ok"], true);
        let stale = request(&manager, "profile_compile", None, json!({"document":document}));
        assert_eq!(stale["ok"], false);
        assert!(stale["error"].as_str().unwrap().contains("revision changed"));
        // An old preview remains an explicit capture of its old revision.
        let old = request(&manager, "profile_compilation_inspect", None,
            json!({"compilationId":preview["compilationId"],"section":"order"}));
        assert_eq!(old["state"]["profileCompilation"]["profileRevision"], 1);
        let assignments = request(&manager, "profile_assignments_get", None, json!({"id":document["id"]}));
        assert_eq!(assignments["state"]["profiles"]["assignments"], json!([]));
        let assigned = request(&manager, "profile_assignments_save", None,
            json!({"id":document["id"],"expectedRevision":2,"assignments":[]}));
        assert_eq!(assigned["ok"], true, "{assigned}");
        assert_eq!(assigned["state"]["profiles"]["document"]["revision"], 3);
        let status = request(&manager, "profile_status", None, json!({}));
        assert_eq!(status["state"]["profiles"]["session"]["selectedProfileId"], osg_profiles::DEFAULT_PROFILE_ID);
        assert_eq!(status["state"]["profiles"]["session"]["state"], "stopped");
        let draft = osg_profiles::new_document("Unsaved preview").unwrap();
        let draft_preview = request(&manager,"profile_compile",None,json!({"document":draft,"assignments":[]}));
        assert_eq!(draft_preview["ok"],true,"{draft_preview}");
        assert_eq!(draft_preview["state"]["profileCompilation"]["profileId"],draft.id);
        assert_eq!(request(&manager,"profile_get",None,json!({"id":draft.id}))["ok"],false);
        let rejected_pins = request(&manager,"profile_compile",None,json!({"document":draft,"assignments":[{"resourceId":"unknown","sourceId":"device","ports":[]}]}));
        assert_eq!(rejected_pins["ok"],false);
        assert!(provider.adapters.lock().unwrap().is_empty());
        assert!(manager.sessions.lock().unwrap().is_empty());
        assert!(manager.companion_host.resource_transport("missing").is_none());
        manager.shutdown();
        assert_eq!(request(&manager, "profile_compile", None, json!({"document":document}))["ok"], false);
    }
    #[test]
    fn compiler_authority_can_only_come_from_the_native_capture() {
        let (manager, provider) = setup();
        for extra in [json!({"snapshot":{}}), json!({"backend":{"available":true,"qualified":true}}),
            json!({"resourceTokens":[]}), json!({"gameContext":{"gameId":"dcs"}})] {
            let mut fields = json!({"document":osg_profiles::default_document()});
            for (key, value) in extra.as_object().unwrap() { fields[key] = value.clone(); }
            let rejected = request(&manager, "profile_compile", None, fields);
            assert_eq!(rejected["ok"], false);
            assert!(rejected["error"].as_str().unwrap().contains("Unknown compiler request field"));
        }
        assert!(provider.adapters.lock().unwrap().is_empty());
        assert!(manager.sessions.lock().unwrap().is_empty());
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
        let state = Arc::get_mut(&mut manager).unwrap();
        state.module_usb = Arc::new(crate::module_usb_fleet::tests::manager_fixture());
        state.profile_panels = Arc::new(ProfilePanelIo::new(state.sessions.clone(),state.module_usb.clone(),state.configuration_gate.clone(),Arc::new(AtomicBool::new(false))));
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
    fn native_profile_and_browser_claims_use_the_same_configuration_gate() {
        let (manager,_,usb) = configuration_setup();
        let start = Arc::new(std::sync::Barrier::new(3));
        let native = manager.profile_panels.clone();
        let native_start = start.clone();
        let route = serde_json::from_value(usb["snapshot"]["route"].clone()).unwrap();
        let key = usb["key"].as_str().unwrap().to_owned();
        let direct = std::thread::spawn(move || { native_start.wait(); native.usb(&key,crate::module_usb::UsbRequest::Claim {route,client_id:CLIENT.into()}).is_ok() });
        let browser = manager.clone();
        let browser_start = start.clone();
        let module = usb["snapshot"]["identity"]["moduleId"].clone();
        let can = std::thread::spawn(move || { browser_start.wait(); request(&browser,"claim",Some(FIRST),json!({"moduleId":module}))["ok"]==true });
        start.wait();
        assert_ne!(can.join().unwrap(),direct.join().unwrap());
        assert_eq!(manager.configuration_claims().len()+manager.module_usb.configuration_claims().len(),1);
    }
    #[test]
    fn manager_shutdown_closes_native_panel_io_after_owned_cleanup_phase() {
        let (manager,_,usb) = configuration_setup();
        manager.closed.store(true,Ordering::Release);
        // Public admission closes first, but private coordinator cleanup still
        // needs current route reads and lease/configuration releases.
        assert_eq!(manager.profile_panels.can(json!({"command":"status","gatewayId":FIRST}))["ok"],true);
        manager.shutdown();
        assert_eq!(manager.profile_panels.can(json!({"command":"status","gatewayId":FIRST}))["ok"],false);
        assert!(manager.profile_panels.usb(usb["key"].as_str().unwrap(),crate::module_usb::UsbRequest::ConfigRead{route:serde_json::from_value(usb["snapshot"]["route"].clone()).unwrap()}).is_err());
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
    fn rejected_gateway_selection_does_not_deadlock_native_dcs_preparation() {
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
        // Qualify the retained native adapter's lock behavior directly. Its
        // retired independent configuration/start IPC is separately rejected.
        manager
            .dcs
            .request(&json!({"command":"dcs_configure","dcsConfig":config}))
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        let pending_manager = manager.clone();
        let started = sender.clone();
        let start = std::thread::spawn(move || {
            let result = pending_manager.dcs.request(&json!({"command":"dcs_start"}));
            started.send(json!({"ok":result.is_ok()})).unwrap();
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
    fn independent_provider_mutations_cannot_bypass_profile_context_with_forged_browser_fields() {
        let (manager, provider) = setup();
        for command in [
            "dcs_configure",
            "dcs_start",
            "integration_save",
            "integration_validate",
            "integration_start",
            "integration_override",
            "integration_simulate",
        ] {
            let reply = request(
                &manager,
                command,
                None,
                json!({"contextSession":"forged","profileOwner":{"session":"browser"},"processIdentity":{"pid":1,"epoch":1,"path":"DCS.exe"}}),
            );
            assert_eq!(reply["ok"], false, "{command}");
            assert!(
                reply["error"].as_str().unwrap().contains("applied profile"),
                "{command}: {reply}"
            );
            assert_eq!(reply["state"]["integration"]["running"], false);
            assert_eq!(reply["state"]["dcsBios"]["running"], false);
        }
        assert!(provider.adapters.lock().unwrap().is_empty());
        assert_eq!(
            request(&manager, "integration_get_state", None, json!({}))["ok"],
            true
        );
        assert_eq!(
            request(&manager, "integration_stop", None, json!({}))["ok"],
            true
        );
        manager.shutdown();
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
