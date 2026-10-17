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
    feedback: Arc<Mutex<(Value, Instant)>>,
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
        json!({"schemaVersion":2,"aircraftModule":"Synthetic","cataloguePath":std::env::temp_dir().join("opensimgear-synthetic-catalogue"),"inputs":[],"outputs":outputs,"freshnessMs":config.connection["freshnessMs"].as_u64().unwrap_or(1500)})
    };
    // Source decoding only needs the numeric/text kind. Actual formatting is
    // authoritative in the portable document and validated against the device.
    for output in value["outputs"].as_array_mut().unwrap() {
        if matches!(output["format"]["kind"].as_str(), Some("threshold" | "rgb")) {
            output["format"] = json!({"kind":"integer"});
        }
    }
    let profile = Profile::parse_telemetry(&value)?;
    if config.provider() == "dcs-bios" {
        let compiled = CompiledProfile::load(profile)?;
        crate::dcs_profile::telemetry_sources(
            &compiled,
            &integration_profile::feedback_outputs(&config.document)?,
        )?;
        Ok(compiled)
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
        ..Controls::default()
    })
}
fn catalogue(config: &Config, request: &Value) -> Result<Value, String> {
    if config.provider() == crate::xplane::PROVIDER {
        return Ok(crate::xplane::catalogue());
    }
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
    for key in ["inputs", "outputs", "feedbackOutputs"] {
        for binding in config.document[key].as_array().into_iter().flatten() {
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
pub(crate) fn feedback_metadata(config: &Config) -> Result<Vec<Value>, String> {
    let catalogue = bound_catalogue(config)?;
    let channels = catalogue["channels"]
        .as_array()
        .or_else(|| catalogue["controls"].as_array())
        .ok_or("Provider catalogue has no output channels.")?;
    let mut result = Vec::new();
    for row in integration_profile::feedback_outputs(&config.document)? {
        let channel = channels
            .iter()
            .find(|c| c["id"] == row["channel"])
            .ok_or("Named feedback channel is missing.")?;
        let index = row["output"].as_u64().unwrap_or(0);
        let metadata = channel["outputs"]
            .as_array()
            .and_then(|a| a.iter().find(|v| v["index"].as_u64() == Some(index)))
            .unwrap_or(channel);
        let kind = match metadata["type"].as_str() {
            Some("boolean") => "boolean",
            Some("string") => "string",
            Some("integer") if metadata["maxValue"] == 1 => "boolean",
            Some("integer" | "number") => "number",
            _ => return Err("Unsupported feedback telemetry type.".into()),
        };
        result.push(json!({"bindingId":row["id"],"channel":row["channel"],"label":channel["label"],"kind":kind,
            "unit":if kind=="boolean"{"state"}else if kind=="string"{"text"}else{metadata["unit"].as_str().unwrap_or("ratio")},
            "minimum":metadata.get("minimum"),"maximum":metadata.get("maximum").or_else(||metadata.get("maxValue"))}));
    }
    Ok(result)
}
pub(crate) fn feedback_revision(config: &Config) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "sha256:{:x}",
        Sha256::digest(
            json!([
                config.document,
                config.connection["source"],
                feedback_metadata(config).unwrap_or_default()
            ])
            .to_string()
            .as_bytes()
        )
    )
}
fn feedback_idle(config: &Config) -> Value {
    json!({"running":false,"fresh":false,"revision":feedback_revision(config),"provider":config.provider(),
    "sourceKind":if config.provider()==crate::game_telemetry::PROVIDER&&config.connection["source"]=="synthetic"{"synthetic"}else{"simulator"},
    "sourceAgeMs":0,"freshnessMs":config.connection["freshnessMs"],"values":{},"sources":feedback_metadata(config).unwrap_or_default()})
}
fn preflight_panel(panel: Panel, value: &Value) -> Result<Panel, String> {
    let rows = value
        .as_array()
        .filter(|rows| rows.len() <= 32)
        .ok_or("Choose at most 32 game peripherals for integration preflight.")?;
    let mut devices = BTreeMap::new();
    for row in rows {
        if !row.as_object().is_some_and(|o| {
            o.keys().all(|k| {
                matches!(
                    k.as_str(),
                    "gatewayId" | "moduleId" | "controller" | "config"
                )
            })
        }) || !matches!(
            row["controller"].as_str(),
            Some("keep" | "enabled" | "disabled")
        ) {
            return Err("Invalid game peripheral projection.".into());
        }
        let gateway = row["gatewayId"]
            .as_str()
            .and_then(|id| id.split_once(':'))
            .and_then(|(backend, serial)| crate::gateways::canonical_id(backend, serial))
            .ok_or("Invalid projected gateway identity.")?;
        let module = row["moduleId"]
            .as_str()
            .filter(|id| {
                id.len() == 32
                    && id.bytes().all(|b| b.is_ascii_hexdigit())
                    && id.bytes().any(|b| b != b'0')
                    && id.bytes().any(|b| b != b'f' && b != b'F')
            })
            .ok_or("Invalid projected peripheral identity.")?
            .to_ascii_lowercase();
        if let Some(config) = row.get("config") {
            if !config.as_object().is_some_and(|o| {
                o.keys().all(|k| {
                    matches!(
                        k.as_str(),
                        "name"
                            | "expanderMask"
                            | "buttons"
                            | "axes"
                            | "hats"
                            | "encoders"
                            | "selectors"
                    )
                })
            }) {
                return Err("Invalid projected peripheral configuration fields.".into());
            }
            crate::runtime::commands(config, 1)?;
        }
        if devices.insert((gateway, module), row.clone()).is_some() {
            return Err("A peripheral cannot be projected more than once.".into());
        }
    }
    Ok(Arc::new(move |request| {
        if !matches!(
            request["command"].as_str(),
            Some("status" | "action_describe" | "events_state")
        ) {
            return json!({"ok":false,"error":"Integration preflight only permits device reads."});
        }
        let route = (
            request["gatewayId"].as_str().unwrap_or("").to_string(),
            request["moduleId"].as_str().unwrap_or("").to_string(),
        );
        let mut reply = panel(request.clone());
        let Some(device) = devices.get(&route) else {
            return reply;
        };
        if reply["ok"] != true {
            return reply;
        }
        if let Some(modules) = reply["state"]["modules"].as_array_mut() {
            if let Some(module) = modules.iter_mut().find(|m| m["id"] == route.1) {
                match device["controller"].as_str() {
                    Some("disabled") => {
                        module["enabled"] = json!(false);
                        module["published"] = json!(false);
                        module["reportId"] = Value::Null;
                    }
                    Some("enabled") => module["enabled"] = json!(true),
                    _ => {}
                }
                // directUsb is an actual physical connection; a profile's
                // desired gateway controller state cannot disconnect that cable.
            }
        }
        if request["command"] == "events_state" {
            if let Some(config) = device.get("config") {
                let events = &mut reply["state"]["moduleEvents"][&route.1];
                if let Err(error) = project_events(events, config) {
                    return json!({"ok":false,"error":error});
                }
            }
        }
        reply
    }))
}
fn project_events(events: &mut Value, config: &Value) -> Result<(), String> {
    // First prove the real firmware's input protocol and typed capabilities.
    // Forecasting a mapping must not invent firmware support or alter its route.
    crate::panel_events::validate(events, None)?;
    if events["contactButtonCount"].as_u64().is_none() || events["axisCount"].as_u64().is_none() {
        return Err("Update the panel firmware to advertise typed input capabilities before projecting a game setup.".into());
    }
    let buttons = config["buttons"].as_array().unwrap();
    let encoder_count = config["encoders"].as_array().unwrap().len() * 2;
    let selector_count = config["selectors"].as_array().unwrap().len();
    let count = buttons.len() + encoder_count + selector_count * 3;
    let axes = config["axes"].as_array().unwrap();
    events["buttonCount"] = json!(count);
    events["contactButtonCount"] = json!(buttons.len());
    events["held"] = json!(vec![false; count]);
    events["maintainedButtons"] = json!(buttons
        .iter()
        .enumerate()
        .filter(|(_, b)| b[1] != 0)
        .map(|(i, _)| i)
        .collect::<Vec<_>>());
    events["encoderButtons"] =
        json!((buttons.len()..buttons.len() + encoder_count).collect::<Vec<_>>());
    events["selectorButtons"] = json!((0..selector_count)
        .map(|i| {
            let start = buttons.len() + encoder_count + i * 3;
            [start, start + 1, start + 2]
        })
        .collect::<Vec<_>>());
    events["axisCount"] = json!(axes.len());
    let neutral: Vec<_> = axes.iter().map(|a| a[4].clone()).collect();
    events["axes"] = json!(neutral);
    events["axisNeutral"] = json!(neutral);
    events["axisUsage"] = json!(axes.iter().map(|a| a[1].clone()).collect::<Vec<_>>());
    events["axisAvailable"] = json!(vec![false; axes.len()]);
    crate::panel_events::validate(events, None)
}
impl IntegrationRuntime {
    /// Provider setup and context determine descriptors. Reading inventory does
    /// not change legacy catalogue queries or start a simulator connection.
    pub(crate) fn resource_catalogue(&self, provider: &str) -> Result<Value, String> {
        if provider == crate::xplane::PROVIDER { return Ok(crate::xplane::catalogue()); }
        if provider == crate::game_telemetry::PROVIDER { return Ok(crate::game_telemetry::catalogue()); }
        if provider != "dcs-bios" { return Err("Unsupported game integration.".into()); }
        let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        let config = settings.config.as_ref().filter(|config| config.provider() == provider)
            .ok_or("Choose DCS catalogue and aircraft in game details to expose its ports.")?;
        let mut query = config.connection.clone();
        query["aircraftModule"] = config.document["context"]["aircraftModule"].clone();
        drop(settings);
        crate::dcs_profile::resource_catalogue(&query)
    }

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
            feedback: Arc::new(Mutex::new((
                saved
                    .as_ref()
                    .and_then(|v| integration_profile::validate(&v["config"]).ok())
                    .map(|c| feedback_idle(&c))
                    .unwrap_or(Value::Null),
                Instant::now(),
            ))),
        }
    }
    pub(crate) fn feedback_snapshot(&self) -> Value {
        let (mut value, updated) = self
            .feedback
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if value.is_object() {
            let age = value["sourceAgeMs"]
                .as_u64()
                .unwrap_or(0)
                .saturating_add(updated.elapsed().as_millis() as u64);
            value["sourceAgeMs"] = json!(age);
            if age > value["freshnessMs"].as_u64().unwrap_or(1000) {
                value["fresh"] = json!(false);
            }
        }
        value
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
        self.inspect_with_panel(config, &self.panel)
    }
    fn inspect_with_panel(&self, config: &Config, panel: &Panel) -> Value {
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
            let targets =
                crate::dcs_profile_runtime::validate_targets(&profile, panel, &controls(config))?;
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
            Some("integration_preflight") => {
                if request.to_string().len() > 65_536
                    || !request.as_object().is_some_and(|o| {
                        o.keys().all(|k| {
                            matches!(
                                k.as_str(),
                                "command"
                                    | "clientId"
                                    | "integrationConfig"
                                    | "gameDevices"
                                    | "gameCompanions"
                            )
                        })
                    })
                {
                    return Err("Invalid integration preflight request.".into());
                }
                let config = integration_profile::validate(&request["integrationConfig"])?;
                let panel = preflight_panel(self.panel.clone(), &request["gameDevices"])?;
                settings.validation = self.inspect_with_panel(&config, &panel);
                settings.validation["preview"] = json!(true);
                Ok(())
            }
            Some("integration_clear") => {
                if settings.worker.as_ref().is_some_and(|w| !w.is_finished()) {
                    return Err("Stop the integration before clearing its settings.".into());
                }
                let revision = settings
                    .revision
                    .checked_add(1)
                    .ok_or("Profile revision exhausted.")?;
                match fs::remove_file(&self.path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.to_string()),
                }
                settings.config = None;
                settings.accepted = None;
                settings.validation = Value::Null;
                settings.catalogue = Value::Null;
                settings.revision = revision;
                *self.state.lock().unwrap_or_else(|p| p.into_inner()) =
                    json!({"running":false,"status":"stopped","error":null});
                *self.provider.lock().unwrap_or_else(|p| p.into_inner()) = Value::Null;
                Ok(())
            }
            Some("integration_catalogue") => {
                let config = if let Some(draft) = request.get("integrationConfig") {
                    let provider = draft["document"]["provider"]
                        .as_str()
                        .filter(|p| matches!(*p, "dcs-bios" | "assetto-corsa-rally" | "x-plane"))
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
                *self.feedback.lock().unwrap_or_else(|p| p.into_inner()) =
                    (feedback_idle(&config), Instant::now());
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
                if config.provider() == crate::game_telemetry::PROVIDER
                    && config.connection["source"] == "shared-memory"
                    && config.connection["processId"].is_null()
                {
                    return Err("Launch this integration from Games, or select a running ACR process for a manual session.".into());
                }
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
                let mut control = controls(&config);
                let decoded = if config.provider() == "dcs-bios" {
                    crate::dcs_profile::telemetry_sources(
                        &profile,
                        &integration_profile::feedback_outputs(&config.document)?,
                    )?
                } else {
                    vec![]
                };
                let observed = self.feedback.clone();
                let metadata = feedback_metadata(&config)?;
                let source_revision = feedback_revision(&config);
                let provider_kind = config.provider().to_string();
                let synthetic = config.provider() == crate::game_telemetry::PROVIDER
                    && config.connection["source"] == "synthetic";
                let freshness = config.connection["freshnessMs"].as_u64().unwrap_or(1000);
                let controls_mut = Arc::get_mut(&mut control).unwrap();
                controls_mut.feedback_sources = decoded;
                controls_mut.source_observer = Some(Arc::new(move |sample| {
                    let mut values = sample.values.clone();
                    for binding in &metadata {
                        if binding["kind"] == "boolean" {
                            if let Some(v) = values.get_mut(binding["bindingId"].as_str().unwrap())
                            {
                                if *v == 0 {
                                    *v = json!(false);
                                } else if *v == 1 {
                                    *v = json!(true);
                                }
                            }
                        }
                    }
                    *observed.lock().unwrap_or_else(|p| p.into_inner()) = (
                        json!({"running":true,"provider":provider_kind,
                        "sourceKind":if synthetic{"synthetic"}else{"simulator"},"fresh":sample.fresh,"sourceAgeMs":sample.age_ms,
                        "freshnessMs":freshness,"revision":source_revision,"epoch":sample.epoch,"values":values,"sources":metadata}),
                        Instant::now(),
                    );
                }));
                let cancel = Arc::new(AtomicBool::new(false));
                let mut fixture = None;
                let feed: Option<Feed> = if config.provider() == crate::xplane::PROVIDER {
                    let outputs: Vec<Value> = config.document["outputs"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .cloned()
                        .chain(integration_profile::feedback_outputs(&config.document)?.into_iter())
                        .collect();
                    let mut source = crate::xplane::Source::new(&config.connection, outputs)?;
                    let provider = self.provider.clone();
                    Some(Box::new(move |_now| {
                        let (snapshot, status) = source.poll(Instant::now());
                        *provider.lock().unwrap_or_else(|p| p.into_inner()) = status;
                        snapshot
                    }))
                } else if config.provider() == crate::game_telemetry::PROVIDER {
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
                    let outputs: Vec<Value> = config.document["outputs"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .cloned()
                        .chain(integration_profile::feedback_outputs(&config.document)?.into_iter())
                        .collect();
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
        let mut feedback = self.feedback.lock().unwrap_or_else(|p| p.into_inner());
        feedback.0["running"] = json!(false);
        feedback.0["fresh"] = json!(false);
        feedback.0["values"] = json!({});
        drop(feedback);
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
