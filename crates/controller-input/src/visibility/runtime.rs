//! Worker-owned standalone visibility sessions, supervised without frontend polling.
use super::{
    journal::{self, Backend, Record, Store},
    policy::{Plan, Selection, State},
    session::{self, Session, Source},
    settings::{Intent, Preferences},
};
use osg_streams::NativeWake;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Condvar, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
mod capture;
mod handoff;
pub(crate) use capture::Capture;
use handoff::inspect_handoff;
pub trait Adapter: Backend + Send {
    fn executable(&mut self, path: &str) -> Result<String, String>;
    fn instance(&mut self, interface: &str) -> Result<String, String>;
    #[cfg(windows)]
    fn watch_policy(&mut self, _: Arc<NativeWake>) -> Option<super::windows::PolicyWatch> { None }
}
pub type Factory = Arc<dyn Fn() -> Result<Box<dyn Adapter>, String> + Send + Sync>;
pub type Observe = Arc<dyn Fn(&[String]) -> Result<Vec<Source>, String> + Send + Sync>;
const READ_BUDGET: Duration = Duration::from_secs(2);
const FRESH_BUDGET: Duration = Duration::from_secs(2);
const DRIVER_AUDIT: Duration = Duration::from_millis(250);
pub(super) struct HandoffInput {
    pub job: String,
    pub record: Record,
    pub preferences: Preferences,
    pub sources: Vec<Source>,
    pub capture: String,
}
struct ReadRequest {
    input: HandoffInput,
    deadline: Instant,
    abandoned: Arc<AtomicBool>,
    reply: SyncSender<Result<(State, Plan), String>>,
}
pub(super) struct ReadTicket {
    receive: Receiver<Result<(State, Plan), String>>,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    abandoned: Arc<AtomicBool>,
}
pub(super) struct HandoffRead {
    pub actual: State,
    pub policy: Plan,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}
fn live_read(deadline: Instant, cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err("Native visibility preparation expired or was cancelled.".into());
    }
    Ok(())
}
impl ReadTicket {
    /// Called only after releasing the service's worker/admission locks.
    pub(super) fn wait(self) -> Result<HandoffRead, String> {
        live_read(self.deadline, &self.cancelled)?;
        let result = self
            .receive
            .recv_timeout(self.deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| {
                "Native visibility preparation did not complete within its bounded ticket."
            })?;
        // A queued reply does not extend the deadline when its caller is delayed.
        live_read(self.deadline, &self.cancelled)?;
        let (actual, policy) = result?;
        Ok(HandoffRead {
            actual,
            policy,
            deadline: self.deadline,
            cancelled: self.cancelled.clone(),
        })
    }
}
impl Drop for ReadTicket {
    fn drop(&mut self) {
        self.abandoned.store(true, Ordering::Release);
    }
}
impl HandoffRead {
    pub(super) fn validate_live(&self) -> Result<(), String> {
        live_read(self.deadline, &self.cancelled)
    }
}
#[derive(Clone)]
pub enum Task {
    Apply {
        expected_plan: Option<Plan>,
        preferences: Preferences,
        ids: Vec<String>,
        capture: String,
        expected: Vec<Source>,
    },
    Recover {
        id: String,
    },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub job_id: String,
    pub active: bool,
    pub phase: String,
    pub message: String,
    pub session_id: Option<String>,
}
pub struct Worker {
    pub status: Arc<Mutex<Option<Status>>>,
    completed: Arc<Condvar>,
    cancel: Arc<AtomicBool>,
    wake: Arc<NativeWake>,
    thread: Option<JoinHandle<()>>,
    reads: Option<SyncSender<ReadRequest>>,
    pub(super) changed: Option<Arc<dyn Fn() + Send + Sync>>,
    pub(super) routing: std::sync::Weak<osg_streams::NativeWake>,
    pub(super) activation: std::sync::Weak<osg_streams::NativeWake>,
}
impl Default for Worker {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(None)),
            completed: Arc::default(),
            cancel: Arc::new(AtomicBool::new(false)),
            wake: Arc::default(),
            thread: None,
            reads: None,
            changed: None,
            routing: Default::default(),
            activation: Default::default(),
        }
    }
}
impl Worker {
    pub(super) fn completion(&self) -> Completion {
        Completion { status: self.status.clone(), completed: self.completed.clone() }
    }
    pub(super) fn handoff_status(&self) -> Result<Status, String> {
        if self.cancel.load(Ordering::Acquire) {
            return Err("The outgoing visibility worker is stopping.".into());
        }
        let status = self
            .snapshot()
            .filter(|s| s.active && s.phase == "active")
            .ok_or("The outgoing visibility worker is not active.")?;
        if self.cancel.load(Ordering::Acquire) {
            return Err("The outgoing visibility worker is stopping.".into());
        }
        Ok(status)
    }
    pub(super) fn prepare_handoff(&self, input: HandoffInput) -> Result<ReadTicket, String> {
        let status = self.handoff_status()?;
        if status.job_id != input.job
            || status.session_id.as_deref() != Some(input.record.id.as_str())
        {
            return Err("The outgoing native visibility job or session changed.".into());
        }
        let deadline = Instant::now() + READ_BUDGET;
        let abandoned = Arc::new(AtomicBool::new(false));
        let (reply, receive) = mpsc::sync_channel(1);
        self.reads
            .as_ref()
            .ok_or("The outgoing visibility reader is unavailable.")?
            .try_send(ReadRequest {
                input,
                deadline,
                abandoned: abandoned.clone(),
                reply,
            })
            .map_err(|_| "The native visibility preparation queue is busy or closed.")?;
        self.wake.notify();
        Ok(ReadTicket {
            receive,
            deadline,
            cancelled: self.cancel.clone(),
            abandoned,
        })
    }
    pub fn active(&self) -> bool {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|s| s.active)
    }
    pub fn snapshot(&self) -> Option<Status> {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub(crate) fn start(
        &mut self,
        task: Task,
        path: PathBuf,
        factory: Factory,
        capture_source: Capture,
    ) -> Result<Status, String> {
        if self.active() {
            return Err(
                "Restore the active visibility session before applying another policy.".into(),
            );
        }
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| "Visibility worker exited unexpectedly.")?;
        }
        self.cancel = Arc::new(AtomicBool::new(false));
        self.wake = Arc::default();
        let status = Status {
            job_id: uuid::Uuid::new_v4().to_string(),
            active: true,
            phase: "preflight".into(),
            message: "Checking capture, policy and recovery state.".into(),
            session_id: None,
        };
        *self.status.lock().unwrap_or_else(|p| p.into_inner()) = Some(status.clone());
        let shared = self.status.clone();
        let completed = self.completed.clone();
        let cancel = self.cancel.clone();
        let wake = self.wake.clone();
        let (sender, reads) = mpsc::sync_channel(1);
        let job = status.job_id.clone();
        let changed = self.changed.clone();
        let routing = self.routing.clone();
        let activation = self.activation.clone();
        let notify = move || {
            if let Some(wake) = activation.upgrade() { wake.notify(); }
            if let Some(wake) = routing.upgrade() { wake.notify(); }
            if let Some(changed) = &changed { changed(); }
        };
        let spawned=std::thread::Builder::new().name("device-visibility".into()).spawn(move||{
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||run(task,path,factory,capture_source,cancel,wake,|id|{
                if let Some(status)=shared.lock().unwrap_or_else(|p|p.into_inner()).as_mut(){status.phase="active".into();status.message="Persistent visibility session active; restore before changing device assignments.".into();status.session_id=Some(id);}
                completed.notify_all();
                notify();
            },reads,job))).unwrap_or_else(|_|Err("Visibility worker failed; inspect the durable recovery journal.".into()));
            if let Some(status)=shared.lock().unwrap_or_else(|p|p.into_inner()).as_mut(){status.active=false;status.phase=if result.is_ok(){"restored"}else{"attention"}.into();status.message=result.err().unwrap_or("Owned visibility changes restored and verified.".into()).chars().take(512).collect();}
            completed.notify_all();
            notify();
        });
        match spawned {
            Ok(thread) => {
                self.thread = Some(thread);
                self.reads = Some(sender);
                Ok(status)
            }
            Err(error) => {
                self.reads = None;
                if let Some(status) = self
                    .status
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .as_mut()
                {
                    status.active = false;
                    status.phase = "attention".into();
                    status.message = error.to_string();
                }
                Err(error.to_string())
            }
        }
    }
    pub fn stop(&self, job: Option<&str>) -> Result<(), String> {
        if let Some(id) = job {
            if self.snapshot().as_ref().is_none_or(|s| s.job_id != id) {
                return Err("Stale visibility job ID.".into());
            }
        }
        self.cancel.store(true, Ordering::Release);
        self.wake.notify();
        Ok(())
    }
    pub fn shutdown(&mut self) {
        let _ = self.stop(None);
        if let Some(thread) = self.thread.take() {
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}

pub(super) struct Completion {
    status: Arc<Mutex<Option<Status>>>,
    completed: Arc<Condvar>,
}
impl Completion {
    pub(super) fn wait(&self, job: &str, restoring: bool, budget: Duration) -> Result<bool, String> {
        let pending = |status: &Status| status.active && (restoring || status.phase != "active");
        let status = self.status.lock().unwrap_or_else(|p| p.into_inner());
        let (status, _) = self.completed.wait_timeout_while(status, budget, |status| {
            status.as_ref().is_some_and(|status| status.job_id == job && pending(status))
        }).unwrap_or_else(|p| p.into_inner());
        status.as_ref().filter(|status| status.job_id == job).map(|status| !pending(status))
            .ok_or_else(|| "Native visibility job changed while waiting for its phase.".into())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn run(
    task: Task,
    path: PathBuf,
    factory: Factory,
    capture_source: Capture,
    cancel: Arc<AtomicBool>,
    wake: Arc<NativeWake>,
    active: impl FnOnce(String),
    reads: Receiver<ReadRequest>,
    job: String,
) -> Result<(), String> {
    let check = || -> Result<(), String> {
        if cancel.load(Ordering::Acquire) {
            Err("Visibility operation cancelled.".to_owned())
        } else {
            Ok(())
        }
    };
    check()?;
    wake.bind_thread(std::thread::current()).map_err(|_| "Visibility wake is already bound.")?;
    let mut adapter = factory()?;
    let mut store = Store::open(path)?;
    match task {
        Task::Recover { id } => {
            journal::restore(&mut adapter, &mut store, &id)?;
            Ok(())
        }
        Task::Apply {
            expected_plan,
            preferences,
            ids,
            capture,
            expected,
        } => {
            preferences.validate()?;
            if !preferences.enabled {
                return Err("Enable optional device hiding before Apply.".into());
            }
            let sources = expected;
            let inventory = Arc::new(NativeWake::child(wake.clone()));
            let report_watch = (capture_source.watch)(&ids, &wake, &inventory)?;
            session::validate_sources(&sources, &capture_source.sources(&ids)?, false)?;
            let image = adapter.executable(&capture)?;
            let mut selections = vec![];
            let mut paths = vec![(capture, image.clone())];
            for source in &sources {
                let policy = match preferences
                    .devices
                    .get(&source.id)
                    .unwrap_or(&Intent::Visible)
                {
                    Intent::Visible => super::policy::Policy::Visible,
                    Intent::BlockOriginalInput => return Err("Input suppression requires the native capture owner; HidHide is not a fallback.".into()),
                    Intent::HideAll => super::policy::Policy::HideAll,
                    Intent::HideFromProcess { executable } => {
                        let image = adapter.executable(executable)?;
                        paths.push((executable.clone(), image.clone()));
                        super::policy::Policy::HideFromProcess { executable: image }
                    }
                };
                selections.push(Selection {
                    instance: adapter.instance(&source.interface)?,
                    policy,
                });
            }
            let policy_wake = Arc::new(NativeWake::child(wake.clone()));
            #[cfg(windows)]
            let policy_watch = adapter.watch_policy(policy_wake.clone());
            #[cfg(windows)]
            let policy_live = || policy_watch.as_ref().is_some_and(|watch| watch.is_live());
            #[cfg(not(windows))]
            let policy_live = || false;
            let plan = Plan::build(adapter.read()?, selections, image)?;
            if expected_plan
                .as_ref()
                .is_some_and(|expected| *expected != plan)
            {
                return Err(
                    "Visibility changed after game preflight; preserve the external policy.".into(),
                );
            }
            check()?;
            let initial = sources.clone();
            let mut last = sources.clone();
            let mut session = Session::start(adapter, store, plan, sources, |fresh| {
                let deadline = Instant::now() + FRESH_BUDGET;
                loop {
                    let observed = wake.observed();
                    check()?;
                    let current = capture_source.sources(&ids)?;
                    session::validate_sources(&initial, &current, false)?;
                    if !fresh {
                        last = current.clone();
                        return Ok(current);
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err("No fresh capture report after hiding; restore and move the selected controls before retrying.".into());
                    }
                    if session::validate_sources(&last, &current, true).is_ok() {
                        check()?;
                        return Ok(current);
                    }
                    wake.wait(observed, remaining);
                }
            })?;
            drop(report_watch);
            let expected_policy = session.record.plan.after.clone();
            let expected_instances: Vec<_> = session.record.plan.selections.iter()
                .map(|selection| selection.instance.clone()).collect();
            let verify_instances = |adapter: &mut Box<dyn Adapter>| {
                for (source, instance) in initial.iter().zip(&expected_instances) {
                    if adapter.instance(&source.interface)? != *instance {
                        return Err("A hidden controller instance changed.".into());
                    }
                }
                Ok(())
            };
            let mut checked_inventory = inventory.observed();
            session.check(capture_source.sources(&ids))?;
            session.verify(&verify_instances)?;
            let verify_policy = |adapter: &mut Box<dyn Adapter>| {
                if adapter.read()? != expected_policy {
                    return Err("External visibility policy changed during this session.".into());
                }
                Ok(())
            };
            let mut checked_policy = policy_wake.observed();
            session.verify(&verify_policy)?;
            let verify_executables = |adapter: &mut Box<dyn Adapter>| {
                for (path, image) in &paths {
                    if adapter.executable(path)? != *image {
                        return Err("A visibility executable moved; stop and select its new path.".into());
                    }
                }
                Ok(())
            };
            session.verify(&verify_executables)?;
            if cancel.load(Ordering::Acquire) {
                return session.stop();
            }
            active(session.record.id.clone());
            let mut next_verification = Instant::now() + DRIVER_AUDIT;
            while !cancel.load(Ordering::Acquire) {
                let observed = wake.observed();
                let inventory_epoch = inventory.observed();
                if checked_inventory != inventory_epoch {
                    session.check(capture_source.sources(&ids))?;
                    session.verify(&verify_instances)?;
                    checked_inventory = inventory_epoch;
                }
                if let Ok(request) = reads.try_recv() {
                    let result = inspect_handoff(
                        &mut session, &request, &job, &cancel, &capture_source.observe, &paths,
                    );
                    // Stop can race the last backend read. The receiver also
                    // checks this same per-start cancellation flag after recv.
                    let result = live_read(request.deadline, &cancel).and(result);
                    let _ = request.reply.try_send(result);
                }
                let audit_due = Instant::now() >= next_verification;
                let policy_epoch = policy_wake.observed();
                if policy_epoch != checked_policy || (audit_due && !policy_live()) {
                    session.verify(&verify_policy)?;
                    checked_policy = policy_epoch;
                }
                if audit_due {
                    session.verify(&verify_executables)?;
                    next_verification = Instant::now() + DRIVER_AUDIT;
                }
                wake.wait(observed, next_verification.saturating_duration_since(Instant::now()));
            }
            session.stop()
        }
    }
}
#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
