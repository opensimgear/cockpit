//! Coalesced retention of passive observations. Input/telemetry producers only
//! replace a bounded pending snapshot; serialization and SQLite run on a worker.
//! User-authored settings and destructive Forget operations must use acknowledged
//! repository commits instead of this observation queue.
use crate::Repository;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

struct State<T> {
    pending: Option<(u64, T)>,
    queued: u64,
    committed: u64,
    closing: bool,
    done: bool,
    error: Option<String>,
    error_observer: Option<Arc<dyn Fn() + Send + Sync>>,
}
pub struct ObservationWriter<T: Serialize + DeserializeOwned + Send + 'static> {
    state: Arc<(Mutex<State<T>>, Condvar)>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl<T: Serialize + DeserializeOwned + Send + 'static> ObservationWriter<T> {
    pub fn new(repository: Repository<T>) -> Result<Self, String> {
        let state = Arc::new((
            Mutex::new(State {
                pending: None,
                queued: 0,
                committed: 0,
                closing: false,
                done: false,
                error: None,
                error_observer: None,
            }),
            Condvar::new(),
        ));
        let shared = state.clone();
        let worker = std::thread::Builder::new()
            .name("sqlite-observations".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| loop {
                    let (lock, wake) = &*shared;
                    let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
                    while state.pending.is_none() && !state.closing {
                        state = wake.wait(state).unwrap_or_else(|p| p.into_inner());
                    }
                    let Some((sequence, value)) = state.pending.take() else {
                        break;
                    };
                    drop(state);
                    let result = repository.save(&value);
                    let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
                    let previous = state.error.clone();
                    let mut retry = false;
                    match result {
                        Ok(()) => {
                            state.committed = sequence;
                            state.error = None;
                        }
                        Err(error) => {
                            state.error = Some(error);
                            retry = true;
                            if !state.closing && state.pending.is_none() {
                                state.pending = Some((sequence, value));
                            }
                        }
                    }
                    let observer = (state.error != previous).then(|| state.error_observer.clone()).flatten();
                    let finished = state.closing && (retry || state.pending.is_none());
                    wake.notify_all();
                    drop(state);
                    notify_error(observer);
                    if finished { break; }
                    if retry {
                        let state = lock.lock().unwrap_or_else(|p| p.into_inner());
                        if !state.closing {
                            drop(wake.wait_timeout(state, Duration::from_millis(250)).unwrap_or_else(|p| p.into_inner()));
                        }
                    }
                }));
                let (lock, wake) = &*shared;
                let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
                let previous = state.error.clone();
                if outcome.is_err() {
                    state.error = Some("Observation persistence worker failed.".into());
                }
                state.done = true;
                wake.notify_all();
                let observer = (state.error != previous).then(|| state.error_observer.clone()).flatten();
                drop(state);
                notify_error(observer);
            })
            .map_err(|_| "Cannot start observation persistence worker.")?;
        Ok(Self {
            state,
            worker: Mutex::new(Some(worker)),
        })
    }
    /// One source-owned notification for error/recovery transitions. The trusted
    /// callback runs after state and repository locks are released; successful
    /// observation commits do not generate status polling notifications.
    pub fn watch_error(&self, observer: Arc<dyn Fn() + Send + Sync>) {
        self.state.0.lock().unwrap_or_else(|p| p.into_inner()).error_observer = Some(observer);
    }
    pub fn queue(&self, value: T) -> Result<(), String> {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
        if state.closing || state.done {
            return Err("Observation persistence has stopped.".into());
        }
        state.queued = state
            .queued
            .checked_add(1)
            .ok_or("Observation persistence revision exhausted.")?;
        state.pending = Some((state.queued, value));
        wake.notify_all();
        Ok(())
    }
    pub fn error(&self) -> Option<String> {
        self.state
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .error
            .clone()
    }
    /// A bounded fence for shutdown/tests, never called by input/telemetry loops.
    pub fn flush(&self) -> Result<(), String> {
        self.wait(false)
    }
    pub fn shutdown(&self) -> Result<(), String> {
        self.wait(true)
    }
    fn wait(&self, closing: bool) -> Result<(), String> {
        let (lock, wake) = &*self.state;
        let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
        let target = state.queued;
        if closing {
            state.closing = true;
            wake.notify_all();
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while (!state.done && closing) || (!closing && state.committed < target && !state.done) {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(state.error.clone().unwrap_or(
                    "Observation persistence did not finish within its shutdown bound.".into(),
                ));
            };
            state = wake
                .wait_timeout(state, remaining)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
        if state.committed < target {
            return Err(state
                .error
                .clone()
                .unwrap_or("Observation retention was not committed.".into()));
        }
        let done = state.done;
        drop(state);
        if done {
            if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
                let _ = worker.join();
            }
        }
        Ok(())
    }
}
fn notify_error(observer: Option<Arc<dyn Fn() + Send + Sync>>) {
    if let Some(observer) = observer {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer()));
    }
}
impl<T: Serialize + DeserializeOwned + Send + 'static> Drop for ObservationWriter<T> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    fn repository(db: &Database) -> Repository<serde_json::Value> {
        let repository = Repository::new(
            db.clone(),
            "observations",
            "vehicles",
            1024,
            |value: &serde_json::Value| {
                if value.is_object() {
                    Ok(())
                } else {
                    Err("Invalid observation".into())
                }
            },
        )
        .unwrap();
        repository.load().unwrap();
        repository
    }
    #[test]
    fn coalesces_bursts_and_shutdown_commits_the_latest_snapshot() {
        let dir = crate::test_directory();
        let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
        let writer = ObservationWriter::new(repository(&db)).unwrap();
        for n in 0..50 {
            writer.queue(serde_json::json!({"n":n})).unwrap();
        }
        writer.flush().unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &db.read("observations", "vehicles")
                    .unwrap()
                    .unwrap()
                    .payload
            )
            .unwrap()["n"],
            49
        );
        writer.queue(serde_json::json!({"n":50})).unwrap();
        writer.shutdown().unwrap();
        assert!(writer.queue(serde_json::json!({})).is_err());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &db.read("observations", "vehicles")
                    .unwrap()
                    .unwrap()
                    .payload
            )
            .unwrap()["n"],
            50
        );
    }
    #[test]
    fn busy_writes_retain_the_previous_commit_and_retry_without_blocking_the_producer() {
        let dir = crate::test_directory();
        let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
        let writer = ObservationWriter::new(repository(&db)).unwrap();
        writer.queue(serde_json::json!({"n":1})).unwrap();
        writer.flush().unwrap();
        let lock = db.connection().unwrap();
        lock.execute_batch("BEGIN IMMEDIATE").unwrap();
        let start = Instant::now();
        writer.queue(serde_json::json!({"n":2})).unwrap();
        assert!(start.elapsed() < Duration::from_millis(100));
        let deadline = Instant::now() + Duration::from_secs(2);
        while writer.error().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(writer.error().unwrap().contains("busy"));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &db.read("observations", "vehicles")
                    .unwrap()
                    .unwrap()
                    .payload
            )
            .unwrap()["n"],
            1
        );
        lock.execute_batch("ROLLBACK").unwrap();
        writer.flush().unwrap();
        assert!(writer.error().is_none());
    }
    #[test]
    fn invalid_observations_have_explicit_errors_and_shutdown_does_not_spin() {
        let dir = crate::test_directory();
        let db = Database::open(dir.path().join("cockpit.sqlite3")).unwrap();
        let writer = ObservationWriter::new(repository(&db)).unwrap();
        writer.queue(serde_json::json!([])).unwrap();
        assert!(writer
            .shutdown()
            .unwrap_err()
            .contains("Invalid observation"));
        assert!(db.read("observations", "vehicles").unwrap().is_none());
        let idle = ObservationWriter::new(repository(&db)).unwrap();
        idle.shutdown().unwrap();
    }
}

#[cfg(test)]
#[path = "observation_status_tests.rs"]
mod status_tests;
