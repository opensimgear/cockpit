//! Game-owned provider preferences and read-only catalogue snapshots.
//! No portable binding document, provider worker or command transport lives here.
use super::*;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
pub(super) type CatalogueCache = BTreeMap<String, (Instant, String, Result<Arc<Value>, String>)>;
fn digest(value: &Value) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("JSON value"))
    )
}
fn catalogue_metadata(cache: &CatalogueCache, id: &str, vehicle: Option<&str>, setup: &str) -> Option<Value> {
    let selected = if let ("dcs", Some(vehicle)) = (id, vehicle) {
        cache.iter().filter(|(key,_)|key.starts_with("dcs:")).find_map(|(_,(_,revision,result))| {
            let value = result.as_ref().ok()?;
            (revision == setup && (value["aircraftModule"] == vehicle || value["vehicleAliases"].as_array().is_some_and(|aliases|aliases.iter().any(|alias|alias == vehicle)))).then_some(value)
        })
    } else {
        cache.get(if id == "dcs" { "dcs:CommonData" } else { id })
            .filter(|(_,revision,_)|revision == setup).and_then(|(_,_,result)|result.as_ref().ok())
    }?;
    Some(json!({"aircraftModule":selected["aircraftModule"],"catalogueRevision":selected["catalogueRevision"],"providerSetupRevision":selected["providerSetupRevision"]}))
}
fn aircraft(value: &Value) -> Result<Option<String>, String> {
    if value.is_null() || value.as_str() == Some("") {
        return Ok(None);
    }
    value
        .as_str()
        .filter(|s| crate::dcs_bios::safe_module_name(s))
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
        self.current_catalogue_revision_for_vehicle(id, None)
    }
    pub(crate) fn current_catalogue_revision_for_vehicle(&self, id: &str, vehicle: Option<&str>) -> Option<String> {
        let setup = self.provider_setup_revision(id);
        let cache = self.inner
            .catalogues
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        catalogue_metadata(&cache, id, vehicle, &setup)?["catalogueRevision"].as_str().map(str::to_owned)
    }
    /// Small captured identity only. Source guards and explicit preparation own
    /// file validation; passive status/observation must not reparse an aircraft
    /// catalogue or clone all its controls while holding the context lock.
    pub(crate) fn integration_catalogue_metadata_for_vehicle(&self, id: &str, vehicle: Option<&str>) -> Result<Value, String> {
        let setup = self.provider_setup_revision(id);
        {
            let cache = self.inner.catalogues.lock().unwrap_or_else(|p|p.into_inner());
            if let Some(metadata) = catalogue_metadata(&cache, id, vehicle, &setup) { return Ok(metadata); }
        }
        let catalogue = self.resource_integration_catalogue_for_vehicle(id, vehicle)?;
        Ok(json!({"aircraftModule":catalogue["aircraftModule"],"catalogueRevision":catalogue["catalogueRevision"],"providerSetupRevision":catalogue["providerSetupRevision"]}))
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
            .get(if id == "dcs" { "dcs:CommonData" } else { id })
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
                    "dcs" | "x-plane-11" | "x-plane-12" | "assetto-corsa-rally" | "assetto-corsa" | "assetto-corsa-competizione" | "assetto-corsa-evo"
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
            .retain(|key, _| key != id && !key.starts_with(&format!("{id}:")));
        self.refresh_admitted(std::slice::from_ref(game));
        Ok(())
    }
    /// Full provider metadata remains available for a closed game. Reading never
    /// launches a process, opens telemetry or imports an alternate binding store.
    pub(crate) fn resource_integration_catalogue(&self, id: &str) -> Result<Value, String> {
        self.resource_integration_catalogue_for_vehicle(id, None)
    }
    /// Independent authoring captures never change the saved provider module or
    /// the current native aircraft. The Base capture contains CommonData only.
    pub(crate) fn resource_integration_catalogue_for_vehicle(&self, id: &str, vehicle: Option<&str>) -> Result<Value, String> {
        if !matches!(
            id,
            "dcs" | "x-plane-11" | "x-plane-12" | "assetto-corsa-rally" | "assetto-corsa" | "assetto-corsa-competizione" | "assetto-corsa-evo"
        ) {
            return Err("Choose a supported game.".into());
        }
        let (record, revision) = self.provider_snapshot(id);
        let module = if id == "dcs" {
            validate_connection(id, &record["connection"])?;
            let path = Path::new(record["connection"]["cataloguePath"].as_str().ok_or("Choose a DCS-BIOS catalogue folder.")?);
            Some(match vehicle { Some(vehicle) => crate::dcs_profile::resolve_aircraft_module(path, vehicle)?, None => "CommonData".into() })
        } else { None };
        let key = module.as_ref().map(|module|format!("dcs:{module}")).unwrap_or_else(||id.into());
        let cached = {
            let cache = self.inner.catalogues.lock().unwrap_or_else(|p|p.into_inner());
            cache.get(&key).filter(|(at,known,_)|known == &revision && at.elapsed() < Duration::from_secs(2)).map(|(_,_,result)|result.clone())
        };
        if let Some(cached) = cached {
            return cached.map(|value|value.as_ref().clone());
        }
        let result = (|| {
            if let Some(error) = &self.inner.settings_error {
                return Err(error.clone());
            }
            let mut value = match id {
                "dcs" => {
                    validate_connection(id, &record["connection"])?;
                    let mut query = record["connection"].clone();
                    query["aircraftModule"] = json!(module.as_ref().unwrap());
                    let mut value = crate::dcs_profile::resource_catalogue(&query)?;
                    if let Some(vehicle) = vehicle {
                        value["vehicleId"] = json!(module.as_ref().unwrap());
                        value["vehicleAliases"] = json!(crate::dcs_profile::aircraft_aliases(Path::new(query["cataloguePath"].as_str().unwrap()), module.as_ref().unwrap())?);
                        if Some(vehicle) != module.as_deref() && !value["vehicleAliases"].as_array().unwrap().iter().any(|alias|alias == vehicle) { return Err("The DCS aircraft alias changed during catalogue capture.".into()); }
                    }
                    value
                }
                "x-plane-11" | "x-plane-12" => crate::xplane::catalogue(),
                _ => crate::game_telemetry::catalogue_for_game(id),
            };
            value["catalogueRevision"] =
                json!(digest(&json!(["game-provider-catalogue-v1", id, value])));
            value["providerSetupRevision"] = json!(revision);
            Ok(value)
        })();
        if self.provider_setup_revision(id) != revision {
            return Err("The provider settings changed during catalogue capture; refresh the aircraft catalogue.".into());
        }
        let cached = result.map(Arc::new);
        self.inner.catalogues.lock().unwrap_or_else(|p|p.into_inner()).insert(key, (Instant::now(), revision, cached.clone()));
        cached.map(|value|value.as_ref().clone())
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
        setup.decorate(&mut games);
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
    fn dcs_vehicle_catalogues_are_independent_and_browsing_never_mutates_settings_or_current_revision() {
        let scratch = Scratch::new();
        let catalogue = scratch.0.join("catalogue");
        fs::create_dir_all(&catalogue).unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios");
        for name in ["A-10C.json", "AircraftAliases.json", "MetadataStart.json", "MetadataEnd.json"] { fs::copy(fixtures.join(name), catalogue.join(name)).unwrap(); }
        fs::copy(fixtures.join("CommonData-altitude.json"), catalogue.join("CommonData.json")).unwrap();
        fs::write(catalogue.join("OtherJet.json"), "{}").unwrap();
        fs::write(catalogue.join("F-14.json"), "{}").unwrap();
        fs::write(catalogue.join("AircraftAliases.json"), json!({"A-10C":["A-10C","CommonData"],"A-10C_2":["A-10C","CommonData"],"OtherJet":["OtherJet","CommonData"],"F-14B":["F-14","CommonData"],"F-14A-135-GR":["F-14","CommonData"]}).to_string()).unwrap();
        let path = scratch.0.join("setup.json");
        let setup = GameIntegrationSetup::new(path.clone(), false);
        setup.save_provider_settings(&json!({"command":"game_integration_settings_save","expectedSetupRevision":setup.provider_setup_revision("dcs"),"connection":dcs_connection(&catalogue),"userFolder":scratch.0,"aircraftModule":"A-10C"}), &game("dcs")).unwrap();
        let saved = fs::read(&path).unwrap();
        let base = setup.resource_integration_catalogue("dcs").unwrap();
        let first = setup.resource_integration_catalogue_for_vehicle("dcs", Some("A-10C_2")).unwrap();
        let second = setup.resource_integration_catalogue_for_vehicle("dcs", Some("OtherJet")).unwrap();
        let tomcat = setup.resource_integration_catalogue_for_vehicle("dcs", Some("F-14B")).unwrap();
        setup.inner.catalogues.lock().unwrap().get_mut("dcs:F-14").unwrap().0 -= Duration::from_secs(3);
        let refreshed_tomcat = setup.resource_integration_catalogue_for_vehicle("dcs", Some("F-14")).unwrap();
        assert_eq!(tomcat, refreshed_tomcat);
        assert_eq!(refreshed_tomcat["vehicleAliases"], json!(["F-14A-135-GR", "F-14B"]));
        assert_eq!(first["aircraftModule"], "A-10C");
        assert_eq!(second["aircraftModule"], "OtherJet");
        assert!(first["controls"].as_array().unwrap().iter().any(|row| row["inputs"].as_array().is_some_and(|inputs|!inputs.is_empty())));
        assert!(second["controls"].as_array().unwrap().iter().all(|row|row["sourceModule"] == "CommonData"));
        assert_ne!(first["catalogueRevision"], second["catalogueRevision"]);
        assert_ne!(base["catalogueRevision"], first["catalogueRevision"]);
        assert_eq!(setup.current_catalogue_revision_for_vehicle("dcs", Some("A-10C_2")), first["catalogueRevision"].as_str().map(str::to_owned));
        assert_eq!(setup.current_catalogue_revision("dcs"), base["catalogueRevision"].as_str().map(str::to_owned));
        assert_eq!(fs::read(path).unwrap(), saved);
        setup.inner.catalogues.lock().unwrap().get_mut("dcs:A-10C").unwrap().0 -= Duration::from_secs(3);
        fs::write(catalogue.join("AircraftAliases.json"), "{changed and invalid}").unwrap();
        // Passive observation keeps reading the small prior capture without
        // touching source files. Full preparation still validates aliases.
        let metadata = setup.integration_catalogue_metadata_for_vehicle("dcs", Some("A-10C_2")).unwrap();
        assert_eq!(metadata["catalogueRevision"], first["catalogueRevision"]);
        assert_eq!(metadata["aircraftModule"], "A-10C");
        assert!(metadata.get("controls").is_none());
        assert!(metadata.to_string().len() < 512);
        assert!(setup.resource_integration_catalogue_for_vehicle("dcs", Some("A-10C_2")).is_err());
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
    #[test]
    fn assetto_provider_settings_and_read_only_catalogues_remain_distinct() {
        let scratch = Scratch::new();
        let path = scratch.0.join("setup.json");
        let setup = GameIntegrationSetup::new(path.clone(), false);
        for id in ["assetto-corsa", "assetto-corsa-competizione", "assetto-corsa-evo"] {
            let connection = json!({"source":"shared-memory","freshnessMs":2100});
            save(&setup, id, connection.clone()).unwrap();
            let catalogue = setup.resource_integration_catalogue(id).unwrap();
            assert_eq!(catalogue["provider"], id);
            assert_eq!(catalogue["commands"], json!([]));
            assert_eq!(catalogue["channels"].as_array().unwrap().len(), 5);
            assert_eq!(setup.provider_connection_snapshot(id).unwrap().0, connection);
            let mut unsupported = connection;
            unsupported["commandPort"] = json!(7778);
            assert!(save(&setup, id, unsupported).is_err());
        }
        let reopened = GameIntegrationSetup::new(path, false);
        for id in ["assetto-corsa", "assetto-corsa-competizione", "assetto-corsa-evo"] {
            assert_eq!(setup.provider_setup_revision(id), reopened.provider_setup_revision(id));
        }
    }

}
