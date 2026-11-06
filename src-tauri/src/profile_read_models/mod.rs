mod emergency;
mod bounds;
mod pages;
pub(crate) use emergency::Emergency;
// Event-driven projections of committed profile owners. Transport encoding and
// Broker publication happen only on this worker, never on an authority lock.
use crate::{observations, profile_session::Status};
use osg_mapping_host::desktop::Readiness;
use osg_profiles::{activation::{Rule, Target}, service::read_model::Snapshot, session::SelectionMode};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::{Arc, Condvar, Mutex}, thread::JoinHandle};

const CAPACITY: usize = 256;
const TOPICS: [&str; 3] = ["profiles.library", "profiles.runtime", "profiles.desktop"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AutomaticSnapshot {
    pub schema_version: u32,
    #[serde(skip)]
    pub preferred_profiles: std::collections::BTreeMap<String, String>,
    pub mode: SelectionMode,
    pub manual_profile_id: String,
    #[serde(skip)]
    pub rules: Vec<Rule>,
    pub status: String,
    pub message: String,
    pub conflicts: Vec<String>,
    pub last_attempt: Option<Target>,
    pub target: Option<Target>,
    pub read_only: bool,
}
#[derive(Clone)]
pub(crate) enum AutomaticChange { Snapshot(AutomaticSnapshot), Emergency { revision: u64, enabled: bool } }
pub(crate) type AutomaticObserver = Arc<dyn Fn(AutomaticChange) + Send + Sync>;

#[derive(Clone)]
enum Event { Library(Snapshot), Native(Status), Automatic(AutomaticChange), Desktop(Readiness) }
#[derive(Clone, Default)]
struct Captured {
    library: Option<Snapshot>,
    native: Option<Status>,
    automatic: Option<AutomaticSnapshot>,
    emergency: Option<(u64, bool)>,
    desktop: Readiness,
}
impl Captured {
    fn apply(&mut self, event: Event) {
        match event {
            Event::Desktop(snapshot) => self.desktop = snapshot,
            Event::Library(snapshot) => self.library = Some(snapshot),
            Event::Native(snapshot) => self.native = Some(snapshot),
            Event::Automatic(AutomaticChange::Snapshot(snapshot)) => self.automatic = Some(snapshot),
            Event::Automatic(AutomaticChange::Emergency { revision, enabled }) => {
                if self.emergency.is_none_or(|old| revision >= old.0) { self.emergency = Some((revision, enabled)); }
            },
        }
    }
    fn complete(&self) -> bool { self.library.is_some() && self.native.is_some() && self.automatic.is_some() && self.emergency.is_some() }
    fn payloads(&self, rules: &Value) -> [Value; 3] {
        let library = self.library.as_ref().expect("registered library source");
        let native = self.native.as_ref().expect("registered native source");
        let mut session = serde_json::to_value(&library.session).expect("typed session");
        let status = serde_json::to_value(native).expect("typed native status");
        for key in ["state", "appliedSnapshot", "fault"] { session[key] = status[key].clone(); }
        let mut automatic = serde_json::to_value(&self.automatic).expect("typed automatic source");
        automatic["rules"] = rules.clone();
        if self.emergency.expect("registered emergency source").1 {
            automatic["status"] = json!("blocked");
            automatic["message"] = json!("Deactivation or a failed profile control prevents automatic activation. Resolve the reported error, then select Automatic explicitly to resume.");
        }
        [serde_json::to_value(library).expect("typed library"), json!({
            "session":session,"diagnostics":library.diagnostics,"readOnly":library.read_only,
            "profileSession":native,"automatic":automatic,
            "preferredProfiles":self.automatic.as_ref().expect("registered automatic source").preferred_profiles,
        }), json!({"state":self.desktop})]
    }
    fn generation(&self) -> u64 { self.library.as_ref().expect("registered library").generation }
}
struct Queue {
    events: VecDeque<Event>,
    latest: Captured,
    overflow: bool,
    bytes: usize,
    failed: bool,
    closed: bool,
    ready: Option<Result<(), String>>,
}
impl Queue {
    fn capture(&mut self, event: Event) {
        if self.closed { return; }
        let bytes = bounds::event(&event);
        let limit = if matches!(&event, Event::Automatic(AutomaticChange::Snapshot(_))) { bounds::MAX_AUTOMATIC_SOURCE_BYTES } else { bounds::MAX_SOURCE_BYTES };
        if bytes > limit {
            self.failed = true;
            self.closed = true;
            self.events.clear();
            self.latest = Captured::default();
            self.bytes = 0;
            return;
        }
        self.latest.apply(event.clone());
        if self.events.len() == CAPACITY || self.bytes.saturating_add(bytes) > bounds::MAX_QUEUE_BYTES {
            self.events.clear();
            self.bytes = 0;
            self.overflow = true;
        }
        if !self.overflow { self.bytes += bytes; self.events.push_back(event); }
    }
}
struct Inner { queue: Mutex<Queue>, wake: Condvar }
impl Inner {
    fn close(&self) {
        let mut queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        queue.closed = true;
        queue.events.clear();
        queue.latest = Captured::default();
        queue.bytes = 0;
        self.wake.notify_all();
    }
    fn capture(&self, event: Event) {
        let mut queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        queue.capture(event);
        self.wake.notify_one();
    }
}
pub(crate) struct Runtime {
    inner: Arc<Inner>,
    observations: observations::Runtime,
    worker: Mutex<Option<JoinHandle<()>>>,
    pages: Arc<pages::Pages>,
    inventory_change: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Runtime {
    pub(crate) fn new(observations: observations::Runtime) -> Self {
        Self { inner: Arc::new(Inner { queue: Mutex::new(Queue { events: VecDeque::new(),
            latest: Captured::default(), overflow: false, bytes: 0, failed: false, closed: false, ready: None }), wake: Condvar::new() }),
            observations, worker: Mutex::new(None), pages: Arc::new(pages::Pages::default()), inventory_change: None }
    }
    pub(crate) fn with_inventory_change(mut self, changed: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.inventory_change = Some(changed);
        self
    }
    pub(crate) fn library_observer(&self) -> osg_profiles::service::read_model::Observer {
        let inner = self.inner.clone();
        Arc::new(move |snapshot| inner.capture(Event::Library(snapshot)))
    }
    pub(crate) fn native_observer(&self) -> Arc<dyn Fn(Status) + Send + Sync> {
        let inner = self.inner.clone();
        Arc::new(move |snapshot| inner.capture(Event::Native(snapshot)))
    }
    pub(crate) fn desktop_observer(&self) -> Arc<dyn Fn(Readiness) + Send + Sync> {
        let inner = self.inner.clone();
        Arc::new(move |snapshot| inner.capture(Event::Desktop(snapshot)))
    }
    pub(crate) fn automatic_observer(&self) -> AutomaticObserver {
        let inner = self.inner.clone();
        Arc::new(move |snapshot| inner.capture(Event::Automatic(snapshot)))
    }
    /// Source registration/capture precedes this call. Readiness means both
    /// profile topics have a snapshot; the Gateway cannot expose a half-bound reader.
    pub(crate) fn start(&self) -> Result<(), String> {
        let mut worker = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if worker.is_none() {
            let inner = self.inner.clone();
            let observations = self.observations.clone();
            let pages = self.pages.clone();
            let inventory_change = self.inventory_change.clone();
            *worker = Some(std::thread::Builder::new().name("profile-read-models".into())
                .spawn(move || run(inner, observations, pages, inventory_change)).map_err(|error| error.to_string())?);
        }
        drop(worker);
        let mut queue = self.inner.queue.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if let Some(ready) = &queue.ready { return ready.clone(); }
            if queue.closed { return Err("Profile read models closed during bootstrap.".into()); }
            queue = self.inner.wake.wait(queue).unwrap_or_else(|p| p.into_inner());
        }
    }
    pub(crate) fn page(&self, request: &Value) -> Result<Value, String> {
        let object = request.as_object().ok_or("Invalid read-model page request.")?;
        if object.len() != 3 || object.keys().any(|key| !["command", "id", "offset"].contains(&key.as_str())) {
            return Err("Read-model pages require exactly id and offset.".into());
        }
        let id = request["id"].as_str().ok_or("Read-model page identity is required.")?;
        let offset = request["offset"].as_u64().ok_or("Read-model page offset must be a nonnegative integer.")?;
        self.pages.get(id, offset)
    }
    pub(crate) fn shutdown(&self) {
        {
            let mut queue = self.inner.queue.lock().unwrap_or_else(|p| p.into_inner());
            queue.closed = true;
            self.inner.wake.notify_all();
        }
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
        self.pages.clear();
    }
}
impl Drop for Runtime { fn drop(&mut self) { self.shutdown(); } }

fn run(inner: Arc<Inner>, observations: observations::Runtime, pages: Arc<pages::Pages>,
    inventory_change: Option<Arc<dyn Fn() + Send + Sync>>,
) {
    let broker = observations.broker();
    let clock = format!("profiles:{}", uuid::Uuid::new_v4());
    let mut captured = Captured::default();
    let mut publishers = Vec::<osg_streams::Publisher>::new();
    let mut previous = None::<[Value; 3]>;
    let mut source_generation = 0;
    let mut previous_inventory_owner = None;
    let mut rules = pages::PreparedRules::default();
    loop {
        let (events, overflow) = {
            let mut queue = inner.queue.lock().unwrap_or_else(|p| p.into_inner());
            while !queue.closed && queue.events.is_empty() && !queue.overflow {
                queue = inner.wake.wait(queue).unwrap_or_else(|p| p.into_inner());
            }
            if queue.closed {
                if queue.failed && queue.ready.is_none() { queue.ready = Some(Err("Profile source projection exceeds its bound.".into())); inner.wake.notify_all(); }
                break;
            }
            if queue.overflow {
                captured = queue.latest.clone();
                queue.overflow = false;
                queue.events.clear();
                queue.bytes = 0;
                (Vec::new(), true)
            } else { queue.bytes = 0; (queue.events.drain(..).collect::<Vec<_>>(), false) }
        };
        // Every retained lifecycle event is published in capture order. Overflow
        // explicitly resets reader baselines instead of silently skipping states.
        let mut steps = events.into_iter().map(Some).collect::<Vec<_>>();
        if overflow { steps.push(None); }
        for event in steps {
            if let Some(event) = event { captured.apply(event); }
            if let (Some(changed), Some(native)) = (&inventory_change, &captured.native) {
                let owner = (native.state.clone(), native.owner.clone(), native.applied_snapshot.clone());
                if previous_inventory_owner.as_ref().is_some_and(|previous| previous != &owner) {
                    changed();
                }
                previous_inventory_owner = Some(owner);
            }
            if !captured.complete() { continue; }
            if let Err(error) = rules.prepare(&captured.automatic.as_ref().expect("registered automatic source").rules, &pages) {
                let mut queue = inner.queue.lock().unwrap_or_else(|p| p.into_inner());
                if queue.ready.is_none() { queue.ready = Some(Err(error)); inner.wake.notify_all(); }
                drop(queue);
                for publisher in &publishers { broker.retire(publisher); }
                inner.close(); pages.clear(); return;
            }
            let payloads = captured.payloads(rules.wire());
            if publishers.is_empty() {
                let result = TOPICS.into_iter().zip(payloads.iter()).try_for_each(|(topic, payload)| {
                    broker.register(osg_streams::TopicSpec { topic: topic.into(), source: "profiles".into(),
                        audiences: vec!["ui".into(), "browser".into(), "mcp".into()] }, payload.clone(),
                        observations.captured_us(), clock.clone())
                        .map(|publisher| publishers.push(publisher)).map_err(|error| error.to_string())
                });
                {
                    let mut queue = inner.queue.lock().unwrap_or_else(|p| p.into_inner());
                    queue.ready = Some(result.clone());
                    inner.wake.notify_all();
                }
                if result.is_err() { for publisher in &publishers { broker.retire(publisher); } inner.close(); pages.clear(); return; }
            } else {
                for (index, publisher) in publishers.iter().enumerate() {
                    let result = if source_generation != captured.generation() {
                        publisher.replace_generation(publisher.generation().saturating_add(1), payloads[index].clone(), observations.captured_us(), &clock)
                            .map(|_| ())
                    } else if overflow {
                        publisher.resynchronize(publisher.generation(), payloads[index].clone(), observations.captured_us(), &clock).map(|_| ())
                    } else if previous.as_ref().is_some_and(|old| old[index] == payloads[index]) { Ok(()) }
                    else if serde_json::to_vec(&payloads[index]).map_or(true, |bytes| bytes.len() > osg_streams::MAX_CHANGE_BYTES) {
                        publisher.resynchronize(publisher.generation(), payloads[index].clone(), observations.captured_us(), &clock).map(|_| ())
                    }
                    else { publisher.publish(publisher.generation(), payloads[index].clone(), payloads[index].clone(), observations.captured_us(), &clock).map(|_| ()) };
                    if result.is_err() { for publisher in &publishers { broker.retire(publisher); } inner.close(); pages.clear(); return; }
                }
            }
            rules.pin(&pages);
            source_generation = captured.generation();
            previous = Some(payloads);
        }
    }
    for publisher in &publishers { broker.retire(publisher); }
    inner.close();
    pages.clear();
}

#[cfg(test)]
mod tests;
