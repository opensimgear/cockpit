//! Passive vehicle discovery. Identity is qualified by native process and fresh
//! simulator telemetry, independently of whether a profile is applied.
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

fn dcs_aliases(connection: &Value) -> Result<Value, String> {
    let path = Path::new(
        connection["cataloguePath"]
            .as_str()
            .ok_or("Set up DCS-BIOS to discover aircraft.")?,
    );
    let aliases = dcs_bios::read_json(&path.join("AircraftAliases.json"), dcs_bios::MAX_DOCUMENT)?;
    let object = aliases
        .as_object()
        .filter(|object| object.len() <= MAX_VEHICLES)
        .ok_or("Invalid DCS aircraft aliases catalogue.")?;
    if object.iter().any(|(id, modules)| {
        (!matches!(id.as_str(), "" | "NONE") && vehicle_id("dcs", id).is_none())
            || modules.as_array().is_none_or(|modules| {
                modules.len() > 32 || modules.iter().any(|module| module.as_str().is_none())
            })
    }) {
        return Err("Invalid DCS aircraft aliases catalogue.".into());
    }
    Ok(aliases)
}
fn dcs_catalogue(connection: &Value) -> Result<Vec<Value>, String> {
    Ok(dcs_aliases(connection)?
        .as_object()
        .unwrap()
        .keys()
        .filter_map(|id| vehicle("dcs", id))
        .collect())
}
fn xplane_catalogue(game: &Value) -> Vec<Value> {
    let Some(root) = game["installPath"].as_str().map(Path::new) else {
        return vec![];
    };
    let mut result = vec![];
    let mut pending = vec![(root.join("Aircraft"), 0)];
    let mut visited = 0;
    while let Some((directory, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 20_000 || result.len() >= MAX_VEHICLES {
                return result;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() && depth < 12 {
                pending.push((entry.path(), depth + 1));
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("acf"))
            {
                if let Ok(relative) = entry.path().strip_prefix(root) {
                    if let Some(value) = vehicle(
                        game["id"].as_str().unwrap_or(""),
                        &relative.to_string_lossy(),
                    ) {
                        result.push(value);
                    }
                }
            }
        }
    }
    result
}

enum Reader {
    Dcs {
        source: dcs_bios::Transport,
        decoder: dcs_profile::ExportSession,
        guard: dcs_profile::DcsCatalogueGuard,
    },
    Rally {
        source: Box<dyn game_telemetry::Source>,
        decoder: game_telemetry::Session,
    },
    Xplane(crate::xplane::Source),
}
impl Reader {
    fn open(id: &str, connection: &Value, process: &ProcessIdentity) -> Result<Self, String> {
        match id {
            "dcs" => {
                let aliases = dcs_aliases(connection)?;
                let names = aliases.as_object().unwrap();
                let module = names
                    .iter()
                    .find(|(id, modules)| {
                        !id.is_empty()
                            && modules.as_array().is_some_and(|modules| {
                                modules.iter().any(|module| module == id.as_str())
                            })
                    })
                    .map(|(id, _)| id)
                    .ok_or("DCS aircraft catalogue is empty.")?;
                let path = Path::new(connection["cataloguePath"].as_str().unwrap());
                let mut query = connection.clone();
                query["aircraftModule"] = json!(module);
                let ports = dcs_profile::resource_catalogue(&query)?;
                let (_, guard, mut catalogue) = dcs_profile::guarded_telemetry_sources(
                    path,
                    module,
                    &[],
                    ports["sourceRevision"]
                        .as_str()
                        .ok_or("DCS catalogue has no captured revision.")?,
                )?;
                catalogue.aircraft_names = names
                    .keys()
                    .filter(|id| vehicle_id("dcs", id).is_some())
                    .cloned()
                    .collect();
                let network = dcs_bios::Network::parse(connection)?;
                let decoder = dcs_profile::ExportSession::from_sources(
                    catalogue,
                    network.freshness_ms,
                    vec![],
                )?;
                Ok(Self::Dcs {
                    source: dcs_bios::Transport::open_network(&network)?,
                    decoder,
                    guard,
                })
            }
            "assetto-corsa-rally" => {
                let image = process
                    .path
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&process.path);
                Ok(Self::Rally {
                    source: Box::new(crate::acr_windows::WindowsSource::new_for_image(
                        process.pid,
                        image,
                    )?),
                    decoder: game_telemetry::Session::new(
                        connection["freshnessMs"].as_u64().unwrap_or(1500),
                    )?,
                })
            }
            "x-plane-11" | "x-plane-12" => {
                let mut connection = connection.clone();
                connection["processIdentity"] = process.value();
                // A separate ephemeral return port avoids taking over a profile's subscriptions.
                connection["listenPort"] = json!(0);
                Ok(Self::Xplane(crate::xplane::Source::new(
                    &connection,
                    vec![],
                )?))
            }
            _ => Err("Vehicle introspection is unavailable for this game.".into()),
        }
    }
    fn poll(&mut self, now_ms: u64) -> Result<Option<String>, String> {
        match self {
            Self::Dcs {
                source,
                decoder,
                guard,
            } => {
                guard.validate(false)?;
                let mut drained = false;
                for _ in 0..64 {
                    let Some(bytes) = source.receive()? else {
                        drained = true;
                        break;
                    };
                    decoder.feed(&bytes, now_ms);
                }
                if !drained && source.receive()?.is_some() {
                    decoder.reset_stream(now_ms);
                    return Err("DCS vehicle observation backlog exceeded its bound.".into());
                }
                decoder.tick(now_ms);
                let snapshot = decoder.snapshot(now_ms);
                Ok(snapshot.aircraft.filter(|_| snapshot.fresh))
            }
            Self::Rally { source, decoder } => {
                let snapshot = decoder.poll(source.as_mut(), now_ms);
                if snapshot.status == "error" {
                    return Err(snapshot
                        .error
                        .unwrap_or_else(|| "Rally vehicle telemetry failed.".into()));
                }
                Ok(snapshot
                    .context
                    .filter(|_| snapshot.fresh)
                    .map(|context| context.car_model))
            }
            Self::Xplane(source) => {
                let snapshot = source.poll(Instant::now()).0;
                Ok(snapshot.aircraft.filter(|_| snapshot.fresh))
            }
        }
    }
}
struct Entry {
    signature: String,
    installation: String,
    setup_revision: String,
    enabled: bool,
    process: Option<ProcessIdentity>,
    reader: Option<Reader>,
    current: Option<Value>,
    catalogued: Instant,
    vehicles: Vec<Value>,
    error: Option<String>,
    retry: Instant,
    observed: Instant,
    observation_epoch: u64,
}
pub(crate) struct Tracker {
    path: PathBuf,
    origin: Instant,
    seen: BTreeMap<String, BTreeMap<String, Value>>,
    entries: BTreeMap<String, Entry>,
}
impl Tracker {
    fn installation(game: &Value) -> String {
        json!([game["installPath"], game["executable"], game["installed"]]).to_string()
    }
    #[cfg(test)]
    pub(crate) fn set_test_vehicle(&mut self, id: &str, selected: Option<&str>) {
        self.observe(&json!({"id":id}), &json!({}), None, "test", false, u64::MAX);
        if let Some(entry) = self.entries.get_mut(id) {
            entry.current = selected.and_then(|selected| vehicle(id, selected));
            entry.observed = Instant::now();
        }
    }
    pub(crate) fn new(path: PathBuf) -> Self {
        let seen = (|| {
            if fs::metadata(&path).ok()?.len() > MAX_SAVED {
                return None;
            }
            let saved: Value = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
            let games = saved["games"].as_object()?;
            if saved["schemaVersion"] != 1 || games.len() > 4 {
                return None;
            }
            let mut result = BTreeMap::new();
            for (game, vehicles) in games {
                if !matches!(
                    game.as_str(),
                    "dcs" | "assetto-corsa-rally" | "x-plane-11" | "x-plane-12"
                ) {
                    return None;
                }
                let vehicles = vehicles
                    .as_array()
                    .filter(|items| items.len() <= MAX_VEHICLES)?;
                let values = vehicles
                    .iter()
                    .map(|value| vehicle(game, value["id"].as_str()?))
                    .collect::<Option<Vec<_>>>()?;
                result.insert(
                    game.clone(),
                    values
                        .into_iter()
                        .map(|v| (v["id"].as_str().unwrap().to_owned(), v))
                        .collect(),
                );
            }
            Some(result)
        })()
        .unwrap_or_default();
        Self {
            path,
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
        let now = Instant::now();
        let signature = json!([
            process.as_ref().map(ProcessIdentity::value),
            setup_revision,
            Self::installation(game),
            enabled
        ])
        .to_string();
        let entry = self.entries.entry(id.into()).or_insert_with(|| Entry {
            signature: String::new(),
            installation: String::new(),
            setup_revision: String::new(),
            enabled: false,
            process: None,
            reader: None,
            current: None,
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
            entry.reader = None;
            entry.current = None;
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
            entry.vehicles = match id {
                "dcs" => dcs_catalogue(connection).unwrap_or_default(),
                "x-plane-11" | "x-plane-12" => xplane_catalogue(game),
                _ => vec![],
            };
            entry.catalogued = now;
        }
        if enabled && entry.reader.is_none() && now >= entry.retry {
            if let Some(process) = &entry.process {
                match Reader::open(id, connection, process) {
                    Ok(reader) => {
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
            let qualified = entry.enabled
                && games.iter().any(|game| game["id"] == id.as_str()
                    && game["installed"] == true && Self::installation(game) == entry.installation)
                && setup_revision(id) == entry.setup_revision
                && entry.process.as_ref().is_some_and(&mut alive);
            if qualified {
                entry.observed = Instant::now();
            } else {
                entry.reader = None;
                entry.current = None;
            }
        }
        self.poll();
    }
    pub(crate) fn poll(&mut self) {
        let now_ms = self.origin.elapsed().as_millis() as u64;
        let mut changed = false;
        for (id, entry) in &mut self.entries {
            // A stalled process scan cannot leave a previous vehicle selected.
            if entry.observed.elapsed() > Duration::from_secs(2) {
                entry.reader = None;
                entry.current = None;
                continue;
            }
            let Some(reader) = &mut entry.reader else {
                continue;
            };
            match reader.poll(now_ms) {
                Ok(current) => {
                    entry.current = current.and_then(|value| vehicle(id, &value));
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
            let _ = crate::dcs_setup::replace(&self.path, value.to_string().as_bytes());
        }
    }
    pub(crate) fn current(&self, id: &str) -> Option<Value> {
        self.entries
            .get(id)
            .filter(|entry| entry.observed.elapsed() <= Duration::from_secs(2))
            .and_then(|entry| entry.current.clone())
    }
    pub(crate) fn decorate(&self, game: &mut Value) {
        let id = game["id"].as_str().unwrap_or("").to_owned();
        let supported = matches!(
            id.as_str(),
            "dcs" | "assetto-corsa-rally" | "x-plane-11" | "x-plane-12"
        );
        let mut values = self.seen.get(&id).cloned().unwrap_or_default();
        if let Some(entry) = self.entries.get(&id) {
            for value in &entry.vehicles {
                values.insert(value["id"].as_str().unwrap().into(), value.clone());
            }
        }
        let message = if id == "assetto-corsa-rally" {
            "Cars are discovered from native telemetry. Load a car once to add it to the vehicle list."
        } else if id == "dcs" {
            "Aircraft come from the installed DCS-BIOS catalogue. Current aircraft requires fresh DCS-BIOS exports."
        } else {
            "Aircraft come from this installation's Aircraft folder. Current aircraft requires fresh RREF telemetry."
        };
        game["vehicleSupport"] = json!({"supported":supported,"kind":if id=="assetto-corsa-rally"{"car"}else{"aircraft"},"vehicles":values.values().collect::<Vec<_>>(),"message":message});
        game["currentVehicle"] = self.current(&id).unwrap_or(Value::Null);
        game["vehicleStatus"] = json!({"status":if game["currentVehicle"].is_object(){"ready"}else if self.entries.get(&id).is_some_and(|entry|entry.error.is_some()){"unavailable"}else{"waiting"},"message":self.entries.get(&id).and_then(|entry|entry.error.as_deref()).unwrap_or("Waiting for a running game and fresh vehicle telemetry.")});
    }
}

#[cfg(test)]
#[path = "game_vehicles_tests.rs"]
mod tests;
