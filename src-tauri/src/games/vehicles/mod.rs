//! Passive vehicle discovery. Identity is qualified by native process and fresh
//! simulator telemetry, independently of whether a profile is applied.
use crate::game_telemetry_inspection::Capture;
use crate::{dcs_bios, dcs_profile, game_discovery::ProcessIdentity, game_telemetry};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_VEHICLES: usize = 1024;
const MAX_SAVED: u64 = 512 * 1024;

pub(crate) fn vehicle_id(game: &str, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 256
        || value.chars().any(char::is_control)
        || value == "NONE"
    {
        return None;
    }
    Some(if game.starts_with("x-plane-") {
        value.replace('\\', "/")
    } else {
        value.to_owned()
    })
}
fn vehicle(game: &str, value: &str) -> Option<Value> {
    let id = vehicle_id(game, value)?;
    let name = if game.starts_with("x-plane-") {
        Path::new(&id).file_stem()?.to_str()?.replace('_', " ")
    } else {
        id.clone()
    };
    Some(json!({"id":id,"name":name}))
}

mod sources;
mod observation;
mod polling;
use sources::*;
fn validate_saved(saved: &Value) -> Result<(), String> {
    let games = saved["games"]
        .as_object()
        .filter(|games| games.len() <= 512)
        .ok_or("Invalid remembered vehicle library.")?;
    if saved["schemaVersion"] != 1 {
        return Err("Unsupported remembered vehicle schema.".into());
    }
    for (game, vehicles) in games {
        if !crate::game_discovery::valid_id(game) {
            return Err("Invalid title in remembered vehicles.".into());
        }
        let vehicles = vehicles
            .as_array()
            .filter(|v| v.len() <= MAX_VEHICLES)
            .ok_or("Remembered vehicles exceed their bound.")?;
        let mut ids = std::collections::BTreeSet::new();
        for value in vehicles {
            let id = value["id"].as_str().ok_or("Invalid remembered vehicle.")?;
            if vehicle("", id).is_none()
                || !ids.insert(id)
                || value["name"].as_str().is_none_or(|name| {
                    name.is_empty() || name.len() > 256 || name.chars().any(char::is_control)
                })
            {
                return Err("Invalid or duplicate remembered vehicle.".into());
            }
        }
    }
    Ok(())
}
struct Entry {
    #[cfg(test)]
    injected: bool,
    signature: String,
    integration: String,
    installation: String,
    setup_revision: String,
    enabled: bool,
    process: Option<ProcessIdentity>,
    reader: Option<Reader>,
    current: Option<Value>,
    telemetry: Option<Capture>,
    inspection: Vec<String>,
    inspected: Instant,
    catalogued: Instant,
    vehicles: Vec<Value>,
    error: Option<String>,
    retry: Instant,
    observed: Instant,
    observation_epoch: u64,
    generation: u64,
    polling: bool,
    opening: Option<u64>,
    detached_physics: Option<(crate::dcs_physics::Snapshot, Instant)>,
}
pub(crate) struct Tracker {
    writer: Option<osg_persistence::ObservationWriter<Value>>,
    retention_error: Option<String>,
    origin: Instant,
    seen: BTreeMap<String, BTreeMap<String, Value>>,
    entries: BTreeMap<String, Entry>,
    retired: Vec<Reader>,
}
pub(crate) struct RetiredReaders { _readers: Vec<Reader> }
impl Tracker {
    pub(crate) fn take_retired(&mut self) -> RetiredReaders {
        RetiredReaders { _readers: std::mem::take(&mut self.retired) }
    }
    fn installation(game: &Value) -> String {
        json!([
            game["installPath"],
            game["executable"],
            game["installed"],
            game["integrationKey"],
            game["provider"]
        ])
        .to_string()
    }
    #[cfg(test)]
    pub(crate) fn set_test_vehicle(&mut self, id: &str, selected: Option<&str>) {
        if !self.entries.contains_key(id) {
            self.observe(&json!({"id":id,"integrationCompatible":true,"integrationKey":id,"provider":crate::game_discovery::integration_provider(id)}), &json!({}), None, "test", false, u64::MAX);
        }
        if let Some(entry) = self.entries.get_mut(id) {
            // Injected simulator state changes only when the fixture changes it.
            // Real telemetry freshness remains covered by the tracker tests.
            entry.injected = true;
            entry.current = selected.and_then(|selected| {
                vehicle(
                    if entry.integration.is_empty() {
                        id
                    } else {
                        &entry.integration
                    },
                    selected,
                )
            });
            entry.observed = Instant::now();
        }
    }
    pub(crate) fn new(path: PathBuf) -> Self {
        let loaded = (|| {
            let store = osg_persistence::Repository::new(
                osg_persistence::Database::for_namespace(&path)?,
                "games",
                "vehicles",
                MAX_SAVED as usize,
                validate_saved,
            )?;
            let saved = store
                .load()?
                .unwrap_or(json!({"schemaVersion":1,"games":{}}));
            let seen = saved["games"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(game, values)| {
                    (
                        game.clone(),
                        values
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|value| {
                                let value = value.clone();
                                (value["id"].as_str().unwrap().to_owned(), value)
                            })
                            .collect(),
                    )
                })
                .collect();
            Ok::<_, String>((seen, osg_persistence::ObservationWriter::new(store)?))
        })();
        let (seen, writer, retention_error) = match loaded {
            Ok((seen, writer)) => (seen, Some(writer), None),
            Err(error) => (BTreeMap::new(), None, Some(error)),
        };
        Self {
            writer,
            retention_error,
            origin: Instant::now(),
            seen,
            entries: BTreeMap::new(),
            retired: vec![],
        }
    }

}

#[cfg(test)]
mod tests;
mod views;
