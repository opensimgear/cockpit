//! Coalescing notification for one native worker. Carries no stream payloads.
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock,
    },
    thread::Thread,
    time::Duration,
};

/// A shared notification epoch and one permanently bound worker thread.
///
/// Observe before draining sources, then wait using that observation. A changed
/// epoch requires another drain even if an unrelated park consumed the token.
#[derive(Default)]
pub struct NativeWake {
    thread: OnceLock<Thread>,
    epoch: AtomicU64,
    parent: Option<Arc<NativeWake>>,
    signal: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl NativeWake {
    /// Also signal the worker's native deadline wait. The bound thread still
    /// receives a token so it can fall back to the portable wait on error.
    pub fn with_signal(signal: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { signal: Some(signal), ..Self::default() }
    }

    /// Track one source's progress while also waking its shared worker.
    pub fn child(parent: Arc<Self>) -> Self {
        Self { parent: Some(parent), ..Self::default() }
    }

    pub fn bind_thread(&self, thread: Thread) -> Result<(), Thread> {
        self.thread.set(thread)?;
        // A notification can precede binding after the worker's epoch check.
        // Leave a token only when work was notified. A concurrent notify after
        // binding sees the thread itself and supplies the token directly.
        if self.observed() > 0 {
            self.thread.get().expect("thread was just bound").unpark();
        }
        Ok(())
    }

    /// Advances an atomic epoch and signals the bound worker. Never acquires
    /// a journal lock or clones a payload.
    pub fn notify(&self) {
        // Saturation permanently disables parking rather than wrapping to an
        // already observed epoch and potentially losing a committed change.
        let _ = self
            .epoch
            .fetch_update(Ordering::Release, Ordering::Relaxed, |epoch| {
                epoch.checked_add(1)
            });
        if let Some(thread) = self.thread.get() {
            thread.unpark();
        }
        if let Some(signal) = &self.signal {
            signal();
        }
        if let Some(parent) = &self.parent {
            parent.notify();
        }
    }

    pub fn observed(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    /// Returns false when the caller must drain again without parking. True
    /// means a park was attempted; it may return spuriously or on timeout.
    pub fn wait(&self, observed: u64, timeout: Duration) -> bool {
        self.wait_scheduled(observed, Some(timeout))
    }

    /// Call only on the bound worker, after draining with the given observation.
    /// `None` waits without a timer; a notification racing the check leaves an
    /// unpark token so the subsequent park returns immediately.
    pub fn wait_scheduled(&self, observed: u64, timeout: Option<Duration>) -> bool {
        let current = self.observed();
        if current != observed || current == u64::MAX {
            return false;
        }
        match timeout {
            Some(timeout) => std::thread::park_timeout(timeout),
            None => std::thread::park(),
        }
        true
    }
}

#[cfg(test)]
#[path = "wake_tests.rs"]
mod tests;
