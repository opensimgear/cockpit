//! A single ordered owner of desktop IPC. Controller calls only touch memory.
use osg_desktop_output::{protocol::{Ack, Command, Request}, Coordinator, Scope, Snapshot};
use osg_mapping::{desktop::Delivery, engine::{Counter, RunToken, typed::Token}};
use std::{collections::BTreeMap, sync::{Arc, Condvar, Mutex, MutexGuard, mpsc}, time::Instant};

#[path = "output/execute.rs"]
mod execute;
#[path = "output/worker.rs"]
mod worker;
use worker::Owner;
#[cfg(test)]
pub(in crate::desktop) use execute::{effective_source_now, same_pointer_batch};

const DELIVERY_LIMIT: usize = 4096;
const INPUT_LIMIT: usize = 1_048_576;

struct AdmissionSize(usize);
impl std::io::Write for AdmissionSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > INPUT_LIMIT {
            return Err(std::io::Error::other("Desktop output batch exceeds 1 MiB."));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
type Wake = Arc<dyn Fn() + Send + Sync>;
type Reply = mpsc::SyncSender<Result<(), String>>;

pub(super) struct Completion {
    pub audit_shadow: u64,
    pub outcomes: Vec<(Delivery, Ack)>,
    pub dropped: Vec<(Delivery, u64)>,
    pub error: Option<String>,
}
struct Job {
    deliveries: Vec<Delivery>,
    source_now: u64,
    admitted: Instant,
    epoch: u64,
    audit_shadow: u64,
}
enum Setup {
    Bind(RunToken, Scope, Reply),
    Activate(RunToken, Scope, Reply),
}
struct State {
    snapshot: Snapshot,
    current: BTreeMap<String, Token>,
    allow_empty: bool,
    epoch: u64,
    executing_epoch: u64,
    fault: Option<String>,
    closed: bool,
    shutdown: bool,
    busy: bool,
    busy_shadow: u64,
    control_active: bool,
    completion: Option<Completion>,
    job: Option<Job>,
    pause: Option<RunToken>,
    stop: Option<Reply>,
    setup: Option<Setup>,
    authorization: Option<(RunToken, Instant, u64)>,
    authorization_id: u64,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    wake: Wake,
}
impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
    fn notify(&self) {
        self.changed.notify_one();
        // A notification callback cannot be allowed to terminate the IPC owner.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.wake)()));
    }
    fn publish(&self, snapshot: Snapshot) {
        self.lock().snapshot = snapshot;
        self.notify();
    }
    fn current(state: &State, delivery: &Delivery) -> bool {
        (state.allow_empty && state.current.is_empty())
            || state.current.get(&delivery.action_id) == Some(&delivery.producer)
    }
    fn permitted(&self, request: &Request) -> bool {
        let state = self.lock();
        let valid = |delivery: &Delivery| {
            state.fault.is_none() && !state.closed && !state.shutdown
                && state.epoch == state.executing_epoch && Self::current(&state, delivery)
        };
        match &request.command {
            Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. } => valid(delivery),
            Command::DeliverPointerBatch { deliveries } => deliveries.iter().all(valid),
            _ => true,
        }
    }
}
enum Mode {
    Threaded,
    #[cfg(test)]
    Inline(Mutex<Owner>),
}
pub(super) struct Output {
    shared: Arc<Shared>,
    mode: Mode,
}
impl Output {
    fn prepare(mut coordinator: Coordinator, wake: Wake, allow_empty: bool, tap_durations: BTreeMap<String, Counter>) -> (Arc<Shared>, Owner) {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                snapshot: coordinator.snapshot(), current: BTreeMap::new(), allow_empty,
                epoch: 0, executing_epoch: 0, fault: None, closed: false, shutdown: false,
                busy: false, busy_shadow: 0, control_active: false, completion: None, job: None,
                pause: None, stop: None, setup: None, authorization: None, authorization_id: 0,
            }),
            changed: Condvar::new(), wake,
        });
        let gate = shared.clone();
        coordinator.set_dispatch_gate(Arc::new(move |request| gate.permitted(request)));
        let owner = Owner::new(coordinator, shared.clone(), tap_durations);
        (shared, owner)
    }
    pub(super) fn threaded(coordinator: Coordinator, wake: Wake, tap_durations: BTreeMap<String, Counter>) -> Result<Self, String> {
        let (shared, mut owner) = Self::prepare(coordinator, wake, false, tap_durations);
        let monitor = shared.clone();
        std::thread::Builder::new().name("desktop-output".into()).spawn(move || {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.run())).is_err() {
                owner.fail("Desktop output owner panicked.".into());
            }
            // The coordinator, its process channel and journal are destroyed here.
            // Output::drop never joins this thread or performs native IPC.
            let mut state = monitor.lock();
            state.shutdown = true;
            state.control_active = false;
            state.stop.take();
            state.setup.take();
            drop(state);
            monitor.notify();
        }).map_err(|error| error.to_string())?;
        Ok(Self { shared, mode: Mode::Threaded })
    }
    #[cfg(test)]
    pub(super) fn inline(coordinator: Coordinator) -> Self {
        Self::inline_with_tap_durations(coordinator, BTreeMap::new())
    }
    #[cfg(test)]
    pub(super) fn inline_with_tap_durations(coordinator: Coordinator, tap_durations: BTreeMap<String, Counter>) -> Self {
        let (shared, owner) = Self::prepare(coordinator, Arc::new(|| {}), true, tap_durations);
        Self { shared, mode: Mode::Inline(Mutex::new(owner)) }
    }
    pub(super) fn snapshot(&self) -> Snapshot {
        let state = self.shared.lock();
        let mut snapshot = state.snapshot.clone();
        if let Some(error) = &state.fault {
            snapshot.error = Some(error.clone());
            snapshot.phase = "faulted".into();
        }
        snapshot
    }
    fn check(state: &State) -> Result<(), String> {
        if let Some(error) = &state.fault { return Err(error.clone()); }
        if state.shutdown { return Err("Desktop output owner exited.".into()); }
        if state.closed { return Err("Desktop output owner is stopped.".into()); }
        Ok(())
    }
    fn setup(&self, make: impl FnOnce(Reply) -> Setup) -> Result<(), String> {
        let (reply, answer) = mpsc::sync_channel(1);
        {
            let mut state = self.shared.lock();
            Self::check(&state)?;
            if state.setup.is_some() || state.control_active {
                return Err("Desktop output setup is already pending.".into());
            }
            state.setup = Some(make(reply));
        }
        self.drive();
        answer.recv().unwrap_or_else(|_| Err(self.exit_error()))
    }
    pub(super) fn bind_scope_before_activate(&self, run: &RunToken, scope: &Scope) -> Result<(), String> {
        self.setup(|reply| Setup::Bind(run.clone(), scope.clone(), reply))
    }
    pub(super) fn activate(&self, run: RunToken, scope: Scope) -> Result<(), String> {
        self.setup(|reply| Setup::Activate(run, scope, reply))
    }
    pub(super) fn renew(&self, run: RunToken) -> Result<(), String> {
        {
            let mut state = self.shared.lock();
            Self::check(&state)?;
            state.authorization_id = state.authorization_id.wrapping_add(1);
            state.authorization = Some((run, Instant::now(), state.authorization_id));
        }
        self.drive();
        Ok(())
    }
    pub(super) fn pause(&self, run: RunToken) -> Result<(), String> {
        {
            let mut state = self.shared.lock();
            Self::check(&state)?;
            state.epoch = state.epoch.wrapping_add(1);
            state.pause = Some(run);
            state.authorization = None;
        }
        self.drive();
        Ok(())
    }
    pub(super) fn stop_all(&self) -> Result<(), String> {
        let (reply, answer) = mpsc::sync_channel(1);
        {
            let mut state = self.shared.lock();
            if state.shutdown { return Err(self.exit_error_locked(&state)); }
            if state.closed && state.stop.is_none() && !state.control_active {
                return state.fault.clone().map_or(Ok(()), Err);
            }
            if state.stop.is_some() { return Err("Desktop output stop is already pending.".into()); }
            state.closed = true;
            state.epoch = state.epoch.wrapping_add(1);
            state.authorization = None;
            state.pause = None;
            state.stop = Some(reply);
        }
        self.drive();
        answer.recv().unwrap_or_else(|_| Err(self.exit_error()))
    }
    pub(super) fn busy(&self) -> bool { self.shared.lock().busy }
    pub(super) fn asynchronous(&self) -> bool { matches!(self.mode, Mode::Threaded) }
    pub(super) fn pending(&self) -> bool {
        let state = self.shared.lock();
        state.busy || state.control_active || state.pause.is_some() || state.stop.is_some()
            || state.setup.is_some() || state.authorization.is_some()
    }
    pub(super) fn reconcile(&self, current: &BTreeMap<String, Token>) {
        let mut state = self.shared.lock();
        if state.current != *current { state.current.clone_from(current); }
    }
    pub(super) fn current(&self, delivery: &Delivery) -> bool {
        Shared::current(&self.shared.lock(), delivery)
    }
    pub(super) fn dispatch(&self, deliveries: Vec<Delivery>, source_now: u64, audit_shadow: u64) -> Result<(), String> {
        let admitted = Instant::now();
        // Validate before serializing: the count bound also bounds serialization work.
        let bounds = if deliveries.len() > DELIVERY_LIMIT {
            Err("Desktop output batch exceeds 4096 deliveries.".into())
        } else {
            // Stop counting at the byte bound; never allocate an oversized
            // serialized copy on the profile input worker just to reject it.
            serde_json::to_writer(AdmissionSize(0), &deliveries).map_err(|error| error.to_string())
        };
        let mut state = self.shared.lock();
        Self::check(&state)?;
        let error = bounds.err().or_else(|| state.busy.then(|| "Desktop output already has an outstanding batch.".into()));
        if let Some(error) = error {
            state.fault = Some(error.clone());
            state.epoch = state.epoch.wrapping_add(1);
            drop(state);
            self.drive();
            return Err(error);
        }
        state.busy = true;
        state.busy_shadow = audit_shadow;
        state.job = Some(Job { deliveries, source_now, admitted, epoch: state.epoch, audit_shadow });
        drop(state);
        self.drive();
        Ok(())
    }
    pub(super) fn take_completion(&self) -> Option<Completion> {
        let mut state = self.shared.lock();
        let completion = state.completion.take()?;
        state.busy = false;
        Some(completion)
    }
    fn exit_error_locked(&self, state: &State) -> String {
        state.fault.clone().unwrap_or_else(|| "Desktop output owner exited.".into())
    }
    fn exit_error(&self) -> String { self.exit_error_locked(&self.shared.lock()) }
    fn drive(&self) {
        self.shared.changed.notify_one();
        #[cfg(test)]
        if let Mode::Inline(owner) = &self.mode {
            let mut owner = owner.lock().unwrap_or_else(|error| error.into_inner());
            while owner.step() {}
        }
        #[cfg(not(test))]
        let _ = &self.mode;
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.shutdown = true;
        state.epoch = state.epoch.wrapping_add(1);
        drop(state);
        self.shared.changed.notify_one();
    }
}

#[cfg(test)]
#[path = "output/tests.rs"]
mod tests;
