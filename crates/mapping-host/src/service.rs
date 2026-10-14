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
    router: Arc<Mutex<Router>>,
    capture: Arc<ControllerService>,
    root: Option<PathBuf>,
    inspector: Inspector,
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
            settings: Mutex::new(settings),
            router,
            capture,
            root,
            inspector,
            stop,
            lifecycle: AtomicU64::new(0),
            #[cfg(test)]
            before_start: Mutex::new(None),
            worker: Mutex::new(worker),
            persistence: Mutex::new(()),
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
                status["virtualDevices"] = json!({"enabled":settings.value.virtual_devices,"state":"setup-required","qualified":false,"reason":QUALIFICATION});
                status["deviceHiding"] = json!({"enabled":settings.value.device_hiding});
                Ok(status)
            }
            "mapping_virtual_enable" => {
                let enabled = request["enabled"]
                    .as_bool()
                    .ok_or("Enablement must be an explicit boolean.")?;
                if enabled {
                    (self.inspector)()?;
                }
                self.settings
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .enable_virtual(enabled)?;
                Ok(json!({"enabled":enabled,"qualified":false,"reason":QUALIFICATION}))
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
            "mapping_preview_start" => {
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
                let token = router.start(profile, assignments, snapshot)?;
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
                r.stop();
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
                    json!({"backend":"vjoy","enabled":self.settings.lock().unwrap_or_else(|p|p.into_inner()).value.virtual_devices,"state":"setup-required","qualified":false,"reason":QUALIFICATION,"slots":slots}),
                ),
                Err(error) => Ok(
                    json!({"backend":"vjoy","enabled":self.settings.lock().unwrap_or_else(|p|p.into_inner()).value.virtual_devices,"state":"unsupported","qualified":false,"reason":error.chars().take(512).collect::<String>(),"slots":[]}),
                ),
            },
            "mapping_virtual_start" | "mapping_virtual_create" | "mapping_virtual_recover" => {
                Err(QUALIFICATION.into())
            }
            _ => Err("Unknown mapping host command.".into()),
        }
    }
    pub fn shutdown(&self) {
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
