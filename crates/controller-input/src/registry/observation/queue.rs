use super::*;
use osg_streams::Publisher;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Condvar, Mutex, OnceLock,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(super) const MAX_PENDING: usize = 128;
const MAX_PENDING_BYTES: usize = 1024 * 1024;
// Recovery has one reserved typed snapshot per admitted/pending source, never an
// unbounded secondary queue. Source descriptors and native samples are bounded.
const MAX_SOURCE_BYTES: usize = 256 * 1024;
const RETIRED: usize = 1 << (usize::BITS - 1);
const READY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(super) struct Update {
    pub id: String,
    pub session: String,
    pub source: Option<Arc<Captured>>,
    pub reason: String,
    pub epoch: u64,
    pub fence: bool,
    pub idle_baseline: bool,
    pub bytes: usize,
    pub failure: Option<String>,
}
impl Update {
    pub fn new(
        id: &str,
        session: &str,
        source: Option<&Source>,
        reason: &str,
        epoch: u64,
        fence: bool,
        slot: &Slot,
    ) -> Self {
        let bytes = source.map_or(0, source_bytes) + id.len() + session.len() + reason.len() + 256;
        Self {
            id: id.into(),
            session: session.into(),
            source: source.map(|source| Arc::new(slot.capture(source))),
            reason: reason.into(),
            epoch,
            fence,
            idle_baseline: false,
            bytes,
            failure: None,
        }
    }
}
pub(super) struct Captured {
    state: Source,
    pub controls: Arc<Vec<Control>>,
}
impl std::ops::Deref for Captured {
    type Target = Source;
    fn deref(&self) -> &Source {
        &self.state
    }
}
impl Captured {
    pub fn ready(&self) -> bool {
        self.online
            && self.controls.iter().any(|c| c.kind != Kind::Diagnostic)
            && self
                .controls
                .iter()
                .filter(|c| c.kind != Kind::Diagnostic)
                .all(|c| self.values.contains_key(&c.id))
    }
    pub fn summary(&self) -> Value {
        let mut summary = self.state.summary();
        summary["controlCount"] = json!(self.controls.len());
        summary["baselineReady"] = json!(self.ready());
        summary
    }
}
fn source_bytes(s: &Source) -> usize {
    let saved = &s.saved;
    let i = &saved.identity;
    // Include collection/tree allocation overhead and each owned string buffer.
    std::mem::size_of::<Source>()
        + saved.id.len()
        + saved.name.len()
        + saved.role.len()
        + i.backend.len()
        + i.collection.len()
        + i.fingerprint.len()
        + i.path.len()
        + i.serial.as_ref().map_or(0, String::len)
        + s.controls
            .iter()
            .map(|c| std::mem::size_of::<Control>() + c.id.len())
            .sum::<usize>()
        + s.values.keys().map(|key| key.len() + 128).sum::<usize>()
        + s.diagnostics
            .iter()
            .map(|text| text.len() + 32)
            .sum::<usize>()
}
#[derive(Default)]
struct Progress {
    epoch: u64,
    error: Option<String>,
}
pub(super) struct Slot {
    pub token: u64,
    pub id: String,
    pub broker: Broker,
    pins: AtomicUsize,
    failed: AtomicBool,
    pub publisher: OnceLock<Publisher>,
    idle_baseline: AtomicBool,
    progress: Mutex<Progress>,
    ready: Condvar,
    descriptor: Mutex<Option<(u64, String, Arc<Vec<Control>>)>>,
}
impl Slot {
    fn capture(&self, source: &Source) -> Captured {
        let mut descriptor = self.descriptor.lock().unwrap_or_else(|p| p.into_inner());
        let fingerprint = &source.saved.identity.fingerprint;
        if !descriptor.as_ref().is_some_and(|(generation, hash, _)| {
            *generation == source.generation && hash == fingerprint
        }) {
            *descriptor = Some((
                source.generation,
                fingerprint.clone(),
                Arc::new(source.controls.clone()),
            ));
        }
        Captured {
            controls: descriptor.as_ref().unwrap().2.clone(),
            state: Source {
                saved: source.saved.clone(),
                online: source.online,
                generation: source.generation,
                sequence: source.sequence,
                captured_us: source.captured_us,
                cursor: source.cursor,
                controls: vec![],
                values: source.values.clone(),
                diagnostics: source.diagnostics.clone(),
            },
        }
    }
    fn pin(&self) -> bool {
        self.pins
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pins| {
                (pins < RETIRED - 1).then_some(pins + 1)
            })
            .is_ok()
    }
    pub fn unpin(&self) {
        self.pins.fetch_sub(1, Ordering::AcqRel);
    }
    pub fn pinned(&self) -> bool {
        self.pins.load(Ordering::Acquire) & !RETIRED != 0
    }
    pub fn begin_retire(&self) -> bool {
        self.pins
            .compare_exchange(0, RETIRED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub fn cancel_retire(&self) {
        self.pins.store(0, Ordering::Release);
    }
    pub fn complete(&self, epoch: u64, result: Result<(), String>) {
        let mut progress = self.progress.lock().unwrap_or_else(|p| p.into_inner());
        progress.epoch = progress.epoch.max(epoch);
        if let Err(error) = result {
            self.failed.store(true, Ordering::Release);
            progress.error = Some(error);
        }
        self.ready.notify_all();
    }
}
pub(super) struct Work {
    pub slot: Arc<Slot>,
    pub update: Update,
    pub resync: bool,
}
struct State {
    slots: BTreeMap<String, Arc<Slot>>,
    pending: VecDeque<Work>,
    recovery: BTreeMap<u64, Work>,
    recovery_order: VecDeque<u64>,
    recovery_turn: bool,
    pending_bytes: usize,
    next: u64,
    stop: bool,
}
pub(super) struct Shared {
    state: Mutex<State>,
    pub wake: Condvar,
}
impl Shared {
    pub fn prepare(
        &self,
        broker: &Broker,
        id: &str,
        session: &str,
        source: &Source,
        epoch: u64,
    ) -> Result<Arc<Slot>, String> {
        if source_bytes(source) + 512 > MAX_SOURCE_BYTES {
            return Err("Controller observation snapshot limit reached.".into());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.stop {
            return Err("Controller observation worker stopped.".into());
        }
        let existing = state.slots.get(id).cloned();
        let slot = if let Some(slot) = existing {
            slot
        } else {
            if state.slots.len() >= MAX_BINDINGS * 2 {
                return Err("Controller pending observation limit reached.".into());
            }
            state.next = state
                .next
                .checked_add(1)
                .ok_or("Controller observation identity exhausted.")?;
            let slot = Arc::new(Slot {
                token: state.next,
                id: id.into(),
                broker: broker.clone(),
                pins: AtomicUsize::new(0),
                failed: AtomicBool::new(false),
                publisher: OnceLock::new(),
                idle_baseline: AtomicBool::new(false),
                progress: Mutex::new(Progress::default()),
                ready: Condvar::new(),
                descriptor: Mutex::new(None),
            });
            state.slots.insert(id.into(), slot.clone());
            slot
        };
        if slot.failed.load(Ordering::Acquire) {
            return Err("Controller observation worker unavailable.".into());
        }
        // Never replace a registration while its idle retirement is unresolved:
        // retirement can fail if an existing reader is still active.
        if !slot.pin() {
            return Err("Controller observation retirement in progress; retry.".into());
        }
        let mut update = Update::new(id, session, Some(source), "metadata", epoch, true, &slot);
        update.idle_baseline = slot.idle_baseline.swap(false, Ordering::Relaxed);
        state.enqueue(slot.clone(), update);
        self.wake.notify_one();
        Ok(slot)
    }
    pub fn capture(&self, id: &str, reason: &str, update: impl FnOnce(&Slot) -> Update) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.stop {
            return;
        }
        if let Some(slot) = state.slots.get(id) {
            if slot.failed.load(Ordering::Acquire) {
                return;
            }
            // Preparation pins the source until its subscription has joined
            // the journal. The pin and lock-free reader check cover that handoff.
            if reason == "change"
                && !slot.pinned()
                && slot
                    .publisher
                    .get()
                    .is_some_and(|publisher| !publisher.has_reader_demand())
            {
                // Both this store and the preparation fence hold the queue lock.
                slot.idle_baseline.store(true, Ordering::Relaxed);
                return;
            }
            let slot = slot.clone();
            let mut update = update(&slot);
            if update.bytes > MAX_SOURCE_BYTES {
                let error = "Controller observation snapshot limit reached.".to_string();
                slot.complete(update.epoch, Err(error.clone()));
                update.source = None;
                update.failure = Some(error);
                update.bytes = 512;
                state.overflow(slot, update);
            } else {
                state.enqueue(slot, update);
            }
            self.wake.notify_one();
        }
    }
    pub fn ids(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .slots
            .keys()
            .cloned()
            .collect()
    }
    #[cfg(test)]
    pub fn idle_baseline_pending(&self, id: &str) -> bool {
        self.state.lock().unwrap().slots[id]
            .idle_baseline
            .load(Ordering::Relaxed)
    }
    #[cfg(test)]
    pub fn wait_processed(&self, id: &str, epoch: u64) {
        let slot = self.state.lock().unwrap().slots[id].clone();
        self.wait(&slot, epoch).unwrap();
    }
    pub fn wait(&self, slot: &Slot, epoch: u64) -> Result<(), String> {
        let deadline = Instant::now() + READY_TIMEOUT;
        let mut progress = slot.progress.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if let Some(error) = &progress.error {
                return Err(error.clone());
            }
            if progress.epoch >= epoch {
                return Ok(());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Controller observation preparation timed out.".into());
            }
            progress = slot
                .ready
                .wait_timeout(progress, remaining)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
    pub fn next(&self) -> Option<Work> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if state.stop {
                return None;
            }
            if !state.recovery.is_empty() && (state.recovery_turn || state.pending.is_empty()) {
                let token = state.recovery_order.pop_front().unwrap();
                state.recovery_turn = false;
                return state.recovery.remove(&token);
            }
            if let Some(work) = state.pending.pop_front() {
                state.pending_bytes -= work.update.bytes;
                state.recovery_turn = true;
                return Some(work);
            }
            state = self.wake.wait(state).unwrap_or_else(|p| p.into_inner());
        }
    }
    pub fn remove(&self, slot: &Slot) {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(slot);
    }
    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.stop = true;
        for slot in state.slots.values() {
            slot.complete(0, Err("Controller observation worker stopped.".into()));
        }
        state.pending.clear();
        state.recovery.clear();
        state.recovery_order.clear();
        state.pending_bytes = 0;
        self.wake.notify_all();
    }
}
impl State {
    fn remove(&mut self, slot: &Slot) {
        if self
            .slots
            .get(&slot.id)
            .is_some_and(|current| current.token == slot.token)
        {
            self.slots.remove(&slot.id);
        }
        self.pending.retain(|work| work.slot.token != slot.token);
        self.pending_bytes = self.pending.iter().map(|work| work.update.bytes).sum();
        self.recovery.remove(&slot.token);
        self.recovery_order.retain(|token| *token != slot.token);
    }
    fn enqueue(&mut self, slot: Arc<Slot>, update: Update) {
        if self.recovery.contains_key(&slot.token)
            || self.pending.len() >= MAX_PENDING
            || self.pending_bytes + update.bytes > MAX_PENDING_BYTES
        {
            self.overflow(slot, update);
        } else {
            self.pending_bytes += update.bytes;
            self.pending.push_back(Work {
                slot,
                update,
                resync: false,
            });
        }
    }
    fn overflow(&mut self, slot: Arc<Slot>, update: Update) {
        self.pending.retain(|work| work.slot.token != slot.token);
        self.pending_bytes = self.pending.iter().map(|work| work.update.bytes).sum();
        if !self.recovery.contains_key(&slot.token) {
            self.recovery_order.push_back(slot.token);
        }
        self.recovery.insert(
            slot.token,
            Work {
                slot,
                update,
                resync: true,
            },
        );
    }
}
pub(super) struct Actor {
    pub shared: Arc<Shared>,
    join: Option<JoinHandle<()>>,
}
impl Actor {
    pub fn start() -> Result<Self, String> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                slots: BTreeMap::new(),
                pending: VecDeque::new(),
                recovery: BTreeMap::new(),
                recovery_order: VecDeque::new(),
                recovery_turn: true,
                pending_bytes: 0,
                next: 0,
                stop: false,
            }),
            wake: Condvar::new(),
        });
        let worker_shared = shared.clone();
        let join = std::thread::Builder::new()
            .name("controller-preview".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    super::worker::run(&worker_shared)
                }));
                if result.is_err() {
                    worker_shared.stop();
                }
            })
            .map_err(|error| format!("Cannot start controller observation worker: {error}"))?;
        Ok(Self {
            shared,
            join: Some(join),
        })
    }
    pub fn shutdown(mut self) {
        self.shared.stop();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
impl Drop for Actor {
    fn drop(&mut self) {
        self.shared.stop();
    } // Never join while Registry is locked.
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
