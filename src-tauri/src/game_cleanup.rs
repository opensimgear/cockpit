//! Retained game journal cleanup only. Nothing here can create a graph,
//! capture a baseline, launch a process or resume an old recording/provider.
use super::*;
use std::collections::BTreeSet;
const MAX_FILE: usize = 2 * 1024 * 1024;
pub(super) struct Lease(fs::File);
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
pub(super) fn lease(path: &Path) -> Result<Lease, String> {
    let path = path.with_extension("cleanup.lock");
    check_path(&path)?;
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("Cleanup ownership requires a regular lock file.".into());
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    check_path(&path)?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.try_lock()
        .map_err(|_| "Another Cockpit process owns the retained game cleanup.".to_owned())?;
    Ok(Lease(file))
}
fn redirected(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    false
}
fn check_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path.file_name().is_none()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("Retained cleanup requires an absolute path without parent traversal.".into());
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part.as_os_str());
        if matches!(part, std::path::Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if redirected(&metadata) => {
                return Err("Retained cleanup cannot follow a symlink or reparse point.".into())
            }
            Ok(metadata) if current != path && !metadata.is_dir() => {
                return Err("A retained cleanup parent is not a directory.".into())
            }
            Ok(metadata) if current == path && !metadata.is_file() => {
                return Err("A retained cleanup journal must be a regular file.".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}
fn read_raw(path: &Path) -> Result<Option<Vec<u8>>, String> {
    check_path(path)?;
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err("A retained cleanup journal must be a regular file.".into())
        }
        Ok(_) => {}
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FILE {
        return Err("The retained game cleanup journal exceeds its size bound.".into());
    }
    Ok(Some(bytes))
}
pub(super) fn fingerprint(path: &Path) -> Result<String, String> {
    use sha2::Digest;
    let bytes = read_raw(path)?.ok_or("The retained game cleanup file disappeared.")?;
    Ok(format!("sha256:{:x}", sha2::Sha256::digest(&bytes)))
}
pub(super) fn load(path: &Path) -> Result<Option<Value>, String> {
    let Some(bytes) = read_raw(path)? else {
        return Ok(None);
    };
    validate_journal(
        serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid retained game cleanup journal: {e}"))?,
    )
    .map(Some)
}
pub(super) fn persist(path: &Path, value: &Value) -> Result<(), String> {
    check_path(path)?;
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FILE {
        return Err("The retained cleanup journal exceeds its size bound.".into());
    }
    let parent = path
        .parent()
        .ok_or("Cleanup journal has no parent directory.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    check_path(path)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary.write_all(&bytes).map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist(path).map_err(|e| e.to_string())?;
    Ok(())
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
pub(super) fn validate_journal(value: Value) -> Result<Value, String> {
    if !matches!(value["schemaVersion"].as_u64(), Some(1 | 2 | 3)) {
        return Err("Unsupported recovery journal.".into());
    }
    if !value["mapping"].is_null() {
        if !matches!(value["schemaVersion"].as_u64(), Some(2 | 3)) {
            return Err("Retained mapping cleanup requires journal schema 2 or 3.".into());
        }
        let mapping = &value["mapping"];
        let owner = mapping["owner"]
            .as_str()
            .ok_or("Missing retained mapping owner.")?;
        if !uuid::Uuid::parse_str(owner).is_ok_and(|id| id.to_string() == owner)
            || ["taken", "armed", "stopped", "resumed"]
                .iter()
                .any(|key| !mapping[key].is_boolean())
            || !mapping["desired"]["profile"]["requirements"]["virtualDevices"].is_boolean()
        {
            return Err("Invalid retained mapping cleanup evidence.".into());
        }
    }
    crate::retained_game_companions::journal(&value)?;
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
    pub(super) fn phase(&self, phase: &str, error: Option<String>) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state["phase"] = json!(phase);
        state["error"] = json!(error);
    }

    pub(super) fn step(&self, message: &str) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !s["steps"].is_array() {
            s["steps"] = json!([]);
        }
        let steps = s["steps"].as_array_mut().unwrap();
        if steps.len() < 32 {
            steps.push(json!(message));
        }
    }

    pub(super) fn call(&self, command: &str, extra: Value) -> Result<Value, String> {
        self.verify_journal()?;
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

    pub(super) fn device_once(&self, row: &Value) -> Result<(Value, Value), String> {
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

    pub(super) fn wait_duration() -> Duration {
        Duration::from_millis(if cfg!(test) { 100 } else { 30_000 })
    }

    pub(super) fn device(&self, row: &Value) -> Result<(Value, Value), String> {
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

    pub(super) fn write_config(&self, row: &Value, desired: &Value) -> Result<(), String> {
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

    pub(super) fn readback(
        &self,
        row: &Value,
        config: &Value,
        enabled: &Value,
    ) -> Result<(), String> {
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

    pub(super) fn publish(
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

    pub(super) fn finish(&self, result: Result<(), String>, activation_error: Option<String>) {
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

    pub(super) fn wait_cancelled(&self) -> Result<(), String> {
        if self.closed.load(Ordering::Acquire) || self.cancel.load(Ordering::Acquire) {
            return Err(
                "Cleanup was interrupted; its journal remains for explicit recovery.".into(),
            );
        }
        Ok(())
    }
    pub(super) fn verify_journal(&self) -> Result<(), String> {
        let expected = self
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .journal
            .clone();
        if load(&self.journal_path)? != expected {
            return Err(
                "The retained game cleanup journal changed outside this owner; it was preserved."
                    .into(),
            );
        }
        Ok(())
    }
    pub(super) fn checkpoint(&self, journal: &Value) -> Result<(), String> {
        self.verify_journal()?;
        persist(&self.journal_path, journal)?;
        self.settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .journal = Some(journal.clone());
        Ok(())
    }
    pub(super) fn restore(&self, mut journal: Value) -> Result<(), String> {
        self.phase("restoring", None);
        self.step("Restoring the previous peripheral and integration setup.");
        if journal["mapping"]["taken"] == true && journal["mapping"]["stopped"] != true {
            let mapping = &journal["mapping"];
            if mapping["desired"]["profile"]["requirements"]["virtualDevices"] == true {
                let captured = self.call(
                    "_retired_mapping_inspect",
                    json!({"owner":mapping["owner"]}),
                )?;
                let reference = captured
                    .get("recovery")
                    .ok_or("Native mapping cleanup returned no exact recovery reference.")?;
                self.call(
                    "_retired_mapping_cleanup",
                    json!({"owner":mapping["owner"],"recovery":reference}),
                )?;
            }
            if !mapping["desired"]["visibility"].is_null() {
                self.call(
                    "_retired_mapping_visibility_cleanup",
                    json!({"owner":mapping["owner"]}),
                )?;
            }
            journal["mapping"]["stopped"] = json!(true);
            journal["mapping"]["resumed"] = json!(true);
            self.checkpoint(&journal)?;
        }
        // Retired bindings are volatile. They are deliberately neither loaded
        // from an authoring store nor restored/restarted from this journal.
        if journal["integrationTaken"] == true {
            journal["integrationTaken"] = json!(false);
            journal["integrationChanged"] = json!(false);
            self.checkpoint(&journal)?;
            self.step("Retired integration bindings remain stopped; no old graph was reapplied.");
        }
        if journal["companions"]["taken"] == true && journal["companions"]["restored"] != true {
            self.call(
                "_game_companion_restore",
                json!({"companions":journal["companions"],"recovery":true}),
            )?;
            journal["companions"]["restored"] = json!(true);
            self.checkpoint(&journal)?;
            self.step("Owned companion assignments restored; controller input remains disarmed.");
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
        self.verify_journal()?;
        fs::remove_file(&self.journal_path).map_err(|e| e.to_string())?;
        self.settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .journal = None;
        Ok(())
    }
}
