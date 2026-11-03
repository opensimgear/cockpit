//! Deadline-driven enrichment lifecycle. No status polling or retained JSON.
use super::*;
use std::{collections::BTreeSet, sync::{Arc, Weak, atomic::{AtomicBool, Ordering}}, thread::JoinHandle};

const MAX_OWNERS: usize = 8;
// One full saved library plus the discovery Apply selection limit.
const MAX_TARGETS: usize = 512 + 1024;
const MAX_SCAN_BYTES: usize = 128;

pub(super) struct Inner {
    pub(super) wake: osg_streams::NativeWake,
    closed: AtomicBool,
}
pub(super) struct Registration {
    root: PathBuf,
    targets: BTreeSet<u32>,
    scan: String,
    inner: Weak<Inner>,
    attempts: BTreeMap<u32, Attempt>,
}
#[derive(Clone)]
enum Attempt { Waiting, Pending(u64, String), Ready, Failed { scan: String, retry: Instant } }
pub(crate) struct Owner {
    id: u64,
    inner: Arc<Inner>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Owner {
    pub(crate) fn new(root: &Path) -> Result<Self, String> {
        let (id, inner) = register(service(), root)?;
        let running = inner.clone();
        let worker = match std::thread::Builder::new().name("steam-artwork-owner".into())
            .spawn(move || run(service(), id, running)) {
                Ok(worker) => worker,
                Err(error) => {
                    unregister(service(), id);
                    return Err(format!("Cannot start Steam artwork owner: {error}"));
                }
            };
        Ok(Self { id, inner, worker: Mutex::new(Some(worker)) })
    }

    pub(crate) fn replace(&self, state: &Value) -> Result<(), String> {
        if self.inner.closed.load(Ordering::Acquire) { return Err("Steam artwork owner closed.".into()); }
        replace(service(), self.id, state)?;
        self.inner.wake.notify();
        Ok(())
    }

    pub(crate) fn shutdown(&self) {
        self.inner.closed.store(true, Ordering::Release);
        unregister(service(), self.id);
        self.inner.wake.notify();
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() { let _ = worker.join(); }
    }
}
impl Drop for Owner { fn drop(&mut self) { self.shutdown(); } }

fn register(pool: &Mutex<Service>, root: &Path) -> Result<(u64, Arc<Inner>), String> {
    let inner = Arc::new(Inner { wake: Default::default(), closed: AtomicBool::new(false) });
    let mut service = pool.lock().unwrap_or_else(|p| p.into_inner());
    if service.owners.len() >= MAX_OWNERS { return Err("Steam artwork owner limit reached.".into()); }
    let id = service.next_owner.checked_add(1).ok_or("Steam artwork owner identity exhausted.")?;
    service.next_owner = id;
    service.owners.insert(id, Registration { root: root.into(), targets: Default::default(),
        scan: String::new(), inner: Arc::downgrade(&inner), attempts: Default::default() });
    Ok((id, inner))
}

fn replace(pool: &Mutex<Service>, id: u64, state: &Value) -> Result<(), String> {
    let scan = state["discovery"]["scanId"].as_str().unwrap_or("");
    if scan.len() > MAX_SCAN_BYTES { return Err("Steam artwork scan identity exceeds its bound.".into()); }
    let targets = if state["discovery"]["status"] == "scanning" { BTreeSet::new() }
        else { targets::collect(state).into_iter().map(|target| target.app_id).collect::<BTreeSet<_>>() };
    if targets.len() > MAX_TARGETS { return Err("Steam artwork target set exceeds its bound.".into()); }
    let mut service = pool.lock().unwrap_or_else(|p| p.into_inner());
    let owner = service.owners.get_mut(&id).ok_or("Steam artwork owner closed.")?;
    let root = owner.root.clone();
    owner.targets = targets;
    let targets = &owner.targets;
    owner.attempts.retain(|app_id, _| targets.contains(app_id));
    owner.scan = scan.into();
    retire_orphans(&mut service, &root);
    Ok(())
}

fn retire_orphans(service: &mut Service, root: &Path) {
    let retained = service.owners.values().filter(|owner| owner.root == root)
        .flat_map(|owner| owner.targets.iter().copied()).collect::<BTreeSet<_>>();
    service.jobs.retain(|job| job.key.root != root || retained.contains(&job.key.app_id));
    let retired = service.states.iter().filter(|(key, state)| key.root == root
        && !retained.contains(&key.app_id) && matches!(state, State::Pending | State::Failed { .. }))
        .map(|(key, _)| key.clone()).collect::<Vec<_>>();
    for key in retired { service.states.remove(&key); service.tickets.remove(&key); }
}

fn unregister(pool: &Mutex<Service>, id: u64) {
    let wakes = {
        let mut service = pool.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(owner) = service.owners.remove(&id) { retire_orphans(&mut service, &owner.root); }
        wakes(&service)
    };
    for inner in wakes { inner.wake.notify(); }
}

pub(super) fn wakes(service: &Service) -> Vec<Arc<Inner>> {
    service.owners.values().filter_map(|owner| owner.inner.upgrade()).collect()
}

/// Accepted work intent is visible before the scheduler first requests its job.
pub(super) fn pending(service: &Service, key: &Key) -> bool {
    service.owners.values().any(|owner| owner.root == key.root && owner.targets.contains(&key.app_id)
        && matches!(owner.attempts.get(&key.app_id), None | Some(Attempt::Waiting | Attempt::Pending(_, _))))
}

fn step(pool: &Mutex<Service>, id: u64, now: Instant, spawn: impl FnMut() -> Result<(), String>) -> Option<Instant> {
    let mut service = pool.lock().unwrap_or_else(|p| p.into_inner());
    let owner = service.owners.get(&id)?;
    let root = owner.root.clone();
    let scan = owner.scan.clone();
    let targets = owner.targets.clone();
    let attempts = owner.attempts.clone();
    let mut deadline = None;
    for app_id in targets {
        match attempts.get(&app_id) {
            Some(Attempt::Pending(_, _) | Attempt::Ready) => continue,
            Some(Attempt::Failed { scan: previous, retry }) if previous == &scan && *retry > now => {
                deadline = Some(deadline.map_or(*retry, |old: Instant| old.min(*retry))); continue;
            }
            _ => {}
        };
        let key = Key { root: root.clone(), app_id };
        let attempt = match service.request(key.clone(), &scan, now) {
            State::Pending => Attempt::Pending(service.tickets[&key], scan.clone()),
            State::Ready(_) => Attempt::Ready,
            State::Failed { scan, retry } => {
                if service.states.contains_key(&key) {
                    deadline = Some(deadline.map_or(retry, |old: Instant| old.min(retry)));
                    Attempt::Failed { scan, retry }
                } else { Attempt::Waiting }
            }
        };
        service.owners.get_mut(&id)?.attempts.insert(app_id, attempt);
    }
    let spawn_failed = start_workers(&mut service, spawn);
    // A spawn failure creates real retry deadlines after request() returned Pending.
    synchronize(&mut service);
    let owner = service.owners.get(&id)?;
    for attempt in owner.attempts.values() {
        if let Attempt::Failed { retry, .. } = attempt {
            deadline = Some(deadline.map_or(*retry, |old| old.min(*retry)));
        }
    }
    let wakes = if spawn_failed { wakes(&service) } else { Vec::new() };
    drop(service);
    for inner in wakes { inner.wake.notify(); }
    deadline
}

/// Copy only terminal outcomes, never image strings, into bounded owner bookkeeping.
pub(super) fn synchronize(service: &mut Service) {
    for owner in service.owners.values_mut() {
        for (app_id, attempt) in &mut owner.attempts {
            let key = Key { root: owner.root.clone(), app_id: *app_id };
            let Attempt::Pending(ticket, attempted_scan) = attempt else { continue; };
            if service.tickets.get(&key) != Some(ticket) { continue; }
            match service.states.get(&key) {
                Some(State::Ready(_)) => *attempt = Attempt::Ready,
                Some(State::Failed { retry, .. }) => *attempt = Attempt::Failed { scan: attempted_scan.clone(), retry: *retry },
                _ => {}
            }
        }
    }
}

pub(super) fn refreshed(service: &mut Service, key: &Key) {
    for owner in service.owners.values_mut().filter(|owner| owner.root == key.root && owner.targets.contains(&key.app_id)) {
        owner.attempts.insert(key.app_id, Attempt::Ready);
    }
}

fn run(pool: &Mutex<Service>, id: u64, inner: Arc<Inner>) {
    let _ = inner.wake.bind_thread(std::thread::current());
    while !inner.closed.load(Ordering::Acquire) {
        let observed = inner.wake.observed();
        let deadline = step(pool, id, Instant::now(), || {
            std::thread::Builder::new().name("steam-artwork".into()).spawn(worker)
                .map(|_| ()).map_err(|error| error.to_string())
        });
        if inner.closed.load(Ordering::Acquire) { break; }
        inner.wake.wait_scheduled(observed, deadline.map(|at| at.saturating_duration_since(Instant::now())));
    }
}

#[cfg(test)]
mod tests;
