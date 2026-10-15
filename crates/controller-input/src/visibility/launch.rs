//! Owned Linux child sessions. Qualification is independent of broad suppression.
use super::{
    runtime::Observe,
    session::{validate_sources, Source},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
};

pub const QUALIFIED: bool = false;
pub const REASON: &str = "Linux isolated launch is implemented but disabled pending native device, hotplug, helper and game qualification. Only direct children in a closed device view are candidates; Steam/Proton, display/input brokers and running processes are unsupported.";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub launch_kind: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub directory: String,
}
impl Request {
    pub fn parse(value: &Value) -> Result<Self, String> {
        if value.get("pid").is_some() || value.get("alreadyRunning").is_some() {
            return Err(
                "Already-running applications cannot enter an owned launch boundary.".into(),
            );
        }
        let request: Self = serde_json::from_value(value.clone()).map_err(|_| "Invalid isolated-launch request; use an explicit direct child, executable, argument array and application directory.")?;
        if request.launch_kind != "direct-child" {
            return Err("Unsupported launch path; Steam, Proton, brokers and existing processes are not qualified.".into());
        }
        for path in [&request.executable, &request.directory] {
            if !path.starts_with('/')
                || path.len() > 4096
                || path.contains('\0')
                || path.split('/').any(|p| p == "." || p == "..")
            {
                return Err("Use an absolute, normalized Linux application path.".into());
            }
        }
        if request.arguments.len() > 64
            || request
                .arguments
                .iter()
                .any(|a| a.len() > 4096 || a.contains('\0'))
            || request.arguments.iter().map(String::len).sum::<usize>() > 16384
        {
            return Err("Launch arguments exceed their bounded protocol.".into());
        }
        Ok(request)
    }
}
pub(crate) enum Poll {
    Starting,
    Running,
    Exited(i32),
}
pub(crate) trait Child: Send {
    fn poll(&mut self) -> Result<Poll, String>;
    fn stop(&mut self) -> Result<(), String>;
}
pub(crate) type Factory =
    Arc<dyn Fn(Request, Vec<Source>) -> Result<Box<dyn Child>, String> + Send + Sync>;
fn native(request: Request, sources: Vec<Source>) -> Result<Box<dyn Child>, String> {
    #[cfg(target_os = "linux")]
    {
        super::launch_linux::start(request, sources)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (request, sources);
        Err("Isolated launch is a Linux-only capability.".into())
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub job_id: String,
    pub active: bool,
    pub phase: String,
    pub message: String,
}
pub struct Worker {
    status: Arc<Mutex<Option<Status>>>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    factory: Factory,
    qualified: bool,
}
impl Default for Worker {
    fn default() -> Self {
        Self {
            status: Default::default(),
            cancel: Default::default(),
            thread: None,
            factory: Arc::new(native),
            qualified: QUALIFIED,
        }
    }
}
impl Worker {
    pub fn snapshot(&self) -> Value {
        json!({"implemented":cfg!(target_os="linux"),"qualified":self.qualified,"reason":REASON,"job":self.status.lock().unwrap_or_else(|p|p.into_inner()).clone()})
    }
    pub fn qualified(&self) -> bool {
        self.qualified
    }
    pub fn active(&self) -> bool {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|s| s.active)
    }
    pub fn start(
        &mut self,
        request: Request,
        sources: Vec<Source>,
        observe: Observe,
    ) -> Result<(), String> {
        if !self.qualified {
            return Err(REASON.into());
        }
        self.start_owned(request, sources, observe)
    }
    fn start_owned(
        &mut self,
        request: Request,
        sources: Vec<Source>,
        observe: Observe,
    ) -> Result<(), String> {
        if self.active() {
            return Err("Stop the current isolated child first.".into());
        }
        validate_sources(&sources, &sources, false)?;
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| "Launch worker failed.")?;
        }
        let ids: Vec<_> = sources.iter().map(|s| s.id.clone()).collect();
        validate_sources(&sources, &observe(&ids)?, false)?;
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let status = self.status.clone();
        let factory = self.factory.clone();
        *status.lock().unwrap_or_else(|p| p.into_inner()) = Some(Status {
            job_id: uuid::Uuid::new_v4().to_string(),
            active: true,
            phase: "starting".into(),
            message: "Preparing the closed device view; no host policy changes.".into(),
        });
        let spawned = std::thread::Builder::new()
            .name("linux-isolated-launch".into())
            .spawn(move || {
                let owned = AtomicBool::new(false);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if cancel.load(Ordering::Acquire) {
                        return Ok("Cancelled before launch.".into());
                    }
                    let mut child = factory(request, sources.clone())?;
                    owned.store(true, Ordering::Release);
                    let outcome = loop {
                        if cancel.load(Ordering::Acquire) {
                            break Ok("Owned child stopped; no session is replayed on restart.".into());
                        }
                        if let Err(error) = observe(&ids)
                            .and_then(|current| validate_sources(&sources, &current, false))
                        {
                            break Err(error);
                        }
                        match child.poll() {
                            Ok(Poll::Starting) => {}
                            Ok(Poll::Running) => {
                                if let Some(s) = status.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                                    s.phase = "running".into();
                                    s.message = "Owned child in a fixed device namespace; host clients are unchanged.".into();
                                }
                            }
                            Ok(Poll::Exited(0)) => break Ok("Owned child exited successfully.".into()),
                            Ok(Poll::Exited(code)) => break Err(format!("Isolated child exited with code {code}.")),
                            Err(error) => break Err(error),
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    };
                    child.stop()?;
                    owned.store(false, Ordering::Release);
                    outcome
                }))
                .unwrap_or_else(|_| Err("Launch supervisor failed; termination needs inspection.".into()));
                if let Some(s) = status.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                    s.active = owned.load(Ordering::Acquire);
                    s.phase = if result.is_ok() { "stopped" } else { "attention" }.into();
                    s.message = result.unwrap_or_else(|e| e).chars().take(512).collect();
                }
            });
        match spawned {
            Ok(thread) => {
                self.thread = Some(thread);
                Ok(())
            }
            Err(e) => {
                if let Some(s) = self
                    .status
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .as_mut()
                {
                    s.active = false;
                    s.phase = "attention".into();
                    s.message = e.to_string();
                }
                Err(e.to_string())
            }
        }
    }
    pub fn stop(&self, job: Option<&str>) -> Result<(), String> {
        if job.is_some_and(|id| {
            self.status
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .is_none_or(|s| s.job_id != id)
        }) {
            return Err("Stale isolated-launch job ID.".into());
        }
        self.cancel.store(true, Ordering::Release);
        Ok(())
    }
    pub fn shutdown(&mut self) {
        let _ = self.stop(None);
        if self.thread.as_ref().is_some_and(JoinHandle::is_finished) {
            let _ = self.thread.take().unwrap().join();
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
impl Worker {
    pub(super) fn fixture(factory: Factory) -> Self {
        let mut worker = Self::default();
        worker.factory = factory;
        worker.qualified = true;
        worker
    }
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
