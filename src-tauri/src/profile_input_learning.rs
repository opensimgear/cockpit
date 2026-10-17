//! One capture-only native HID learner. UI polling reads cached state and never
//! pumps reports, starts outputs or changes a profile/local assignment.
use osg_controller_input::{registry::MAX_CONTROLS, stream};
use osg_profiles::resources::{ResourceToken, Route, Transport};
use osg_profiles::resources_ports::PortDescriptor;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WINDOW_US: u64 = 30_000_000;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Candidate {
    descriptor: PortDescriptor,
    contract_hash: String,
    route_ids: Vec<String>,
    current: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    schema_version: u32,
    learn_id: String,
    resource_id: String,
    generation: String,
    capability_revision: String,
    phase: String,
    reason: String,
    captured_us: String,
    until_us: String,
    candidates: Vec<Candidate>,
}
#[derive(Clone)]
struct Pin {
    resource_id: String,
    generation: String,
    capability_revision: String,
    physical_id: String,
    route_id: String,
    provider_generation: String,
    token: ResourceToken,
    ports: BTreeMap<String, Candidate>,
}
type Inventory = Arc<dyn Fn(Value) -> Result<Value, String> + Send + Sync>;
struct Capture {
    inventory: Inventory,
    snapshot: Arc<dyn Fn(&[String]) -> Result<stream::Snapshot, String> + Send + Sync>,
    page: Arc<dyn Fn(&str, u64) -> stream::Page + Send + Sync>,
    validate: Arc<dyn Fn(&ResourceToken) -> Result<Route, String> + Send + Sync>,
}
impl Capture {
    fn native(
        resources: Arc<crate::profile_resources::Runtime>,
        controllers: Arc<osg_controller_input::ControllerService>,
    ) -> Self {
        let inventory = resources.clone();
        let snapshots = controllers.clone();
        Self {
            inventory: Arc::new(move |request| inventory.request(&request)),
            snapshot: Arc::new(move |ids| snapshots.stream_snapshot(ids)),
            page: Arc::new(move |session, cursor| controllers.stream_page(session, cursor)),
            validate: Arc::new(move |token| resources.validate_delivery(token)),
        }
    }
    fn prepare(
        &self,
        resource_id: &str,
        generation: &str,
        capability_revision: &str,
    ) -> Result<(Pin, stream::Snapshot), String> {
        let first = (self.inventory)(
            json!({"command":"profile_resources_inspect","resourceId":resource_id,"offset":0,"limit":128}),
        )?;
        let resource = first["resource"].clone();
        if resource["id"] != resource_id
            || resource["resourceType"] != "host-controller"
            || resource["availability"] != "online"
            || resource["generation"] != generation
            || resource["capabilityRevision"] != capability_revision
        {
            return Err(
                "Learning requires the exact current physical host-controller resource.".into(),
            );
        }
        let connections = resource["connections"]
            .as_array()
            .filter(|items| items.len() == 1)
            .ok_or("Choose one unambiguous physical HID route.")?;
        let route: Route = serde_json::from_value(connections[0]["route"].clone())
            .map_err(|_| "The native HID route is malformed.")?;
        let physical = route
            .id
            .strip_prefix("hid:")
            .filter(|id| canonical_uuid(id))
            .ok_or("The native HID route has no exact physical identity.")?
            .to_owned();
        if connections[0]["scope"] != "host-controllers"
            || connections[0]["key"] != physical
            || route.transport != Transport::Hid
            || !route.baseline_ready
        {
            return Err("Learning requires a live, baseline-ready physical HID route.".into());
        }
        let total = first["total"]
            .as_u64()
            .filter(|total| *total > 0 && *total <= MAX_CONTROLS as u64)
            .ok_or("The native controller catalogue is empty or exceeds its control limit.")?
            as usize;
        let session = first["session"]
            .as_str()
            .ok_or("The inventory capture has no session.")?
            .to_owned();
        let revision = first["revision"]
            .as_str()
            .ok_or("The inventory capture has no revision.")?
            .to_owned();
        let mut ports = BTreeMap::new();
        let mut next = first;
        let mut offset = 0;
        loop {
            if next["session"] != session
                || next["revision"] != revision
                || next["resource"] != resource
                || next["offset"] != offset
                || next["total"] != total
            {
                return Err("The controller catalogue changed during learning capture.".into());
            }
            let page = next["ports"]
                .as_array()
                .filter(|ports| {
                    !ports.is_empty() && ports.len() <= 128 && offset + ports.len() <= total
                })
                .ok_or("The controller catalogue page is incomplete.")?;
            for value in page {
                let candidate: Candidate = serde_json::from_value(value.clone())
                    .map_err(|_| "The controller port descriptor is malformed.")?;
                if !candidate.current
                    || candidate.descriptor.direction != osg_profiles::Direction::Output
                    || candidate.route_ids != [route.id.clone()]
                    || !sha256(&candidate.contract_hash)
                    || ports
                        .insert(candidate.descriptor.id.clone(), candidate)
                        .is_some()
                {
                    return Err("Learning only accepts current exact physical HID ports.".into());
                }
            }
            offset += page.len();
            if offset == total {
                break;
            }
            next = (self.inventory)(
                json!({"command":"profile_resources_inspect","resourceId":resource_id,"offset":offset,"limit":128}),
            )?;
        }
        let snapshot = (self.snapshot)(&[physical.clone()])?;
        let source = snapshot
            .sources
            .first()
            .filter(|_| snapshot.sources.len() == 1)
            .filter(|source| {
                source.id == physical
                    && source.baseline.reason == stream::Reason::Baseline
                    && source.baseline.generation.to_string() == route.provider_generation
                    && source.controls.len() <= MAX_CONTROLS
            })
            .ok_or("The native controller needs a fresh complete baseline.")?;
        let known = crate::profile_resource_ports::controls(&source.controls)
            .into_iter()
            .map(|port| (port.id.clone(), port))
            .collect::<BTreeMap<_, _>>();
        if known.len() != ports.len()
            || known.iter().any(|(id, descriptor)| {
                ports
                    .get(id)
                    .is_none_or(|port| port.descriptor != *descriptor)
                    || !source.baseline.values.contains_key(id)
            })
        {
            return Err("The retained catalogue differs from actual native HID controls.".into());
        }
        let observed = ports
            .keys()
            .next()
            .ok_or("The controller has no learnable controls.")?;
        let raw = (self.inventory)(
            json!({"command":"profile_resources_token","resourceId":resource_id,"portId":observed,"access":"read"}),
        )?;
        let token: ResourceToken = serde_json::from_value(raw["resourceToken"].clone())
            .map_err(|_| "The native read token is malformed.")?;
        let pin = Pin {
            resource_id: resource_id.into(),
            generation: generation.into(),
            capability_revision: capability_revision.into(),
            physical_id: physical,
            route_id: route.id,
            provider_generation: route.provider_generation,
            token,
            ports,
        };
        self.validate_pin(&pin)?;
        Ok((pin, snapshot))
    }
    fn validate_pin(&self, pin: &Pin) -> Result<(), String> {
        let token = &pin.token;
        if token.resource_id != pin.resource_id
            || token.generation != pin.generation
            || token.capability_revision != pin.capability_revision
            || token.route_id != pin.route_id
            || token.route_id != format!("hid:{}", pin.physical_id)
            || token.provider_generation != pin.provider_generation
            || token.access != osg_profiles::resources::Access::Read
            || !pin
                .ports
                .get(&token.port_id)
                .is_some_and(|port| port.contract_hash == token.contract_hash)
        {
            return Err(
                "The native learning read token does not match its exact resource/ports.".into(),
            );
        }
        let route = (self.validate)(token)?;
        if route.transport != Transport::Hid
            || route.id != pin.route_id
            || route.provider_generation != pin.provider_generation
            || !route.baseline_ready
        {
            return Err("The physical HID route changed during input learning.".into());
        }
        Ok(())
    }
}
fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
fn sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
fn counter(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|number| number.to_string() == value)
}
struct Entry {
    pin: Pin,
    learner: Option<osg_mapping_host::Learn>,
    status: Status,
    epoch: u64,
    started: Instant,
}
impl Entry {
    fn stop(&mut self, reason: &str) {
        self.learner = None;
        self.status.phase = "stopped".into();
        self.status.reason = reason.into();
        self.status.candidates.clear();
    }
}
#[derive(Default)]
struct State {
    serial: u64,
    entry: Option<Entry>,
}
type Shared = Arc<(Mutex<State>, Condvar)>;
pub(crate) struct Runtime {
    shared: Shared,
    capture: Arc<Capture>,
    epoch: Arc<AtomicU64>,
    stopped: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl Runtime {
    pub(crate) fn new(
        resources: Arc<crate::profile_resources::Runtime>,
        controllers: Arc<osg_controller_input::ControllerService>,
        epoch: Arc<AtomicU64>,
    ) -> Self {
        Self::with_capture(Capture::native(resources, controllers), epoch)
    }
    fn with_capture(capture: Capture, epoch: Arc<AtomicU64>) -> Self {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let capture = Arc::new(capture);
        let args = (
            shared.clone(),
            capture.clone(),
            epoch.clone(),
            stopped.clone(),
        );
        let worker = thread::Builder::new()
            .name("profile-input-learning".into())
            .spawn(move || worker(args.0, args.1, args.2, args.3))
            .ok();
        Self {
            shared,
            capture,
            epoch,
            stopped,
            worker: Mutex::new(worker),
        }
    }
    pub(crate) fn start(
        &self,
        resource_id: &str,
        generation: &str,
        capability_revision: &str,
    ) -> Result<Value, String> {
        self.start_at(
            resource_id,
            generation,
            capability_revision,
            self.epoch.load(Ordering::Acquire),
        )
    }
    /// Admission is proved by Manager before releasing its lock for baseline
    /// capture. An intervening output request cannot renew that proof here.
    pub(crate) fn start_at(
        &self,
        resource_id: &str,
        generation: &str,
        capability_revision: &str,
        expected_epoch: u64,
    ) -> Result<Value, String> {
        if self.stopped.load(Ordering::Acquire)
            || self
                .worker
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_none()
        {
            return Err("Native input learning is unavailable.".into());
        }
        if resource_id.is_empty()
            || resource_id.len() > 4096
            || resource_id.chars().any(char::is_control)
            || !counter(generation)
            || !sha256(capability_revision)
        {
            return Err(
                "Choose a current native resource generation and capability revision.".into(),
            );
        }
        let epoch = expected_epoch;
        if self.epoch.load(Ordering::Acquire) != epoch {
            return Err("Input learning admission changed before native capture.".into());
        }
        let serial = {
            let mut state = self.shared.0.lock().unwrap_or_else(|p| p.into_inner());
            state.serial = state
                .serial
                .checked_add(1)
                .ok_or("Input learning generation exhausted.")?;
            if let Some(entry) = state.entry.as_mut() {
                entry.stop("Replaced by a new input-learning capture.");
            }
            state.serial
        };
        let (pin, snapshot) = self
            .capture
            .prepare(resource_id, generation, capability_revision)?;
        let until = snapshot
            .captured_us
            .checked_add(WINDOW_US)
            .ok_or("The native learning clock exhausted.")?;
        let at = snapshot.captured_us;
        let learner = osg_mapping_host::Learn::new(snapshot)?;
        let status=Status{schema_version:1,learn_id:uuid::Uuid::new_v4().to_string(),resource_id:pin.resource_id.clone(),generation:pin.generation.clone(),capability_revision:pin.capability_revision.clone(),phase:"listening".into(),reason:"Move one physical control deliberately. Choose a candidate explicitly; no profile was changed.".into(),captured_us:at.to_string(),until_us:until.to_string(),candidates:vec![]};
        let result = serde_json::to_value(&status).map_err(|error| error.to_string())?;
        let mut state = self.shared.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.serial != serial
            || self.epoch.load(Ordering::Acquire) != epoch
            || self.stopped.load(Ordering::Acquire)
        {
            return Err("Input learning was cancelled during native capture.".into());
        }
        state.entry = Some(Entry {
            pin,
            learner: Some(learner),
            status,
            epoch,
            started: Instant::now(),
        });
        drop(state);
        self.shared.1.notify_one();
        Ok(result)
    }
    pub(crate) fn status(&self, learn_id: &str) -> Result<Value, String> {
        let state = self.shared.0.lock().unwrap_or_else(|p| p.into_inner());
        let entry = state
            .entry
            .as_ref()
            .filter(|entry| canonical_uuid(learn_id) && entry.status.learn_id == learn_id)
            .ok_or("The input-learning capture expired; start a new one explicitly.")?;
        serde_json::to_value(&entry.status).map_err(|error| error.to_string())
    }
    pub(crate) fn cancel(&self, learn_id: &str) -> Result<Value, String> {
        let mut state = self.shared.0.lock().unwrap_or_else(|p| p.into_inner());
        if !state
            .entry
            .as_ref()
            .is_some_and(|entry| canonical_uuid(learn_id) && entry.status.learn_id == learn_id)
        {
            return Err("The input-learning capture expired; start a new one explicitly.".into());
        }
        state.serial = state
            .serial
            .checked_add(1)
            .ok_or("Input learning generation exhausted.")?;
        let entry = state.entry.as_mut().unwrap();
        entry.stop("Input learning cancelled explicitly.");
        let result = serde_json::to_value(&entry.status).map_err(|error| error.to_string());
        drop(state);
        self.shared.1.notify_one();
        result
    }
    pub(crate) fn invalidate(&self, reason: &str) {
        let mut state = self.shared.0.lock().unwrap_or_else(|p| p.into_inner());
        state.serial = state.serial.saturating_add(1);
        if let Some(entry) = state.entry.as_mut() {
            entry.stop(reason);
        }
        drop(state);
        self.shared.1.notify_one();
    }
    pub(crate) fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        self.invalidate("Input learning stopped with the native service.");
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn worker(shared: Shared, capture: Arc<Capture>, epoch: Arc<AtomicU64>, stopped: Arc<AtomicBool>) {
    while !stopped.load(Ordering::Acquire) {
        let mut state = shared.0.lock().unwrap_or_else(|p| p.into_inner());
        let serial = state.serial;
        let Some(entry) = state
            .entry
            .as_mut()
            .filter(|entry| entry.status.phase != "stopped")
        else {
            drop(
                shared
                    .1
                    .wait_timeout(state, Duration::from_millis(20))
                    .unwrap_or_else(|p| p.into_inner()),
            );
            continue;
        };
        let pin = entry.pin.clone();
        let mut learner = entry.learner.take();
        let id = entry.status.learn_id.clone();
        let expected_epoch = entry.epoch;
        let started = entry.started;
        let previous = entry.status.captured_us.parse::<u64>().unwrap();
        let until = entry.status.until_us.parse::<u64>().unwrap();
        drop(state);
        let result = (|| -> Result<Option<(u64, Value)>, String> {
            if epoch.load(Ordering::Acquire) != expected_epoch {
                return Err("Native control changed; restart input learning explicitly.".into());
            }
            if started.elapsed() >= Duration::from_secs(30) {
                return Err("Input learning expired after 30 seconds.".into());
            }
            capture.validate_pin(&pin)?;
            if let Some(learner) = learner.as_mut() {
                let page = (capture.page)(&learner.session, learner.cursor);
                if page.captured_us < previous {
                    return Err("The native input clock changed; restart learning.".into());
                }
                let at = page.captured_us;
                learner.pump(page);
                capture.validate_pin(&pin)?;
                Ok(Some((at, learner.status())))
            } else if previous >= until {
                Err("Input learning expired after 30 seconds.".into())
            } else {
                Ok(None)
            }
        })();
        let mut state = shared.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.serial == serial {
            if let Some(entry) = state
                .entry
                .as_mut()
                .filter(|entry| entry.status.learn_id == id && entry.status.phase != "stopped")
            {
                if epoch.load(Ordering::Acquire) != expected_epoch {
                    entry.stop("Native control changed; restart input learning explicitly.");
                } else {
                    match result {
                        Err(reason) => entry.stop(&reason),
                        Ok(Some((at, status))) => {
                            entry.status.captured_us = at.to_string();
                            entry.status.phase =
                                status["phase"].as_str().unwrap_or("stopped").into();
                            entry.status.reason = status["reason"].as_str().unwrap_or("").into();
                            let candidates = status["candidates"]
                                .as_array()
                                .map(|ids| {
                                    ids.iter()
                                        .filter_map(Value::as_str)
                                        .map(|id| pin.ports.get(id).cloned())
                                        .collect::<Option<Vec<_>>>()
                                })
                                .flatten();
                            if let Some(candidates) = candidates.filter(|items| items.len() <= 32) {
                                entry.status.candidates = candidates;
                            } else {
                                entry.stop("The learned controls no longer match the exact native catalogue.");
                            }
                            if entry.status.phase == "listening" {
                                entry.learner = learner;
                            } else if entry.status.phase == "stopped" {
                                entry.status.candidates.clear();
                            }
                        }
                        Ok(None) => {}
                    }
                }
            }
        }
        drop(
            shared
                .1
                .wait_timeout(state, Duration::from_millis(20))
                .unwrap_or_else(|p| p.into_inner()),
        );
    }
}

#[cfg(test)]
#[path = "profile_input_learning_tests.rs"]
mod tests;
#[cfg(test)]
pub(crate) fn manager_fixture(epoch:Arc<AtomicU64>,gate:Option<Arc<std::sync::Barrier>>)->(Runtime,Value){tests::manager_fixture(epoch,gate)}
