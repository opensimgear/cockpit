//! Explicit IPC for local intent, read-only inspection, Apply and emergency Restore.
use super::{
    journal,
    policy::State,
    runtime::{Factory, Observe, Task, Worker},
    settings::{Intent, Settings},
};
use crate::registry::Registry;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
pub struct Service {
    launch: Mutex<super::launch::Worker>,
    suppression: super::suppression::Control,
    platform: super::capabilities::Platform,
    instances: Mutex<std::collections::BTreeMap<String, (super::session::Source, String)>>,
    admission: Mutex<()>,
    settings: Mutex<Settings>,
    worker: Mutex<Worker>,
    snapshot: Mutex<Result<Option<State>, String>>,
    registry: Arc<Mutex<Registry>>,
    path: Option<PathBuf>,
    factory: Factory,
    qualified: bool,
}
fn native() -> Result<Box<dyn super::runtime::Adapter>, String> {
    #[cfg(windows)]
    {
        Ok(Box::new(super::windows::installed()?))
    }
    #[cfg(not(windows))]
    {
        Err(
            "HidHide is a Windows dependency. This platform's visibility adapter is not qualified."
                .into(),
        )
    }
}
impl Service {
    pub fn new(path: Option<PathBuf>, registry: Arc<Mutex<Registry>>) -> Self {
        let settings = Settings::new(path.as_ref().map(|p| p.join("controller-visibility.json")));
        Self {
            launch: Mutex::new(super::launch::Worker::default()),
            suppression: Default::default(),
            platform: super::capabilities::current(),
            instances: Mutex::new(std::collections::BTreeMap::new()),
            admission: Mutex::new(()),
            settings: Mutex::new(settings),
            worker: Mutex::new(Worker::default()),
            snapshot: Mutex::new(Ok(None)),
            registry,
            path: path.map(|p| p.join("controller-visibility-session.json")),
            factory: Arc::new(native),
            qualified: false,
        }
    }
    pub fn with_suppression(mut self, control: super::suppression::Control) -> Self {
        self.suppression = control;
        self
    }
    pub fn active(&self) -> bool {
        self.launch
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .active()
            || self.suppression.active()
            || self
                .worker
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .active()
    }
    pub fn shutdown(&self) {
        self.launch
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .shutdown();
        let _ = self.suppression.stop(None);
        self.worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .shutdown();
    }
    fn status(&self, id: &str) -> Value {
        let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        let recovery = self.path.as_ref().map_or(Ok(None), |p| journal::inspect(p));
        let recovery = match recovery {
            Ok(Some(record)) if !record.clean => {
                json!({"required":true,"id":record.id,"instances":record.plan.selections.iter().map(|s|&s.instance).collect::<Vec<_>>()})
            }
            Ok(_) => json!({"required":false}),
            Err(error) => json!({"required":true,"error":error}),
        };
        let snapshot = self.snapshot.lock().unwrap_or_else(|p| p.into_inner());
        let global = match &*snapshot {
            Ok(Some(s)) => {
                json!({"inspected":true,"active":s.active,"inverse":s.inverse,"hiddenCount":s.hidden.len(),"applicationCount":s.applications.len(),"applications":s.applications.iter().take(16).collect::<Vec<_>>()})
            }
            Ok(None) => json!({"inspected":false}),
            Err(error) => json!({"inspected":true,"error":error}),
        };
        let effective = self.instances.lock().unwrap_or_else(|p|p.into_inner()).get(id).and_then(|(source,instance)| {
            let current = self.registry.lock().unwrap_or_else(|p|p.into_inner()).visibility_sources(&[id.to_owned()]).ok()?;
            super::session::validate_sources(std::slice::from_ref(source),&current,false).ok()?;
            match &*snapshot { Ok(Some(state)) => Some(json!({"hidden":state.active && super::policy::contains(&state.hidden,instance),"configured":super::policy::contains(&state.hidden,instance),"inverse":state.inverse,"scope":"last-inspected"})), _ => None }
        });
        json!({"offset":0,"total":0,"limit":32,"visibility":{
            "platform":format!("{:?}",self.platform).to_lowercase(),"modes":super::capabilities::modes(self.platform),"enabled":settings.value.enabled,"qualified":self.qualified,"effective":effective,"selectedCount":settings.value.devices.values().filter(|p|!matches!(p,Intent::Visible)).count(),"intent":settings.value.devices.get(id).cloned().unwrap_or_default(),
            "selected":settings.value.devices.iter().filter(|(_,p)|!matches!(p,Intent::Visible)).map(|(id,_)|id).take(16).collect::<Vec<_>>(),
            "settingsError":settings.error,"global":global,"job":if self.platform==super::capabilities::Platform::Windows{json!(self.worker.lock().unwrap_or_else(|p|p.into_inner()).snapshot())}else{self.suppression.snapshot()},"recovery":recovery,"launch":self.launch.lock().unwrap_or_else(|p|p.into_inner()).snapshot(),
            "reason":if self.qualified{"Apply affects every instance of the selected executable path. Existing applications may need a qualified device-handle refresh."}else{super::capabilities::reason(self.platform)}
        }})
    }
    pub fn request(&self, request: &Value) -> Result<Value, String> {
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        let command = request["command"]
            .as_str()
            .ok_or("Missing visibility command.")?;
        let id = request["sourceId"].as_str().unwrap_or("");
        match command {
            "controller_visibility_status" => {}
            "controller_visibility_processes" => {
                #[cfg(windows)]
                {
                    let offset = request.get("offset").map_or(Ok(0), |v| {
                        v.as_u64()
                            .filter(|n| *n <= 256)
                            .map(|n| n as usize)
                            .ok_or("Invalid process page.")
                    })?;
                    let mut status = self.status(id);
                    let processes = super::windows::applications(offset)?;
                    status["visibility"]["processes"] = processes["processes"].clone();
                    status["visibility"]["nextProcess"] = processes["next"].clone();
                    return Ok(status);
                }
                #[cfg(not(windows))]
                {
                    return Err("Running executable selection is available on Windows.".into());
                }
            }

            "controller_visibility_inspect" => {
                if self.platform != super::capabilities::Platform::Windows {
                    *self.snapshot.lock().unwrap_or_else(|p| p.into_inner()) = Ok(None);
                    return Ok(self.status(id));
                }
                if self.active() {
                    return Err("Restore the active visibility session before opening another driver inspection handle.".into());
                }
                *self.snapshot.lock().unwrap_or_else(|p| p.into_inner()) = (self.factory)()
                    .and_then(|mut adapter| {
                        let state = adapter.read()?;
                        if let Ok(sources) = self
                            .registry
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .visibility_sources(&[id.to_owned()])
                        {
                            if let Ok(instance) = adapter.instance(&sources[0].interface) {
                                self.instances
                                    .lock()
                                    .unwrap_or_else(|p| p.into_inner())
                                    .insert(id.to_owned(), (sources[0].clone(), instance));
                            }
                        }
                        Ok(Some(state))
                    });
            }
            "controller_visibility_save" | "controller_visibility_enable" => {
                if self.active() {
                    return Err("Restore visibility before changing its captured settings.".into());
                }
                let mut settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
                let mut value = settings.value.clone();
                if command.ends_with("enable") {
                    value.enabled = request["enabled"]
                        .as_bool()
                        .ok_or("Explicit hiding opt-in is required.")?;
                } else {
                    self.registry
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .inspect(id, 0)?;
                    let intent: Intent = serde_json::from_value(request["intent"].clone())
                        .map_err(|_| "Invalid visibility intent.")?;
                    intent.validate()?;
                    let mode = match &intent {
                        Intent::Visible => "visible",
                        Intent::HideAll => "hide-all",
                        Intent::HideFromProcess { .. } => "hide-from-process",
                        Intent::BlockOriginalInput => "block-original-input",
                    };
                    super::capabilities::validate(self.platform, mode)?;
                    value.devices.insert(id.to_owned(), intent);
                }
                settings.save(value)?;
            }
            "controller_visibility_launch" => {
                if self.platform != super::capabilities::Platform::Linux {
                    return Err("Isolated launch is a Linux-only capability.".into());
                }
                if request.get("pid").is_some() || request.get("alreadyRunning").is_some() {
                    return Err(
                        "Already-running applications cannot enter an owned launch boundary."
                            .into(),
                    );
                }
                let launch = super::launch::Request::parse(&request["launch"])?;
                if !self
                    .launch
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .qualified()
                {
                    return Err(super::launch::REASON.into());
                }
                if self.active() {
                    return Err(
                        "Restore the active visibility session before isolated launch.".into(),
                    );
                }
                if !self
                    .settings
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .value
                    .enabled
                {
                    return Err(
                        "Explicitly enable optional visibility before isolated launch.".into(),
                    );
                }
                let ids: Vec<String> = serde_json::from_value(request["sourceIds"].clone())
                    .map_err(|_| "Choose exact local source IDs.")?;
                let sources = self
                    .registry
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .physical_sources(&ids, "linux-evdev")?;
                let registry = self.registry.clone();
                let observe = Arc::new(move |ids: &[String]| {
                    registry
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .physical_sources(ids, "linux-evdev")
                });
                self.launch
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .start(launch, sources, observe)?;
            }
            "controller_visibility_restore" => {
                if self.platform != super::capabilities::Platform::Windows {
                    let launch = self.launch.lock().unwrap_or_else(|p| p.into_inner());
                    if launch.active() {
                        launch.stop(request["jobId"].as_str())?;
                        drop(launch);
                        return Ok(self.status(id));
                    }
                    drop(launch);
                    self.suppression.stop(request["jobId"].as_str())?;
                    return Ok(self.status(id));
                }
                let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
                if worker.active() {
                    worker.stop(request["jobId"].as_str())?;
                } else {
                    if !self.qualified {
                        return Err("Persistent HidHide recovery is not qualified on this platform; no driver settings changed.".into());
                    }
                    let id = request["sessionId"]
                        .as_str()
                        .ok_or("Choose the recorded visibility session.")?
                        .to_owned();
                    worker.start(
                        Task::Recover { id },
                        self.path
                            .clone()
                            .ok_or("Local recovery storage is unavailable.")?,
                        self.factory.clone(),
                        self.observer(),
                    )?;
                }
            }
            "controller_visibility_apply" => {
                if !self.qualified {
                    return Err(format!(
                        "Apply is disabled: {}",
                        super::capabilities::reason(self.platform)
                    ));
                }
                let preferences = self
                    .settings
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .value
                    .clone();
                if !preferences.enabled {
                    return Err("Enable optional device hiding explicitly before Apply.".into());
                }
                let ids: Vec<String> = serde_json::from_value(request["sourceIds"].clone())
                    .map_err(|_| "Choose exact local source IDs.")?;
                if self.platform != super::capabilities::Platform::Windows {
                    if !matches!(
                        self.platform,
                        super::capabilities::Platform::Linux | super::capabilities::Platform::Macos
                    ) {
                        return Err("No suppression adapter.".into());
                    }
                    if ids.iter().any(|id| {
                        !matches!(
                            preferences.devices.get(id),
                            Some(Intent::BlockOriginalInput)
                        )
                    }) {
                        return Err("Only explicit broad event suppression is supported; no fallback is applied.".into());
                    }
                    let backend = if self.platform == super::capabilities::Platform::Linux {
                        "linux-evdev"
                    } else {
                        "macos-iohid"
                    };
                    let sources = self
                        .registry
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .physical_sources(&ids, backend)?;
                    self.suppression.apply(sources)?;
                    return Ok(self.status(id));
                }
                let expected = self
                    .registry
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .visibility_sources(&ids)?;
                if expected.iter().any(|s| {
                    preferences
                        .devices
                        .get(&s.id)
                        .is_none_or(|p| matches!(p, Intent::Visible))
                }) {
                    return Err("Choose saved non-visible policies before Apply.".into());
                }
                let capture = std::env::current_exe()
                    .map_err(|e| e.to_string())?
                    .to_str()
                    .ok_or("Unsupported capture executable path.")?
                    .to_owned();
                self.worker
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .start(
                        Task::Apply {
                            preferences,
                            ids,
                            capture,
                            expected,
                        },
                        self.path
                            .clone()
                            .ok_or("Durable recovery storage is required.")?,
                        self.factory.clone(),
                        self.observer(),
                    )?;
            }
            _ => return Err("Unknown visibility command.".into()),
        }
        Ok(self.status(id))
    }
    fn observer(&self) -> Observe {
        let registry = self.registry.clone();
        Arc::new(move |ids| {
            registry
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .visibility_sources(ids)
        })
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
