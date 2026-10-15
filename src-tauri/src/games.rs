//! Saved simulator setups and a journaled, native session coordinator.
//!
//! Hardware writes are issued once. Recovery reads authoritative state before
//! restoring, and keeps its journal when a device is absent or changed elsewhere.
use crate::{dcs_profile_runtime::Panel, game_discovery::Platform};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CLIENT: &str = "cockpit-game-session-coordinator";
const GAMES: [&str; 2] = ["dcs", "assetto-corsa-rally"];
const MAX_FILE: usize = 2 * 1024 * 1024;
const MAX_SAVED: usize = 48 * 1024;
const BINDING_WARNING: &str = "Controller and input mapping changes can rebuild USB descriptors and change game bindings. Accept these changes before activating this profile.";

fn known(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.keys().all(|k| keys.contains(&k.as_str())))
}
fn identifier(value: &Value, max: usize) -> Result<&str, String> {
    value
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= max
                && s.as_bytes()[0].is_ascii_alphanumeric()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
        .ok_or_else(|| "Choose a valid game profile identity.".into())
}
fn game_id(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .filter(|s| GAMES.contains(s))
        .ok_or_else(|| "Choose a supported game.".into())
}
fn target(value: &Value) -> Result<(String, String), String> {
    let gateway = value["gatewayId"]
        .as_str()
        .and_then(|s| s.split_once(':'))
        .and_then(|(b, s)| crate::gateways::canonical_id(b, s))
        .ok_or("Choose a valid gateway identity.")?;
    let module = value["moduleId"]
        .as_str()
        .filter(|s| {
            s.len() == 32
                && s.bytes().all(|b| b.is_ascii_hexdigit())
                && s.bytes().any(|b| b != b'0')
                && s.bytes().any(|b| b != b'f' && b != b'F')
        })
        .ok_or("Choose a valid peripheral identity.")?
        .to_ascii_lowercase();
    Ok((gateway, module))
}
fn validate_config(value: &Value) -> Result<(), String> {
    if !known(
        value,
        &[
            "name",
            "expanderMask",
            "buttons",
            "axes",
            "hats",
            "encoders",
            "selectors",
        ],
    ) {
        return Err("Invalid peripheral configuration fields.".into());
    }
    crate::runtime::commands(value, 1)?;
    let mut contacts = BTreeSet::new();
    let mask = value["expanderMask"].as_u64().unwrap();
    for kind in ["buttons", "hats", "encoders", "selectors"] {
        for row in value[kind].as_array().unwrap() {
            if kind == "buttons" && row[1] == 1 && row[2] != 0 {
                return Err("Maintained switches cannot belong to an exclusive group.".into());
            }
            let inputs = if kind == "buttons" {
                &row.as_array().unwrap()[..1]
            } else {
                row.as_array().unwrap().as_slice()
            };
            for input in inputs {
                let input = input.as_u64().unwrap();
                if input >= 24 && mask & (1 << ((input - 24) / 16)) == 0 {
                    return Err(format!("Contact {input} needs an enabled expansion bank."));
                }
                if !contacts.insert(input) {
                    return Err("A contact is assigned to more than one control.".into());
                }
            }
        }
    }
    for column in [0, 1] {
        let mut axes = BTreeSet::new();
        for axis in value["axes"].as_array().unwrap() {
            if !axes.insert(axis[column].as_u64().unwrap()) {
                return Err("Each axis needs a unique ADC channel and HID usage.".into());
            }
        }
    }
    Ok(())
}
fn game_integration(value: &Value) -> Result<crate::integration_profile::Config, String> {
    let mut draft = value.clone();
    if draft["document"]["provider"] == "assetto-corsa-rally"
        && draft["connection"]["source"] == "shared-memory"
        && draft["connection"]["processId"].is_null()
    {
        // A game setup is saved before a process exists. The actual verified
        // PID is inserted by the session worker, never persisted as this value.
        draft["connection"]["processId"] = json!(1);
    }
    crate::integration_profile::validate(&draft)
}
fn profile(value: &Value) -> Result<Value, String> {
    if value.to_string().len() > MAX_SAVED
        || !known(
            value,
            &[
                "schemaVersion",
                "id",
                "gameId",
                "name",
                "devices",
                "integration",
                "mapping",
            ],
        )
        || !matches!(value["schemaVersion"].as_u64(), Some(1 | 2))
    {
        return Err("Invalid game profile document.".into());
    }
    crate::game_mapping::validate(value)?;
    identifier(&value["id"], 64)?;
    let game = game_id(&value["gameId"])?;
    value["name"]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 80 && !s.chars().any(char::is_control))
        .ok_or("Profile names need 1–80 UTF-8 bytes without control characters.")?;
    let rows = value["devices"]
        .as_array()
        .filter(|d| d.len() <= 32)
        .ok_or("Choose at most 32 peripherals.")?;
    let mut seen = BTreeSet::new();
    let mut devices = Vec::new();
    for row in rows {
        if !known(row, &["gatewayId", "moduleId", "controller", "config"])
            || !matches!(
                row["controller"].as_str(),
                Some("keep" | "enabled" | "disabled")
            )
        {
            return Err("Invalid game peripheral settings.".into());
        }
        let (gateway, module) = target(row)?;
        if !seen.insert((gateway.clone(), module.clone())) {
            return Err(
                "A peripheral cannot be configured more than once through the same gateway.".into(),
            );
        }
        let mut row = row.clone();
        row["gatewayId"] = json!(gateway);
        row["moduleId"] = json!(module);
        if let Some(config) = row.get("config") {
            validate_config(config)?;
        }
        devices.push(row);
    }
    let mut result = value.clone();
    result["devices"] = json!(devices);
    if !value["integration"].is_null() {
        let config = game_integration(&value["integration"])?;
        let expected = if game == "dcs" {
            "dcs-bios"
        } else {
            "assetto-corsa-rally"
        };
        if config.provider() != expected {
            return Err("The integration provider must match this game.".into());
        }
        result["integration"] = config.value();
        if value["integration"]["connection"]["processId"].is_null()
            && result["integration"]["connection"]["source"] == "shared-memory"
        {
            result["integration"]["connection"]
                .as_object_mut()
                .unwrap()
                .remove("processId");
        }
    } else {
        result["integration"] = Value::Null;
    }
    Ok(result)
}
fn default_profile(game: &str) -> Value {
    json!({"schemaVersion":1,"id":format!("{game}-default"),"gameId":game,"name":"Default","devices":[],"integration":null})
}

/// Atomic replacement is also used for the recovery journal before each write.
fn persist(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid games settings path.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(value).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
fn load(path: &Path) -> Result<Option<Value>, String> {
    match fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
        Ok(m) if m.len() > MAX_FILE as u64 => Err("Games settings exceed their size limit.".into()),
        Ok(_) => {
            let mut bytes = Vec::new();
            fs::File::open(path)
                .map_err(|e| e.to_string())?
                .take(MAX_FILE as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > MAX_FILE {
                return Err("Games settings exceed their size limit.".into());
            }
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| e.to_string())
        }
    }
}
#[derive(Clone)]
struct Saved {
    profiles: BTreeMap<String, Value>,
    defaults: BTreeMap<String, String>,
    manual: BTreeMap<String, String>,
}
impl Saved {
    fn fresh() -> Self {
        Self {
            profiles: GAMES
                .iter()
                .map(|g| (format!("{g}-default"), default_profile(g)))
                .collect(),
            defaults: GAMES
                .iter()
                .map(|g| (g.to_string(), format!("{g}-default")))
                .collect(),
            manual: BTreeMap::new(),
        }
    }
    fn parse(value: &Value) -> Result<Self, String> {
        if value.to_string().len() > MAX_SAVED
            || !known(
                value,
                &[
                    "schemaVersion",
                    "profiles",
                    "defaultProfiles",
                    "manualInstallations",
                ],
            )
            || value["schemaVersion"] != 1
        {
            return Err("Invalid saved games settings.".into());
        }
        let rows = value["profiles"]
            .as_array()
            .filter(|v| v.len() <= 32)
            .ok_or("Invalid saved game profiles.")?;
        let mut saved = Self::fresh();
        saved.profiles.clear();
        for row in rows {
            let row = profile(row)?;
            let id = row["id"].as_str().unwrap().to_string();
            if saved.profiles.insert(id, row).is_some() {
                return Err("Duplicate saved game profile.".into());
            }
        }
        for game in GAMES {
            let id = identifier(&value["defaultProfiles"][game], 64)?;
            if saved.profiles.get(id).is_none_or(|p| p["gameId"] != game) {
                return Err("Invalid default game profile.".into());
            }
            saved.defaults.insert(game.into(), id.into());
        }
        let manual = value["manualInstallations"]
            .as_object()
            .filter(|o| o.len() <= 2)
            .ok_or("Invalid manual installations.")?;
        for (game, executable) in manual {
            game_id(&json!(game))?;
            let executable = executable
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control))
                .ok_or("Invalid manual executable path.")?;
            saved.manual.insert(game.into(), executable.into());
        }
        Ok(saved)
    }
    fn value(&self) -> Value {
        json!({"schemaVersion":1,"profiles":self.profiles.values().collect::<Vec<_>>(),"defaultProfiles":self.defaults,"manualInstallations":self.manual})
    }
}
struct Settings {
    saved: Saved,
    catalogue: Vec<Value>,
    journal: Option<Value>,
    error: Option<String>,
}
struct Inner {
    panel: Panel,
    platform: Arc<dyn Platform>,
    path: PathBuf,
    journal_path: PathBuf,
    setup: crate::game_integration_setup::GameIntegrationSetup,
    settings: Mutex<Settings>,
    state: Mutex<Value>,
    busy: AtomicBool,
    closed: AtomicBool,
    cancel: AtomicBool,
}
pub(crate) struct GamesRuntime {
    inner: Arc<Inner>,
    admission: Mutex<()>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl GamesRuntime {
    pub(crate) fn new(panel: Panel, path: PathBuf, platform: Arc<dyn Platform>) -> Self {
        let (saved, mut error) =
            match load(&path).and_then(|v| v.as_ref().map(Saved::parse).transpose()) {
                Ok(s) => (s.unwrap_or_else(Saved::fresh), None),
                Err(e) => (
                    Saved::fresh(),
                    Some(format!("Saved games settings could not be loaded: {e}")),
                ),
            };
        let journal_path = path.with_extension("recovery.json");
        let journal = match load(&journal_path).and_then(|v| v.map(validate_journal).transpose()) {
            Ok(v) => v,
            Err(e) => {
                error = Some(format!("Recovery journal is invalid: {e}. Preserve it and inspect the hardware before making changes."));
                Some(json!({"invalid":true}))
            }
        };
        let state = if journal.is_some() {
            json!({"phase":"recovery-needed","gameId":journal.as_ref().unwrap()["gameId"],"profileId":journal.as_ref().unwrap()["profileId"],"error":"A previous game session needs explicit recovery. Reconnect its peripherals, then recover."})
        } else {
            json!({"phase":"idle","error":null})
        };
        let catalogue = platform.catalogue(&saved.manual);
        Self {
            inner: Arc::new(Inner {
                setup: crate::game_integration_setup::GameIntegrationSetup::new(
                    path.with_extension("integration-setup.json"),
                    platform.integration_setup_supported(),
                ),
                panel,
                platform,
                path,
                journal_path,
                settings: Mutex::new(Settings {
                    saved,
                    catalogue,
                    journal,
                    error,
                }),
                state: Mutex::new(state),
                busy: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                cancel: AtomicBool::new(false),
            }),
            admission: Mutex::new(()),
            worker: Mutex::new(None),
        }
    }
    pub(crate) fn status(&self) -> Value {
        self.status_with_telemetry(&Value::Null, &Value::Null)
    }
    pub(crate) fn status_with_telemetry(&self, integration: &Value, dcs: &Value) -> Value {
        let s = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut catalogue = s.catalogue.clone();
        self.inner
            .setup
            .auto_check(&catalogue, self.inner.platform.clone(), integration);
        self.inner.setup.decorate(&mut catalogue, integration, dcs);
        json!({"catalogue":catalogue,"profiles":s.saved.profiles.values().collect::<Vec<_>>(),"defaultProfiles":s.saved.defaults,
            "manualInstallations":s.saved.manual,"session":self.inner.state.lock().unwrap_or_else(|p|p.into_inner()).clone(),"error":s.error})
    }
    pub(crate) fn active(&self) -> bool {
        self.inner.busy.load(Ordering::Acquire)
            || self.inner.setup.busy()
            || self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .journal
                .is_some()
    }
    pub(crate) fn guard_request(&self, request: &Value) -> Result<(), String> {
        if !self.active() {
            return Ok(());
        }
        let command = request["command"].as_str().unwrap_or("");
        // An independent telemetry verification must not delay emergency Stop.
        if matches!(command, "integration_stop" | "dcs_stop")
            && !self.inner.busy.load(Ordering::Acquire)
            && self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .journal
                .is_none()
        {
            return Ok(());
        }
        if command.starts_with("game_")
            || matches!(
                command,
                "ports"
                    | "status"
                    | "config_read"
                    | "action_describe"
                    | "action_state"
                    | "action_ownership"
                    | "events_state"
                    | "events_read"
                    | "integration_get_state"
                    | "integration_catalogue"
                    | "dcs_status"
                    | "controller_list"
                    | "controller_inspect"
                    | "mapping_panel_sources"
                    | "mapping_panel_inspect"
                    | "mapping_capture"
                    | "mapping_virtual_inspect"
                    | "mapping_trace_export"
                    | "controller_visibility_status"
                    | "controller_visibility_restore"
                    | "controller_visibility_inspect"
                    | "controller_visibility_processes"
                    | "mapping_status"
                    | "mapping_recording"
                    | "mapping_stop"
                    | "dcs_catalogue"
                    | "integration_override"
                    | "integration_simulate"
            )
        {
            return Ok(());
        }
        // Connections and manual edits can change ownership or the global USB
        // descriptor layout, including when another module is edited.
        if command == "connect" {
            return Ok(());
        }
        Err("The game session owns peripheral and integration settings. Deactivate or recover it before making manual changes.".into())
    }
    pub(crate) fn request(&self, request: &Value) -> Result<(), String> {
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        if self.inner.closed.load(Ordering::Acquire) {
            return Err("The games runtime has closed.".into());
        }
        if !request.is_object() || request.to_string().len() > 65536 {
            return Err("Invalid game request.".into());
        }
        let command = request["command"].as_str().ok_or("Missing game command.")?;
        if command == "game_get_state" {
            return Ok(());
        }
        if command.starts_with("game_integration_") {
            let game_id = game_id(&request["gameId"])?;
            let settings = self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let game = settings
                .catalogue
                .iter()
                .find(|g| g["id"] == game_id)
                .cloned()
                .ok_or("Choose a supported game.")?;
            let session_active =
                self.inner.busy.load(Ordering::Acquire) || settings.journal.is_some();
            drop(settings);
            if session_active
                && !matches!(
                    command,
                    "game_integration_check" | "game_integration_cancel"
                )
            {
                return Err(
                    "Deactivate or recover the current game session before integration setup."
                        .into(),
                );
            }
            return self.inner.setup.request(
                request,
                &game,
                self.inner.platform.clone(),
                &self.inner.panel,
            );
        }
        if command == "game_deactivate" {
            if self.inner.busy.load(Ordering::Acquire) {
                self.inner.cancel.store(true, Ordering::Release);
                return Ok(());
            }
            return self.recover();
        }
        if command == "game_recover" {
            return self.recover();
        }
        if command == "game_abandon" {
            if request["preserveCurrentSetup"] != true {
                return Err("Confirm that the current setup should be kept and the previous setup will not be restored.".into());
            }
            if self.inner.busy.load(Ordering::Acquire) {
                return Err("Wait for the current game operation to finish before keeping the current setup.".into());
            }
            let mut settings = self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if settings.journal.is_none() {
                return Err("There is no game recovery journal to archive.".into());
            }
            let archive = self
                .inner
                .journal_path
                .with_file_name(format!("games.abandoned-{}.json", uuid::Uuid::new_v4()));
            fs::rename(&self.inner.journal_path, &archive).map_err(|e| e.to_string())?;
            let abandoned = settings.journal.take();
            settings.error = None;
            drop(settings);
            if let Some(journal) = abandoned.filter(|j| !j["mapping"].is_null()) {
                self.inner.call(
                    "_game_mapping_abandon",
                    json!({"mapping":journal["mapping"]}),
                )?;
            }
            self.inner.phase("idle", None);
            self.inner.step("Recovery journal archived. The current hardware and integration setup was kept; the previous setup was not restored.");
            self.inner.state.lock().unwrap_or_else(|p| p.into_inner())["recoveryArchive"] =
                json!(archive.to_string_lossy());
            return Ok(());
        }
        if self.active() {
            return Err("Deactivate or recover the current game session first.".into());
        }
        if matches!(command, "game_launch" | "game_activate") {
            let game = game_id(&request["gameId"])?;
            let s = self
                .inner
                .settings
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let id = request
                .get("profileId")
                .map(|v| identifier(v, 64))
                .transpose()?
                .unwrap_or(&s.saved.defaults[game]);
            let profile = s
                .saved
                .profiles
                .get(id)
                .filter(|p| p["gameId"] == game)
                .ok_or("Choose a profile belonging to this game.")?
                .clone();
            let installation = s
                .catalogue
                .iter()
                .find(|g| g["id"] == game && g["installed"] == true)
                .cloned()
                .ok_or("Find this game's installation before activating it.")?;
            if command == "game_launch" && installation["launchSupported"] != true {
                return Err("Game launching is unavailable on this platform.".into());
            }
            self.inner.setup.preflight(&profile, &installation)?;
            drop(s);
            self.inner.cancel.store(false, Ordering::Release);
            self.inner.busy.store(true, Ordering::Release);
            self.inner.phase("validating", None);
            {
                let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
                state["gameId"] = profile["gameId"].clone();
                state["profileId"] = profile["id"].clone();
                state["steps"] = json!([]);
                state["pid"] = Value::Null;
                state["bindingWarning"] = Value::Null;
            }
            let inner = self.inner.clone();
            let launch = command == "game_launch";
            let accepted = request["acceptBindingChanges"] == true;
            self.spawn(move || inner.run(profile, installation, launch, accepted));
            return Ok(());
        }
        let mut s = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut draft = s.saved.clone();
        match command {
            "game_scan" => {
                s.catalogue = self.inner.platform.catalogue(&s.saved.manual);
                self.inner.setup.refresh(&s.catalogue);
                return Ok(());
            }
            "game_installation" => {
                let game = game_id(&request["gameId"])?;
                if request["executable"].is_null() {
                    draft.manual.remove(game);
                } else {
                    let executable = request["executable"]
                        .as_str()
                        .filter(|s| {
                            !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control)
                        })
                        .ok_or("Choose a game executable.")?;
                    self.inner.platform.validate(game, executable)?;
                    draft.manual.insert(game.into(), executable.into());
                }
            }
            "game_profile_save" => {
                let p = profile(&request["gameProfile"])?;
                let id = p["id"].as_str().unwrap().to_string();
                if draft
                    .profiles
                    .get(&id)
                    .is_some_and(|old| old["gameId"] != p["gameId"])
                {
                    return Err("A saved profile cannot change games.".into());
                }
                if draft.profiles.len() >= 32 && !draft.profiles.contains_key(&id) {
                    return Err("Choose at most 32 game profiles.".into());
                }
                draft.profiles.insert(id, p);
            }
            "game_profile_delete" => {
                let id = identifier(&request["profileId"], 64)?;
                if draft.defaults.values().any(|v| v == id) {
                    return Err("Choose another default before deleting this profile.".into());
                }
                if draft.profiles.remove(id).is_none() {
                    return Err("Unknown game profile.".into());
                }
            }
            "game_profile_default" => {
                let game = game_id(&request["gameId"])?;
                let id = identifier(&request["profileId"], 64)?;
                if draft.profiles.get(id).is_none_or(|p| p["gameId"] != game) {
                    return Err("Choose a profile belonging to this game.".into());
                }
                draft.defaults.insert(game.into(), id.into());
            }
            _ => return Err("Unknown game command.".into()),
        }
        if draft.value().to_string().len() > MAX_SAVED {
            return Err("Saved game profiles exceed the 48 KiB library limit. Remove unused profiles or mappings.".into());
        }
        persist(&self.inner.path, &draft.value())?;
        s.catalogue = self.inner.platform.catalogue(&draft.manual);
        self.inner.setup.refresh(&s.catalogue);
        s.saved = draft;
        s.error = None;
        Ok(())
    }
    fn spawn(&self, work: impl FnOnce() + Send + 'static) {
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(previous) = worker.take() {
            let _ = previous.join();
        }
        *worker = Some(thread::spawn(work));
    }
    fn recover(&self) -> Result<(), String> {
        if self.inner.busy.load(Ordering::Acquire) {
            return Err("Wait for the current game operation to finish.".into());
        }
        let journal = self
            .inner
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .journal
            .clone();
        let Some(journal) = journal else {
            self.inner.phase("idle", None);
            return Ok(());
        };
        let journal = validate_journal(journal)?;
        self.inner.busy.store(true, Ordering::Release);
        self.inner.cancel.store(false, Ordering::Release);
        self.inner.phase("restoring", None);
        let inner = self.inner.clone();
        self.spawn(move || {
            let result = inner.restore(journal);
            inner.finish(result, None);
        });
        Ok(())
    }
    pub(crate) fn shutdown(&self) {
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        self.inner.closed.store(true, Ordering::Release);
        self.inner.setup.shutdown();
        self.inner.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
    }
}
impl Drop for GamesRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn validate_journal(value: Value) -> Result<Value, String> {
    if !matches!(value["schemaVersion"].as_u64(), Some(1 | 2)) {
        return Err("Unsupported recovery journal.".into());
    }
    crate::game_mapping::journal(&value)?;
    game_id(&value["gameId"])?;
    identifier(&value["profileId"], 64)?;
    let rows = value["devices"]
        .as_array()
        .filter(|d| d.len() <= 32)
        .ok_or("Invalid recovery devices.")?;
    let mut seen = BTreeSet::new();
    for row in rows {
        let (gateway, module) = target(row)?;
        if row["gatewayId"] != gateway
            || row["moduleId"] != module
            || !seen.insert((gateway, module))
            || row["boot"].as_u64().is_none_or(|b| b == 0)
            || !row["baselineEnabled"].is_boolean()
            || !row["desiredEnabled"].is_boolean()
            || !row["configAttempted"].is_boolean()
            || !row["controllerAttempted"].is_boolean()
            || !row["publicationAttempted"].is_boolean()
        {
            return Err("Invalid recovery device state.".into());
        }
        validate_config(&row["baselineConfig"])?;
        validate_config(&row["desiredConfig"])?;
    }
    for key in ["baselineIntegration", "desiredIntegration"] {
        if !value[key].is_null() {
            game_integration(&value[key])?;
        }
    }
    if !value["integrationTaken"].is_boolean()
        || !value["integrationChanged"].is_boolean()
        || !value["baselineIntegrationRunning"].is_boolean()
        || !value["baselineDcsRunning"].is_boolean()
    {
        return Err("Invalid integration recovery state.".into());
    }
    Ok(value)
}
impl Inner {
    fn phase(&self, phase: &str, error: Option<String>) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state["phase"] = json!(phase);
        state["error"] = json!(error);
    }
    fn step(&self, message: &str) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !s["steps"].is_array() {
            s["steps"] = json!([]);
        }
        let steps = s["steps"].as_array_mut().unwrap();
        if steps.len() < 32 {
            steps.push(json!(message));
        }
    }
    fn call(&self, command: &str, extra: Value) -> Result<Value, String> {
        let mut request = extra;
        request["command"] = json!(command);
        request["clientId"] = json!(CLIENT);
        let reply = (self.panel)(request);
        if reply["ok"] != true {
            return Err(reply["error"]
                .as_str()
                .unwrap_or("The game setup request failed.")
                .into());
        }
        Ok(reply["state"].clone())
    }
    fn device_once(&self, row: &Value) -> Result<(Value, Value), String> {
        let state = self.call(
            "config_read",
            json!({"gatewayId":row["gatewayId"],"moduleId":row["moduleId"]}),
        )?;
        let module = state["modules"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|m| m["id"] == row["moduleId"])
            .ok_or("Reconnect the game profile peripheral.")?;
        if state["connected"] != true
            || state["reconnecting"] == true
            || state["layoutPending"] == true
            || module["online"] != true
            || module["boot"].as_u64().is_none_or(|b| b == 0)
            || !module["enabled"].is_boolean()
        {
            return Err(
                "Reconnect the game profile peripheral and wait for its layout to settle.".into(),
            );
        }
        let config = &state["moduleConfigs"][row["moduleId"]
            .as_str()
            .ok_or("Invalid peripheral identity.")?];
        crate::runtime::validate(config)?;
        if config["view"] != "active"
            || config["pendingApply"] != false
            || config["owner"]
                .as_u64()
                .is_some_and(|o| o != 0 && o != crate::runtime::owner(CLIENT) as u64)
        {
            return Err(
                "End the peripheral's setup session before activating a game profile.".into(),
            );
        }
        Ok((module.clone(), config.clone()))
    }
    fn wait_duration() -> Duration {
        Duration::from_millis(if cfg!(test) { 100 } else { 30_000 })
    }
    fn wait_cancelled(&self) -> Result<(), String> {
        if self.state.lock().unwrap_or_else(|p| p.into_inner())["phase"] != "restoring" {
            self.cancelled()?;
        }
        Ok(())
    }
    fn device(&self, row: &Value) -> Result<(Value, Value), String> {
        let until = Instant::now() + Self::wait_duration();
        loop {
            self.wait_cancelled()?;
            match self.device_once(row) {
                Ok(device) => return Ok(device),
                Err(e) if Instant::now() >= until => return Err(e),
                Err(_) => thread::sleep(Duration::from_millis(if cfg!(test) { 2 } else { 100 })),
            }
        }
    }
    fn checkpoint(&self, journal: &Value) -> Result<(), String> {
        persist(&self.journal_path, journal)?;
        self.settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .journal = Some(journal.clone());
        Ok(())
    }
    fn cancelled(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::Acquire) {
            Err("Game activation was cancelled.".into())
        } else {
            Ok(())
        }
    }
    fn snapshot(&self, profile: &Value, accepted: bool) -> Result<Value, String> {
        crate::game_mapping::overlap(profile)?;
        let mapping = if profile["mapping"].is_null() {
            Value::Null
        } else {
            self.call(
                "_game_mapping_snapshot",
                json!({"capture":profile["mapping"]}),
            )?
        };
        let integration = self.call("integration_get_state", json!({}))?["integration"].clone();
        let dcs = self.call("dcs_status", json!({}))?["dcsBios"].clone();
        if !integration.is_object() || !dcs.is_object() {
            return Err("Refresh the integration state before activating a game.".into());
        }
        if integration["running"] == true && dcs["running"] == true {
            return Err("Stop competing integrations before activating a game.".into());
        }
        let mut rows = profile["devices"].as_array().unwrap().clone();
        if !profile["integration"].is_null() {
            for assignment in game_integration(&profile["integration"])?.assignments {
                if !rows.iter().any(|r| {
                    r["gatewayId"] == assignment.gateway_id && r["moduleId"] == assignment.module_id
                }) {
                    rows.push(json!({"gatewayId":assignment.gateway_id,"moduleId":assignment.module_id,"controller":"keep"}));
                }
            }
        }
        if rows.len() > 32 {
            return Err(
                "The combined profile and integration need more than 32 peripherals.".into(),
            );
        }
        let mut devices = Vec::new();
        let mut changed = false;
        for row in rows {
            self.cancelled()?;
            let (module, config) = self.device(&row)?;
            if module["enabled"] == true && module["profileMatch"] == false {
                return Err("An enabled peripheral's HID profile already differs from its configuration. Reconcile it before activating a game profile.".into());
            }
            let desired_config = row.get("config").unwrap_or(&config["config"]);
            let desired_enabled = match row["controller"].as_str() {
                Some("enabled") => true,
                Some("disabled") => false,
                _ => module["enabled"].as_bool().unwrap(),
            };
            changed |= desired_config != &config["config"] || module["enabled"] != desired_enabled;
            devices.push(json!({"gatewayId":row["gatewayId"],"moduleId":row["moduleId"],"boot":module["boot"],
                "baselineConfig":config["config"],"desiredConfig":desired_config,"baselineEnabled":module["enabled"],"desiredEnabled":desired_enabled,
                "configAttempted":false,"controllerAttempted":false,"publicationAttempted":false}));
        }
        if !profile["integration"].is_null() {
            let validation = self.call("integration_preflight", json!({"integrationConfig":game_integration(&profile["integration"])?.value(),"gameDevices":profile["devices"]}))?["integration"]["validation"].clone();
            if validation["valid"] != true {
                return Err(validation["diagnostics"][0]["message"].as_str().unwrap_or("Game integration preflight failed. Review its catalogue, bindings and peripheral capabilities.").into());
            }
        }
        if changed && !accepted {
            self.state.lock().unwrap_or_else(|p| p.into_inner())["bindingWarning"] =
                json!(BINDING_WARNING);
            return Err(BINDING_WARNING.into());
        }
        Ok(
            json!({"schemaVersion":profile["schemaVersion"],"mapping":mapping,"gameId":profile["gameId"],"profileId":profile["id"],"devices":devices,
            "baselineIntegration":integration["config"],"baselineIntegrationRunning":integration["running"]==true,
            "baselineDcs":dcs["config"],"baselineDcsRunning":dcs["running"]==true,"desiredIntegration":profile["integration"],
            "integrationTaken":false,"integrationChanged":false}),
        )
    }
    fn write_config(&self, row: &Value, desired: &Value) -> Result<(), String> {
        let route = json!({"gatewayId":row["gatewayId"],"moduleId":row["moduleId"]});
        self.call("claim", route.clone())?;
        let result = (|| {
            // Claim must still address the same boot; the adapter also guards
            // the CAN route throughout its one-shot apply and readback.
            let (module, current) = self.device(row)?;
            if module["boot"] != row["boot"] {
                return Err("The peripheral restarted; recovery needs inspection.".into());
            }
            if current["config"] != row["baselineConfig"]
                && current["config"] != row["desiredConfig"]
            {
                return Err(
                    "Peripheral settings changed before the setup claim. Recovery preserved them."
                        .into(),
                );
            }
            let mut request = route.clone();
            request["config"] = desired.clone();
            self.call("config_apply", request).map(|_| ())
        })();
        let released = self.call("release", route).map(|_| ());
        result.and(released)
    }
    fn readback(&self, row: &Value, config: &Value, enabled: &Value) -> Result<(), String> {
        let until = Instant::now() + Self::wait_duration();
        loop {
            self.wait_cancelled()?;
            let result = self.device_once(row).and_then(|(module, current)| {
                if module["boot"] != row["boot"] {
                    return Err("The peripheral restarted during the game session. Recovery cannot overwrite its new setup.".into());
                }
                if current["config"] != *config || module["enabled"] != *enabled {
                    return Err("Peripheral readback does not confirm the requested game setup. No write was repeated.".into());
                }
                Ok(())
            });
            match result {
                Ok(()) => return Ok(()),
                Err(e) if Instant::now() >= until => return Err(e),
                Err(_) => thread::sleep(Duration::from_millis(if cfg!(test) { 2 } else { 100 })),
            }
        }
    }
    fn publish(
        &self,
        journal: &mut Value,
        index: usize,
        config: &Value,
        enabled: &Value,
    ) -> Result<(), String> {
        if *enabled != true {
            return Ok(());
        }
        let row = journal["devices"][index].clone();
        let (module, current) = self.device(&row)?;
        if module["boot"] != row["boot"]
            || current["config"] != *config
            || module["enabled"] != *enabled
        {
            return Err("The peripheral changed before its HID profile could be published.".into());
        }
        if module["profileMatch"] != false {
            return Ok(());
        }
        journal["devices"][index]["publicationAttempted"] = json!(true);
        self.checkpoint(journal)?;
        let result = self.call(
            "update-profile",
            json!({"gatewayId":row["gatewayId"],"moduleId":row["moduleId"],"boot":row["boot"]}),
        );
        self.readback(&row, config, enabled)?;
        let (module, _) = self.device(&row)?;
        if module["profileMatch"] != true {
            return result.map(|_| ()).and(Err(
                "HID profile readback did not confirm publication. No update was repeated.".into(),
            ));
        }
        Ok(())
    }
    fn apply(&self, journal: &mut Value) -> Result<(), String> {
        self.phase("applying", None);
        if !journal["mapping"].is_null() {
            journal["mapping"]["taken"] = json!(true);
            self.checkpoint(journal)?;
            self.call("_game_mapping_take", json!({"mapping":journal["mapping"]}))?;
        }
        journal["integrationTaken"] = json!(true);
        self.checkpoint(journal)?;
        self.call("integration_stop", json!({}))?;
        self.call("dcs_stop", json!({}))?;
        self.step("Previous integration stopped.");
        let count = journal["devices"].as_array().unwrap().len();
        for index in 0..count {
            self.cancelled()?;
            let row = journal["devices"][index].clone();
            self.readback(&row, &row["baselineConfig"], &row["baselineEnabled"])?;
            if row["desiredConfig"] != row["baselineConfig"] {
                journal["devices"][index]["configAttempted"] = json!(true);
                self.checkpoint(journal)?;
                let result = self.write_config(&row, &row["desiredConfig"]);
                // A lost acknowledgement is reconciled only by reads.
                let verified = self.readback(&row, &row["desiredConfig"], &row["baselineEnabled"]);
                if verified.is_err() {
                    return result.and(verified);
                }
            }
            if row["desiredEnabled"] != row["baselineEnabled"] {
                journal["devices"][index]["controllerAttempted"] = json!(true);
                self.checkpoint(journal)?;
                let command = if row["desiredEnabled"] == true {
                    "enable"
                } else {
                    "disable"
                };
                let result = self.call(
                    command,
                    json!({"gatewayId":row["gatewayId"],"moduleId":row["moduleId"],"boot":row["boot"]}),
                );
                let verified = self.readback(&row, &row["desiredConfig"], &row["desiredEnabled"]);
                if verified.is_err() {
                    return result.map(|_| ()).and(verified);
                }
            }
            self.publish(
                journal,
                index,
                &row["desiredConfig"],
                &row["desiredEnabled"],
            )?;
            self.step("Peripheral setup confirmed.");
        }
        Ok(())
    }
    fn start_integration(
        &self,
        journal: &mut Value,
        process: Option<&crate::game_discovery::ProcessIdentity>,
    ) -> Result<(), String> {
        if journal["desiredIntegration"].is_null() {
            return Ok(());
        }
        let mut config = journal["desiredIntegration"].clone();
        if config["document"]["provider"] == "assetto-corsa-rally"
            && config["connection"]["source"] == "shared-memory"
        {
            let process = process.ok_or("Launch Rally before starting live telemetry.")?;
            config["connection"]["processId"] = json!(process.pid);
        }
        journal["desiredIntegration"] = config.clone();
        journal["integrationChanged"] = json!(true);
        self.checkpoint(journal)?;
        self.call("integration_save", json!({"integrationConfig":config}))?;
        self.call("integration_start", json!({}))?;
        self.step(
            "Game integration started; telemetry updates hardware through its saved bindings.",
        );
        Ok(())
    }
    fn run(self: Arc<Self>, profile: Value, installation: Value, launch: bool, accepted: bool) {
        let mut journal = None;
        let result = (|| {
            self.platform.validate(
                profile["gameId"].as_str().unwrap(),
                installation["executable"]
                    .as_str()
                    .ok_or("No executable was found.")?,
            )?;
            self.cancelled()?;
            self.setup.preflight(&profile, &installation)?;
            let snapshot = self.snapshot(&profile, accepted)?;
            self.checkpoint(&snapshot)?;
            journal = Some(snapshot);
            let j = journal.as_mut().unwrap();
            self.apply(j)?;
            if !j["mapping"].is_null() {
                self.cancelled()?;
                j["mapping"]["armed"] = json!(true);
                self.checkpoint(j)?;
                self.call("_game_mapping_arm", json!({"mapping":j["mapping"]}))?;
                self.step("Captured mapping armed with fresh input baselines.");
            }
            let delayed = j["desiredIntegration"]["document"]["provider"] == "assetto-corsa-rally"
                && j["desiredIntegration"]["connection"]["source"] == "shared-memory";
            if !delayed {
                self.start_integration(j, None)?;
            }
            self.cancelled()?;
            let process = if launch {
                self.phase("launching", None);
                let existing = self.platform.find(&installation)?;
                if existing.is_none() {
                    self.phase("waiting", None);
                }
                Some(match existing {
                    Some(p) => p,
                    None => self.platform.launch(&installation)?,
                })
            } else if installation["processMonitoringSupported"]
                .as_bool()
                .unwrap_or(installation["launchSupported"] == true)
            {
                self.platform.find(&installation)?
            } else {
                None
            };
            self.cancelled()?;
            if delayed {
                self.start_integration(j, process.as_ref())?;
            }
            self.phase("running", None);
            {
                let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
                s["pid"] = json!(process.as_ref().map(|p| p.pid));
                s["process"] = process.as_ref().map(|p| p.value()).unwrap_or(Value::Null);
            }
            self.step(if launch {
                "Game process found and profile active."
            } else {
                "Game profile active."
            });
            while !self.cancel.load(Ordering::Acquire) {
                if let Some(process) = &process {
                    if !self.platform.alive(process)? {
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(if cfg!(test) { 5 } else { 250 }));
            }
            Ok(())
        })();
        let activation_error = result.err();
        let recovery = journal
            .map(|j| self.restore_with_context(j, false))
            .unwrap_or(Ok(()));
        self.finish(recovery, activation_error);
    }
    fn restore(&self, journal: Value) -> Result<(), String> {
        self.restore_with_context(journal, true)
    }
    fn restore_with_context(&self, mut journal: Value, recovery: bool) -> Result<(), String> {
        self.phase("restoring", None);
        self.step("Restoring the previous peripheral and integration setup.");
        if journal["mapping"]["taken"] == true {
            self.call("_game_mapping_stop", json!({"mapping":journal["mapping"]}))?;
            journal["mapping"]["stopped"] = json!(true);
            self.checkpoint(&journal)?;
        }
        if journal["integrationTaken"] == true {
            let current = self.call("integration_get_state", json!({}))?["integration"].clone();
            let expected = if journal["integrationChanged"] == true {
                &journal["desiredIntegration"]
            } else {
                &journal["baselineIntegration"]
            };
            // A save can fail before replacement. Both expected and baseline
            // are acknowledged safe states; a third setup belongs to someone else.
            if current["config"] != *expected && current["config"] != journal["baselineIntegration"]
            {
                return Err("Integration settings changed outside the game session. Recovery preserved them.".into());
            }
            let dcs = self.call("dcs_status", json!({}))?["dcsBios"].clone();
            if dcs["config"] != journal["baselineDcs"] {
                return Err(
                    "DCS-BIOS settings changed outside the game session. Recovery preserved them."
                        .into(),
                );
            }
            self.call("integration_stop", json!({}))?;
            self.call("dcs_stop", json!({}))?;
        }
        for index in (0..journal["devices"].as_array().unwrap().len()).rev() {
            let row = journal["devices"][index].clone();
            if row["configAttempted"] != true
                && row["controllerAttempted"] != true
                && row["publicationAttempted"] != true
            {
                continue;
            }
            let (module, current) = self.device(&row)?;
            if module["boot"] != row["boot"] {
                if current["config"] == row["baselineConfig"]
                    && module["enabled"] == row["baselineEnabled"]
                    && (module["enabled"] != true || module["profileMatch"] == true)
                {
                    journal["devices"][index]["configAttempted"] = json!(false);
                    journal["devices"][index]["controllerAttempted"] = json!(false);
                    journal["devices"][index]["publicationAttempted"] = json!(false);
                    self.checkpoint(&journal)?;
                    continue;
                }
                return Err("A game peripheral restarted. Recovery preserved the new boot's setup for inspection.".into());
            }
            if current["config"] != row["baselineConfig"]
                && current["config"] != row["desiredConfig"]
            {
                return Err("Peripheral configuration changed outside the game session. Recovery preserved it.".into());
            }
            if module["enabled"] != row["baselineEnabled"]
                && module["enabled"] != row["desiredEnabled"]
            {
                return Err("Peripheral controller state changed outside the game session.".into());
            }
            if row["configAttempted"] == true && current["config"] != row["baselineConfig"] {
                self.checkpoint(&journal)?;
                let result = self.write_config(&row, &row["baselineConfig"]);
                let verified = self.readback(&row, &row["baselineConfig"], &module["enabled"]);
                if verified.is_err() {
                    return result.and(verified);
                }
            }
            if row["controllerAttempted"] == true && module["enabled"] != row["baselineEnabled"] {
                self.checkpoint(&journal)?;
                let command = if row["baselineEnabled"] == true {
                    "enable"
                } else {
                    "disable"
                };
                let result = self.call(
                    command,
                    json!({"gatewayId":row["gatewayId"],"moduleId":row["moduleId"],"boot":row["boot"]}),
                );
                let verified = self.readback(&row, &row["baselineConfig"], &row["baselineEnabled"]);
                if verified.is_err() {
                    return result.map(|_| ()).and(verified);
                }
            }
            self.publish(
                &mut journal,
                index,
                &row["baselineConfig"],
                &row["baselineEnabled"],
            )?;
            journal["devices"][index]["configAttempted"] = json!(false);
            journal["devices"][index]["controllerAttempted"] = json!(false);
            journal["devices"][index]["publicationAttempted"] = json!(false);
            self.checkpoint(&journal)?;
        }
        if journal["integrationTaken"] == true {
            if journal["integrationChanged"] == true {
                if journal["baselineIntegration"].is_null() {
                    self.call("integration_clear", json!({}))?;
                } else {
                    self.call(
                        "integration_save",
                        json!({"integrationConfig":journal["baselineIntegration"]}),
                    )?;
                }
            }
            if journal["baselineIntegrationRunning"] == true {
                self.call("integration_start", json!({}))?;
            }
            if journal["baselineDcsRunning"] == true {
                self.call("dcs_start", json!({}))?;
            }
            journal["integrationTaken"] = json!(false);
            journal["integrationChanged"] = json!(false);
            self.checkpoint(&journal)?;
        }
        if journal["mapping"]["taken"] == true && journal["mapping"]["resumed"] != true {
            self.checkpoint(&journal)?;
            self.call(
                "_game_mapping_resume",
                json!({"mapping":journal["mapping"],"recovery":recovery}),
            )?;
            journal["mapping"]["resumed"] = json!(true);
            self.checkpoint(&journal)?;
            if recovery {
                self.step("Mapping stayed stopped after recovery; start it explicitly when ready.");
            }
        }
        fs::remove_file(&self.journal_path).map_err(|e| e.to_string())?;
        self.settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .journal = None;
        Ok(())
    }
    fn finish(&self, result: Result<(), String>, activation_error: Option<String>) {
        match result {
            Ok(()) => {
                self.step("Previous setup restored.");
                self.phase(
                    if activation_error.is_some() {
                        "failed"
                    } else {
                        "idle"
                    },
                    activation_error,
                );
            }
            Err(e) => self.phase(
                "recovery-needed",
                Some(match activation_error {
                    Some(original) => format!("{original} Recovery needs attention: {e}"),
                    None => e,
                }),
            ),
        }
        self.busy.store(false, Ordering::Release);
    }
}

#[cfg(test)]
#[path = "games_tests.rs"]
mod tests;
