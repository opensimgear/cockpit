use super::{details::{self, Registry}, pages::Pages, signals::{Batch, Domain, Job, Signal}};
use crate::{games::GamesRuntime, observations};
use osg_streams::{Broker, Publisher, TopicSpec};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::{atomic::{AtomicBool, Ordering}, Arc, Condvar, Mutex, MutexGuard},
    thread::JoinHandle, time::Instant};

pub(super) struct Inner {
    registry: Mutex<Registry>,
    pub changed: Condvar,
    pub closed: AtomicBool,
    pub signal: Arc<Signal>,
}
impl Inner {
    pub fn lock(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(|error| error.into_inner())
    }
}

pub(crate) struct Runtime {
    inner: Arc<Inner>,
    observations: observations::Runtime,
    games: Arc<GamesRuntime>,
    pages: Arc<Pages>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl Runtime {
    pub(crate) fn new(observations: observations::Runtime, games: Arc<GamesRuntime>) -> Self {
        Self { inner: Arc::new(Inner { registry: Mutex::new(Registry::default()), changed: Condvar::new(),
            closed: AtomicBool::new(false), signal: games.read_model_signal() }),
            observations, games, pages: Arc::new(Pages::default()), worker: Mutex::new(None) }
    }

    pub(crate) fn start(&self) -> Result<(), String> {
        let mut worker = self.worker.lock().unwrap_or_else(|error| error.into_inner());
        if self.inner.closed.load(Ordering::Acquire) { return Err("Game read models are closed.".into()); }
        if worker.is_none() {
            let (inner, observations, games, pages) = (self.inner.clone(), self.observations.clone(), self.games.clone(), self.pages.clone());
            let spawned = std::thread::Builder::new().name("game-read-models".into())
                .spawn(move || run(inner, observations, games, pages)).map_err(|error| error.to_string())?;
            let _ = self.inner.signal.wake.bind_thread(spawned.thread().clone());
            *worker = Some(spawned);
        }
        drop(worker);
        let mut registry = self.inner.lock();
        loop {
            if let Some(ready) = &registry.ready { return ready.clone(); }
            if self.inner.closed.load(Ordering::Acquire) { return Err("Game read models closed during bootstrap.".into()); }
            registry = self.inner.changed.wait(registry).unwrap_or_else(|error| error.into_inner());
        }
    }

    pub(crate) fn page(&self, request: &Value) -> Result<Value, String> {
        page_request(&self.pages, self.inner.closed.load(Ordering::Acquire), request)
    }

    pub(crate) fn with_detail<T>(&self, game_id: &str, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        if !crate::game_installations::valid_game_id(game_id) { return Err("Choose a registered game.".into()); }
        self.games.registered_profile_game(game_id)?;
        let lease = details::Lease::acquire(&self.inner, game_id)?;
        lease.wait_ready()?;
        action()
    }

    pub(crate) fn shutdown(&self) {
        self.inner.closed.store(true, Ordering::Release);
        self.inner.signal.close();
        self.inner.changed.notify_all();
        if let Some(worker) = self.worker.lock().unwrap_or_else(|error| error.into_inner()).take() {
            if worker.thread().id() != std::thread::current().id() { let _ = worker.join(); }
        }
        self.pages.clear();
    }
}
impl Drop for Runtime { fn drop(&mut self) { self.shutdown(); } }

fn page_request(pages: &Pages, closed: bool, request: &Value) -> Result<Value, String> {
    if closed { return Err("Game read models are closed.".into()); }
    let object = request.as_object().ok_or("Invalid game read-model page request.")?;
    if object.len() != 3 || object.keys().any(|key| !["command", "id", "offset"].contains(&key.as_str()))
        || request["command"] != "game_read_model_page"
    { return Err("Game read-model pages require exactly command, id and offset.".into()); }
    let id = request["id"].as_str().ok_or("Game read-model page identity is required.")?;
    let offset = request["offset"].as_u64().ok_or("Game read-model page offset must be a nonnegative integer.")?;
    pages.get(id, offset)
}

fn topic(broker: &Broker, observations: &observations::Runtime, clock: &str, name: &str, payload: Value) -> Result<Publisher, String> {
    broker.register(TopicSpec { topic: name.into(), source: "games".into(),
        audiences: vec!["ui".into(), "browser".into(), "mcp".into()] }, payload,
        observations.captured_us(), clock.into()).map_err(|error| error.to_string())
}

fn publish(observations: &observations::Runtime, publisher: &Publisher, clock: &str, value: Value,
    previous: Option<&Value>, lost: bool, ordered: bool) -> Result<(), String>
{
    if !lost && previous == Some(&value) { return Ok(()); }
    let oversized = serde_json::to_vec(&value).map_err(|error| error.to_string())?.len() > osg_streams::MAX_CHANGE_BYTES;
    let generation = publisher.generation();
    let captured_us = observations.captured_us();
    if lost || oversized { publisher.resynchronize(generation, value, captured_us, clock).map(|_| ()) }
    else if ordered { publisher.publish(generation, value.clone(), value, captured_us, clock).map(|_| ()) }
    else { publisher.publish_latest_state(generation, value, captured_us, clock).map(|_| ()) }
        .map_err(|error| error.to_string())
}

#[derive(Default)]
struct Jobs { values: BTreeMap<String, Job>, previous: Option<Value> }
impl Jobs {
    fn steps(&mut self, batch: &Batch, bootstrap: bool) -> Vec<(Value, bool)> {
        if bootstrap || batch.lost {
            self.values = batch.jobs.iter().map(|job| (job.key.clone(), job.clone())).collect();
            return vec![(json!({"jobs":self.values.values().collect::<Vec<_>>()}), batch.lost)];
        }
        batch.events.iter().map(|job| {
            self.values.insert(job.key.clone(), job.clone());
            (json!({"jobs":self.values.values().collect::<Vec<_>>()}), false)
        }).collect()
    }
}

#[derive(Default)]
struct Worker {
    state: Option<Publisher>,
    jobs: Option<Publisher>,
    telemetry: Option<Publisher>,
    previous: Option<Value>,
    job_state: Jobs,
    discovery: Option<Value>,
}
impl Worker {
    fn jobs(&mut self, broker: &Broker, observations: &observations::Runtime, clock: &str, batch: &Batch, bootstrap: bool) -> Result<(), String> {
        for (payload, lost) in self.job_state.steps(batch, bootstrap) {
            if let Some(publisher) = &self.jobs {
                publish(observations, publisher, clock, payload.clone(), self.job_state.previous.as_ref(), lost, true)?;
            } else { self.jobs = Some(topic(broker, observations, clock, "games.jobs", payload.clone())?); }
            self.job_state.previous = Some(payload);
        }
        Ok(())
    }

    fn project(&mut self, inner: &Inner, observations: &observations::Runtime, games: &GamesRuntime, pages: &Pages,
        tasks: &[details::Task], clock: &str, dirty: u32, lost: bool) -> Result<(), String>
    {
        let broker = observations.broker();
        let discovery_dirty = Domain::Discovery.bit() | Domain::Library.bit() | Domain::Artwork.bit();
        let refresh_discovery = self.discovery.is_none() || dirty & discovery_dirty != 0;
        let mut projection = pages.begin();
        let ids = tasks.iter().map(|task| task.game_id.clone()).collect::<Vec<_>>();
        let mut parts = games.capture_read_model_parts(refresh_discovery, &ids, &mut |image| projection.asset(image.clone()))?;
        let _library_revision = parts.library_revision;
        let mut details = Vec::new();
        let mut revisions = BTreeMap::new();
        let mut missing = Vec::new();
        for task in tasks {
            let Some(value) = parts.details.remove(&task.game_id) else { missing.push(task); continue; };
            let node = projection.encode(&value)?;
            let scope = scope(&node)?;
            revisions.insert(task.game_id.clone(), scope.clone());
            details.push((task, json!({"gameId":task.game_id,"scope":scope,"value":node})));
        }
        if let Some(rows) = parts.catalogue.as_array_mut() {
            for row in rows {
                if let Some(revision) = row.get("id").and_then(Value::as_str).and_then(|id| revisions.get(id)) {
                    row["detailsRevision"] = json!(revision);
                }
            }
        } else { return Err("Invalid game read-model catalogue.".into()); }
        let discovery = if refresh_discovery {
            projection.encode(&parts.discovery.ok_or("Game read-model discovery source is missing.")?)?
        } else {
            projection.encode(self.discovery.as_ref().expect("encoded discovery baseline"))?
        };
        let catalogue = projection.encode(&parts.catalogue)?;
        let state = parts.meta.as_object_mut().ok_or("Invalid game read-model metadata.")?;
        state.insert("catalogue".into(), catalogue);
        state.insert("discovery".into(), discovery.clone());
        let payload = json!({"value":projection.encode(&parts.meta)?});
        // All current roots, including unchanged detail/discovery nodes, are retained.
        projection.commit("games.state")?;
        for task in missing { details::retire_missing(inner, &broker, task); }
        for (task, detail) in details {
            let publisher = if let Some(publisher) = &task.publisher {
                publish(observations, publisher, clock, detail.clone(), task.last.as_ref(), lost, false)?;
                publisher.clone()
            } else { topic(&broker, observations, clock, &format!("games.detail.{}", task.game_id), detail.clone())? };
            if !details::accept(inner, task, publisher.clone(), detail) { broker.retire(&publisher); }
        }
        if let Some(publisher) = &self.state {
            publish(observations, publisher, clock, payload.clone(), self.previous.as_ref(), lost, false)?;
        } else { self.state = Some(topic(&broker, observations, clock, "games.state", payload.clone())?); }
        self.previous = Some(payload);
        self.discovery = Some(discovery);
        Ok(())
    }
}

fn scope(node: &Value) -> Result<String, String> {
    let bytes = serde_json::to_vec(node).map_err(|error| error.to_string())?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn run(inner: Arc<Inner>, observations: observations::Runtime, games: Arc<GamesRuntime>, pages: Arc<Pages>) {
    let mut worker = Worker::default();
    let result = catch_worker(|| run_loop(&inner, &observations, &games, &pages, &mut worker));
    let broker = observations.broker();
    finish(&inner, &broker, &pages, worker, result);
}

fn catch_worker(action: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)).unwrap_or_else(|panic| {
        let message = panic.downcast_ref::<String>().map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied()).unwrap_or("unknown panic payload");
        Err(format!("Game read-model worker panicked: {message}"))
    })
}

fn finish(inner: &Inner, broker: &Broker, pages: &Pages, worker: Worker, result: Result<(), String>) {
    let mut registry = inner.lock();
    for slot in registry.slots.values_mut() {
        if let Some(publisher) = slot.publisher.take() { broker.retire(&publisher); }
        slot.ready = Some(Err(result.as_ref().err().cloned().unwrap_or_else(|| "Game read models are closed.".into())));
    }
    if let Some(publisher) = worker.state { broker.retire(&publisher); }
    if let Some(publisher) = worker.jobs { broker.retire(&publisher); }
    if let Some(publisher) = worker.telemetry { broker.retire(&publisher); }
    if let Err(error) = result { registry.ready = Some(Err(error)); }
    inner.closed.store(true, Ordering::Release);
    inner.changed.notify_all();
    drop(registry);
    inner.signal.close();
    pages.clear();
}

fn run_loop(inner: &Inner, observations: &observations::Runtime, games: &GamesRuntime, pages: &Pages, worker: &mut Worker) -> Result<(), String> {
    let broker = observations.broker();
    let clock = format!("games:{}", uuid::Uuid::new_v4());
    let mut bootstrap = true;
    let mut pending_lost = false;
    let mut telemetry_deadline = None;
    loop {
        let observed = inner.signal.wake.observed();
        if inner.closed.load(Ordering::Acquire) { return Ok(()); }
        let batch = inner.signal.take(bootstrap)?;
        if let Some(batch) = &batch { worker.jobs(&broker, observations, &clock, batch, bootstrap)?; }
        bootstrap = false;
        let (tasks, retired) = details::tasks(inner, &broker);
        let dirty = batch.as_ref().map_or(0, |batch| batch.dirty);
        pending_lost |= batch.as_ref().is_some_and(|batch| batch.lost);
        let capture_deadline = games.read_model_deadline();
        let telemetry_changed = inner.signal.take_telemetry();
        if worker.telemetry.is_none() || telemetry_changed || dirty != 0 || expired_deadline(telemetry_deadline, Instant::now()) {
            telemetry_deadline = games.telemetry_deadline();
            if let Some(publisher) = &worker.telemetry {
                publish(observations, publisher, &clock, json!({}), None, false, false)?;
            } else {
                worker.telemetry = Some(topic(&broker, observations, &clock, "games.telemetry", json!({}))?);
            }
        }
        if dirty != 0 || retired || tasks.iter().any(|task| task.publisher.is_none()) || worker.state.is_none() {
            match worker.project(inner, observations, games, pages, &tasks, &clock, dirty, pending_lost) {
                Err(error) if error.starts_with("Game read-model source changed") => {
                    inner.signal.mark(Domain::Library); inner.signal.mark(Domain::Runtime); continue;
                }
                result => result?,
            }
            pending_lost = false;
        }
        if expired_deadline(capture_deadline, Instant::now()) {
            inner.signal.mark(Domain::Runtime);
            continue;
        }
        if worker.state.is_some() && worker.jobs.is_some() {
            let mut registry = inner.lock();
            if registry.ready.is_none() { registry.ready = Some(Ok(())); inner.changed.notify_all(); }
        }
        let deadline = pending_deadline(capture_deadline, games.read_model_deadline());
        let next_deadline = pending_deadline(deadline, telemetry_deadline);
        inner.signal.wake.wait_scheduled(observed, next_deadline.map(|at| at.saturating_duration_since(Instant::now())));
        if expired_deadline(deadline, Instant::now())
            && !inner.closed.load(Ordering::Acquire)
        { inner.signal.mark(Domain::Runtime); }
    }
}

fn pending_deadline(captured: Option<Instant>, current: Option<Instant>) -> Option<Instant> {
    // Future-only source helpers may omit a fence that elapsed during capture.
    // Keep the observed fence until this worker has invalidated its projection.
    captured.into_iter().chain(current).min()
}

fn expired_deadline(scheduled: Option<Instant>, now: Instant) -> bool {
    // Another source's later deadline or a renewal cannot suppress this expiry.
    // An unchanged projection is suppressed by equality at publication.
    scheduled.is_some_and(|at| at <= now)
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
