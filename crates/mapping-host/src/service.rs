//! The preview clock runs independently of UI request timing.
use crate::router::Router;
use osg_controller_input::{stream, ControllerService};
use osg_mapping::{
    engine::{Assignment, Change, Counter, RunToken, Source},
    profile::Origin,
    store::Store,
};
use osg_virtual_controller::vjoy::Slot;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
};
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub const QUALIFICATION: &str = if osg_controller_input::visibility::capabilities::WINDOWS_NATIVE_OUTPUTS {
    osg_controller_input::visibility::capabilities::WINDOWS_REASON
} else {
    "Live vJoy output and layout changes remain disabled until Windows validation is complete. You can use recording previews and inspect installed devices."
};
#[cfg(target_os = "linux")]
pub const QUALIFICATION: &str = osg_virtual_controller::uinput::REASON;
#[cfg(target_os = "macos")]
pub const QUALIFICATION: &str = osg_virtual_controller::apple_helper::REASON;
pub const BACKEND: &str = osg_virtual_controller::entry::BACKEND;
pub const SCRIPT_LIVE_QUALIFIED: bool = false;
/// Shared activation gate for every native coordinator using this backend.
pub const LIVE_OUTPUT_QUALIFIED: bool = osg_controller_input::visibility::capabilities::WINDOWS_NATIVE_OUTPUTS;
fn native_readiness() -> Value {
    #[cfg(target_os = "linux")]
    {
        osg_virtual_controller::linux_uinput::readiness()
    }
    #[cfg(target_os = "macos")]
    {
        osg_virtual_controller::apple_helper::readiness()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        json!({})
    }
}
pub type Inspector = Arc<dyn Fn() -> Result<Vec<Slot>, String> + Send + Sync>;
pub struct Service {
    #[cfg(windows)]
    provisioning: Mutex<osg_virtual_controller::windows_jobs::Manager>,
    router: Arc<Mutex<Router>>,
    capture: Arc<ControllerService>,
    panels: Mutex<Option<Arc<dyn crate::panels::Provider>>>,
    companions: Mutex<Option<Arc<crate::companions::CompanionProvider>>>,
    companion_blueprints: Mutex<Option<companion_runs::BlueprintProvider>>,
    companion_runs: Arc<Mutex<companion_runs::Manager>>,
    root: Option<PathBuf>,
    inspector: Inspector,
    output_factory: osg_virtual_controller::session::Factory,
    qualified: bool,
    script_live_qualified: bool,
    script_factory: Arc<dyn Fn(&str) -> Result<osg_logic::worker::Worker, String> + Send + Sync>,
    game_visibility: crate::router::VisibilityCall,
    stop: Arc<AtomicBool>,
    lifecycle: AtomicU64,
    #[cfg(test)]
    before_start: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    persistence: Mutex<()>,
    compilation: Mutex<()>,
    game_lease: Mutex<Option<game::Lease>>,
    settings: Mutex<crate::settings::Settings>,
}
impl Service {
    pub fn new(
        capture: Arc<ControllerService>,
        root: Option<PathBuf>,
        inspector: Inspector,
    ) -> Self {
        Self::with_output(
            capture,
            root,
            inspector,
            osg_virtual_controller::entry::factory(),
            LIVE_OUTPUT_QUALIFIED,
        )
    }
    fn with_output(
        capture: Arc<ControllerService>,
        root: Option<PathBuf>,
        inspector: Inspector,
        output_factory: osg_virtual_controller::session::Factory,
        qualified: bool,
    ) -> Self {
        let router = Arc::new(Mutex::new(Router::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let state = router.clone();
        let input = capture.clone();
        let stopped = stop.clone();
        let companion_runs = Arc::new(Mutex::new(companion_runs::Manager::new(root.clone())));
        let worker = std::thread::Builder::new()
            .name("mapping-preview".into())
            .spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    {
                        if let Ok(mut router) = state.try_lock() {
                            if router.runtime.token().is_ok() {
                                let (session, cursor) = router.cursor();
                                let page = input.stream_page(session, cursor);
                                let _ = router.pump(page);
                            }
                            let _ = router.flush_output();
                            if let Some(learner) = router.learner.as_mut() {
                                let page = input.stream_page(&learner.session, learner.cursor);
                                learner.pump(page);
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                state.lock().unwrap_or_else(|p| p.into_inner()).stop();
            })
            .ok();
        if worker.is_none() {
            stop.store(true, Ordering::Release);
        }
        let settings =
            crate::settings::Settings::open(root.as_ref().map(|p| p.join("local-settings.json")));
        let visibility_capture = capture.clone();
        Self {
            game_visibility: Arc::new(move |operation, value| {
                visibility_capture.game_visibility(operation, value)
            }),
            #[cfg(windows)]
            provisioning: Mutex::new(osg_virtual_controller::windows_jobs::Manager::default()),
            panels: Mutex::new(None),
            companions: Mutex::new(None),
            companion_blueprints: Mutex::new(None),
            companion_runs,
            settings: Mutex::new(settings),
            router,
            capture,
            root,
            inspector,
            output_factory,
            qualified,
            script_live_qualified: SCRIPT_LIVE_QUALIFIED,
            script_factory: Arc::new(|source| {
                osg_logic::worker::Worker::spawn(
                    &std::env::current_exe().map_err(|e| e.to_string())?,
                    source,
                )
            }),
            stop,
            lifecycle: AtomicU64::new(0),
            #[cfg(test)]
            before_start: Mutex::new(None),
            worker: Mutex::new(worker),
            persistence: Mutex::new(()),
            compilation: Mutex::new(()),
            game_lease: Mutex::new(None),
        }
    }
    pub fn set_panel_provider(&self, provider: Arc<dyn crate::panels::Provider>) {
        *self.panels.lock().unwrap_or_else(|p| p.into_inner()) = Some(provider);
    }
    fn panel_provider(&self) -> Result<Arc<dyn crate::panels::Provider>, String> {
        self.panels
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .ok_or("Panel event capture is unavailable.".into())
    }
    pub fn set_companion_provider(&self, provider: Arc<crate::companions::CompanionProvider>) {
        *self.companions.lock().unwrap_or_else(|p| p.into_inner()) = Some(provider);
    }
    pub fn set_companion_blueprint_provider(&self, provider: companion_runs::BlueprintProvider) {
        *self
            .companion_blueprints
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(provider);
    }
    fn companion_provider(&self) -> Result<Arc<crate::companions::CompanionProvider>, String> {
        self.companions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .ok_or("Companion input service is unavailable.".into())
    }
    fn prepare_companions(
        &self,
        profile: &osg_mapping::profile::Profile,
        assignments: &[Assignment],
    ) -> Result<Option<crate::companions::Prepared>, String> {
        if assignments
            .iter()
            .any(|a| a.source.origin == Origin::Companion)
        {
            Ok(Some(
                self.companion_provider()?.prepare(profile, assignments)?,
            ))
        } else {
            Ok(None)
        }
    }
    fn recovery_path(&self) -> Result<PathBuf, String> {
        self.root
            .as_ref()
            .map(|root| root.join("output-recovery.json"))
            .ok_or("Local storage is required for live output recovery.".into())
    }
    pub fn provisioning_active(&self) -> bool {
        #[cfg(windows)]
        {
            return self
                .provisioning
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .status()
                .is_some_and(|s| s.active);
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    #[cfg(windows)]
    fn provisioning_recovery(&self) -> Value {
        match osg_virtual_controller::windows_provision::NativeBackend::native(
            "00000000-0000-0000-0000-000000000000",
        )
        .and_then(|backend| backend.pending())
        {
            Ok(pending) => json!({"pending":pending}),
            Err(error) => json!({"pending":[],"error":error.chars().take(512).collect::<String>()}),
        }
    }
    fn recovery_status(&self) -> Value {
        match self
            .recovery_path()
            .and_then(|path| osg_virtual_controller::output_journal::inspect(&path))
        {
            Ok(Some(record)) if record.pending() => {
                json!({"required":true,"phase":record.phase,"token":record.token,"slots":record.bindings.iter().map(|b| b.slot).collect::<Vec<_>>()})
            }
            Ok(_) => json!({"required":false}),
            Err(error) => json!({"required":true,"error":error}),
        }
    }
    pub fn handles(command: &str) -> bool {
        matches!(
            command,
            "mapping_capture"
                | "mapping_panel_sources"
                | "mapping_panel_inspect"
                | "mapping_companion_sources"
                | "mapping_companion_inspect"
                | "mapping_companion_route"
                | "mapping_script_validate"
                | "mapping_trace_export"
                | "mapping_status"
                | "mapping_learn_start"
                | "mapping_preview_start"
                | "mapping_stop"
                | "mapping_recording"
                | "mapping_simulate"
                | "mapping_assignments_save"
                | "mapping_assignments_get"
                | "mapping_virtual_inspect"
                | "mapping_virtual_start"
                | "mapping_virtual_create"
                | "mapping_virtual_recover"
                | "mapping_virtual_acknowledge"
                | "mapping_provision_cancel"
                | "mapping_provision_recover"
                | "mapping_virtual_enable"
                | "mapping_targets_save"
                | "mapping_targets_get"
        )
    }
    fn store(&self) -> Result<Store, String> {
        self.root
            .clone()
            .map(Store::new)
            .ok_or_else(|| "Mapping storage is unavailable.".into())
    }
    pub fn request(&self, request: &Value) -> Result<Value, String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Mapping service has stopped.".into());
        }
        if !request.is_object() || request.to_string().len() > 65_536 {
            return Err("Invalid mapping request.".into());
        }
        let command = request["command"]
            .as_str()
            .ok_or("Missing mapping command.")?;
        let profile_id = || {
            request["profileId"]
                .as_str()
                .ok_or_else(|| "Select a saved mapping profile.".to_string())
        };
        match command {
            "mapping_companion_route" => self.companion_request(&request["companion"]),
            "mapping_capture" => self
                .capture_saved(
                    profile_id()?,
                    request.get("visibility").filter(|v| !v.is_null()).cloned(),
                )
                .map(|capture| json!({"capture":capture})),
            "mapping_panel_sources" => self.panel_provider()?.list(),
            "mapping_companion_sources" => self.companion_sources(),
            "mapping_companion_inspect" => self.companion_provider()?.inspect(
                request["sourceId"]
                    .as_str()
                    .ok_or("Choose a companion source.")?,
            ),
            "mapping_panel_inspect" => self.panel_provider()?.inspect(
                request["sourceId"]
                    .as_str()
                    .ok_or("Choose a panel event source.")?,
            ),
            "mapping_script_validate" => {
                let _compile = self
                    .compilation
                    .try_lock()
                    .map_err(|_| "A script compilation is already running.")?;
                let source = request["sourceCode"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 16384)
                    .ok_or("Script source must contain 1–16384 bytes.")?;
                let worker = (self.script_factory)(source)?;
                Ok(json!({"compiled":true,"sourceHash":worker.source_hash}))
            }

            "mapping_trace_export" => {
                let router = self.router.lock().unwrap_or_else(|p| p.into_inner());
                if router.runtime.token().is_ok() {
                    return Err("Stop mapping before exporting its completed trace.".into());
                }
                let bytes = router.runtime.trace.export()?;
                let offset = request["offset"]
                    .as_u64()
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or("Trace offset required.")?;
                if offset > bytes.len() {
                    return Err("Invalid trace offset.".into());
                }
                let end = (offset + 8192).min(bytes.len());
                Ok(json!({"bytes":&bytes[offset..end],"next":end,"total":bytes.len()}))
            }

            "mapping_learn_start" => {
                let id = request["sourceId"]
                    .as_str()
                    .ok_or("Choose a physical source to learn.")?;
                let snapshot = self.capture.stream_snapshot(&[id.into()])?;
                let learner = crate::learn::Learn::new(snapshot)?;
                let mut router = self.router.lock().unwrap_or_else(|p| p.into_inner());
                if router.runtime.token().is_ok() || router.status()["recoveryRequired"] == true {
                    return Err(
                        "Stop mapping and resolve output recovery before learning input.".into(),
                    );
                }
                router.learner = Some(learner);
                Ok(json!({"learning":router.learner.as_ref().map(|l|l.status())}))
            }
            "mapping_status" => {
                let mut status = self
                    .router
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .status();
                {
                    let router = self.router.lock().unwrap_or_else(|p| p.into_inner());
                    status["learning"] = json!(router.learner.as_ref().map(|l| l.status()));
                    if request["inspect"].as_bool() == Some(true) {
                        let offset = request.get("offset").map_or(Ok(0), |v| {
                            v.as_u64()
                                .filter(|v| *v <= 256)
                                .ok_or("Invalid inspector page.")
                        })? as usize;
                        status["inspector"] = router
                            .runtime
                            .engine()
                            .map(|e| e.inspect(offset))
                            .unwrap_or(json!(null));
                        if let Some(nodes) = status["inspector"]
                            .get_mut("nodes")
                            .and_then(Value::as_array_mut)
                        {
                            let ids = nodes
                                .iter()
                                .flat_map(|n| n["inputs"].as_array().into_iter().flatten())
                                .filter(|i| i["native"]["source"]["origin"] == "host-controller")
                                .filter_map(|i| i["native"]["source"]["id"].as_str())
                                .map(str::to_owned)
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect::<Vec<_>>();
                            if !ids.is_empty() {
                                if let Ok(snapshot) = self.capture.stream_snapshot(&ids) {
                                    for node in nodes {
                                        if let Some(inputs) = node["inputs"].as_array_mut() {
                                            for input in inputs {
                                                if let Some(source) =
                                                    snapshot.sources.iter().find(|s| {
                                                        Some(s.id.as_str())
                                                            == input["native"]["source"]["id"]
                                                                .as_str()
                                                    })
                                                {
                                                    if let Some(value) = input["native"]["control"]
                                                        .as_str()
                                                        .and_then(|id| {
                                                            source.baseline.values.get(id)
                                                        })
                                                    {
                                                        input["raw"] = json!(value);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
                status["storageError"] = json!(settings.error);
                status["outputRecovery"] = self.recovery_status();
                #[cfg(windows)]
                {
                    status["provisioning"] = json!(self
                        .provisioning
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .status());
                    status["provisioningRecovery"] = self.provisioning_recovery();
                    status["provisioningQualified"] = json!(
                        self.qualified && osg_virtual_controller::windows_launcher::QUALIFIED
                    );
                }
                status["virtualDevices"] = json!({"backend":BACKEND,"enabled":settings.value.virtual_devices,"state":"setup-required","qualified":self.qualified,"reason":QUALIFICATION});
                status["deviceHiding"] = json!({"enabled":settings.value.device_hiding});
                Ok(status)
            }
            "mapping_virtual_enable" => {
                let enabled = request["enabled"]
                    .as_bool()
                    .ok_or("Enablement must be an explicit boolean.")?;
                let stop_error = if enabled {
                    if !cfg!(any(target_os = "linux", target_os = "macos")) {
                        (self.inspector)()?;
                    }
                    None
                } else {
                    let mut router = self.router.lock().unwrap_or_else(|p| p.into_inner());
                    if router.live() {
                        router.stop_result().err()
                    } else {
                        None
                    }
                };
                self.settings
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .enable_virtual(enabled)?;
                if !enabled {
                    companion_runs::stop_all(&self.companion_runs, true);
                }
                if let Some(error) = stop_error {
                    return Err(error);
                }
                Ok(json!({"enabled":enabled,"qualified":self.qualified,"reason":QUALIFICATION}))
            }
            "mapping_targets_save" => {
                let profile = self.store()?.load(profile_id()?)?;
                let bindings: Vec<osg_virtual_controller::vjoy::Binding> =
                    serde_json::from_value(request["bindings"].clone())
                        .map_err(|e| e.to_string())?;
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                for binding in &bindings {
                    osg_virtual_controller::uinput::Template::new(binding.clone())?;
                }
                self.settings
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .assign_targets(&profile, bindings)?;
                Ok(json!({"saved":true}))
            }
            "mapping_targets_get" => {
                let profile = self.store()?.load(profile_id()?)?;
                let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
                Ok(json!({"bindings":settings.value.targets.get(&profile.id)}))
            }
            "mapping_assignments_save" => {
                let _guard = self.persistence.lock().unwrap_or_else(|p| p.into_inner());
                let assignments: Vec<Assignment> =
                    serde_json::from_value(request["assignments"].clone())
                        .map_err(|e| e.to_string())?;
                self.store()?
                    .save_assignments(profile_id()?, &assignments)?;
                Ok(json!({"saved":true}))
            }
            "mapping_assignments_get" => {
                let all = self.store()?.load_assignments(profile_id()?)?;
                let offset = match request.get("offset") {
                    None => 0,
                    Some(value) => value
                        .as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .ok_or("Invalid assignment cursor.")?,
                };
                // One role's controls are paged separately so maximum-size assignments fit.
                let role = request["role"].as_str();
                if let Some(role) = role {
                    let assignment = all
                        .iter()
                        .find(|a| a.role == role)
                        .ok_or("Unknown assigned role.")?;
                    if offset > assignment.controls.len() {
                        return Err("Invalid assignment cursor.".into());
                    }
                    Ok(
                        json!({"role":role,"source":assignment.source,"offset":offset,"total":assignment.controls.len(),"controls":assignment.controls.iter().skip(offset).take(32).collect::<Vec<_>>()}),
                    )
                } else {
                    Ok(
                        json!({"roles":all.iter().map(|a|json!({"role":a.role,"source":a.source,"controlCount":a.controls.len()})).collect::<Vec<_>>()}),
                    )
                }
            }
            "mapping_preview_start" | "mapping_virtual_start" => {
                let live = command == "mapping_virtual_start";
                if live && self.provisioning_active() {
                    return Err(
                        "Wait for provisioning and verified cleanup before starting live output."
                            .into(),
                    );
                }
                if live && !self.qualified {
                    return Err(QUALIFICATION.into());
                }
                let epoch = self
                    .lifecycle
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
                    .map_err(|_| "Mapping lifecycle counter exhausted.")?
                    + 1;
                #[cfg(test)]
                if let Some(wait) = self.before_start.lock().unwrap().take() {
                    wait();
                }
                let store = self.store()?;
                let profile = store.load(profile_id()?)?;
                let assignments = store.load_assignments(profile_id()?)?;
                // Compile outside the router/capture locks. Stop invalidates the lifecycle while this waits.
                let _compile = self
                    .compilation
                    .try_lock()
                    .map_err(|_| "A script compilation is already running.")?;
                let mut workers = std::collections::BTreeMap::new();
                if live && !profile.scripts.is_empty() && !self.script_live_qualified {
                    return Err("Live script output remains disabled pending qualification.".into());
                }
                for script in &profile.scripts {
                    if self.lifecycle.load(Ordering::Acquire) != epoch {
                        return Err(
                            "Script compilation was superseded by Stop or a newer Start.".into(),
                        );
                    }
                    workers.insert(
                        script.id.clone(),
                        (self.script_factory)(&script.source)
                            .map_err(|error| format!("Script {}: {error}", script.id))?,
                    );
                }
                let ids: Vec<String> = assignments
                    .iter()
                    .filter(|a| a.source.origin == Origin::HostController)
                    .map(|a| a.source.id.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                let mut snapshot = if ids.is_empty() {
                    let page = self.capture.stream_page("", 0);
                    stream::Snapshot {
                        session: page.session,
                        cursor: page.cursor,
                        captured_us: page.captured_us,
                        sources: Vec::new(),
                    }
                } else {
                    self.capture.stream_snapshot(&ids)?
                };
                let panels = if assignments
                    .iter()
                    .any(|a| a.source.origin == Origin::OsgPanelEvents)
                {
                    let prepared = self.panel_provider()?.prepare(&profile, &assignments)?;
                    snapshot.sources.extend(prepared.snapshots);
                    snapshot.captured_us = self.capture.stream_page("", 0).captured_us;
                    Some(prepared.feed)
                } else {
                    None
                };
                let companions = self.prepare_companions(&profile, &assignments)?;
                let mut router = self.router.lock().unwrap_or_else(|p| p.into_inner());
                if self.stop.load(Ordering::Acquire) {
                    return Err("Mapping service has stopped.".into());
                }
                if self.lifecycle.load(Ordering::Acquire) != epoch {
                    return Err("Preview start was superseded by a newer start or stop.".into());
                }
                if live {
                    self.confirm_companion_pins(&assignments)?;
                }
                let reserved_bindings = self
                    .settings
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .value
                    .targets
                    .get(&profile.id)
                    .cloned()
                    .unwrap_or_default();
                let _resources = self.reserve_manual(&profile, &reserved_bindings, live)?;
                let token = if live {
                    let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
                    if !settings.value.virtual_devices {
                        return Err("Enable virtual devices explicitly before Start.".into());
                    }
                    let bindings = settings
                        .value
                        .targets
                        .get(&profile.id)
                        .cloned()
                        .ok_or("Assign a local slot to every target before Start.")?;
                    drop(settings);
                    router.start_live_sources(
                        profile,
                        assignments,
                        snapshot,
                        bindings,
                        self.output_factory.clone(),
                        Some(self.recovery_path()?),
                        workers,
                        panels,
                        companions,
                    )?
                } else {
                    router.start_with_sources(
                        profile,
                        assignments,
                        snapshot,
                        workers,
                        panels,
                        companions,
                    )?
                };
                Ok(json!({"token":token,"preview":router.status()}))
            }
            "mapping_stop" => {
                if self
                    .lifecycle
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
                    .is_err()
                {
                    self.stop.store(true, Ordering::Release);
                }
                if let Some(lease) = self
                    .game_lease
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .as_mut()
                {
                    lease.emergency = true;
                }
                let mut r = self.router.lock().unwrap_or_else(|p| p.into_inner());
                r.stop_result()?;
                Ok(r.status())
            }
            "mapping_recording" => {
                let after: Counter =
                    serde_json::from_value(request["after"].clone()).map_err(|e| e.to_string())?;
                let r = self.router.lock().unwrap_or_else(|p| p.into_inner());
                serde_json::to_value(r.runtime.recording.page(after, 32)?)
                    .map_err(|e| e.to_string())
            }
            "mapping_simulate" => {
                let token: RunToken =
                    serde_json::from_value(request["token"].clone()).map_err(|e| e.to_string())?;
                let source: Source =
                    serde_json::from_value(request["source"].clone()).map_err(|e| e.to_string())?;
                let changes: Vec<Change> = serde_json::from_value(request["changes"].clone())
                    .map_err(|e| e.to_string())?;
                if changes.len() > 256 {
                    return Err("A preview input frame permits at most 256 controls.".into());
                }
                let mut r = self.router.lock().unwrap_or_else(|p| p.into_inner());
                r.simulate(&token, &source, changes)?;
                Ok(r.status())
            }
            "mapping_virtual_inspect" => {
                if cfg!(any(target_os = "linux", target_os = "macos")) {
                    let mut readiness = native_readiness();
                    readiness["backend"] = json!(BACKEND);
                    readiness["enabled"] = json!(
                        self.settings
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .value
                            .virtual_devices
                    );
                    readiness["slots"] = json!([]);
                    readiness["limits"] = json!({"axes":8,"buttons":32,"hats":4,"targets":8});
                    return Ok(readiness);
                }
                match (self.inspector)() {
                    Ok(slots) => Ok(
                        json!({"backend":"vjoy","enabled":self.settings.lock().unwrap_or_else(|p|p.into_inner()).value.virtual_devices,"state":"setup-required","qualified":self.qualified,"reason":QUALIFICATION,"slots":slots}),
                    ),
                    Err(error) => Ok(
                        json!({"backend":"vjoy","enabled":self.settings.lock().unwrap_or_else(|p|p.into_inner()).value.virtual_devices,"state":"unsupported","qualified":self.qualified,"reason":error.chars().take(512).collect::<String>(),"slots":[]}),
                    ),
                }
            }
            "mapping_virtual_create" | "mapping_provision_recover" => {
                if !self.qualified {
                    return Err(QUALIFICATION.into());
                }
                #[cfg(windows)]
                {
                    use osg_virtual_controller::windows_launcher::{Action, QUALIFIED};
                    if !QUALIFIED {
                        return Err(QUALIFICATION.into());
                    }
                    if command == "mapping_virtual_create"
                        && !self
                            .settings
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .value
                            .virtual_devices
                    {
                        return Err("Enable virtual devices explicitly before Create.".into());
                    }
                    let router = self.router.lock().unwrap_or_else(|p| p.into_inner());
                    if router.runtime.token().is_ok() || router.live() {
                        return Err("Stop mapping before changing controller layouts.".into());
                    }
                    if osg_virtual_controller::output_journal::inspect(&self.recovery_path()?)?
                        .is_some_and(|r| r.pending())
                    {
                        return Err(
                            "Resolve interrupted virtual output before changing layouts.".into(),
                        );
                    }
                    let (action, operation) = if command == "mapping_virtual_create" {
                        let slot = request["slot"]
                            .as_u64()
                            .and_then(|n| u8::try_from(n).ok())
                            .ok_or("Choose an unused vJoy slot.")?;
                        let layout: osg_virtual_controller::descriptor::Layout =
                            serde_json::from_value(request["layout"].clone())
                                .map_err(|_| "Choose a bounded vJoy layout.")?;
                        layout.validate()?;
                        (Action::CreateLayout { slot, layout }, None)
                    } else {
                        (
                            Action::Recover,
                            Some(
                                request["operation"]
                                    .as_str()
                                    .ok_or("Choose the recorded provisioning operation.")?
                                    .to_owned(),
                            ),
                        )
                    };
                    let status = self
                        .provisioning
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .start(action, operation)?;
                    Ok(json!({"provisioning":status}))
                }
                #[cfg(not(windows))]
                {
                    Err("vJoy provisioning requires Windows.".into())
                }
            }
            "mapping_provision_cancel" => {
                #[cfg(windows)]
                {
                    let id = request["jobId"]
                        .as_str()
                        .ok_or("A current provisioning job ID is required.")?;
                    let status = self
                        .provisioning
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .cancel(id)?;
                    Ok(json!({"provisioning":status}))
                }
                #[cfg(not(windows))]
                {
                    Err("vJoy provisioning requires Windows.".into())
                }
            }
            "mapping_virtual_acknowledge" => {
                if !self.qualified {
                    return Err(QUALIFICATION.into());
                }
                let router = self.router.lock().unwrap_or_else(|p| p.into_inner());
                if router.live() {
                    return Err("Stop live output before acknowledging recovery.".into());
                }
                let token: RunToken = serde_json::from_value(
                    request
                        .get("token")
                        .cloned()
                        .ok_or("Inspect the interrupted run first.")?,
                )
                .map_err(|_| "Invalid recovery acknowledgement.")?;
                osg_virtual_controller::output_journal::Store::open(self.recovery_path()?)?
                    .acknowledge(&token)?;
                Ok(json!({"acknowledged":true,"outputChanged":false}))
            }
            "mapping_virtual_recover" => {
                if !self.qualified {
                    return Err(QUALIFICATION.into());
                }
                let mut router = self.router.lock().unwrap_or_else(|p| p.into_inner());
                router.recover_durable(self.output_factory.clone(), self.recovery_path()?)?;
                Ok(router.status())
            }
            _ => Err("Unknown mapping host command.".into()),
        }
    }
    pub fn shutdown(&self) {
        #[cfg(windows)]
        self.provisioning
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .shutdown();
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
        self.router.lock().unwrap_or_else(|p| p.into_inner()).stop();
        companion_runs::stop_all(&self.companion_runs, false);
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown();
    }
}
#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "live_tests.rs"]
mod live_tests;

#[path = "game.rs"]
pub mod game;

#[path = "companion_runs.rs"]
pub mod companion_runs;
