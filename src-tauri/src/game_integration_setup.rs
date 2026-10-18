//! Asynchronous game-side provisioning, separate from captured binding profiles.
use crate::game_discovery::Platform;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

const MAX_SETTINGS: usize = 32768;
struct Inner {
    path: PathBuf,
    work: PathBuf,
    native: bool,
    settings_error: Option<String>,
    saved: Mutex<Value>,
    states: Mutex<BTreeMap<String, Value>>,
    catalogues: Mutex<provider_settings::CatalogueCache>,
    cancel: AtomicBool,
    busy: AtomicBool,
    closed: AtomicBool,
}
#[path = "game_provider_settings.rs"]
mod provider_settings;
pub(crate) struct GameIntegrationSetup {
    inner: Arc<Inner>,
    worker: Mutex<Option<JoinHandle<()>>>,
    admission: Mutex<()>,
}
fn write_settings(path: &Path, value: &Value) -> Result<(), String> {
    crate::dcs_setup::replace(path, value.to_string().as_bytes())
}

fn read_settings(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({"schemaVersion":1,"games":{}}));
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_SETTINGS as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_SETTINGS {
        return Err("Integration setup settings exceed their size limit.".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value.as_object().is_none_or(|object| {
        object
            .keys()
            .any(|key| !["schemaVersion", "games"].contains(&key.as_str()))
    }) || value["schemaVersion"] != 1
        || !value["games"].is_object()
        || value["games"].as_object().unwrap().keys().any(|k| {
            !matches!(
                k.as_str(),
                "dcs" | "assetto-corsa-rally" | "x-plane-11" | "x-plane-12"
            )
        })
    {
        return Err(
            "Invalid integration setup settings. Preserve the file and repair it before setup."
                .into(),
        );
    }
    for (game, record) in value["games"].as_object().unwrap() {
        if !record.is_object() || !record["connection"].is_object() {
            return Err("Invalid saved integration setup record.".into());
        }
        validate_connection(game, &record["connection"])?;
        provider_settings::validate_saved_record(game, record)?;
        if game == "dcs" {
            absolute_folder(&record["userFolder"])?;
        }
    }
    Ok(value)
}
fn absolute_folder(value: &Value) -> Result<PathBuf, String> {
    let path = value
        .as_str()
        .filter(|p| !p.is_empty() && p.len() <= 4096 && !p.chars().any(char::is_control))
        .ok_or("Choose an absolute DCS Saved Games user folder.")?;
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err("Choose an absolute DCS Saved Games user folder.".into());
    }
    Ok(path)
}
fn folder(value: &Value) -> Result<PathBuf, String> {
    let path = absolute_folder(value)?;
    if !path.is_dir() {
        return Err(
            "Choose an existing DCS Saved Games user folder. Start DCS once if it is missing."
                .into(),
        );
    }
    Ok(path)
}
fn reject_game_install_folder(target: &Path, game: &Value) -> Result<(), String> {
    let Some(installation) = game["installPath"].as_str() else {
        return Ok(());
    };
    let target = fs::canonicalize(target).map_err(|e| e.to_string())?;
    let installation = fs::canonicalize(installation)
        .map_err(|e| format!("Cannot verify the game installation boundary: {e}"))?;
    let target_parts = target.components().collect::<Vec<_>>();
    let install_parts = installation.components().collect::<Vec<_>>();
    let inside = target_parts.len() >= install_parts.len()
        && target_parts.iter().zip(install_parts.iter()).all(|(a, b)| {
            if cfg!(windows) {
                a.as_os_str().to_string_lossy().to_lowercase()
                    == b.as_os_str().to_string_lossy().to_lowercase()
            } else {
                a == b
            }
        });
    if inside {
        return Err("Choose a DCS Saved Games user folder outside the game installation. Setup never writes inside the installed game's folders.".into());
    }
    Ok(())
}
pub(crate) fn dcs_connection(path: &Path) -> Value {
    // Match upstream's default multicast route instead of restricting membership
    // to the loopback interface. Existing explicit interface overrides are kept.
    // https://learn.microsoft.com/en-us/windows/win32/api/ws2ipdef/ns-ws2ipdef-ip_mreq
    json!({"cataloguePath":path,"freshnessMs":1500,"commandHost":"127.0.0.1","commandPort":7778,"listenAddress":"0.0.0.0","listenPort":5010,"multicastGroup":"239.255.50.10","multicastInterface":"0.0.0.0"})
}
fn validate_connection(game: &str, value: &Value) -> Result<(), String> {
    if game.starts_with("x-plane-") {
        crate::xplane::validate_connection(value)?;
        if value["version"].as_u64() != game.trim_start_matches("x-plane-").parse::<u64>().ok() {
            return Err("X-Plane connection version must match the game.".into());
        }
        return Ok(());
    }
    if !value.is_object() || value.to_string().len() > 8192 {
        return Err("Invalid local game connection settings.".into());
    }
    let keys = if game == "dcs" {
        vec![
            "cataloguePath",
            "freshnessMs",
            "commandHost",
            "commandPort",
            "listenAddress",
            "listenPort",
            "multicastGroup",
            "multicastInterface",
        ]
    } else {
        vec!["freshnessMs", "source"]
    };
    if value
        .as_object()
        .unwrap()
        .keys()
        .any(|k| !keys.contains(&k.as_str()))
    {
        return Err("Unsupported local game connection setting.".into());
    }
    if value["freshnessMs"]
        .as_u64()
        .is_none_or(|v| !(100..=10000).contains(&v))
    {
        return Err("Use a freshness timeout from 100 to 10000 ms.".into());
    }
    if game != "dcs" {
        return if value["source"] == "shared-memory" {
            Ok(())
        } else {
            Err("ACR setup requires the native shared-memory source.".into())
        };
    }
    if !value["cataloguePath"].as_str().is_some_and(|s| {
        s.len() <= 4096 && Path::new(s).is_absolute() && !s.chars().any(char::is_control)
    }) {
        return Err("Choose an absolute DCS catalogue directory.".into());
    }
    crate::dcs_bios::Network::parse(value)?;
    Ok(())
}
fn compatible_dcs_connection(value: &Value) -> Result<(), String> {
    validate_connection("dcs", value)?;
    let network = crate::dcs_bios::Network::parse(value)?;
    if !network.command_host.is_loopback()
        || network.command_port != 7778
        || network.listen_port != 5010
        || network.multicast_group.map(|g| g.to_string()).as_deref() != Some("239.255.50.10")
    {
        return Err("Saved UDP overrides were preserved, but they do not match the installed DCS-BIOS defaults (local command port 7778; multicast 239.255.50.10:5010). Review the connection settings before integration.".into());
    }
    Ok(())
}
fn blocking_acr_status(proof: &Value) -> bool {
    matches!(
        proof["status"].as_str(),
        Some(
            "unsupported-layout"
                | "access-denied"
                | "conflicting-producer"
                | "executable-changed"
                | "unsupported-platform"
        )
    )
}
impl GameIntegrationSetup {
    pub(crate) fn new(path: PathBuf, native: bool) -> Self {
        let loaded = read_settings(&path);
        let states = BTreeMap::new();
        Self {
            inner: Arc::new(Inner {
                work: path.with_extension("setup-work"),
                path,
                native,
                settings_error: loaded.as_ref().err().cloned(),
                saved: Mutex::new(loaded.unwrap_or(json!({"schemaVersion":1,"games":{}}))),
                states: Mutex::new(states),
                catalogues: Mutex::new(BTreeMap::new()),
                cancel: AtomicBool::new(false),
                busy: AtomicBool::new(false),
                closed: AtomicBool::new(false),
            }),
            worker: Mutex::new(None),
            admission: Mutex::new(()),
        }
    }
    pub(crate) fn busy(&self) -> bool {
        self.inner.busy.load(Ordering::Acquire)
    }
    pub(crate) fn pending(&self) -> bool {
        crate::dcs_setup::recovery_pending(&self.inner.work)
    }
    pub(crate) fn decorate(&self, catalogue: &mut [Value]) {
        for game in catalogue {
            let id = game["id"].as_str().unwrap_or("");
            let cached = self
                .inner
                .states
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(id)
                .cloned();
            let mut status = cached.unwrap_or_else(|| {
                let status = self.inner.inspect(game, None);
                self.inner
                    .states
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(id.into(), status.clone());
                status
            });
            status["live"] = json!("waiting");
            status["liveMessage"]=json!("Setup is configuration metadata. Inspect the applied profile for actual source delivery.");
            if let Some(object) = status.as_object_mut() {
                object.remove("channels");
                object.remove("optionalMaxRpm");
            }
            if id == "dcs" {
                status["version"] = json!(crate::dcs_setup::VERSION);
            }
            self.decorate_provider_settings(id, &mut status);
            game["integrationSetup"] = status;
        }
    }
    pub(crate) fn refresh(&self, catalogue: &[Value]) {
        if self.busy() {
            return;
        }
        for game in catalogue {
            let status = self.inner.inspect(game, None);
            self.inner
                .states
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(game["id"].as_str().unwrap_or("").into(), status);
        }
    }
    pub(crate) fn request(
        &self,
        request: &Value,
        game: &Value,
        platform: Arc<dyn Platform>,
    ) -> Result<(), String> {
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        if self.inner.closed.load(Ordering::Acquire) {
            return Err("The integration setup runtime has closed.".into());
        }
        let command = request["command"].as_str().unwrap_or("");
        if command == "game_integration_cancel" {
            self.inner.cancel.store(true, Ordering::Release);
            return Ok(());
        }
        if self.busy() {
            return Err("Wait for integration setup or cancel it first.".into());
        }
        let id = game["id"].as_str().ok_or("Choose a supported game.")?;
        if command == "game_integration_settings_save" {
            return self.save_provider_settings(request, game);
        }
        if command == "game_integration_check"
            && id == "assetto-corsa-rally"
            && self.inner.native
            && self.inner.settings_error.is_none()
        {
            return self.begin_acr(request, game, platform, false);
        }
        if command == "game_integration_check" {
            let status = self.inner.inspect(game, request.get("userFolder"));
            self.inner
                .states
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(id.into(), status);
            return Ok(());
        }
        if !self.inner.native {
            return Err("Native integration setup is available on Windows.".into());
        }
        if command == "game_integration_recover" {
            if id != "dcs" {
                return Err("DCS setup recovery belongs to the DCS game.".into());
            }
            if platform.find(game)?.is_some() {
                return Err("Close DCS before restoring its integration files.".into());
            }
            let restored = crate::dcs_setup::recover(&self.inner.work)?;
            if restored["restored"] != true {
                return Err(format!(
                    "Setup recovery preserved external changes: {}",
                    restored["conflicts"]
                ));
            }
            let status = self.inner.inspect(game, None);
            self.inner
                .states
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(id.into(), status);
            return Ok(());
        }
        if let Some(error) = &self.inner.settings_error {
            return Err(error.clone());
        }
        if command == "game_integration_setup" && id.starts_with("x-plane-") {
            if game["installed"] != true {
                return Err("Choose an X-Plane installation first.".into());
            }
            let executable = game["executable"]
                .as_str()
                .ok_or("Choose the X-Plane executable.")?;
            platform.validate(id, executable)?;
            let version = id.trim_start_matches("x-plane-").parse::<u64>().unwrap();
            let mut saved = self.inner.saved.lock().unwrap_or_else(|p| p.into_inner());
            let mut connection = saved["games"][id]["connection"]
                .as_object()
                .map(|_| saved["games"][id]["connection"].clone())
                .unwrap_or_else(|| crate::xplane::defaults(version));
            if let Some(overrides) = request.get("connection") {
                for (k, v) in overrides
                    .as_object()
                    .ok_or("Invalid X-Plane connection overrides.")?
                {
                    if !["host", "port", "listenPort", "updateHz", "freshnessMs"]
                        .contains(&k.as_str())
                    {
                        return Err("Unsupported X-Plane setup override.".into());
                    }
                    connection[k] = v.clone();
                }
            }
            validate_connection(id, &connection)?;
            let mut draft = saved.clone();
            draft["games"][id] = json!({"connection":connection,"executable":executable});
            write_settings(&self.inner.path, &draft)?;
            *saved = draft;
            drop(saved);
            self.refresh(std::slice::from_ref(game));
            return Ok(());
        }
        if command == "game_integration_setup" && id == "assetto-corsa-rally" {
            return self.begin_acr(request, game, platform, true);
        }
        if command != "game_integration_setup" || id != "dcs" {
            return Err("Automatic setup for this game is not yet available.".into());
        }
        if game["installed"] != true {
            return Err("Find the game installation before setting up integration.".into());
        }
        if self.pending() {
            return Err("Recover the interrupted DCS setup before installing again.".into());
        }
        if platform.find(game)?.is_some() {
            return Err(
                "Close DCS before changing its export scripts, then run setup again.".into(),
            );
        }
        let checked = self.inner.inspect(game, request.get("userFolder"));
        let target = folder(&checked["userFolder"])?;
        reject_game_install_folder(&target, game)?;
        let saved =
            self.inner.saved.lock().unwrap_or_else(|p| p.into_inner())["games"]["dcs"].clone();
        let mut connection = dcs_connection(&target.join("Scripts/DCS-BIOS/doc/json"));
        if let Some(previous) = saved["connection"].as_object() {
            for (k, v) in previous {
                if k != "cataloguePath" {
                    connection[k] = v.clone();
                }
            }
        }
        if let Some(overrides) = request.get("connection") {
            let values = overrides
                .as_object()
                .ok_or("Invalid connection overrides.")?;
            for (k, v) in values {
                if k != "cataloguePath" {
                    connection[k] = v.clone();
                }
            }
        }
        validate_connection(id, &connection)?;
        self.inner.cancel.store(false, Ordering::Release);
        self.inner.busy.store(true, Ordering::Release);
        self.inner.states.lock().unwrap_or_else(|p| p.into_inner()).insert(id.into(), json!({"state":"setting-up","message":"Preparing DCS-BIOS setup.","userFolder":target,"version":crate::dcs_setup::VERSION,"progress":"Preparing"}));
        let inner = self.inner.clone();
        let game = game.clone();
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(previous) = worker.take() {
            let _ = previous.join();
        }
        *worker = Some(thread::spawn(move || {
            let result = crate::dcs_setup::setup_for_installation(
                game["installPath"].as_str().map(Path::new),
                &target,
                &inner.work,
                || inner.cancel.load(Ordering::Acquire),
                |message, percent| {
                    let mut states = inner.states.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(status) = states.get_mut("dcs") {
                        status["progress"] = json!(message);
                        status["percent"] = json!(percent);
                    }
                },
                || {
                    if platform.find(&game)?.is_some() {
                        Err("DCS started during integration setup. Close DCS, then run setup again before its export scripts are changed.".into())
                    } else {
                        Ok(())
                    }
                },
            )
            .and_then(|_| {
                let mut draft = inner
                    .saved
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                let record=&mut draft["games"]["dcs"];
                if !record.is_object(){*record=json!({});}
                record["userFolder"]=json!(target);record["connection"]=connection;
                write_settings(&inner.path, &draft)?;
                *inner.saved.lock().unwrap_or_else(|p| p.into_inner()) = draft;
                Ok(())
            });
            let mut status = inner.inspect(&game, None);
            if let Err(error) = result {
                status["state"] = json!("needs-attention");
                status["message"] = json!(error);
            }
            inner
                .states
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert("dcs".into(), status);
            inner.busy.store(false, Ordering::Release);
        }));
        Ok(())
    }
    fn begin_acr(
        &self,
        request: &Value,
        game: &Value,
        platform: Arc<dyn Platform>,
        configure: bool,
    ) -> Result<(), String> {
        if game["installed"] != true {
            return Err("Find the ACR installation before setting up telemetry.".into());
        }
        let previous = self.inner.saved.lock().unwrap_or_else(|p| p.into_inner())["games"]
            ["assetto-corsa-rally"]
            .clone();
        let mut connection = if previous["connection"].is_object() {
            previous["connection"].clone()
        } else {
            json!({"source":"shared-memory","freshnessMs":1500})
        };
        if configure {
            if let Some(overrides) = request.get("connection") {
                for (k, v) in overrides
                    .as_object()
                    .ok_or("Invalid ACR connection overrides.")?
                {
                    connection[k] = v.clone();
                }
            }
        }
        validate_connection("assetto-corsa-rally", &connection)?;
        let executable = game["executable"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Choose the ACR executable.")?;
        let executable = PathBuf::from(executable);
        if !executable.is_absolute() || !executable.is_file() {
            return Err("Choose an existing absolute ACR executable.".into());
        }
        if self
            .inner
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("An integration setup check is already running.".into());
        }
        self.inner.cancel.store(false, Ordering::Release);
        self.inner.states.lock().unwrap_or_else(|p| p.into_inner()).insert("assetto-corsa-rally".into(), json!({"state":"setting-up","message":"Checking ACR native telemetry. No plugin or game-file changes are required.","progress":"Checking executable and native telemetry","connection":connection,"verified":false}));
        let inner = self.inner.clone();
        let game = game.clone();
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(previous_worker) = worker.take() {
            let _ = previous_worker.join();
        }
        *worker = Some(thread::spawn(move || {
            let result: Result<Value, String> = (|| {
                let identity = crate::acr_setup::fingerprint(&executable)?;
                if inner.cancel.load(Ordering::Acquire) {
                    return Err("ACR telemetry check was cancelled.".into());
                }
                if configure {
                    let mut draft = inner
                        .saved
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .clone();
                    let verified = if previous["verifiedFingerprint"] == identity {
                        identity.clone()
                    } else {
                        Value::Null
                    };
                    let issue = if previous["configuredFingerprint"] == identity {
                        previous["verificationIssue"].clone()
                    } else {
                        Value::Null
                    };
                    draft["games"]["assetto-corsa-rally"] = json!({"connection":connection,"configuredFingerprint":identity,"verifiedFingerprint":verified,"verificationIssue":issue});
                    write_settings(&inner.path, &draft)?;
                    *inner.saved.lock().unwrap_or_else(|p| p.into_inner()) = draft;
                }
                let proof = crate::acr_setup::probe(
                    &game,
                    platform.as_ref(),
                    || inner.cancel.load(Ordering::Acquire),
                    |message, percent| {
                        let mut states = inner.states.lock().unwrap_or_else(|p| p.into_inner());
                        if let Some(status) = states.get_mut("assetto-corsa-rally") {
                            status["progress"] = json!(message);
                            status["percent"] = json!(percent);
                        }
                    },
                )?;
                inner.accept_acr_proof(&proof)?;
                Ok(proof)
            })();
            let mut status = inner.inspect(&game, None);
            match result {
                Ok(proof) => {
                    let channels = proof["channels"].as_object().map(|rows| rows.iter().map(|(name,channel)| (name.clone(),json!({"status":if channel["status"]=="ready" {"available"} else {"unknown"}}))).collect::<serde_json::Map<String,Value>>()).unwrap_or_default();
                    status["probeChannels"] = json!(channels);
                    status["optionalMaxRpm"] = json!(proof["maxRpmAvailable"] == true);
                    if proof["nativeVerified"] != true && status["verified"] != true {
                        status["message"] = proof["diagnostics"][0].as_str().map(|s| json!(s)).unwrap_or_else(|| json!("Telemetry is configured. Start a live ACR stage to verify this game build."));
                    }
                    if blocking_acr_status(&proof) {
                        status["state"] = json!("needs-attention");
                        status["verified"] = json!(false);
                        status["message"] = proof["diagnostics"][0].clone();
                    }
                }
                Err(error) => {
                    status["state"] = json!("needs-attention");
                    status["message"] = json!(error);
                }
            }
            inner
                .states
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert("assetto-corsa-rally".into(), status);
            inner.busy.store(false, Ordering::Release);
        }));
        Ok(())
    }
    pub(crate) fn shutdown(&self) {
        let _admission = self.admission.lock().unwrap_or_else(|p| p.into_inner());
        self.inner.closed.store(true, Ordering::Release);
        self.inner.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
    }
}
impl Inner {
    fn accept_acr_proof(&self, proof: &Value) -> Result<(), String> {
        let mut draft = self.saved.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let record = &mut draft["games"]["assetto-corsa-rally"];
        if !record.is_object() || record["configuredFingerprint"] != proof["fingerprint"] {
            return Ok(());
        }
        if proof["nativeVerified"] == true {
            record["verifiedFingerprint"] = proof["fingerprint"].clone();
            record["verificationIssue"] = Value::Null;
        } else if blocking_acr_status(proof) {
            record["verificationIssue"] = proof["diagnostics"][0].clone();
            if matches!(
                proof["status"].as_str(),
                Some("unsupported-layout" | "executable-changed")
            ) {
                record["verifiedFingerprint"] = Value::Null;
            }
        } else {
            return Ok(());
        }
        write_settings(&self.path, &draft)?;
        *self.saved.lock().unwrap_or_else(|p| p.into_inner()) = draft;
        Ok(())
    }

    fn inspect_acr(&self, game: &Value) -> Value {
        let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner())["games"]
            ["assetto-corsa-rally"]
            .clone();
        let mut status = json!({"state":"setup-required","message":"Set up ACR native telemetry. No extra module or game-file changes are required.","verified":false});
        if !saved["connection"].is_object() {
            return status;
        }
        status["connection"] = saved["connection"].clone();
        let inspected = (|| {
            validate_connection("assetto-corsa-rally", &saved["connection"])?;
            let executable = game["executable"]
                .as_str()
                .ok_or("Choose the ACR executable.")?;
            crate::acr_setup::fingerprint(Path::new(executable))
        })();
        match inspected {
            Err(error) => {
                status["state"] = json!("needs-attention");
                status["message"] = json!(error);
            }
            Ok(identity) => {
                status["version"] = identity["fileVersion"].clone();
                if saved["configuredFingerprint"] != identity {
                    status["state"] = json!("needs-attention");
                    status["message"]=json!("The ACR executable/build changed. Set up telemetry again and verify a live stage.");
                } else if let Some(issue) = saved["verificationIssue"].as_str() {
                    status["state"] = json!("needs-attention");
                    status["message"] = json!(issue);
                } else if saved["verifiedFingerprint"] == identity {
                    status["state"] = json!("ready");
                    status["verified"] = json!(true);
                    status["message"]=json!("Native ACR telemetry was verified for this exact executable. Waiting for a live session when the game is closed.");
                } else {
                    status["state"] = json!("needs-attention");
                    status["message"]=json!("Telemetry is configured. This game build needs its first native verification in a live stage.");
                }
            }
        }
        status
    }
    fn inspect(&self, game: &Value, override_folder: Option<&Value>) -> Value {
        if let Some(error) = &self.settings_error {
            return json!({"state":"needs-attention","message":error,"settingsError":true,"recoveryNeeded":crate::dcs_setup::recovery_pending(&self.work)});
        }
        if !self.native {
            return json!({"state":"unsupported-platform","message":"Native game integration setup requires Windows. Profile editing and installation metadata remain available."});
        }
        if game["installed"] != true {
            return json!({"state":"not-installed","message":"Find the game installation first."});
        }
        if game["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("x-plane-"))
        {
            let id = game["id"].as_str().unwrap();
            let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner())["games"][id].clone();
            if saved["executable"] != game["executable"]
                || validate_connection(id, &saved["connection"]).is_err()
            {
                return json!({"state":"setup-required","message":"Set up RREF UDP for this X-Plane installation. No game files are changed.","verified":false});
            }
            return json!({"state":"ready","connection":saved["connection"],"verified":false,"message":"RREF UDP configured. Start a flight in the selected installation. Check its UDP receive port (default 49000), firewall, pause/replay and the live status separately. Simulator/hardware qualification is pending."});
        }
        if game["id"] == "assetto-corsa-rally" {
            return self.inspect_acr(game);
        }
        let saved = self.saved.lock().unwrap_or_else(|p| p.into_inner())["games"]["dcs"].clone();
        let candidates =
            crate::dcs_setup::discover_user_folders(game["installPath"].as_str().map(Path::new))
                .unwrap_or_default();
        let chosen = override_folder
            .filter(|v| !v.is_null())
            .cloned()
            .or_else(|| saved["userFolder"].as_str().map(|s| json!(s)))
            .or_else(|| (candidates.len() == 1).then(|| json!(candidates[0])));
        let mut status = json!({"state":"setup-required","message":"Choose the DCS Saved Games user folder, then set up integration.","candidates":candidates,"userFolder":chosen,"version":crate::dcs_setup::VERSION,"recoveryNeeded":crate::dcs_setup::recovery_pending(&self.work)});
        if status["recoveryNeeded"] == true {
            status["state"] = json!("needs-attention");
            status["message"] = json!("A previous setup was interrupted. Recover its owned file changes before continuing.");
            return status;
        }
        let Some(chosen) = chosen else {
            return status;
        };
        match folder(&chosen).and_then(|target| {
            reject_game_install_folder(&target, game)?;
            crate::dcs_setup::inspect_for_installation(
                &target,
                game["installPath"].as_str().map(Path::new),
            )
        }) {
            Err(error) => {
                status["message"] = json!(error);
            }
            Ok(inspected) => {
                status["cataloguePath"] = inspected["cataloguePath"].clone();
                status["diagnostics"] = inspected["diagnostics"].clone();
                status["repairable"] = inspected["repairable"].clone();
                status["aircraftExports"] = inspected["aircraftExports"].clone();
                if inspected["ready"] == true {
                    let connection = if saved["userFolder"] == chosen
                        && saved["connection"].is_object()
                    {
                        saved["connection"].clone()
                    } else {
                        dcs_connection(Path::new(inspected["cataloguePath"].as_str().unwrap_or("")))
                    };
                    status["connection"] = connection.clone();
                    match compatible_dcs_connection(&connection) {
                        Ok(()) => {
                            status["state"] = json!("ready");
                            status["message"] = json!("DCS-BIOS module, export loader, catalogue and local connection settings are ready.");
                        }
                        Err(error) => {
                            status["state"] = json!("needs-attention");
                            status["message"] = json!(error);
                        }
                    }
                } else if inspected["repairable"] != true {
                    status["state"] = json!("needs-attention");
                    status["message"] = inspected["diagnostics"][0].clone();
                } else {
                    status["message"] = json!("DCS-BIOS or its export loader needs setup. Existing scripts will be preserved.");
                }
            }
        }
        status
    }
}
#[cfg(test)]
#[path = "game_integration_setup_tests.rs"]
mod tests;
