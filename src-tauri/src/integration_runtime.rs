//! Explicit saved runs over provider-neutral roles. Acquisition and panel I/O
//! use separate loops; every provider shares the same bounded gateway workers.
use crate::{
    dcs_profile::{CompiledOutput, CompiledProfile, ExportSnapshot, OutputSource, Profile},
    dcs_profile_runtime::{Controls, Feed, Panel},
    integration_profile::{self, Config},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

struct Settings {
    config: Option<Config>,
    revision: u64,
    validation: Value,
    catalogue: Value,
    accepted: Option<String>,
    closed: bool,
    worker: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    controls: Arc<Controls>,
    fixture: Option<Arc<Mutex<crate::game_telemetry::FixtureSource>>>,
}
pub(crate) struct IntegrationRuntime {
    panel: Panel,
    path: PathBuf,
    settings: Mutex<Settings>,
    state: Arc<Mutex<Value>>,
    provider: Arc<Mutex<Value>>,
}
fn diagnostic(code: &str, message: &str) -> Value {
    json!({"severity":"error","code":code,"message":message})
}
fn signature(value: &Value) -> String {
    // Deterministic semantic JSON, including routes/descriptors, never addresses
    // from an aircraft's export memory or ownership capability tokens.
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.to_string().hash(&mut h);
    format!("{:016x}", h.finish())
}
fn compile(config: &Config) -> Result<CompiledProfile, String> {
    let mut value = if config.provider() == "dcs-bios" {
        config.resolved_dcs_profile()?
    } else {
        let mut outputs = Vec::new();
        for output in config.document["outputs"].as_array().unwrap() {
            let target = config
                .assignments
                .iter()
                .find(|a| json!(a.role) == output["role"])
                .ok_or_else(|| format!("Assign device role {} before starting.", output["role"]))?;
            let mut output = output.clone();
            output["gatewayId"] = json!(target.gateway_id);
            output["moduleId"] = json!(target.module_id);
            output["control"] = output["channel"].clone();
            outputs.push(output);
        }
        json!({"schemaVersion":2,"aircraftModule":"Synthetic","cataloguePath":"/synthetic","inputs":[],"outputs":outputs,"freshnessMs":config.connection["freshnessMs"].as_u64().unwrap_or(1500)})
    };
    // Source decoding only needs the numeric/text kind. Actual formatting is
    // authoritative in the portable document and validated against the device.
    for output in value["outputs"].as_array_mut().unwrap() {
        if matches!(output["format"]["kind"].as_str(), Some("threshold" | "rgb")) {
            output["format"] = json!({"kind":"integer"});
        }
    }
    let profile = Profile::parse(&value)?;
    if config.provider() == "dcs-bios" {
        CompiledProfile::load(profile)
    } else {
        let source = crate::dcs_bios::IntegerOutput::parse(
            &json!({"address":2,"mask":65535,"shift_by":0,"max_value":65535,"type":"integer"}),
        )?;
        let outputs = profile
            .outputs
            .iter()
            .cloned()
            .map(|binding| CompiledOutput {
                binding,
                source: OutputSource::Integer(source.clone()),
            })
            .collect();
        Ok(CompiledProfile {
            profile,
            catalogue: crate::dcs_bios::Catalogue::output_only(),
            inputs: vec![],
            outputs,
        })
    }
}
fn controls(config: &Config) -> Arc<Controls> {
    Arc::new(Controls {
        formats: config.document["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| (o["id"].as_str().unwrap().into(), o.clone()))
            .collect(),
        overrides: Mutex::new(BTreeMap::new()),
        require_ownership: true,
    })
}
fn catalogue(config: &Config, request: &Value) -> Result<Value, String> {
    if config.provider() == crate::game_telemetry::PROVIDER {
        return Ok(crate::game_telemetry::catalogue());
    }
    let mut query = config.connection.clone();
    query["aircraftModule"] = config.document["context"]["aircraftModule"].clone();
    for key in ["query", "offset", "limit", "kind"] {
        if let Some(value) = request.get(key) {
            query[key] = value.clone();
        }
    }
    crate::dcs_profile::query_catalogue(&query)
}
fn bound_catalogue(config: &Config) -> Result<Value, String> {
    if config.provider() != "dcs-bios" {
        return catalogue(config, &json!({}));
    }
    let mut controls = BTreeMap::new();
    for key in ["inputs", "outputs"] {
        for binding in config.document[key].as_array().unwrap() {
            let name = &binding[if key == "inputs" {
                "command"
            } else {
                "channel"
            }];
            let page = catalogue(config, &json!({"query":name,"limit":64}))?;
            for control in page["controls"].as_array().unwrap() {
                if control["id"] == *name {
                    controls.insert(name.as_str().unwrap().to_owned(), control.clone());
                }
            }
        }
    }
    Ok(
        json!({"provider":"dcs-bios","aircraftModule":config.document["context"]["aircraftModule"],"controls":controls.into_values().collect::<Vec<_>>()}),
    )
}
impl IntegrationRuntime {
    pub(crate) fn new(panel: Panel, path: PathBuf) -> Self {
        let saved = fs::read(&path)
            .ok()
            .filter(|b| b.len() <= 65536)
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let config = saved
            .as_ref()
            .and_then(|v| integration_profile::validate(&v["config"]).ok());
        let error = if path.exists() && config.is_none() {
            Some("Saved integration settings are invalid; import or save a valid profile.")
        } else {
            None
        };
        Self {
            panel,
            path,
            settings: Mutex::new(Settings {
                config,
                revision: saved
                    .as_ref()
                    .and_then(|v| v["revision"].as_u64())
                    .unwrap_or(0),
                accepted: saved
                    .as_ref()
                    .and_then(|v| v["signature"].as_str())
                    .map(str::to_owned),
                validation: Value::Null,
                catalogue: Value::Null,
                closed: false,
                worker: None,
                cancel: Arc::new(AtomicBool::new(false)),
                controls: Arc::new(Controls::default()),
                fixture: None,
            }),
            state: Arc::new(Mutex::new(
                json!({"running":false,"status":"stopped","error":error}),
            )),
            provider: Arc::new(Mutex::new(Value::Null)),
        }
    }
    pub(crate) fn status(&self) -> Value {
        let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner()).clone();
        state["configured"] = json!(settings.config.is_some());
        state["config"] = settings
            .config
            .as_ref()
            .map(Config::value)
            .unwrap_or(Value::Null);
        state["revision"] = json!(settings.revision);
        state["validation"] = settings.validation.clone();
        state["catalogue"] = settings.catalogue.clone();
        state["providerState"] = self
            .provider
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        state["telemetry"] = state["providerState"].clone();
        let overrides = settings
            .controls
            .overrides
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(outputs) = state["outputs"].as_array_mut() {
            for output in outputs {
                let until = overrides
                    .get(output["id"].as_str().unwrap_or(""))
                    .map(|(_, until)| *until);
                output["overrideRemainingMs"] = json!(until
                    .map(|u| u.saturating_duration_since(Instant::now()).as_millis() as u64)
                    .unwrap_or(0));
            }
        }
        state
    }
    fn inspect(&self, config: &Config) -> Value {
        let result = (|| -> Result<Value, String> {
            let metadata = bound_catalogue(config)?;
            let diagnostics: Vec<_> = integration_profile::catalogue_diagnostics(config, &metadata)
                .iter()
                .map(|d| d.value())
                .collect();
            if !diagnostics.is_empty() {
                return Ok(json!({"valid":false,"diagnostics":diagnostics,"catalogue":metadata}));
            }
            let profile = compile(config)?;
            let targets = crate::dcs_profile_runtime::validate_targets(
                &profile,
                &self.panel,
                &controls(config),
            )?;
            Ok(
                json!({"valid":true,"diagnostics":[],"signature":signature(&json!([metadata,targets])),"catalogueRevision":signature(&metadata),"provider":config.provider(),"catalogue":metadata,"targets":targets}),
            )
        })();
        result.unwrap_or_else(
            |e| json!({"valid":false,"diagnostics":[diagnostic("validationFailed",&e)]}),
        )
    }
    pub(crate) fn request(&self, request: &Value) -> Result<(), String> {
        if request["command"] == "integration_stop" {
            self.stop(false);
            return Ok(());
        }
        let mut settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        if settings.closed {
            return Err("The integration runtime has closed.".into());
        }
        match request["command"].as_str() {
            Some("integration_get_state") => Ok(()),
            Some("integration_catalogue") => {
                let config = if let Some(draft) = request.get("integrationConfig") {
                    let provider = draft["document"]["provider"]
                        .as_str()
                        .filter(|p| matches!(*p, "dcs-bios" | "assetto-corsa-rally"))
                        .ok_or("Choose a supported telemetry provider.")?;
                    let _ = provider;
                    Config {
                        document: draft["document"].clone(),
                        connection: draft["connection"].clone(),
                        assignments: vec![],
                    }
                } else {
                    settings
                        .config
                        .clone()
                        .ok_or("Create or import a profile first.")?
                };
                settings.catalogue = catalogue(&config, request)?;
                Ok(())
            }
            Some("integration_validate" | "integration_save") => {
                let config = integration_profile::validate(&request["integrationConfig"])?;
                if settings.worker.as_ref().is_some_and(|w| !w.is_finished()) {
                    return Err(
                        "Stop the integration before changing or revalidating its profile.".into(),
                    );
                }
                settings.validation = self.inspect(&config);
                if settings.validation["valid"] != true {
                    return if request["command"] == "integration_save" {
                        Err(
                            "Profile validation failed; review its diagnostics before saving."
                                .into(),
                        )
                    } else {
                        Ok(())
                    };
                }
                if request["command"] == "integration_validate" {
                    if settings.config.as_ref() == Some(&config) {
                        settings.accepted =
                            settings.validation["signature"].as_str().map(str::to_owned);
                    }
                    return Ok(());
                }
                if let Some(expected) = request.get("revision") {
                    if expected.as_u64() != Some(settings.revision) {
                        return Err("The saved profile changed. Refresh before saving.".into());
                    }
                }
                let revision = settings
                    .revision
                    .checked_add(1)
                    .ok_or("Profile revision exhausted.")?;
                let parent = self
                    .path
                    .parent()
                    .ok_or("Invalid integration settings path.")?;
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                let temporary = self
                    .path
                    .with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
                let bytes=serde_json::to_vec(&json!({"config":config.value(),"revision":revision,"signature":settings.validation["signature"]})).map_err(|e|e.to_string())?;
                use std::io::Write;
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let saved = (|| -> Result<(), String> {
                    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
                    file.write_all(&bytes).map_err(|e| e.to_string())?;
                    file.sync_all().map_err(|e| e.to_string())?;
                    fs::rename(&temporary, &self.path).map_err(|e| e.to_string())?;
                    Ok(())
                })();
                if saved.is_err() {
                    let _ = fs::remove_file(&temporary);
                }
                saved?;
                settings.accepted = settings.validation["signature"].as_str().map(str::to_owned);
                settings.config = Some(config);
                settings.revision = revision;
                Ok(())
            }
            Some("integration_start") => {
                if settings.worker.as_ref().is_some_and(|w| !w.is_finished()) {
                    return Err("The integration is already running.".into());
                }
                if let Some(worker) = settings.worker.take() {
                    let _ = worker.join();
                }
                if request
                    .get("revision")
                    .is_some_and(|r| r.as_u64() != Some(settings.revision))
                {
                    return Err("Refresh the saved profile before starting.".into());
                }
                let config = settings
                    .config
                    .clone()
                    .ok_or("Save a validated integration profile before starting.")?;
                settings.validation = self.inspect(&config);
                if settings.validation["valid"] != true {
                    return Err("Current profile validation failed; review its diagnostics.".into());
                }
                let signature = settings.validation["signature"].as_str().unwrap();
                if settings.accepted.as_deref() != Some(signature) {
                    settings.validation["diagnostics"]=json!([diagnostic("catalogueChanged","Catalogue semantics, device capabilities or route changed. Validate explicitly before restarting.")]);
                    return Err(
                        "Revalidate this profile after its catalogue or device route changed."
                            .into(),
                    );
                }
                let profile = compile(&config)?;
                let control = controls(&config);
                let cancel = Arc::new(AtomicBool::new(false));
                let mut fixture = None;
                let feed: Option<Feed> = if config.provider() == crate::game_telemetry::PROVIDER {
                    let freshness = config.connection["freshnessMs"].as_u64().unwrap_or(1500);
                    let mut session = crate::game_telemetry::Session::new(freshness)?;
                    let mut source: Box<dyn crate::game_telemetry::Source + Send> =
                        if config.connection["source"] == "shared-memory" {
                            let pid = config.connection["processId"]
                                .as_u64()
                                .and_then(|v| u32::try_from(v).ok())
                                .filter(|v| *v > 0)
                                .ok_or("Select the Rally process ID.")?;
                            Box::new(crate::acr_windows::WindowsSource::new(pid)?)
                        } else {
                            let queue =
                                Arc::new(Mutex::new(crate::game_telemetry::FixtureSource::new()));
                            fixture = Some(queue.clone());
                            Box::new(SharedFixture(queue))
                        };
                    let provider = self.provider.clone();
                    let outputs = config.document["outputs"].as_array().unwrap().clone();
                    Some(Box::new(move |now| {
                        let snapshot = session.poll(source.as_mut(), now);
                        *provider.lock().unwrap_or_else(|p| p.into_inner()) = snapshot.value();
                        let channels = snapshot.values();
                        let values = outputs
                            .iter()
                            .filter_map(|o| {
                                channels
                                    .get(o["channel"].as_str().unwrap())
                                    .map(|v| (o["id"].as_str().unwrap().to_owned(), v.clone()))
                            })
                            .collect();
                        ExportSnapshot {
                            epoch: snapshot.epoch,
                            aircraft: snapshot.context.as_ref().map(|c| c.car_model.clone()),
                            mission_active: snapshot.fresh,
                            fresh: snapshot.fresh,
                            age_ms: snapshot.age_ms,
                            counter: snapshot.packet_id.map(|n| (n & 65535) as u16),
                            values,
                            malformed_frames: 0,
                        }
                    }))
                } else {
                    None
                };
                *self.provider.lock().unwrap_or_else(|p| p.into_inner()) = Value::Null;
                let worker = crate::dcs_profile_runtime::start_source(
                    profile,
                    self.panel.clone(),
                    self.state.clone(),
                    cancel.clone(),
                    control.clone(),
                    feed,
                )?;
                settings.cancel = cancel;
                settings.controls = control;
                settings.fixture = fixture;
                settings.worker = Some(worker);
                Ok(())
            }
            Some("integration_simulate") => {
                let sample =
                    crate::game_telemetry::RawSample::parse_json(&request["telemetryFrame"])?;
                settings
                    .fixture
                    .as_ref()
                    .ok_or("Start a synthetic Rally profile first.")?
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(sample)
            }
            Some("integration_override") => {
                if !settings.worker.as_ref().is_some_and(|w| !w.is_finished()) {
                    return Err("Start the integration before overriding an output.".into());
                }
                let id = request["bindingId"]
                    .as_str()
                    .ok_or("Choose an output binding.")?;
                let duration = if request["resume"] == true {
                    Some(0)
                } else {
                    request["durationMs"].as_u64()
                }
                .filter(|v| *v <= 30000)
                .ok_or("Override duration must be 0 to 30000 ms.")?;
                let config = settings.config.as_ref().unwrap();
                let output = config.document["outputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|o| o["id"] == id)
                    .ok_or("Unknown output binding.")?;
                let prepared = crate::dcs_profile_runtime::validate_targets(
                    &compile(config)?,
                    &self.panel,
                    &settings.controls,
                )?;
                let role = &output["role"];
                let target = config
                    .assignments
                    .iter()
                    .find(|a| json!(a.role) == *role)
                    .unwrap();
                let action = prepared
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|t| {
                        t["gatewayId"] == target.gateway_id && t["moduleId"] == target.module_id
                    })
                    .and_then(|t| t["actions"][id].as_object())
                    .map(|o| Value::Object(o.clone()))
                    .ok_or("Refresh the output descriptor.")?;
                let mut overrides = settings
                    .controls
                    .overrides
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                if duration == 0 {
                    overrides.remove(id);
                } else {
                    crate::actions::arguments(&action, &request["values"])?;
                    overrides.insert(
                        id.into(),
                        (
                            request["values"].clone(),
                            Instant::now() + Duration::from_millis(duration),
                        ),
                    );
                }
                Ok(())
            }
            _ => Err("Unknown integration command.".into()),
        }
    }
    pub(crate) fn running(&self) -> bool {
        self.settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .worker
            .as_ref()
            .is_some_and(|w| !w.is_finished())
    }
    pub(crate) fn stop(&self, terminal: bool) {
        let mut settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        settings.closed |= terminal;
        settings.cancel.store(true, Ordering::Release);
        if let Some(worker) = settings.worker.take() {
            let _ = worker.join();
        }
        settings.fixture = None;
        *self.provider.lock().unwrap_or_else(|p| p.into_inner()) = Value::Null;
        settings
            .controls
            .overrides
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state["running"] = json!(false);
        state["feedbackFresh"] = json!(false);
        state["status"] = json!("stopped");
    }
}
struct SharedFixture(Arc<Mutex<crate::game_telemetry::FixtureSource>>);
impl crate::game_telemetry::Source for SharedFixture {
    fn poll(&mut self) -> Result<Option<crate::game_telemetry::RawSample>, String> {
        crate::game_telemetry::Source::poll(&mut *self.0.lock().unwrap_or_else(|p| p.into_inner()))
    }
    fn close(&mut self) {
        crate::game_telemetry::Source::close(
            &mut *self.0.lock().unwrap_or_else(|p| p.into_inner()),
        );
    }
}
impl Drop for IntegrationRuntime {
    fn drop(&mut self) {
        self.stop(true);
    }
}

#[cfg(test)]
#[path = "integration_runtime_tests.rs"]
mod tests;
