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
    sync::Arc,
};

mod inspection;
mod catalogues;
mod capture_wakes;
pub(crate) use catalogues::CatalogueSource;
pub(crate) use capture_wakes::CaptureWakes;

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
    Some(if game == "x-plane" || game.starts_with("x-plane-") {
        value.replace('\\', "/")
    } else {
        value.to_owned()
    })
}
fn vehicle(game: &str, value: &str) -> Option<Value> {
    let id = vehicle_id(game, value)?;
    let name = if game == "x-plane" || game.starts_with("x-plane-") {
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
    installation_fields: Arc<[Value; 10]>,
    setup_revision: String,
    enabled: bool,
    process: Option<ProcessIdentity>,
    reader: Option<Reader>,
    current: Option<Value>,
    telemetry: Option<Arc<Capture>>,
    inspection: inspection::Demand,
    error: Option<String>,
    retry: Instant,
    observed: Instant,
    observation_epoch: u64,
    generation: u64,
    polling: bool,
    opening: Option<u64>,
    detached_physics: Option<crate::dcs_physics::SourceView>,
    capture_watch: Option<CaptureBinding>,
    inventory_revision: u64,
    published: Visible,
    published_capture: Option<std::sync::Weak<Capture>>,
}
struct CaptureBinding {
    _watch: sources::CaptureWatch,
    wake: Arc<osg_streams::NativeWake>,
    drained: u64,
}
impl Entry {
    fn retained_process_current(&self) -> Option<bool> {
        self.capture_watch.as_ref()?._watch.retained_process_current()
    }

    fn process_current(&self) -> bool {
        self.retained_process_current().unwrap_or_else(|| self.observed.elapsed() <= Duration::from_secs(2))
    }

    fn process_deadline(&self) -> Option<Instant> {
        if self.retained_process_current().is_some() { return None; }
        self.observed.checked_add(Duration::from_secs(2) + Duration::from_nanos(1))
    }
}
impl CaptureBinding {
    fn open(reader: &Reader, worker: &Arc<osg_streams::NativeWake>) -> Result<Self, String> {
        let wake = Arc::new(osg_streams::NativeWake::child(worker.clone()));
        let watch = reader.capture_watch(wake.clone())?
            .ok_or("Vehicle telemetry has no native capture notification.")?;
        Ok(Self { _watch: watch, wake, drained: 0 })
    }

    fn require_live(reader: &Reader, worker: &Arc<osg_streams::NativeWake>) -> Result<Self, String> {
        let binding = Self::open(reader, worker)?;
        if binding._watch.retained_process_current() != Some(true) {
            return Err("Vehicle telemetry requires a live watch on the exact game process.".into());
        }
        Ok(binding)
    }

    fn begin_drain(&mut self) -> bool {
        let epoch = self.wake.observed();
        std::mem::replace(&mut self.drained, epoch) != epoch || epoch == u64::MAX
    }
}
#[derive(Default, PartialEq)]
struct Visible { current: Option<Value>, error: Option<String>, inventory_revision: u64 }
pub(crate) struct Tracker {
    writer: Option<osg_persistence::ObservationWriter<Value>>,
    retention_error: Option<String>,
    origin: Instant,
    seen: BTreeMap<String, BTreeMap<String, Value>>,
    entries: BTreeMap<String, Entry>,
    catalogues: BTreeMap<CatalogueSource, Result<Vec<Value>, String>>,
    retired: Vec<Reader>,
    changes: Option<Arc<crate::game_read_models::signals::Signal>>,
    published_deadline: Option<Instant>,
    capture_wakes: Option<Arc<CaptureWakes>>,
}
pub(crate) struct RetiredReaders { _readers: Vec<Reader> }
impl Tracker {
    const INSTALLATION_FIELDS: [&'static str; 10] = [
        "installPath", "executable", "installed", "integrationKey", "provider",
        "integrationCompatible", "selectedInstallationId", "processExecutables",
        "processMonitoringSupported", "launchSupported",
    ];

    fn installation_fields(game: &Value) -> Arc<[Value; 10]> {
        Arc::new(Self::INSTALLATION_FIELDS.map(|field| game[field].clone()))
    }

    fn installation_matches(known: &[Value; 10], game: &Value) -> bool {
        known.iter().zip(Self::INSTALLATION_FIELDS).all(|(value, field)| value == &game[field])
    }

    pub(crate) fn watch_capture_lanes(&mut self, wakes: Arc<CaptureWakes>) {
        #[cfg(test)]
        for entry in self.entries.values_mut() {
            entry.capture_watch = entry.reader.as_ref().and_then(|reader|
                CaptureBinding::open(reader, wakes.get(&entry.integration)?).ok());
        }
        self.capture_wakes = Some(wakes);
    }
    #[cfg(test)]
    fn watch_captures(&mut self, wake: Arc<osg_streams::NativeWake>) {
        self.watch_capture_lanes(Arc::new(CaptureWakes::shared(wake)));
    }
    pub(crate) fn context_deadline(&self, configured: &[String], serviced_at: Instant) -> Option<Instant> {
        configured.iter().filter_map(|id| {
            let entry = self.entries.get(id)?;
            (entry.enabled && entry.process.is_some() && entry.reader.is_none()
                && entry.opening.is_none() && !entry.polling && entry.retry > serviced_at).then_some(entry.retry)
        }).min()
    }
    #[cfg(test)]
    fn capture_deadline(&self) -> Option<Instant> {
        self.capture_deadline_for(None)
    }
    pub(crate) fn capture_deadline_for(&self, integration: Option<&str>) -> Option<Instant> {
        let now = Instant::now();
        self.entries.values()
            .filter(|entry| integration.is_none_or(|id| id == entry.integration))
            .filter_map(|entry| {
                let reader = entry.reader.as_ref()?;
                let deadline = entry.capture_watch.as_ref().and_then(|_| reader.assetto_deadline());
                deadline.into_iter().chain(reader.ownship_retry_deadline())
                    .chain(entry.inspection.deadline(now)).min()
            }).min()
    }
    pub(crate) fn watch_changes(&mut self, changes: Arc<crate::game_read_models::signals::Signal>) {
        let weak = Arc::downgrade(&changes);
        if let Some(writer) = &self.writer {
            writer.watch_error(Arc::new(move || {
                if let Some(changes) = weak.upgrade() { changes.mark(crate::game_read_models::signals::Domain::Runtime); }
            }));
        }
        self.changes = Some(changes);
    }
    fn changed(&self) {
        if let Some(changes) = &self.changes { changes.mark(crate::game_read_models::signals::Domain::Runtime); }
    }
    fn publish_visible(&mut self, id: &str) {
        let current = self.current(id);
        let Some(entry) = self.entries.get_mut(id) else { return; };
        let visible = Visible { current, error: entry.error.clone(), inventory_revision: entry.inventory_revision };
        let capture = entry.telemetry.as_ref().map(Arc::downgrade);
        let changed = match (&entry.published_capture, &capture) {
            (Some(old), Some(new)) => !old.ptr_eq(new),
            (None, None) => false,
            _ => true,
        };
        if changed {
            entry.published_capture = capture;
            if let Some(changes) = &self.changes { changes.mark_telemetry(); }
        }
        if entry.published != visible {
            let vehicle_changed = entry.published.current != visible.current;
            let context_changed = entry.published.current.as_ref().and_then(|vehicle| vehicle["id"].as_str())
                != visible.current.as_ref().and_then(|vehicle| vehicle["id"].as_str());
            entry.published = visible;
            self.changed();
            if let Some(changes) = &self.changes {
                if context_changed { changes.mark_vehicle_context(); }
                if vehicle_changed { changes.mark_resources(); }
            }
        }
        let deadline = self.view_deadline();
        if self.published_deadline != deadline {
            self.published_deadline = deadline;
            if let Some(changes) = &self.changes { changes.wake.notify(); }
        }
    }
    /// The current cached view expires at its earliest process-observation or
    /// telemetry-capture fence. An elapsed fence is consumed once, not polled.
    pub(crate) fn view_deadline(&self) -> Option<Instant> {
        let now = Instant::now();
        self.entries.values().filter_map(|entry| {
            #[cfg(test)]
            if entry.injected { return None; }
            entry.current.as_ref()?;
            let capture = entry.telemetry.as_ref()?;
            if !entry.process_current() { return None; }
            let captured = capture.at.checked_add(Duration::from_secs(2))?;
            let deadline = entry.process_deadline().map_or(captured, |at| at.min(captured));
            (deadline > now).then_some(deadline)
        }).min()
    }
    pub(crate) fn telemetry_deadlines(&self) -> Vec<(Option<Instant>, Arc<Capture>)> {
        self.entries.values().filter_map(|entry| {
            if !entry.process_current() { return None; }
            Some((entry.process_deadline(), entry.telemetry.as_ref()?.clone()))
        }).collect()
    }
    pub(crate) fn take_retired(&mut self) -> RetiredReaders {
        RetiredReaders { _readers: std::mem::take(&mut self.retired) }
    }
    fn installation(game: &Value) -> String {
        json!(Self::INSTALLATION_FIELDS.map(|field| &game[field])).to_string()
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
        self.publish_visible(id);
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
            catalogues: BTreeMap::new(),
            retired: vec![],
            changes: None,
            published_deadline: None,
            capture_wakes: None,
        }
    }

}

#[cfg(test)]
mod tests;
mod views;
#[cfg(test)]
mod stream_tests;
