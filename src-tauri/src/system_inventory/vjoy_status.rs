use super::{Runtime, Status};
use osg_virtual_controller::inspection::{SlotStatus, StatusReader};
use serde_json::{json, Value};
use std::{
    sync::{atomic::Ordering, Arc, Mutex, TryLockError, Weak},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const SAMPLE_PERIOD: Duration = Duration::from_millis(500);
const RETRY_PERIOD: Duration = Duration::from_secs(3);

#[derive(Default)]
pub(super) struct Owner {
    pub(super) wake: Arc<osg_streams::NativeWake>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Owner {
    pub(super) fn shutdown(&self) {
        self.wake.notify();
        let worker = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(worker) = worker.filter(|worker| worker.thread().id() != thread::current().id()) {
            let _ = worker.join();
        }
    }
}

impl Runtime {
    pub(crate) fn start_vjoy_status(self: &Arc<Self>) -> Result<(), String> {
        if !self.native || !cfg!(windows) { return Ok(()); }
        self.start_vjoy_status_with(StatusReader::open, StatusReader::read)
    }

    pub(crate) fn vjoy_reader_changed(&self) {
        self.vjoy_status.wake.notify();
    }

    fn start_vjoy_status_with<R: 'static>(self: &Arc<Self>,
        open: impl Fn() -> Result<R, String> + Send + 'static,
        read: impl Fn(&mut R) -> Result<Vec<SlotStatus>, String> + Send + 'static,
    ) -> Result<(), String> {
        let mut worker = self.vjoy_status.worker.lock().unwrap_or_else(|p| p.into_inner());
        if worker.is_some() || self.stopped.load(Ordering::Acquire) {
            return Err("vJoy status worker cannot start.".into());
        }
        let wake = self.vjoy_status.wake.clone();
        let runtime = Arc::downgrade(self);
        *worker = Some(thread::Builder::new().name("system-vjoy-status".into())
            .spawn(move || sample_visible(runtime, wake, open, read)).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn vjoy_readers(&self) -> bool {
        self.publishers.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
            .is_some_and(|publishers| publishers.vjoy_demand.has_readers())
    }

    fn visible_vjoy_revision(&self) -> Option<u64> {
        if !self.vjoy_readers() { return None; }
        let snapshot = self.snapshot.lock().unwrap_or_else(|p| p.into_inner());
        let value = &snapshot.as_ref()?.value;
        if value["drivers"]["vjoy"]["installed"] == false ||
            !value["virtualDevices"].as_array()?.iter().any(|row| row["backend"] == "vjoy") {
            return None;
        }
        value["observedAt"].as_u64()
    }

    fn commit_vjoy_sample(&self, expected: u64, slots: Option<&[SlotStatus]>) {
        // Never wait behind a full inventory refresh or overwrite a newer one.
        let _capture = match self.capture_gate.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        if self.stopped.load(Ordering::Acquire) || !self.vjoy_readers() { return; }
        let mut value = {
            let snapshot = self.snapshot.lock().unwrap_or_else(|p| p.into_inner());
            let Some(snapshot) = snapshot.as_ref().filter(|snapshot| snapshot.value["observedAt"] == expected) else { return; };
            snapshot.value.clone()
        };
        let Some(rows) = value["virtualDevices"].as_array_mut() else { return; };
        for row in rows.iter_mut().filter(|row| row["backend"] == "vjoy") {
            project_ownership(row, slots);
        }
        value["observedAt"] = json!(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64);
        self.commit(value);
    }
}

fn project_ownership(row: &mut Value, slots: Option<&[SlotStatus]>) {
    let slot = slots.and_then(|slots| slots.iter().find(|slot| row["slot"].as_u64() == Some(u64::from(slot.id))));
    let status = slot.map_or(Status::Unknown, |slot| slot.status);
    let owner = slot.and_then(|slot| slot.owner_pid);
    let available = match status {
        Status::Missing => Some(false),
        Status::Unknown => None,
        _ => Some(true),
    };
    row["status"] = json!(status);
    row["available"] = json!(available);
    row["ownerPid"] = json!(owner);
    row["owner"] = json!(owner.map(|pid| format!("Process {pid}")));
}

fn sample_visible<R>(runtime: Weak<Runtime>, wake: Arc<osg_streams::NativeWake>,
    open: impl Fn() -> Result<R, String>, read: impl Fn(&mut R) -> Result<Vec<SlotStatus>, String>,
) {
    wake.bind_thread(thread::current()).expect("vJoy status wake has one worker");
    let mut reader = None;
    let mut deadline: Option<Instant> = None;
    loop {
        let observed = wake.observed();
        let Some(runtime) = runtime.upgrade() else { break; };
        if runtime.stopped.load(Ordering::Acquire) { break; }
        if let Some(revision) = runtime.visible_vjoy_revision() {
            if deadline.is_none_or(|deadline| deadline <= Instant::now()) {
                let result = if let Some(reader) = reader.as_mut() { read(reader) } else {
                    match open() {
                        Ok(opened) => {
                            reader = Some(opened);
                            // Visibility and the full inventory may change during startup.
                            continue;
                        }
                        Err(error) => Err(error),
                    }
                };
                runtime.commit_vjoy_sample(revision, result.as_deref().ok());
                let delay = if result.is_ok() { SAMPLE_PERIOD } else {
                    reader = None;
                    RETRY_PERIOD
                };
                deadline = Some(Instant::now() + delay);
            }
        } else {
            reader = None;
            deadline = None;
        }
        drop(runtime);
        wake.wait_scheduled(observed, deadline.map(|at| at.saturating_duration_since(Instant::now())));
    }
}

#[cfg(test)]
#[path = "vjoy_status_tests.rs"]
mod tests;
