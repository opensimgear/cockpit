//! Asynchronous game-side provisioning, separate from captured binding profiles.
use crate::{dcs_profile_runtime::Panel, game_discovery::Platform};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, thread::{self, JoinHandle}};

const CLIENT: &str = "cockpit-game-session-coordinator";
const MAX_SETTINGS: usize = 32768;
struct Inner {
    path: PathBuf,
    work: PathBuf,
    native: bool,
    settings_error: Option<String>,
    saved: Mutex<Value>,
    states: Mutex<BTreeMap<String, Value>>,
    cancel: AtomicBool,
    busy: AtomicBool,
}
pub(crate) struct GameIntegrationSetup {
    inner: Arc<Inner>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
fn write_settings(path: &Path, value: &Value) -> Result<(), String> {
    crate::dcs_setup::replace(path, value.to_string().as_bytes())
}

fn read_settings(path: &Path) -> Result<Value, String> {
    if !path.exists() { return Ok(json!({"schemaVersion":1,"games":{}})); }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_SETTINGS { return Err("Integration setup settings exceed their size limit.".into()); }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value["schemaVersion"] != 1 || !value["games"].is_object() || value["games"].as_object().unwrap().keys().any(|k| !matches!(k.as_str(), "dcs" | "assetto-corsa-rally")) {
        return Err("Invalid integration setup settings. Preserve the file and repair it before setup.".into());
    }
    for (game, record) in value["games"].as_object().unwrap() {
        if !record.is_object() || !record["connection"].is_object() { return Err("Invalid saved integration setup record.".into()); }
        validate_connection(game, &record["connection"])?;
        if game == "dcs" { absolute_folder(&record["userFolder"])?; }
    }
    Ok(value)
}
fn absolute_folder(value: &Value) -> Result<PathBuf, String> {
    let path = value.as_str().filter(|p| !p.is_empty() && p.len() <= 4096 && !p.chars().any(char::is_control)).ok_or("Choose an absolute DCS Saved Games user folder.")?;
    let path = PathBuf::from(path);
    if !path.is_absolute() { return Err("Choose an absolute DCS Saved Games user folder.".into()); }
    Ok(path)
}
fn folder(value: &Value) -> Result<PathBuf, String> {
    let path = absolute_folder(value)?;
    if !path.is_dir() { return Err("Choose an existing DCS Saved Games user folder. Start DCS once if it is missing.".into()); }
    Ok(path)
}
pub(crate) fn dcs_connection(path: &Path) -> Value {
    json!({"cataloguePath":path,"freshnessMs":1500,"commandHost":"127.0.0.1","commandPort":7778,"listenAddress":"0.0.0.0","listenPort":5010,"multicastGroup":"239.255.50.10","multicastInterface":"127.0.0.1"})
}
fn validate_connection(game: &str, value: &Value) -> Result<(), String> {
    if !value.is_object() || value.to_string().len() > 8192 { return Err("Invalid local game connection settings.".into()); }
    let keys = if game == "dcs" { vec!["cataloguePath","freshnessMs","commandHost","commandPort","listenAddress","listenPort","multicastGroup","multicastInterface"] } else { vec!["freshnessMs","source"] };
    if value.as_object().unwrap().keys().any(|k| !keys.contains(&k.as_str())) { return Err("Unsupported local game connection setting.".into()); }
    if value["freshnessMs"].as_u64().is_none_or(|v| !(100..=10000).contains(&v)) { return Err("Use a freshness timeout from 100 to 10000 ms.".into()); }
    if game != "dcs" {
        return if value["source"] == "shared-memory" { Ok(()) } else { Err("ACR setup requires the native shared-memory source.".into()) };
    }
    if !value["cataloguePath"].as_str().is_some_and(|s| s.len() <= 4096 && Path::new(s).is_absolute() && !s.chars().any(char::is_control)) { return Err("Choose an absolute DCS catalogue directory.".into()); }
    crate::dcs_bios::Network::parse(value)?;
    Ok(())
}
fn compatible_dcs_connection(value: &Value) -> Result<(), String> {
    validate_connection("dcs", value)?;
    let network = crate::dcs_bios::Network::parse(value)?;
    if !network.command_host.is_loopback() || network.command_port != 7778 || network.listen_port != 5010 || network.multicast_group.map(|g| g.to_string()).as_deref() != Some("239.255.50.10") {
        return Err("Saved UDP overrides were preserved, but they do not match the installed DCS-BIOS defaults (local command port 7778; multicast 239.255.50.10:5010). Review the connection settings before integration.".into());
    }
    Ok(())
}
impl GameIntegrationSetup {
    pub(crate) fn new(path: PathBuf, native: bool) -> Self {
        let loaded = read_settings(&path);
        let mut states = BTreeMap::new();
        if let Err(error) = &loaded {
            states.insert("dcs".into(), json!({"state":"needs-attention","message":error,"settingsError":true}));
        }
        Self { inner: Arc::new(Inner { work: path.with_extension("setup-work"), path, native, settings_error: loaded.as_ref().err().cloned(), saved: Mutex::new(loaded.unwrap_or(json!({"schemaVersion":1,"games":{}}))), states: Mutex::new(states), cancel: AtomicBool::new(false), busy: AtomicBool::new(false) }), worker: Mutex::new(None) }
    }
    pub(crate) fn busy(&self) -> bool { self.inner.busy.load(Ordering::Acquire) }
    pub(crate) fn pending(&self) -> bool { crate::dcs_setup::recovery_pending(&self.inner.work) }
    pub(crate) fn decorate(&self, catalogue: &mut [Value], integration: &Value, dcs: &Value) {
        for game in catalogue {
            let id = game["id"].as_str().unwrap_or("");
            let cached = self.inner.states.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned();
            let mut status = cached.unwrap_or_else(|| {
                let status = self.inner.inspect(game, None);
                self.inner.states.lock().unwrap_or_else(|p| p.into_inner()).insert(id.into(), status.clone());
                status
            });
            let running = integration["running"] == true && integration["config"]["document"]["provider"] == game["provider"];
            let feed = if running { integration } else if id == "dcs" && dcs["running"] == true { dcs } else { &Value::Null };
            let fresh = feed["feedbackFresh"] == true || feed["telemetry"]["fresh"] == true;
            status["live"] = json!(if fresh { "receiving" } else if feed["status"] == "stale" || feed["telemetry"]["status"] == "stale" || feed["status"] == "error" { "stale" } else { "waiting" });
            status["liveMessage"] = json!(if fresh { "Receiving fresh game telemetry." } else if feed.is_null() { "Waiting for a game integration. Setup does not start bindings." } else { "Waiting for fresh game/session telemetry." });
            if id == "dcs" { status["version"] = json!(crate::dcs_setup::VERSION); }
            game["integrationSetup"] = status;
        }
    }
    pub(crate) fn refresh(&self, catalogue: &[Value]) {
        if self.busy() { return; }
        for game in catalogue {
            let status = self.inner.inspect(game, None);
            self.inner.states.lock().unwrap_or_else(|p| p.into_inner()).insert(game["id"].as_str().unwrap_or("").into(), status);
        }
    }
    pub(crate) fn request(&self, request: &Value, game: &Value, platform: Arc<dyn Platform>, panel: &Panel) -> Result<(), String> {
        let command = request["command"].as_str().unwrap_or("");
        if command == "game_integration_cancel" { self.inner.cancel.store(true, Ordering::Release); return Ok(()); }
        if self.busy() { return Err("Wait for integration setup or cancel it first.".into()); }
        let id = game["id"].as_str().ok_or("Choose a supported game.")?;
        if command == "game_integration_check" {
            let status = self.inner.inspect(game, request.get("userFolder"));
            self.inner.states.lock().unwrap_or_else(|p| p.into_inner()).insert(id.into(), status);
            return Ok(());
        }
        if !self.inner.native { return Err("Native integration setup is available on Windows.".into()); }
        let state = panel(json!({"command":"integration_get_state","clientId":CLIENT}));
        if state["state"]["integration"]["running"] == true || state["state"]["dcsBios"]["running"] == true { return Err("Stop the active simulator integration before changing its setup.".into()); }
        if command == "game_integration_recover" {
            if id != "dcs" { return Err("DCS setup recovery belongs to the DCS game.".into()); }
            if platform.find(game)?.is_some() { return Err("Close DCS before restoring its integration files.".into()); }
            let restored = crate::dcs_setup::recover(&self.inner.work)?;
            if restored["restored"] != true { return Err(format!("Setup recovery preserved external changes: {}", restored["conflicts"])); }
            let status = self.inner.inspect(game, None);
            self.inner.states.lock().unwrap_or_else(|p| p.into_inner()).insert(id.into(), status);
            return Ok(());
        }
        if let Some(error) = &self.inner.settings_error { return Err(error.clone()); }
        if command != "game_integration_setup" || id != "dcs" { return Err("Automatic setup for this game is not yet available.".into()); }
        if game["installed"] != true { return Err("Find the game installation before setting up integration.".into()); }
        if self.pending() { return Err("Recover the interrupted DCS setup before installing again.".into()); }
        if platform.find(game)?.is_some() { return Err("Close DCS before changing its export scripts, then run setup again.".into()); }
        let checked = self.inner.inspect(game, request.get("userFolder"));
        let target = folder(&checked["userFolder"])?;
        let saved = self.inner.saved.lock().unwrap_or_else(|p| p.into_inner())["games"]["dcs"].clone();
        let mut connection = dcs_connection(&target.join("Scripts/DCS-BIOS/doc/json"));
        if let Some(previous) = saved["connection"].as_object() { for (k,v) in previous { if k != "cataloguePath" { connection[k] = v.clone(); } } }
        // A saved matching provider carries the user's connection overrides.
        let current = &state["state"]["integration"]["config"];
        if current["document"]["provider"] == "dcs-bios" {
            if let Some(previous) = current["connection"].as_object() { for (k,v) in previous { if k != "cataloguePath" { connection[k] = v.clone(); } } }
        }
        if let Some(overrides) = request.get("connection") {
            let values = overrides.as_object().ok_or("Invalid connection overrides.")?;
            for (k,v) in values { if k != "cataloguePath" { connection[k] = v.clone(); } }
        }
        validate_connection(id, &connection)?;
        self.inner.cancel.store(false, Ordering::Release);
        self.inner.busy.store(true, Ordering::Release);
        self.inner.states.lock().unwrap_or_else(|p| p.into_inner()).insert(id.into(), json!({"state":"setting-up","message":"Preparing DCS-BIOS setup.","userFolder":target,"version":crate::dcs_setup::VERSION,"progress":"Preparing"}));
        let inner = self.inner.clone();
        let game = game.clone();
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(previous) = worker.take() { let _ = previous.join(); }
        *worker = Some(thread::spawn(move || {
            let result = crate::dcs_setup::setup(&target, &inner.work, || inner.cancel.load(Ordering::Acquire), |message, percent| {
                let mut states = inner.states.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(status) = states.get_mut("dcs") { status["progress"] = json!(message); status["percent"] = json!(percent); }
            }).and_then(|_| {
                let mut draft = inner.saved.lock().unwrap_or_else(|p| p.into_inner()).clone();
                draft["games"]["dcs"] = json!({"userFolder":target,"connection":connection});
                write_settings(&inner.path, &draft)?;
                *inner.saved.lock().unwrap_or_else(|p| p.into_inner()) = draft;
                Ok(())
            });
            let mut status = inner.inspect(&game, None);
            if let Err(error) = result { status["state"] = json!("needs-attention"); status["message"] = json!(error); }
            inner.states.lock().unwrap_or_else(|p| p.into_inner()).insert("dcs".into(), status);
            inner.busy.store(false, Ordering::Release);
        }));
        Ok(())
    }
    pub(crate) fn preflight(&self, profile: &Value) -> Result<(), String> {
        if !self.inner.native || profile["integration"].is_null() { return Ok(()); }
        if profile["gameId"] == "dcs" {
            let catalogue = profile["integration"]["connection"]["cataloguePath"].as_str().ok_or("Choose a DCS catalogue.")?;
            let path = Path::new(catalogue);
            let target = path.ancestors().nth(4).ok_or("Set up DCS-BIOS in the selected Saved Games folder.")?;
            let inspected = crate::dcs_setup::inspect(target)?;
            let expected = inspected["cataloguePath"].as_str().ok_or("DCS catalogue is unavailable.")?;
            if fs::canonicalize(path).map_err(|e| e.to_string())? != fs::canonicalize(expected).map_err(|e| e.to_string())? { return Err("The profile catalogue does not belong to the configured DCS-BIOS module. Review its connection settings.".into()); }
            if inspected["moduleReady"] != true || inspected["loaderReady"] != true || self.pending() { return Err("DCS integration setup needs attention. Recheck or repair it before activating this profile.".into()); }
            compatible_dcs_connection(&profile["integration"]["connection"])?;
        }
        Ok(())
    }
    pub(crate) fn shutdown(&self) {
        self.inner.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() { let _ = worker.join(); }
    }
}
impl Inner {
    fn inspect(&self, game: &Value, override_folder: Option<&Value>) -> Value {
        if let Some(error) = &self.settings_error { return json!({"state":"needs-attention","message":error,"settingsError":true,"recoveryNeeded":crate::dcs_setup::recovery_pending(&self.work)}); }
        if !self.native { return json!({"state":"unsupported-platform","message":"Native game integration setup requires Windows. Profile editing and synthetic telemetry remain available."}); }
        if game["installed"] != true { return json!({"state":"not-installed","message":"Find the game installation first."}); }
        if game["id"] != "dcs" { return json!({"state":"setup-required","message":"ACR automatic setup will be available after DCS setup."}); }
        let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner())["games"]["dcs"].clone();
        let candidates = crate::dcs_setup::discover_user_folders(game["installPath"].as_str().map(Path::new)).unwrap_or_default();
        let chosen = override_folder.filter(|v| !v.is_null()).cloned().or_else(|| saved["userFolder"].as_str().map(|s| json!(s))).or_else(|| (candidates.len() == 1).then(|| json!(candidates[0])));
        let mut status = json!({"state":"setup-required","message":"Choose the DCS Saved Games user folder, then set up integration.","candidates":candidates,"userFolder":chosen,"version":crate::dcs_setup::VERSION,"recoveryNeeded":crate::dcs_setup::recovery_pending(&self.work)});
        if status["recoveryNeeded"] == true { status["state"] = json!("needs-attention"); status["message"] = json!("A previous setup was interrupted. Recover its owned file changes before continuing."); return status; }
        let Some(chosen) = chosen else { return status; };
        match folder(&chosen).and_then(|target| crate::dcs_setup::inspect(&target)) {
            Err(error) => { status["message"] = json!(error); }
            Ok(inspected) => {
                status["cataloguePath"] = inspected["cataloguePath"].clone();
                status["diagnostics"] = inspected["diagnostics"].clone();
                if inspected["moduleReady"] == true && inspected["loaderReady"] == true {
                    let connection = if saved["userFolder"] == chosen && saved["connection"].is_object() { saved["connection"].clone() } else { dcs_connection(Path::new(inspected["cataloguePath"].as_str().unwrap_or(""))) };
                    status["connection"] = connection.clone();
                    match compatible_dcs_connection(&connection) {
                        Ok(()) => { status["state"] = json!("ready"); status["message"] = json!("DCS-BIOS module, export loader, catalogue and local connection settings are ready."); },
                        Err(error) => { status["state"] = json!("needs-attention"); status["message"] = json!(error); }
                    }
                } else if inspected["repairable"] != true {
                    status["state"] = json!("needs-attention");
                    status["message"] = inspected["diagnostics"][0].clone();
                } else { status["message"] = json!("DCS-BIOS or its export loader needs setup. Existing scripts will be preserved."); }
            }
        }
        status
    }
}
#[cfg(test)]
#[path = "game_integration_setup_tests.rs"]
mod tests;
