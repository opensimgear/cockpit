//! Worker-owned standalone visibility sessions, supervised without frontend polling.
use super::{
    journal::{self, Backend, Store},
    policy::{Plan, Selection},
    session::{self, Session, Source},
    settings::{Intent, Preferences},
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
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
#[derive(Clone)]
pub enum Task {
    Apply {
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
}
impl Default for Worker {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(None)),
            cancel: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }
}
impl Worker {
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
        let spawned=std::thread::Builder::new().name("device-visibility".into()).spawn(move||{
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||run(task,path,factory,observe,cancel,|id|{
                if let Some(status)=shared.lock().unwrap_or_else(|p|p.into_inner()).as_mut(){status.phase="active".into();status.message="Persistent visibility session active; restore before changing device assignments.".into();status.session_id=Some(id);}
            }))).unwrap_or_else(|_|Err("Visibility worker failed; inspect the durable recovery journal.".into()));
            if let Some(status)=shared.lock().unwrap_or_else(|p|p.into_inner()).as_mut(){status.active=false;status.phase=if result.is_ok(){"restored"}else{"attention"}.into();status.message=result.err().unwrap_or("Owned visibility changes restored and verified.".into()).chars().take(512).collect();}
        });
        match spawned {
            Ok(thread) => {
                self.thread = Some(thread);
                Ok(status)
            }
            Err(error) => {
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
) -> Result<(), String> {
    let check = || {
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
#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
