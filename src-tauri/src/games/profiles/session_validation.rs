//! Independently scheduled process and catalogue proofs share bounded worker
//! machinery. Their observation ages retain their separate safety horizons.
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc};
use std::time::{Duration, Instant};

const FRESH: Duration = Duration::from_millis(100);
const REFRESH: Duration = Duration::from_millis(25);

#[derive(Clone, Copy)]
struct Policy {
    fresh: Duration,
    refresh: Duration,
}
impl Default for Policy {
    fn default() -> Self { Self { fresh: FRESH, refresh: REFRESH } }
}

struct Worker {
    requests: mpsc::Sender<()>,
    results: mpsc::Receiver<Result<Instant, String>>,
    cancelled: Arc<AtomicBool>,
    pending: bool,
}

#[derive(Default)]
pub(super) struct Validation {
    verified: Option<Instant>,
    worker: Option<Worker>,
    policy: Policy,
}

impl Validation {
    pub(super) fn catalogue() -> Self {
        let mut validation = Self::default();
        validation.policy = Policy { fresh: Duration::from_millis(250),
            refresh: Duration::from_millis(125) };
        validation
    }

    pub(super) fn seed(&mut self, began: Instant) { self.verified = Some(began); }

    pub(super) fn started(&self) -> bool { self.worker.is_some() }

    pub(super) fn start(&mut self, mut validate: impl FnMut() -> Result<Instant, String> + Send + 'static) {
        if self.started() { return; }
        let (requests, incoming) = mpsc::channel();
        let (completed, results) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stopped = cancelled.clone();
        std::thread::spawn(move || {
            while incoming.recv().is_ok() {
                if stopped.load(Ordering::Acquire) { break; }
                let result = validate();
                if stopped.load(Ordering::Acquire) { break; }
                if completed.send(result).is_err() { break; }
            }
        });
        self.worker = Some(Worker { requests, results, cancelled, pending: false });
    }

    #[cfg(test)]
    fn poll(&mut self) -> Result<bool, String> {
        self.poll_at(Instant::now())
    }

    /// Poll every independent proof before deciding availability. A failure in
    /// either proof must be delivered even when the other one is still pending.
    pub(super) fn poll_with(&mut self, other: Option<&mut Self>) -> Result<bool, String> {
        self.poll_with_at(other, Instant::now())
    }

    fn poll_with_at(&mut self, other: Option<&mut Self>, now: Instant) -> Result<bool, String> {
        let first = self.poll_at(now);
        let second = other.map_or(Ok(true), |other| other.poll_at(now));
        let first = first?;
        let second = second?;
        Ok(first && second)
    }

    fn poll_at(&mut self, now: Instant) -> Result<bool, String> {
        let Some(worker) = self.worker.as_mut() else { return Ok(false); };
        match worker.results.try_recv() {
            Ok(result) => {
                worker.pending = false;
                self.verified = Some(result?);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => return Err("The native game validation worker stopped.".into()),
        }
        if !worker.pending && self.verified.is_none_or(|began| now.saturating_duration_since(began) >= self.policy.refresh) {
            worker.requests.send(()).map_err(|_| "The native game validation worker stopped.".to_owned())?;
            worker.pending = true;
        }
        Ok(self.verified.is_some_and(|began| now.saturating_duration_since(began) < self.policy.fresh))
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
