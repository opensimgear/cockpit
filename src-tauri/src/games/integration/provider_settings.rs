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
fn sdk_stamp(id: &str, record: &Value) -> Value {
    if !id.starts_with("x-plane-") {
        return Value::Null;
    }
    let path = record["executable"]
        .as_str()
        .map(Path::new)
        .and_then(Path::parent)
        .map(|root| root.join("Resources/plugins/DataRefs.txt"));
    path.and_then(|path| fs::metadata(path).ok())
        .map(|metadata| {
            json!([
                metadata.len(),
                metadata
                    .modified()
                    .ok()
                    .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos().to_string())
            ])
        })
        .unwrap_or(Value::Null)
}
fn catalogue_metadata(
    cache: &CatalogueCache,
    id: &str,
    vehicle: Option<&str>,
    setup: &str,
) -> Option<Value> {
    let selected = if let ("dcs", Some(vehicle)) = (id, vehicle) {
        cache
            .iter()
            .filter(|(key, _)| key.starts_with("dcs:"))
            .find_map(|(_, (_, revision, result))| {
                let value = result.as_ref().ok()?;
                (revision == setup
                    && (value["aircraftModule"] == vehicle
                        || value["vehicleAliases"]
                            .as_array()
                            .is_some_and(|aliases| aliases.iter().any(|alias| alias == vehicle))))
                .then_some(value)
            })
    } else {
        cache
            .get(if id == "dcs" { "dcs:CommonData" } else { id })
            .filter(|(_, revision, _)| revision == setup)
            .and_then(|(_, _, result)| result.as_ref().ok())
    }?;
    Some(
        json!({"aircraftModule":selected["aircraftModule"],"catalogueRevision":selected["catalogueRevision"],"providerSetupRevision":selected["providerSetupRevision"]}),
    )
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
            if id == "dcs" {
                Some(crate::dcs_physics::revision())
            } else {
                None
            },
            self.inner.settings_error
        ]));
        (record, revision)
    }
    pub(crate) fn provider_setup_revision(&self, id: &str) -> String {
        self.provider_snapshot(id).1
    }
    pub(crate) fn provider_preferences_snapshot(&self, id: &str) -> (Value, String) {
        let (saved, revision) = self.provider_snapshot(id);
        let mut settings =
            json!({"connection":saved.get("connection").cloned().unwrap_or_else(|| defaults(id))});
        if id == "dcs" {
            for key in ["userFolder", "aircraftModule"] {
                if let Some(value) = saved.get(key) {
                    settings[key] = value.clone();
                }
            }
        }
        (settings, revision)
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
    pub(crate) fn current_catalogue_revision_for_vehicle(
        &self,
        id: &str,
        vehicle: Option<&str>,
    ) -> Option<String> {
        let setup = self.provider_setup_revision(id);
        let cache = self
            .inner
            .catalogues
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        catalogue_metadata(&cache, id, vehicle, &setup)?["catalogueRevision"]
            .as_str()
            .map(str::to_owned)
    }
    /// Small captured identity only. Source guards and explicit preparation own
    /// file validation; passive status/observation must not reparse an aircraft
    /// catalogue or clone all its controls while holding the context lock.
    pub(crate) fn integration_catalogue_metadata_for_vehicle(
        &self,
        id: &str,
        vehicle: Option<&str>,
    ) -> Result<Value, String> {
        let setup = self.provider_setup_revision(id);
        {
            let cache = self
                .inner
                .catalogues
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(metadata) = catalogue_metadata(&cache, id, vehicle, &setup) {
                return Ok(metadata);
            }
        }
        let catalogue = self.resource_integration_catalogue_for_vehicle(id, vehicle)?;
        Ok(
            json!({"aircraftModule":catalogue["aircraftModule"],"catalogueRevision":catalogue["catalogueRevision"],"providerSetupRevision":catalogue["providerSetupRevision"]}),
        )
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
            .filter(|id| crate::game_discovery::integration_provider(id).is_some())
            .ok_or("Choose an available telemetry integration.")?;
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
            if id == "dcs" {
                Some(crate::dcs_physics::revision())
            } else {
                None
            },
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
        write_settings(&self.inner, &draft)?;
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
}

#[path = "catalogues.rs"]
mod catalogues;
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
#[path = "provider_tests.rs"]
mod tests;
