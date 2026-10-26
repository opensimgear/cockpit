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
}
pub(crate) struct Tracker {
    writer: Option<osg_persistence::ObservationWriter<Value>>,
    retention_error: Option<String>,
    origin: Instant,
    seen: BTreeMap<String, BTreeMap<String, Value>>,
    entries: BTreeMap<String, Entry>,
}
impl Tracker {
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
        }
    }
    pub(crate) fn observe(
        &mut self,
        game: &Value,
        connection: &Value,
        process: Option<ProcessIdentity>,
        setup_revision: &str,
        enabled: bool,
        observation_epoch: u64,
    ) {
        let id = game["id"].as_str().unwrap_or("");
        if !crate::game_discovery::valid_id(id)
            || (!self.entries.contains_key(id) && self.entries.len() >= 512)
        {
            return;
        }
        let integration = crate::game_discovery::integration_key(game).unwrap_or("");
        let enabled = enabled && !integration.is_empty();
        let now = Instant::now();
        let signature = json!([
            process.as_ref().map(ProcessIdentity::value),
            setup_revision,
            Self::installation(game),
            enabled
        ])
        .to_string();
        let entry = self.entries.entry(id.into()).or_insert_with(|| Entry {
            #[cfg(test)]
            injected: false,
            signature: String::new(),
            integration: String::new(),
            installation: String::new(),
            setup_revision: String::new(),
            enabled: false,
            process: None,
            reader: None,
            current: None,
            telemetry: None,
            inspection: vec![],
            inspected: now,
            catalogued: now - Duration::from_secs(31),
            vehicles: vec![],
            error: None,
            retry: now,
            observed: now,
            observation_epoch: 0,
        });
        if observation_epoch <= entry.observation_epoch {
            return;
        }
        entry.observation_epoch = observation_epoch;
        if entry.signature != signature {
            entry.integration = integration.into();
            entry.reader = None;
            entry.current = None;
            entry.telemetry = None;
            entry.error = None;
            entry.process = process;
            entry.installation = Self::installation(game);
            entry.setup_revision = setup_revision.into();
            entry.enabled = enabled;
            entry.signature = signature;
            entry.retry = now;
            entry.catalogued = now - Duration::from_secs(31);
        }
        entry.observed = now;
        if entry.catalogued.elapsed() >= Duration::from_secs(30) {
            entry.vehicles = match integration {
                "dcs" => dcs_catalogue(connection).unwrap_or_default(),
                "x-plane-11" | "x-plane-12" => xplane_catalogue(game),
                _ => vec![],
            };
            entry.catalogued = now;
        }
        if enabled && entry.reader.is_none() && now >= entry.retry {
            if let Some(process) = &entry.process {
                match Reader::open(integration, connection, process) {
                    Ok(mut reader) => {
                        let _ = reader.inspect(integration, &entry.inspection);
                        entry.reader = Some(reader);
                        entry.error = None;
                    }
                    Err(error) => {
                        entry.error = Some(error);
                        entry.retry = now + Duration::from_secs(2);
                    }
                }
            }
        }
    }
    /// A separate native worker renews retained process identity without waiting
    /// for setup inspection or aircraft catalogue construction. Telemetry still
    /// has to advance through its own decoder freshness policy below.
    pub(crate) fn poll_verified(
        &mut self,
        games: &[Value],
        mut setup_revision: impl FnMut(&str) -> String,
        mut alive: impl FnMut(&ProcessIdentity) -> bool,
    ) {
        for (id, entry) in &mut self.entries {
            #[cfg(test)]
            if entry.injected {
                continue;
            }
            let qualified = entry.enabled
                && games.iter().any(|game| {
                    game["id"] == id.as_str()
                        && game["installed"] == true
                        && Self::installation(game) == entry.installation
                })
                && setup_revision(id) == entry.setup_revision
                && entry.process.as_ref().is_some_and(&mut alive);
            if qualified {
                entry.observed = Instant::now();
            } else {
                entry.reader = None;
                entry.current = None;
                entry.telemetry = None;
            }
        }
        self.poll();
    }
    pub(crate) fn poll(&mut self) {
        let now_ms = self.origin.elapsed().as_millis() as u64;
        let mut changed = false;
        for (id, entry) in &mut self.entries {
            #[cfg(test)]
            if entry.injected {
                continue;
            }
            // A stalled process scan cannot leave a previous vehicle selected.
            if entry.observed.elapsed() > Duration::from_secs(2) {
                entry.reader = None;
                entry.current = None;
                entry.telemetry = None;
                continue;
            }
            let Some(reader) = &mut entry.reader else {
                continue;
            };
            if !entry.inspection.is_empty() && entry.inspected.elapsed() > Duration::from_secs(5) {
                entry.inspection.clear();
                let _ = reader.inspect(&entry.integration, &[]);
            }
            match reader.poll_telemetry(now_ms) {
                Ok((current, capture)) => {
                    entry.telemetry = Some(capture);
                    entry.current = current.and_then(|value| vehicle(&entry.integration, &value));
                    if let Some(value) = &entry.current {
                        let seen = self.seen.entry(id.clone()).or_default();
                        let key = value["id"].as_str().unwrap();
                        if !seen.contains_key(key) && seen.len() < MAX_VEHICLES {
                            seen.insert(key.into(), value.clone());
                            changed = true;
                        }
                    }
                }
                Err(error) => {
                    entry.current = None;
                    entry.telemetry = None;
                    entry.reader = None;
                    entry.error = Some(error);
                    entry.retry = Instant::now() + Duration::from_secs(2);
                }
            }
        }
        if changed {
            let games = self
                .seen
                .iter()
                .map(|(id, values)| (id.clone(), json!(values.values().collect::<Vec<_>>())))
                .collect::<serde_json::Map<_, _>>();
            let value = json!({"schemaVersion":1,"games":games});
            if let Some(writer) = &self.writer {
                if let Err(error) = writer.queue(value) {
                    self.retention_error = Some(error);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
mod views;
