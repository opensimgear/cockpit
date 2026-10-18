//! Worker-owned standalone visibility sessions, supervised without frontend polling.
use super::{
    journal::{self, Backend, Record, Store},
    policy::{Plan, Selection, State},
    session::{self, Session, Source},
    settings::{Intent, Preferences},
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
pub trait Adapter: Backend + Send {
    fn executable(&mut self, path: &str) -> Result<String, String>;
    fn instance(&mut self, interface: &str) -> Result<String, String>;
}
pub type Factory = Arc<dyn Fn() -> Result<Box<dyn Adapter>, String> + Send + Sync>;
pub type Observe = Arc<dyn Fn(&[String]) -> Result<Vec<Source>, String> + Send + Sync>;
const READ_BUDGET: Duration = Duration::from_secs(2);
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
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    reads: Option<SyncSender<ReadRequest>>,
}
impl Default for Worker {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(None)),
            cancel: Arc::new(AtomicBool::new(false)),
            thread: None,
            reads: None,
        }
    }
}
impl Worker {
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
    pub fn start(
        &mut self,
        task: Task,
        path: PathBuf,
        factory: Factory,
        observe: Observe,
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
        let status = Status {
            job_id: uuid::Uuid::new_v4().to_string(),
            active: true,
            phase: "preflight".into(),
            message: "Checking capture, policy and recovery state.".into(),
            session_id: None,
        };
        *self.status.lock().unwrap_or_else(|p| p.into_inner()) = Some(status.clone());
        let shared = self.status.clone();
        let cancel = self.cancel.clone();
        let (sender, reads) = mpsc::sync_channel(1);
        let job = status.job_id.clone();
        let spawned=std::thread::Builder::new().name("device-visibility".into()).spawn(move||{
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||run(task,path,factory,observe,cancel,|id|{
                if let Some(status)=shared.lock().unwrap_or_else(|p|p.into_inner()).as_mut(){status.phase="active".into();status.message="Persistent visibility session active; restore before changing device assignments.".into();status.session_id=Some(id);}
            },reads,job))).unwrap_or_else(|_|Err("Visibility worker failed; inspect the durable recovery journal.".into()));
            if let Some(status)=shared.lock().unwrap_or_else(|p|p.into_inner()).as_mut(){status.active=false;status.phase=if result.is_ok(){"restored"}else{"attention"}.into();status.message=result.err().unwrap_or("Owned visibility changes restored and verified.".into()).chars().take(512).collect();}
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
impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn run(
    task: Task,
    path: PathBuf,
    factory: Factory,
    observe: Observe,
    cancel: Arc<AtomicBool>,
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
            session::validate_sources(&sources, &observe(&ids)?, false)?;
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
            let cancelled = cancel.clone();
            let mut session = Session::start(adapter, store, plan, sources, |fresh| {
                let deadline = Instant::now() + Duration::from_secs(2);
                loop {
                    if cancelled.load(Ordering::Acquire) {
                        return Err("Visibility operation cancelled.".into());
                    }
                    let current = observe(&ids)?;
                    session::validate_sources(&initial, &current, false)?;
                    if !fresh {
                        last = current.clone();
                        return Ok(current);
                    }
                    if session::validate_sources(&last, &current, true).is_ok() {
                        return Ok(current);
                    }
                    if Instant::now() >= deadline {
                        return Err("No fresh capture report after hiding; restore and move the selected controls before retrying.".into());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            })?;
            let expected_policy = session.record.plan.after.clone();
            let expected_instances = session
                .record
                .plan
                .selections
                .iter()
                .map(|s| s.instance.clone())
                .collect::<Vec<_>>();
            active(session.record.id.clone());
            let mut last_verification = Instant::now() - Duration::from_secs(1);
            while !cancel.load(Ordering::Acquire) {
                session.check(observe(&ids))?;
                if let Ok(request) = reads.try_recv() {
                    let result =
                        inspect_handoff(&mut session, &request, &job, &cancel, &observe, &paths);
                    // Stop can race the last backend read. The receiver also
                    // checks this same per-start cancellation flag after recv.
                    let result = live_read(request.deadline, &cancel).and(result);
                    let _ = request.reply.try_send(result);
                }
                if last_verification.elapsed() >= Duration::from_millis(250) {
                    session.verify(|adapter| {
                        for (path, image) in &paths {
                            if adapter.executable(path)? != *image {
                                return Err(
                                    "A visibility executable moved; stop and select its new path."
                                        .into(),
                                );
                            }
                        }
                        for (source, instance) in initial.iter().zip(&expected_instances) {
                            if adapter.instance(&source.interface)? != *instance {
                                return Err("A hidden controller instance changed.".into());
                            }
                        }
                        if adapter.read()? != expected_policy {
                            return Err(
                                "External visibility policy changed during this session.".into()
                            );
                        }
                        Ok(())
                    })?;
                    last_verification = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            session.stop()
        }
    }
}
fn inspect_handoff(
    session: &mut Session<Box<dyn Adapter>>,
    request: &ReadRequest,
    job: &str,
    cancel: &AtomicBool,
    observe: &Observe,
    paths: &[(String, String)],
) -> Result<(State, Plan), String> {
    let check = || -> Result<(), String> {
        live_read(request.deadline, cancel)?;
        if request.abandoned.load(Ordering::Acquire) || request.input.job != job {
            return Err(
                "The native visibility preparation ticket no longer owns this worker.".into(),
            );
        }
        Ok(())
    };
    check()?;
    let input = &request.input;
    input.preferences.validate()?;
    if !input.preferences.enabled {
        return Err("Enable optional visibility before preparation.".into());
    }
    let ids = input
        .sources
        .iter()
        .map(|s| s.id.clone())
        .collect::<Vec<_>>();
    let outgoing = session.sources.clone();
    let outgoing_ids = outgoing.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    session.inspect(|adapter, record| {
        check()?;
        if serde_json::to_vec(record).map_err(|e| e.to_string())?
            != serde_json::to_vec(&input.record).map_err(|e| e.to_string())?
        {
            return Err(
                "The outgoing native visibility journal no longer matches the requested session."
                    .into(),
            );
        }
        let steps = record.validate()?;
        if record.clean
            || record.restoring
            || record.pending.is_some()
            || record.position + 1 != steps.len()
        {
            return Err(
                "Only a fully applied native visibility session can prepare handoff.".into(),
            );
        }
        session::validate_sources(&input.sources, &observe(&ids)?, false)?;
        session::validate_sources(&outgoing, &observe(&outgoing_ids)?, false)?;
        let actual = adapter.read()?;
        check()?;
        if actual != record.plan.after {
            return Err("The outgoing native driver policy changed; preserve it.".into());
        }
        let image = adapter.executable(&input.capture)?;
        check()?;
        // The live session's own executable and instances remain immutable.
        if image != record.plan.capture {
            return Err("The capture executable changed during native handoff.".into());
        }
        for (path, image) in paths {
            if adapter.executable(path)? != *image {
                return Err("An outgoing visibility executable changed.".into());
            }
            check()?;
        }
        if outgoing.len() != record.plan.selections.len() {
            return Err("The outgoing native selections changed.".into());
        }
        for (source, selection) in outgoing.iter().zip(&record.plan.selections) {
            let instance = adapter.instance(&source.interface)?;
            check()?;
            if instance != selection.instance {
                return Err("An outgoing hidden controller instance changed.".into());
            }
        }
        let mut selections = Vec::new();
        for source in &input.sources {
            let policy = match input.preferences.devices.get(&source.id) {
                Some(Intent::HideAll) => super::policy::Policy::HideAll,
                Some(Intent::HideFromProcess { executable }) => {
                    let image = adapter.executable(executable)?;
                    check()?;
                    super::policy::Policy::HideFromProcess { executable: image }
                }
                _ => return Err("Unsupported captured policy in native visibility handoff.".into()),
            };
            let instance = adapter.instance(&source.interface)?;
            check()?;
            selections.push(Selection { instance, policy });
        }
        let policy = Plan::build(record.plan.before.clone(), selections, image)?;
        session::validate_sources(&input.sources, &observe(&ids)?, false)?;
        session::validate_sources(&outgoing, &observe(&outgoing_ids)?, false)?;
        if adapter.read()? != actual {
            return Err("The native driver policy changed during handoff preparation.".into());
        }
        check()?;
        Ok((actual, policy))
    })
}
#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
