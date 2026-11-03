//! One connection's bounded native input capture. No device queries or UI codec.
mod model;
pub(crate) use model::{Batch, Edge, Snapshot};
use super::UsbRouteToken;
use crate::module_console::{actor::Input, push};
use osg_streams::{NativeLimits, NativeStream, NativeSubscription, NativeWake, NativeWatch};
use serde_json::Value;
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const FRESHNESS: Duration = Duration::from_millis(400);
const LIMITS: NativeLimits = NativeLimits {
    max_subscribers: 8, max_snapshot_bytes: 32 * 1024, max_frame_bytes: 32 * 1024,
    max_pending_frames: 64, max_pending_bytes: 512 * 1024,
};
type Journal = NativeStream<Snapshot, Batch>;
struct State {
    decoder: push::Decoder,
    snapshot: Snapshot,
    generation: u64,
    pending: Vec<Edge>,
    pages: usize,
    sample: Option<u32>,
    last_advance_at: Option<Instant>,
    last_received_at: Option<Instant>,
    last_edge_sample: Option<u32>,
    deadline: Option<Instant>,
    terminal: bool,
    closed: bool,
}
struct Shared {
    state: Mutex<State>,
    journal: Journal,
    wake: Arc<NativeWake>,
    origin: Instant,
    clock_id: String,
}
pub(crate) struct Capture {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn micros(&self, at: Instant) -> u64 {
        at.saturating_duration_since(self.origin).as_micros().min(u64::MAX as u128) as u64
    }
    fn captured_us(&self, snapshot: &Snapshot, unavailable_at: Instant) -> u64 {
        // Repeated sample timestamps and later pages do not create evidence.
        // Their individual RX clocks remain available on the typed edges.
        let at = snapshot.received_at.filter(|_| snapshot.ready).unwrap_or(unavailable_at);
        self.micros(at)
    }
    fn replacement(&self, state: &mut State, at: Instant) {
        let Some(generation) = state.generation.checked_add(1) else {
            state.terminal = true;
            state.deadline = None;
            state.pending.clear();
            state.pages = 0;
            state.snapshot.ready = false;
            state.snapshot.state = None;
            state.snapshot.error = Some("USB input generation exhausted.".into());
            self.journal.close();
            return;
        };
        state.generation = generation;
        if self.journal.replace_generation(generation, state.snapshot.clone(),
            self.captured_us(&state.snapshot, at), &self.clock_id).is_err()
        { self.publication_failed(state); }
    }
    fn publication_failed(&self, state: &mut State) {
        state.terminal = true;
        state.deadline = None;
        state.pending.clear();
        state.pages = 0;
        state.snapshot.ready = false;
        state.snapshot.state = None;
        state.snapshot.error = Some("USB input publication failed.".into());
        self.journal.close();
    }
    fn invalidate(&self, state: &mut State, error: &str, at: Instant) {
        if state.terminal { return; }
        state.terminal = true;
        state.deadline = None;
        state.pending.clear();
        state.pages = 0;
        state.snapshot.ready = false;
        state.snapshot.state = None;
        state.snapshot.error = Some(error.chars().take(256).collect());
        self.replacement(state, at);
    }
    fn expire(&self, state: &mut State, now: Instant) {
        if state.deadline.is_some_and(|deadline| now > deadline) {
            self.invalidate(state, "USB input source stopped advancing; reconnect the module.", now);
        }
    }
    fn close(&self) {
        let mut state = self.lock();
        if state.closed { return; }
        self.invalidate(&mut state, "USB input capture was closed.", Instant::now());
        state.closed = true;
        self.journal.close();
        self.wake.notify();
    }
    fn accept(&self, input: Input, now: Instant) {
        let mut state = self.lock();
        if state.closed { return; }
        if let Input::Lost(error) = &input {
            self.invalidate(&mut state, error, now);
            state.closed = true;
            self.journal.close();
            self.wake.notify();
            return;
        }
        if state.terminal { return; }
        let Input::Line { line, received_at } = input else { unreachable!() };
        self.expire(&mut state, received_at);
        if state.terminal { return; }
        let result = self.frame(&mut state, &line, received_at, now);
        if let Err(error) = result { self.invalidate(&mut state, &error, received_at); }
        self.wake.notify();
    }
    fn frame(&self, state: &mut State, line: &str, at: Instant, now: Instant) -> Result<(), String> {
        if at < self.origin || state.last_received_at.is_some_and(|prior| at < prior) {
            return Err("USB input receive clock moved backwards.".into());
        }
        state.last_received_at = Some(at);
        let mut frame = state.decoder.decode(line, at)?;
        if frame.stream != state.snapshot.route.configuration_generation {
            return Err("USB input configuration changed; establish a freshly verified route.".into());
        }
        let advance = match state.sample {
            None => true,
            Some(previous) => {
                let delta = frame.sampled_at_ms.wrapping_sub(previous);
                if delta > i32::MAX as u32 { return Err("USB input sample clock moved backwards.".into()); }
                delta > 0
            }
        };
        if advance {
            state.sample = Some(frame.sampled_at_ms);
            state.last_advance_at = Some(at);
            state.deadline = at.checked_add(FRESHNESS);
        }
        self.expire(state, now);
        if state.terminal { return Ok(()); }
        if frame.exhausted {
            return Err("USB input source exhausted; reconnect the module.".into());
        }
        if matches!(frame.kind, push::Kind::Baseline | push::Kind::Loss) {
            state.pending.clear();
            state.pages = 0;
            model::metadata(&mut frame);
            state.snapshot.state = Some(frame.state);
            state.snapshot.received_at = state.last_advance_at;
            state.snapshot.stream = frame.stream;
            state.snapshot.ready = true;
            state.snapshot.error = None;
            state.last_edge_sample = Some(frame.sampled_at_ms);
            self.replacement(state, at);
            return Ok(());
        }
        let events = frame.state["events"].as_array().ok_or("USB input page omitted events.")?;
        if state.pages >= 4 || state.pending.len() + events.len() > 64 {
            return Err("USB input atomic batch overflowed; reconnect the module.".into());
        }
        state.pages += 1;
        for event in events {
            let captured_at_ms = event["capturedAtMs"].as_u64().unwrap() as u32;
            let age = frame.sampled_at_ms.wrapping_sub(captured_at_ms);
            if age > i32::MAX as u32 || state.last_edge_sample.is_some_and(|previous| {
                captured_at_ms.wrapping_sub(previous) > i32::MAX as u32
            }) { return Err("USB input edge capture clock is future or regressed.".into()); }
            state.last_edge_sample = Some(captured_at_ms);
            state.pending.push(Edge {
                sequence: event["sequence"].as_u64().unwrap() as u32,
                control: event["control"].as_u64().unwrap() as u8,
                pressed: event["pressed"].as_bool().unwrap(),
                captured_at_ms,
                received_at: at,
            });
        }
        if frame.state["more"] == true { return Ok(()); }
        model::metadata(&mut frame);
        state.snapshot.state = Some(frame.state.clone());
        state.snapshot.received_at = state.last_advance_at;
        state.snapshot.ready = true;
        state.snapshot.error = None;
        let events = std::mem::take(&mut state.pending);
        state.pages = 0;
        let witnessed_at = state.last_advance_at.ok_or("USB input batch has no sample witness.")?;
        let batch = Batch { events, witnessed_at, latest: frame };
        if self.journal.publish(state.generation, batch, state.snapshot.clone(),
            self.captured_us(&state.snapshot, at), &self.clock_id).is_err()
        { self.publication_failed(state); }
        Ok(())
    }
}

impl Capture {
    pub(crate) fn new(route: UsbRouteToken) -> Result<Arc<Self>, String> {
        Self::create(route, true)
    }
    fn create(route: UsbRouteToken, start_worker: bool) -> Result<Arc<Self>, String> {
        let origin = Instant::now(); // Created before the receive callback exists.
        if route.boot == 0 || route.configuration_generation == 0 || route.connection_generation == 0
            || route.module_id.is_empty() || route.module_id.len() > 128
            || route.serial.is_empty() || route.serial.len() > 128
        { return Err("Invalid USB input route.".into()); }
        let snapshot = Snapshot { stream: route.configuration_generation, route: route.clone(),
            state: None, received_at: None, ready: false, error: None };
        let clock_id = format!("usb-input:{}", uuid::Uuid::new_v4());
        let journal = Journal::new(1, snapshot.clone(), 0, &clock_id, LIMITS)
            .map_err(|error| format!("Cannot prepare USB input capture: {error:?}"))?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State { decoder: push::Decoder::new(route.boot), snapshot,
                generation: 1, pending: Vec::new(), pages: 0, sample: None,
                last_advance_at: None, last_received_at: None, last_edge_sample: None,
                deadline: origin.checked_add(FRESHNESS),
                terminal: false, closed: false }),
            journal, origin, clock_id, wake: Arc::default(),
        });
        let capture = Arc::new(Self { shared: shared.clone(), worker: Mutex::new(None) });
        if start_worker {
            let weak = Arc::downgrade(&shared);
            let wake = shared.wake.clone();
            let worker = thread::Builder::new().name("usb-input-deadline".into()).spawn(move || {
                let _ = wake.bind_thread(thread::current());
                let outcome = catch_unwind(AssertUnwindSafe(|| loop {
                    let observed = wake.observed();
                    let Some(shared) = weak.upgrade() else { return; };
                    let mut state = shared.lock();
                    shared.expire(&mut state, Instant::now());
                    if state.closed || state.terminal { return; }
                    let timeout = state.deadline.map(|at| at.saturating_duration_since(Instant::now())
                        .saturating_add(Duration::from_nanos(1)));
                    drop(state);
                    drop(shared);
                    wake.wait_scheduled(observed, timeout);
                }));
                if outcome.is_err() {
                    if let Some(shared) = weak.upgrade() {
                        let mut state = shared.lock();
                        shared.invalidate(&mut state, "USB input deadline worker panicked.", Instant::now());
                        shared.journal.close();
                    }
                }
            }).map_err(|error| format!("Cannot start USB input deadline: {error}"))?;
            *capture.worker.lock().unwrap_or_else(|p| p.into_inner()) = Some(worker);
        }
        Ok(capture)
    }
    pub(crate) fn accept(&self, input: Input) { self.shared.accept(input, Instant::now()); }
    pub(crate) fn subscribe(&self) -> Result<NativeSubscription<Snapshot, Batch>, String> {
        let state = self.shared.lock();
        if state.deadline.is_some_and(|at| Instant::now() > at) {
            self.shared.wake.notify();
            return Err("USB input freshness expired; wait for native withdrawal.".into());
        }
        self.shared.journal.subscribe().map_err(|error| format!("{error:?}"))
    }
    pub(crate) fn watch(&self, wake: Arc<NativeWake>) -> Result<NativeWatch<Snapshot, Batch>, String> {
        self.shared.journal.watch(wake).map_err(|error| format!("{error:?}"))
    }
    pub(crate) fn snapshot(&self) -> Snapshot {
        let state = self.shared.lock();
        let mut snapshot = state.snapshot.clone();
        // A delayed deadline worker cannot make an observer see stale values.
        // This passive projection neither commits nor renews source evidence.
        if state.deadline.is_some_and(|at| Instant::now() > at) {
            snapshot.ready = false;
            snapshot.state = None;
            snapshot.error = Some("USB input freshness expired.".into());
        }
        snapshot
    }
    pub(crate) fn close(&self) { self.shared.close(); }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.close();
        if let Some(worker) = self.worker.get_mut().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests;
