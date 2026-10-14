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
pub const QUALIFICATION: &str = "Live vJoy output and layout changes remain disabled until Windows validation is complete. You can use recording previews and inspect installed devices.";
pub type Inspector = Arc<dyn Fn() -> Result<Vec<Slot>, String> + Send + Sync>;
pub struct Service {
    #[cfg(windows)]
    provisioning: Mutex<osg_virtual_controller::windows_jobs::Manager>,
    router: Arc<Mutex<Router>>,
    capture: Arc<ControllerService>,
    root: Option<PathBuf>,
    inspector: Inspector,
    output_factory: osg_virtual_controller::session::Factory,
    qualified: bool,
    stop: Arc<AtomicBool>,
    lifecycle: AtomicU64,
    #[cfg(test)]
    before_start: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    persistence: Mutex<()>,
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
            false,
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
        let worker = std::thread::Builder::new()
            .name("mapping-preview".into())
            .spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    {
                        let mut router = state.lock().unwrap_or_else(|p| p.into_inner());
                        if router.runtime.token().is_ok() {
                            let (session, cursor) = router.cursor();
                            let page = input.stream_page(session, cursor);
                            let _ = router.pump(page);
                        }
                        let _ = router.flush_output();
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
        Self {
            #[cfg(windows)]
            provisioning: Mutex::new(osg_virtual_controller::windows_jobs::Manager::default()),
            settings: Mutex::new(settings),
            router,
            capture,
            root,
            inspector,
            output_factory,
            qualified,
            stop,
            lifecycle: AtomicU64::new(0),
            #[cfg(test)]
            before_start: Mutex::new(None),
            worker: Mutex::new(worker),
            persistence: Mutex::new(()),
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
            "mapping_status"
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
            "mapping_status" => {
                let mut status = self
                    .router
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .status();
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
                status["virtualDevices"] = json!({"enabled":settings.value.virtual_devices,"state":"setup-required","qualified":self.qualified,"reason":QUALIFICATION});
                status["deviceHiding"] = json!({"enabled":settings.value.device_hiding});
                Ok(status)
            }
            "mapping_virtual_enable" => {
                let enabled = request["enabled"]
                    .as_bool()
                    .ok_or("Enablement must be an explicit boolean.")?;
                let stop_error = if enabled {
                    (self.inspector)()?;
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
                if let Some(error) = stop_error {
                    return Err(error);
                }
                Ok(json!({"enabled":enabled,"qualified":self.qualified,"reason":QUALIFICATION}))
            }
            "mapping_targets_save" => {
                let profile = self.store()?.load(profile_id()?)?;
                let bindings = serde_json::from_value(request["bindings"].clone())
                    .map_err(|e| e.to_string())?;
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
                let ids: Vec<String> = assignments
                    .iter()
                    .filter(|a| a.source.origin == Origin::HostController)
                    .map(|a| a.source.id.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                let snapshot = if ids.is_empty() {
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
                let mut router = self.router.lock().unwrap_or_else(|p| p.into_inner());
                if self.stop.load(Ordering::Acquire) {
                    return Err("Mapping service has stopped.".into());
                }
                if self.lifecycle.load(Ordering::Acquire) != epoch {
                    return Err("Preview start was superseded by a newer start or stop.".into());
                }
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
                    router.start_live_with_journal(
                        profile,
                        assignments,
                        snapshot,
                        bindings,
                        self.output_factory.clone(),
                        Some(self.recovery_path()?),
                    )?
                } else {
                    router.start(profile, assignments, snapshot)?
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
            "mapping_virtual_inspect" => match (self.inspector)() {
                Ok(slots) => Ok(
                    json!({"backend":"vjoy","enabled":self.settings.lock().unwrap_or_else(|p|p.into_inner()).value.virtual_devices,"state":"setup-required","qualified":self.qualified,"reason":QUALIFICATION,"slots":slots}),
                ),
                Err(error) => Ok(
                    json!({"backend":"vjoy","enabled":self.settings.lock().unwrap_or_else(|p|p.into_inner()).value.virtual_devices,"state":"unsupported","qualified":self.qualified,"reason":error.chars().take(512).collect::<String>(),"slots":[]}),
                ),
            },
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
