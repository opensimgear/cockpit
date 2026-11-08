//! Capture-owned ephemeral suppression. Commands never open a second input handle.
use super::session::{validate_sources, Source};
use serde_json::{json, Value};
use std::{sync::{Arc, Condvar, Mutex}, time::Duration};
#[derive(Clone)]
pub enum Command {
    Apply { id: String, sources: Vec<Source> },
    Stop,
}
#[derive(Default)]
struct State {
    pending: Option<Command>,
    id: String,
    phase: String,
    reason: String,
    activation: std::sync::Weak<osg_streams::NativeWake>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    capture: std::sync::Weak<crate::CaptureWake>,
}
#[derive(Clone, Default)]
pub struct Control(Arc<Mutex<State>>, Arc<Condvar>);
impl Control {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn watch_capture(&self, wake: &Arc<crate::CaptureWake>) {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).capture = Arc::downgrade(wake);
    }
    pub(super) fn watch_activation(&self, wake: &Arc<osg_streams::NativeWake>) {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).activation = Arc::downgrade(wake);
    }
    pub fn apply(&self, sources: Vec<Source>) -> Result<String, String> {
        validate_sources(&sources, &sources, false)?;
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if busy(&s.phase) {
            return Err("Restore the current suppression session first.".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        s.id = id.clone();
        s.phase = "queued".into();
        s.reason.clear();
        s.pending = Some(Command::Apply {
            id: id.clone(),
            sources,
        });
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if let Some(wake) = s.capture.upgrade() { let _ = wake.signal(); }
        Ok(id)
    }
    pub fn stop(&self, id: Option<&str>) -> Result<(), String> {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if id.is_some_and(|id| id != s.id) {
            return Err("Suppression session changed; refresh before Stop.".into());
        }
        if busy(&s.phase) {
            s.pending = Some(Command::Stop);
            s.phase = "restoring".into();
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            if let Some(wake) = s.capture.upgrade() { let _ = wake.signal(); }
        }
        Ok(())
    }
    pub fn take(&self) -> Option<Command> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pending
            .take()
    }
    pub fn update(&self, phase: &str, reason: &str) {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        s.phase = phase.into();
        s.reason = reason.into();
        self.1.notify_all();
        if let Some(wake) = s.activation.upgrade() { wake.notify(); }
    }
    pub(super) fn wait_phase(&self, job: &str, restoring: bool, budget: Duration) -> Result<bool, String> {
        let pending = |state: &State| busy(&state.phase) && (restoring || state.phase != "active");
        let state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let (state, _) = self.1.wait_timeout_while(state, budget, |state| {
            state.id == job && pending(state)
        }).unwrap_or_else(|p| p.into_inner());
        if state.id != job {
            return Err("Suppression session changed while waiting for its phase.".into());
        }
        Ok(!pending(&state))
    }
    pub fn active(&self) -> bool {
        busy(&self.0.lock().unwrap_or_else(|p| p.into_inner()).phase)
    }
    pub fn snapshot(&self) -> Value {
        let s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        json!({"jobId":s.id,"active":busy(&s.phase),"message":s.reason,"phase":if s.phase.is_empty(){"idle"}else{&s.phase},"reason":s.reason,"scope":"event-delivery"})
    }
}
fn busy(phase: &str) -> bool {
    matches!(
        phase,
        "queued" | "applying" | "baseline" | "active" | "restoring"
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restoration_wait_is_bounded_and_fenced_to_the_exact_session() {
        let control = Control::default();
        let job = control.apply(vec![source()]).unwrap();
        control.stop(Some(&job)).unwrap();
        assert!(!control.wait_phase(&job, true, Duration::ZERO).unwrap());
        let reader = control.clone();
        let exact = job.clone();
        let waiter = std::thread::spawn(move || reader.wait_phase(&exact, true, Duration::from_secs(2)));
        let activation = Arc::new(osg_streams::NativeWake::default());
        control.watch_activation(&activation);
        let revision = activation.observed();
        control.update("restored", "Done");
        assert_ne!(activation.observed(), revision);
        assert!(waiter.join().unwrap().unwrap());
        control.apply(vec![source()]).unwrap();
        assert!(control.wait_phase(&job, true, Duration::ZERO).is_err());
    }
    #[test]
    fn poisoned_capture_lock_preserves_the_owned_job_and_activation_wake() {
        let control = Control::default();
        let job = control.apply(vec![source()]).unwrap();
        let activation = Arc::new(osg_streams::NativeWake::default());
        control.watch_activation(&activation);

        let capture = control.clone();
        assert!(std::thread::spawn(move || {
            let _locked = capture.0.lock().unwrap();
            panic!("Capture exited while holding its state");
        }).join().is_err());

        assert!(matches!(control.take(), Some(Command::Apply { id, .. }) if id == job));
        assert!(control.active());
        let revision = activation.observed();
        control.update("active", "Capture active");
        assert_ne!(activation.observed(), revision);
        assert!(control.wait_phase(&job, false, Duration::ZERO).unwrap());
        control.stop(Some(&job)).unwrap();
        assert!(matches!(control.take(), Some(Command::Stop)));
    }

    fn source() -> Source {
        Source {
            id: uuid::Uuid::new_v4().to_string(),
            session: uuid::Uuid::new_v4().to_string(),
            generation: 1,
            sequence: 1,
            interface: "physical".into(),
        }
    }
    #[test]
    fn bounded_commands_stop_and_stale_jobs() {
        let c = Control::default();
        assert!(!c.active());
        assert_eq!(c.snapshot()["phase"], "idle");
        assert!(c.take().is_none());
        assert!(c.apply(vec![]).is_err());
        let id = c.apply(vec![source()]).unwrap();
        assert!(c.active());
        assert!(c.apply(vec![source()]).is_err());
        assert!(c.stop(Some("stale")).is_err());
        assert!(matches!(c.take(), Some(Command::Apply { .. })));
        c.update("baseline", "");
        assert!(c.active());
        c.update("active", "");
        assert_eq!(c.snapshot()["jobId"], id);
        c.stop(Some(&id)).unwrap();
        assert!(matches!(c.take(), Some(Command::Stop)));
        c.update("stopped", "released");
        assert!(!c.active());
        c.stop(None).unwrap();
        c.apply(vec![source()]).unwrap();
        c.stop(None).unwrap();
        assert!(matches!(c.take(), Some(Command::Stop)));
    }
}
