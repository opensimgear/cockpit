use super::*;
use std::{collections::BTreeMap, sync::{atomic::{AtomicUsize, Ordering}, Condvar, Mutex}, thread::JoinHandle,
    time::{Duration, Instant}};

const RETIRING: usize = 1 << (usize::BITS - 1);
const MAX_PREPARATIONS: usize = 64;
const READY_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct Update {
    pub view: Arc<View>,
    pub epoch: u64,
    pub captured_us: u64,
    pub resync: bool,
}
pub(super) struct Batch {
    pub view: Option<Arc<View>>,
    pub rows: Vec<Summary>,
    pub epoch: u64,
    pub captured_us: u64,
    pub resync: bool,
}
pub(super) enum Work { Publish(Batch), Retire }
#[derive(Default)]
struct State {
    pending: Option<Update>,
    rows: BTreeMap<String, Summary>,
    epoch: u64,
    captured_us: u64,
    resync: bool,
    maintenance: bool,
    processed: u64,
    closed: bool,
    error: Option<String>,
}
#[derive(Default)]
pub(super) struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    pins: AtomicUsize,
}
impl Shared {
    pub fn prepare(&self, update: Update) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed { return Err(state.error.clone().unwrap_or_else(|| "Controller inventory worker stopped.".into())); }
        self.pins.fetch_update(Ordering::AcqRel, Ordering::Acquire, |pins|
            (pins < MAX_PREPARATIONS).then_some(pins + 1))
            .map_err(|_| "Controller inventory preparation limit or retirement in progress.".to_string())?;
        Self::enqueue(&mut state, update);
        self.wake.notify_all();
        Ok(())
    }
    pub fn capture(&self, update: Update) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed { return; }
        Self::enqueue(&mut state, update);
        self.wake.notify_all();
    }
    fn enqueue(state: &mut State, mut update: Update) {
        // One bounded latest snapshot replaces pending metadata. Dropped states
        // always force explicit loss/resynchronization for existing readers.
        if let Some(previous) = state.pending.take() {
            update.resync |= previous.resync || previous.view != update.view;
        }
        state.resync |= update.resync || !state.rows.is_empty();
        state.rows.clear();
        state.epoch = update.epoch;state.captured_us = update.captured_us;
        state.pending = Some(update);
    }
    pub fn capture_row(&self, row: Summary, epoch: u64, captured_us: u64) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed { return; }
        if state.rows.len() == super::super::MAX_SOURCES && !state.rows.contains_key(&row.id) {
            state.closed = true;state.error = Some("Controller inventory row handoff limit reached.".into());
            state.pending = None;state.rows.clear();self.wake.notify_all();return;
        }
        let overwritten = state.pending.is_some() || state.rows.get(&row.id).is_some_and(|previous| previous != &row);
        state.resync |= overwritten;
        state.rows.insert(row.id.clone(), row);
        state.epoch = epoch;state.captured_us = captured_us;
        self.wake.notify_all();
    }
    pub fn release(&self) {
        self.pins.fetch_sub(1, Ordering::AcqRel);
        self.state.lock().unwrap_or_else(|p| p.into_inner()).maintenance = true;
        self.wake.notify_all();
    }
    pub fn pinned(&self) -> bool { self.pins.load(Ordering::Acquire) & !RETIRING != 0 }
    pub fn begin_retire(&self) -> bool {
        self.pins.compare_exchange(0, RETIRING, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }
    pub fn finish_retire(&self) { self.pins.store(0, Ordering::Release); }
    pub fn next(&self) -> Option<Work> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if state.closed { return None; }
            if state.pending.is_some() || !state.rows.is_empty() {
                let view = state.pending.take().map(|update| update.view);
                let rows = std::mem::take(&mut state.rows).into_values().collect();
                return Some(Work::Publish(Batch { view, rows, epoch: state.epoch, captured_us: state.captured_us,
                    resync: std::mem::take(&mut state.resync) }));
            }
            if std::mem::take(&mut state.maintenance) { return Some(Work::Retire); }
            state = self.wake.wait(state).unwrap_or_else(|p| p.into_inner());
        }
    }
    pub fn complete(&self, epoch: u64) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.processed = state.processed.max(epoch);
        self.wake.notify_all();
    }
    pub fn wait(&self, epoch: u64) -> Result<(), String> {
        let deadline = Instant::now() + READY_TIMEOUT;
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if state.closed { return Err(state.error.clone().unwrap_or_else(|| "Controller inventory worker stopped.".into())); }
            if state.processed >= epoch { return Ok(()); }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { return Err("Controller inventory preparation timed out.".into()); }
            state = self.wake.wait_timeout(state, remaining).unwrap_or_else(|p| p.into_inner()).0;
        }
    }
    pub fn fail(&self, error: String) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.closed = true;
        state.error = Some(error);
        state.pending = None;
        state.rows.clear();
        self.wake.notify_all();
    }
    pub fn stop(&self) { self.fail("Controller inventory worker stopped.".into()); }
}
pub(super) struct Actor { pub shared: Arc<Shared>, join: Option<JoinHandle<()>> }
impl Actor {
    pub fn start(broker: Broker) -> Result<Self, String> {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        let join = std::thread::Builder::new().name("controller-inventory".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::worker::run(&worker, broker)));
            if result.is_err() { worker.fail("Controller inventory worker panicked.".into()); }
        }).map_err(|error| format!("Cannot start controller inventory worker: {error}"))?;
        Ok(Self { shared, join: Some(join) })
    }
    pub fn shutdown(mut self) {
        self.shared.stop();
        if let Some(join) = self.join.take() { let _ = join.join(); }
    }
}
impl Drop for Actor {
    fn drop(&mut self) { self.shared.stop(); } // Never join under the native registry lock.
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latest_handoff_is_bounded_and_marks_overwritten_state_as_loss() {
        let shared = Shared::default();
        let mut registry = Registry::default();
        let view = Arc::new(View::capture(&registry).unwrap());
        shared.prepare(Update { view: view.clone(), epoch: 1, captured_us: 0, resync: false }).unwrap();
        registry.backend_status = "ready".into();
        for epoch in 2..1000 {
            shared.capture(Update { view: Arc::new(View::capture(&registry).unwrap()), epoch, captured_us: epoch, resync: false });
        }
        let Some(Work::Publish(update)) = shared.next() else { panic!("missing latest state"); };
        assert_eq!(update.epoch, 999);assert!(update.resync);
        assert!(shared.state.lock().unwrap().pending.is_none());
        shared.complete(update.epoch);shared.wait(1).unwrap();shared.release();
        shared.stop();assert!(shared.next().is_none());assert!(shared.wait(1000).is_err());
    }
    #[test]
    fn retirement_excludes_preparation_and_never_overwrites_a_pin() {
        let shared = Shared::default();
        assert!(shared.begin_retire());
        let view = Arc::new(View::capture(&Registry::default()).unwrap());
        assert!(shared.prepare(Update { view: view.clone(), epoch: 1, captured_us: 0, resync: false }).is_err());
        shared.finish_retire();
        shared.prepare(Update { view, epoch: 2, captured_us: 0, resync: false }).unwrap();
        assert!(!shared.begin_retire());assert!(shared.pinned());
        shared.release();assert!(shared.begin_retire());shared.finish_retire();
    }
    #[test]
    fn full_replacements_fence_older_rows_and_later_readiness_merges_without_losing_metadata() {
        let shared = Shared::default();let mut registry = Registry::default();
        let id = registry.connect(crate::tests::fixture("row-ordering", None)).unwrap();
        shared.prepare(Update { view: Arc::new(View::capture(&registry).unwrap()), epoch: 1, captured_us: 1, resync: false }).unwrap();
        super::super::tests::baseline(&mut registry, &id);
        shared.capture_row(Summary::capture(&registry.sources[&id]), 2, 2);
        assert_eq!(shared.state.lock().unwrap().rows.len(), 1);
        registry.disconnect(&id);
        shared.capture(Update { view: Arc::new(View::capture(&registry).unwrap()), epoch: 3, captured_us: 3, resync: false });
        assert!(shared.state.lock().unwrap().rows.is_empty());
        let Some(Work::Publish(batch)) = shared.next() else { panic!("missing replacement"); };
        assert_eq!(batch.epoch, 3);assert!(batch.resync);assert!(batch.rows.is_empty());
        let payload = serde_json::to_value(batch.view.unwrap().as_ref()).unwrap();
        assert_eq!(payload["sources"][0]["online"], false);
        registry.connect(crate::tests::fixture("row-ordering", None)).unwrap();
        let mut current = View::capture(&registry).unwrap();
        super::super::tests::baseline(&mut registry, &id);
        shared.capture_row(Summary::capture(&registry.sources[&id]), 4, 4);
        let Some(Work::Publish(batch)) = shared.next() else { panic!("missing readiness row"); };
        assert_eq!(batch.epoch, 4);assert!(batch.view.is_none());assert_eq!(batch.rows.len(), 1);
        current.apply_row(batch.rows.into_iter().next().unwrap()).unwrap();
        let payload = serde_json::to_value(current).unwrap();assert_eq!(payload["sources"][0]["baselineReady"], true);
        shared.release();
    }
}
