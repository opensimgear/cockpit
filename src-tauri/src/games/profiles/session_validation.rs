//! One read-only validator per reader. A result expires from the beginning of
//! its work, so a slow successful validation cannot manufacture fresh evidence.
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc};
use std::time::{Duration, Instant};

const FRESH: Duration = Duration::from_millis(100);

struct Worker {
    requests: mpsc::Sender<Instant>,
    results: mpsc::Receiver<(Instant, Result<(), String>)>,
    cancelled: Arc<AtomicBool>,
    pending: bool,
}

#[derive(Default)]
pub(super) struct Validation {
    verified: Option<Instant>,
    worker: Option<Worker>,
}

impl Validation {
    pub(super) fn seed(&mut self, began: Instant) { self.verified = Some(began); }

    pub(super) fn started(&self) -> bool { self.worker.is_some() }

    pub(super) fn start(&mut self, mut validate: impl FnMut() -> Result<(), String> + Send + 'static) {
        if self.started() { return; }
        let (requests, incoming) = mpsc::channel();
        let (completed, results) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stopped = cancelled.clone();
        std::thread::spawn(move || {
            while let Ok(began) = incoming.recv() {
                if stopped.load(Ordering::Acquire) { break; }
                let result = validate();
                if stopped.load(Ordering::Acquire) { break; }
                if completed.send((began, result)).is_err() { break; }
            }
        });
        self.worker = Some(Worker { requests, results, cancelled, pending: false });
    }

    pub(super) fn poll(&mut self) -> Result<bool, String> {
        let now = Instant::now();
        let Some(worker) = self.worker.as_mut() else { return Ok(false); };
        match worker.results.try_recv() {
            Ok((began, result)) => {
                worker.pending = false;
                result?;
                self.verified = Some(began);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => return Err("The native game validation worker stopped.".into()),
        }
        if !worker.pending {
            worker.requests.send(now).map_err(|_| "The native game validation worker stopped.".to_owned())?;
            worker.pending = true;
        }
        Ok(self.verified.is_some_and(|began| now.saturating_duration_since(began) < FRESH))
    }

    pub(super) fn cancel(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancelled.store(true, Ordering::Release);
        }
        self.verified = None;
    }
}

impl Drop for Validation {
    fn drop(&mut self) { self.cancel(); }
}

#[cfg(test)]
#[path = "session_validation_tests.rs"]
mod tests;
