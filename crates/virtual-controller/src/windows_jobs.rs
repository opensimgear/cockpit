//! One asynchronous provisioning operation. UAC and native refresh do not block
//! the request thread. Cancellation remains available while UAC is pending.
use crate::{
    windows_coordination::Cancellation,
    windows_launcher::{Action, Request, Ticket, QUALIFIED},
    windows_provision::{NativeBackend, Outcome},
};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub job_id: String,
    pub operation: String,
    pub active: bool,
    pub state: String,
    pub message: String,
}
struct Shared {
    status: Status,
    cancelled: bool,
    signal: Option<Arc<Cancellation>>,
}
#[derive(Clone)]
struct Control(Arc<Mutex<Shared>>);
impl Control {
    fn cancelled(&self) -> bool {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).cancelled
    }
    fn register(&self, signal: Arc<Cancellation>) -> Result<(), String> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.cancelled {
            signal.cancel()?;
        }
        state.signal = Some(signal);
        Ok(())
    }
    fn running(&self) {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        state.status.state = if state.cancelled {
            "cancelling"
        } else {
            "running"
        }
        .into();
    }
}
type Runner = Arc<dyn Fn(Request, Control) -> Result<Outcome, String> + Send + Sync>;
pub struct Manager {
    current: Option<Control>,
    worker: Option<thread::JoinHandle<()>>,
    runner: Runner,
}
impl Default for Manager {
    fn default() -> Self {
        Self {
            current: None,
            worker: None,
            runner: Arc::new(native),
        }
    }
}
impl Manager {
    pub fn status(&self) -> Option<Status> {
        self.current
            .as_ref()
            .map(|c| c.0.lock().unwrap_or_else(|p| p.into_inner()).status.clone())
    }
    pub fn start(&mut self, action: Action, operation: Option<String>) -> Result<Status, String> {
        if self.status().is_some_and(|s| s.active) {
            return Err("A provisioning operation is already active.".into());
        }
        if matches!(
            (&action, &operation),
            (Action::Create(_), Some(_)) | (Action::Recover, None)
        ) {
            return Err(
                "Creation requires a fresh operation; recovery requires its recorded identity."
                    .into(),
            );
        }
        let request = Request {
            operation: operation.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            owner: crate::windows_process::current()?,
            action,
        };
        request.arguments()?;
        let status = Status {
            job_id: uuid::Uuid::new_v4().to_string(),
            operation: request.operation.clone(),
            active: true,
            state: "authorizing".into(),
            message: "Waiting for Windows administrator authorization.".into(),
        };
        let control = Control(Arc::new(Mutex::new(Shared {
            status: status.clone(),
            cancelled: false,
            signal: None,
        })));
        let task = control.clone();
        let runner = self.runner.clone();
        let worker = thread::Builder::new()
            .name("vjoy-provision".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    runner(request, task.clone())
                }))
                .unwrap_or_else(|_| {
                    Err("Provisioning worker failed; inspect its durable operation record.".into())
                });
                let mut shared = task.0.lock().unwrap_or_else(|p| p.into_inner());
                shared.status.active = false;
                let (success, message) = match outcome {
                    Ok(outcome) => (outcome.success, outcome.message),
                    Err(error) => (false, error),
                };
                shared.status.state = if success { "complete" } else { "attention" }.into();
                shared.status.message = message.chars().take(512).collect();
                shared.signal = None;
            })
            .map_err(|e| e.to_string())?;
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            let _ = self.worker.take().unwrap().join();
        }
        self.current = Some(control);
        self.worker = Some(worker);
        Ok(status)
    }
    pub fn shutdown(&self) {
        if let Some(status) = self.status() {
            if status.active {
                let _ = self.cancel(&status.job_id);
            }
        }
    }
    pub fn cancel(&self, job_id: &str) -> Result<Status, String> {
        let control = self
            .current
            .as_ref()
            .ok_or("No provisioning operation is active.")?;
        let mut state = control.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.status.job_id != job_id || !state.status.active {
            return Err("Provisioning job identity is stale or completed.".into());
        }
        state.cancelled = true;
        state.status.state = "cancelling".into();
        state.status.message = "Cancellation requested; waiting for verified cleanup.".into();
        if let Some(signal) = &state.signal {
            signal.cancel()?;
        }
        Ok(state.status.clone())
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        self.shutdown();
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            let _ = self.worker.take().unwrap().join();
        }
        // Never block shutdown on a UAC prompt. The exact parent monitor and
        // pending cancellation signal make a subsequently launched helper refuse.
    }
}
fn native(request: Request, control: Control) -> Result<Outcome, String> {
    if !QUALIFIED {
        return Err("Elevated vJoy provisioning remains unqualified and disabled.".into());
    }
    if control.cancelled() {
        return Err("Provisioning cancelled before launch.".into());
    }
    let signal = Arc::new(Cancellation::create(&request.operation)?);
    control.register(signal.clone())?;
    if control.cancelled() {
        return Err("Provisioning cancelled before launch.".into());
    }
    let ticket = Ticket::start(request.clone(), signal)?;
    control.running();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(code) = ticket.poll()? {
            let outcome = NativeBackend::native(&request.operation)?.outcome()?;
            return match outcome {
                Some(outcome) if code == 0 && outcome.success => Ok(outcome),
                Some(outcome) => Err(outcome.message),
                None => Err(format!("Provisioning helper exited ({code}) without a verified outcome; inspect the driver and operation record.")),
            };
        }
        if Instant::now() >= deadline {
            ticket.cancel()?;
            return Err("Helper completion is unconfirmed; cancellation was requested. Preserve the operation for explicit recovery.".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}
#[cfg(test)]
#[path = "windows_jobs_tests.rs"]
mod tests;
