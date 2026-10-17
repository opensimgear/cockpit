//! Game-owned provider preferences and read-only catalogue snapshots.
//! No portable binding document, provider worker or command transport lives here.
use super::*;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
pub(super) type CatalogueCache = BTreeMap<String, (Instant, String, Result<Value, String>)>;
fn digest(value: &Value) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("JSON value"))
    )
}
fn aircraft(value: &Value) -> Result<Option<String>, String> {
    if value.is_null() || value.as_str() == Some("") {
        return Ok(None);
    }
    value
        .as_str()
        .filter(|s| {
            s.len() <= 63
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
        .map(|s| Some(s.to_owned()))
        .ok_or("Choose a valid DCS catalogue aircraft/module name.".into())
}
pub(super) fn validate_saved_record(id: &str, record: &Value) -> Result<(), String> {
    let keys = if id == "dcs" {
        vec!["connection", "userFolder", "aircraftModule"]
    } else if id.starts_with("x-plane-") {
        vec!["connection", "executable"]
    } else {
        vec![
            "connection",
            "executable",
            "configuredFingerprint",
            "verifiedFingerprint",
            "verificationIssue",
        ]
    };
    if record
        .as_object()
        .is_none_or(|object| object.keys().any(|key| !keys.contains(&key.as_str())))
    {
        return Err(
            "Saved provider settings contain unsupported binding or runtime fields.".into(),
        );
    }
    if id == "dcs" {
        aircraft(&record["aircraftModule"])?;
    }
    if id.starts_with("x-plane-")
        && record["connection"].as_object().is_none_or(|object| {
            object.keys().any(|key| {
                ![
                    "source",
                    "version",
                    "host",
                    "port",
                    "listenPort",
                    "updateHz",
                    "freshnessMs",
                ]
                .contains(&key.as_str())
            })
        })
    {
        return Err("Saved provider settings cannot contain transient process identity.".into());
    }
    Ok(())
}
fn defaults(id: &str) -> Value {
    match id {
        "dcs" => dcs_connection(Path::new("")),
        "x-plane-11" => crate::xplane::defaults(11),
        "x-plane-12" => crate::xplane::defaults(12),
        _ => json!({"source":"shared-memory","freshnessMs":1500}),
    }
}
impl GameIntegrationSetup {
    fn provider_snapshot(&self, id: &str) -> (Value, String) {
        let saved = self.inner.saved.lock().unwrap_or_else(|p| p.into_inner());
        let record = saved["games"][id].clone();
        let revision = digest(&json!([
            "game-provider-setup-v1",
            id,
            record,
            self.inner.settings_error
        ]));
        (record, revision)
    }
    pub(crate) fn provider_setup_revision(&self, id: &str) -> String {
        self.provider_snapshot(id).1
    }
    pub(crate) fn provider_connection_snapshot(
        &self,
        id: &str,
    ) -> Result<(Value, String, Option<String>), String> {
        if let Some(error) = &self.inner.settings_error {
            return Err(error.clone());
        }
        let (record, revision) = self.provider_snapshot(id);
        let connection = if record["connection"].is_object() {
            record["connection"].clone()
        } else {
            defaults(id)
        };
        validate_connection(id, &connection)?;
        Ok((
            connection,
            revision,
            if id == "dcs" {
                aircraft(&record["aircraftModule"])?
            } else {
                None
            },
        ))
    }
    pub(crate) fn current_catalogue_revision(&self, id: &str) -> Option<String> {
        let setup = self.provider_setup_revision(id);
        self.inner
            .catalogues
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .filter(|(_, revision, _)| *revision == setup)
            .and_then(|(_, _, result)| result.as_ref().ok())
            .and_then(|catalogue| catalogue["catalogueRevision"].as_str())
            .map(str::to_owned)
    }
    pub(super) fn decorate_provider_settings(&self, id: &str, status: &mut Value) {
        let (saved, setup_revision) = self.provider_snapshot(id);
        status["providerSetupRevision"] = json!(setup_revision);
        // Retain editable metadata even while the executable or process is absent.
        if saved["connection"].is_object() {
            status["connection"] = saved["connection"].clone();
        } else if !status["connection"].is_object() {
            status["connection"] = defaults(id);
        }
        if id == "dcs" {
            status["aircraftModule"] = saved["aircraftModule"].clone();
            if saved["userFolder"].is_string() {
                status["userFolder"] = saved["userFolder"].clone();
            }
            status["cataloguePath"] = status["connection"]["cataloguePath"].clone();
        }
        let revision = self
            .inner
            .catalogues
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .and_then(|(_, setup, result)| {
                (setup == status["providerSetupRevision"].as_str().unwrap_or("")).then_some(result)
            })
            .and_then(|result| result.as_ref().ok())
            .map(|catalogue| catalogue["catalogueRevision"].clone());
        status["catalogueRevision"] = revision.unwrap_or(Value::Null);
    }
    pub(super) fn save_provider_settings(
        &self,
        request: &Value,
        game: &Value,
    ) -> Result<(), String> {
        if let Some(error) = &self.inner.settings_error {
            return Err(error.clone());
        }
        let id = game["id"]
            .as_str()
            .filter(|id| {
                matches!(
                    *id,
                    "dcs" | "x-plane-11" | "x-plane-12" | "assetto-corsa-rally"
                )
            })
            .ok_or("Choose a supported game.")?;
        if request.as_object().is_none_or(|object| {
            object.keys().any(|key| {
                ![
                    "command",
                    "clientId",
                    "gameId",
                    "connection",
                    "userFolder",
                    "aircraftModule",
                    "expectedSetupRevision",
                ]
                .contains(&key.as_str())
            })
        }) {
            return Err("Unsupported game provider settings field.".into());
        }
        let connection = &request["connection"];
        validate_connection(id, connection)?;
        if id.starts_with("x-plane-")
            && connection.as_object().is_none_or(|values| {
                values.keys().any(|key| {
                    ![
                        "source",
                        "version",
                        "host",
                        "port",
                        "listenPort",
                        "updateHz",
                        "freshnessMs",
                    ]
                    .contains(&key.as_str())
                })
            })
        {
            return Err("X-Plane process identity is verified at runtime and cannot be stored in provider settings.".into());
        }
        if id != "dcs"
            && (request.get("userFolder").is_some() || request.get("aircraftModule").is_some())
        {
            return Err("DCS folder and aircraft settings belong to the DCS game.".into());
        }
        let mut saved = self.inner.saved.lock().unwrap_or_else(|p| p.into_inner());
        let revision = digest(&json!([
            "game-provider-setup-v1",
            id,
            saved["games"][id],
            self.inner.settings_error
        ]));
        if request["expectedSetupRevision"].as_str() != Some(revision.as_str()) {
            return Err(
                "Game provider settings changed. Reload their current revision before saving."
                    .into(),
            );
        }
        if read_settings(&self.inner.path)? != *saved {
            return Err("Game provider settings changed on disk. Preserve the file and reopen settings before saving.".into());
        }
        let mut draft = saved.clone();
        let record = &mut draft["games"][id];
        if !record.is_object() {
            *record = json!({});
        }
        record["connection"] = connection.clone();
        if id == "dcs" {
            let chosen = request.get("userFolder").unwrap_or(&record["userFolder"]);
            let folder = absolute_folder(chosen)?;
            reject_game_install_folder_for_settings(&folder, game)?;
            record["userFolder"] = json!(folder);
            if let Some(value) = request.get("aircraftModule") {
                record["aircraftModule"] =
                    aircraft(value)?.map(Value::String).unwrap_or(Value::Null);
            }
        } else {
            record["executable"] = game["executable"].clone();
        }
        if serde_json::to_vec(&draft).map_err(|e| e.to_string())?.len() > MAX_SETTINGS {
            return Err("Game provider settings exceed their bounded library size.".into());
        }
        write_settings(&self.inner.path, &draft)?;
        *saved = draft;
        drop(saved);
        self.inner
            .catalogues
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id);
        self.refresh(std::slice::from_ref(game));
        Ok(())
    }
    /// Full provider metadata remains available for a closed game. Reading never
    /// launches a process, opens telemetry or imports an alternate binding store.
    pub(crate) fn resource_integration_catalogue(&self, id: &str) -> Result<Value, String> {
        if !matches!(
            id,
            "dcs" | "x-plane-11" | "x-plane-12" | "assetto-corsa-rally"
        ) {
            return Err("Choose a supported game.".into());
        }
        let (record, revision) = self.provider_snapshot(id);
        let mut cache = self
            .inner
            .catalogues
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some((at, known, result)) = cache.get(id) {
            if known == &revision && at.elapsed() < Duration::from_secs(2) {
                return result.clone();
            }
        }
        let result = (|| {
            if let Some(error) = &self.inner.settings_error {
                return Err(error.clone());
            }
            let mut value = match id {
                "dcs" => {
                    validate_connection(id, &record["connection"])?;
                    let selected=aircraft(&record["aircraftModule"])?.ok_or("Choose a catalogue aircraft/module in DCS game details to expose its ports.")?;
                    let mut query = record["connection"].clone();
                    query["aircraftModule"] = json!(selected);
                    crate::dcs_profile::resource_catalogue(&query)?
                }
                "x-plane-11" | "x-plane-12" => crate::xplane::catalogue(),
                _ => crate::game_telemetry::catalogue(),
            };
            value["catalogueRevision"] =
                json!(digest(&json!(["game-provider-catalogue-v1", id, value])));
            value["providerSetupRevision"] = json!(revision);
            Ok(value)
        })();
        cache.insert(id.into(), (Instant::now(), revision, result.clone()));
        result
    }
}
fn reject_game_install_folder_for_settings(target: &Path, game: &Value) -> Result<(), String> {
    // Preferences may describe a folder that is currently absent. Existing
    // folders still enforce the install boundary before they can become setup.
    if target.exists() {
        reject_game_install_folder(target, game)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("osg-provider-settings-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn game(id: &str) -> Value {
        json!({"id":id,"provider":if id=="dcs"{"dcs-bios"}else if id.starts_with("x-plane-"){"x-plane"}else{id},"installed":false,"executable":null,"launchSupported":false})
    }
    fn save(setup: &GameIntegrationSetup, id: &str, connection: Value) -> Result<(), String> {
        setup.save_provider_settings(&json!({"command":"game_integration_settings_save","gameId":id,"expectedSetupRevision":setup.provider_setup_revision(id),"connection":connection}),&game(id))
    }
    #[test]
    fn settings_save_is_game_owned_optimistic_and_never_creates_binding_documents() {
        let scratch = Scratch::new();
        let path = scratch.0.join("setup.json");
        let setup = GameIntegrationSetup::new(path.clone(), false);
        let revision = setup.provider_setup_revision("x-plane-11");
        let mut connection = crate::xplane::defaults(11);
        connection["port"] = json!(49005);
        save(&setup, "x-plane-11", connection.clone()).unwrap();
        assert_ne!(revision, setup.provider_setup_revision("x-plane-11"));
        let saved = read_settings(&path).unwrap();
        assert_eq!(saved["games"]["x-plane-11"]["connection"], connection);
        assert!(saved["games"]["x-plane-11"].get("document").is_none());
        assert!(!scratch.0.join("integration.json").exists());
        assert!(setup.save_provider_settings(&json!({"command":"game_integration_settings_save","expectedSetupRevision":revision,"connection":crate::xplane::defaults(11)}),&game("x-plane-11")).is_err());
        assert_eq!(read_settings(&path).unwrap(), saved);
        let reopened = GameIntegrationSetup::new(path, false);
        assert_eq!(
            reopened.provider_setup_revision("x-plane-11"),
            setup.provider_setup_revision("x-plane-11")
        );
    }
    #[test]
    fn closed_games_keep_settings_and_catalogues_without_starting_integrations() {
        let scratch = Scratch::new();
        let setup = GameIntegrationSetup::new(scratch.0.join("setup.json"), false);
        save(&setup, "x-plane-11", crate::xplane::defaults(11)).unwrap();
        let first = setup.resource_integration_catalogue("x-plane-11").unwrap();
        let second = setup.resource_integration_catalogue("x-plane-12").unwrap();
        assert_ne!(first["catalogueRevision"], second["catalogueRevision"]);
        let mut games = vec![game("x-plane-11")];
        setup.decorate(&mut games, &Value::Null, &Value::Null);
        assert_eq!(games[0]["integrationSetup"]["connection"]["port"], 49000);
        assert!(games[0]["integrationSetup"]["providerSetupRevision"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert_eq!(
            games[0]["integrationSetup"]["catalogueRevision"],
            first["catalogueRevision"]
        );
        let old = first["providerSetupRevision"].clone();
        let mut connection = crate::xplane::defaults(11);
        connection["freshnessMs"] = json!(2000);
        save(&setup, "x-plane-11", connection).unwrap();
        let fresh = setup.resource_integration_catalogue("x-plane-11").unwrap();
        assert_ne!(old, fresh["providerSetupRevision"]);
    }
    #[test]
    fn runtime_identity_bindings_wrong_game_and_external_file_edits_are_rejected() {
        let scratch = Scratch::new();
        let path = scratch.0.join("setup.json");
        let setup = GameIntegrationSetup::new(path.clone(), false);
        let mut connection = crate::xplane::defaults(11);
        connection["processIdentity"] = json!({"pid":42,"epoch":1,"path":"X-Plane.exe"});
        assert!(save(&setup, "x-plane-11", connection).is_err());
        assert!(save(&setup, "x-plane-12", crate::xplane::defaults(11)).is_err());
        assert!(setup.save_provider_settings(&json!({"command":"game_integration_settings_save","expectedSetupRevision":setup.provider_setup_revision("x-plane-11"),"connection":crate::xplane::defaults(11),"aircraftModule":"Aircraft"}),&game("x-plane-11")).is_err());
        assert!(setup.save_provider_settings(&json!({"command":"game_integration_settings_save","expectedSetupRevision":setup.provider_setup_revision("x-plane-11"),"connection":crate::xplane::defaults(11),"document":{}}),&game("x-plane-11")).is_err());
        save(&setup, "x-plane-11", crate::xplane::defaults(11)).unwrap();
        let mut external = read_settings(&path).unwrap();
        external["games"]["x-plane-11"]["connection"]["port"] = json!(49001);
        write_settings(&path, &external).unwrap();
        assert!(save(&setup, "x-plane-11", crate::xplane::defaults(11)).is_err());
        assert_eq!(read_settings(&path).unwrap(), external);
    }
    #[test]
    fn dcs_aircraft_catalogue_and_folder_live_only_in_game_settings() {
        let scratch = Scratch::new();
        let path = scratch.0.join("setup.json");
        let setup = GameIntegrationSetup::new(path.clone(), false);
        let connection = dcs_connection(&scratch.0.join("catalogue"));
        let request = json!({"command":"game_integration_settings_save","expectedSetupRevision":setup.provider_setup_revision("dcs"),"connection":connection,"userFolder":scratch.0,"aircraftModule":"FA-18C_hornet"});
        setup
            .save_provider_settings(&request, &game("dcs"))
            .unwrap();
        let saved = read_settings(&path).unwrap();
        assert_eq!(saved["games"]["dcs"]["aircraftModule"], "FA-18C_hornet");
        assert!(setup.resource_integration_catalogue("dcs").is_err());
        let mut bad = request.clone();
        bad["expectedSetupRevision"] = json!(setup.provider_setup_revision("dcs"));
        bad["aircraftModule"] = json!("../outside");
        assert!(setup.save_provider_settings(&bad, &game("dcs")).is_err());
        assert_eq!(read_settings(&path).unwrap(), saved);
    }
    #[test]
    fn corrupt_or_oversized_preferences_remain_visible_and_read_only() {
        let scratch = Scratch::new();
        let path = scratch.0.join("setup.json");
        fs::write(&path, b"{broken").unwrap();
        let setup = GameIntegrationSetup::new(path.clone(), false);
        assert!(save(&setup, "x-plane-11", crate::xplane::defaults(11)).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{broken");
        fs::write(&path, vec![b' '; MAX_SETTINGS + 1]).unwrap();
        assert!(read_settings(&path).is_err());
        let mut invalid = json!({"schemaVersion":1,"games":{"x-plane-11":{"connection":crate::xplane::defaults(11),"document":{}}}});
        write_settings(&path, &invalid).unwrap();
        assert!(read_settings(&path).is_err());
        invalid["games"]["x-plane-11"]
            .as_object_mut()
            .unwrap()
            .remove("document");
        invalid["games"]["x-plane-11"]["connection"]["processIdentity"] = json!({"pid":1});
        write_settings(&path, &invalid).unwrap();
        assert!(read_settings(&path).is_err());
    }
}
