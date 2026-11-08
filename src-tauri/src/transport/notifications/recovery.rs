//! Shared recovery signal for operations that depend on serial topology.
use osg_streams::NativeWake;
use crate::transport::PortInfo;
use std::{
    sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex, Weak},
    time::Instant,
};

pub(crate) type SerialPorts = Arc<dyn Fn() -> Result<Vec<PortInfo>, String> + Send + Sync>;

#[derive(Clone)]
pub(crate) struct RecoverySource {
    pub(crate) ports: SerialPorts,
    pub(crate) events: Arc<PortRecovery>,
}

impl RecoverySource {
    pub(crate) fn watch(&self, worker: &Arc<NativeWake>) -> RecoveryWatch {
        let child = self.events.subscribe(worker);
        let observed = child.observed();
        RecoveryWatch { source: self.clone(), child, observed }
    }
}

pub(crate) struct RecoveryWatch {
    source: RecoverySource,
    child: Arc<NativeWake>,
    observed: u64,
}

impl RecoveryWatch {
    /// Return cached topology only when a new invalidation has been covered
    /// by the workspace monitor. Always advance the epoch baseline.
    pub(crate) fn cached_ports(&mut self) -> Option<Vec<PortInfo>> {
        let observed = self.child.observed();
        let changed = observed != self.observed;
        self.observed = observed;
        if changed && self.covered() { (self.source.ports)().ok() } else { None }
    }

    pub(crate) fn covered(&self) -> bool {
        self.source.events.covered()
    }

    /// Wait until the sampled topology epoch changes or the recovery window
    /// expires. Cancellation is reported separately so callers can preserve
    /// their own cancellation error.
    pub(crate) fn wait_for_change(
        &self,
        worker: &NativeWake,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> bool {
        loop {
            let wake = worker.observed();
            if cancel.load(Ordering::Acquire) { return false; }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if self.child.observed() != self.observed || remaining.is_zero() { return true; }
            worker.wait(wake, remaining);
        }
    }
}

#[derive(Default)]
struct State {
    covered: bool,
    subscribers: Vec<Weak<NativeWake>>,
}

/// Relays native topology invalidations from the workspace's single watcher
/// to operations that need to recover a serial connection.
#[derive(Default)]
pub(crate) struct PortRecovery {
    state: Mutex<State>,
}

impl PortRecovery {
    /// Register an operation wake without keeping the operation alive.
    pub(crate) fn subscribe(&self, worker: &Arc<NativeWake>) -> Arc<NativeWake> {
        let child = Arc::new(NativeWake::child(worker.clone()));
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.subscribers.retain(|subscriber| subscriber.strong_count() != 0);
        state.subscribers.push(Arc::downgrade(&child));
        child
    }

    pub(crate) fn covered(&self) -> bool {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).covered
    }

    /// Publish monitor coverage and any observed raw topology invalidation.
    /// A raw invalidation wakes subscribers even when coverage and cached rows
    /// have not changed.
    pub(crate) fn report(&self, covered: bool, invalidated: bool) {
        let wakes = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let coverage_changed = state.covered != covered;
            state.covered = covered;
            state.subscribers.retain(|subscriber| subscriber.strong_count() != 0);
            if coverage_changed || invalidated {
                state.subscribers.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        for wake in wakes {
            wake.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_and_raw_invalidations_wake_active_operations() {
        let recovery = PortRecovery::default();
        let worker = Arc::new(NativeWake::default());
        let child = recovery.subscribe(&worker);
        assert!(!recovery.covered());

        recovery.report(true, false);
        assert!(recovery.covered());
        assert_eq!(child.observed(), 1);
        assert_eq!(worker.observed(), 1);

        recovery.report(true, false);
        assert_eq!(child.observed(), 1, "unchanged coverage is quiet");

        recovery.report(true, true);
        assert_eq!(child.observed(), 2, "raw invalidation wakes even with unchanged coverage");
        assert_eq!(worker.observed(), 2);

        recovery.report(false, false);
        assert!(!recovery.covered());
        assert_eq!(child.observed(), 3);
    }

    #[test]
    fn subscribers_are_weak_and_dead_entries_are_pruned() {
        let recovery = PortRecovery::default();
        let worker = Arc::new(NativeWake::default());
        let child = recovery.subscribe(&worker);
        drop(child);
        {
            let state = recovery.state.lock().unwrap();
            assert_eq!(state.subscribers.len(), 1);
            assert!(state.subscribers[0].upgrade().is_none());
        }

        let replacement = recovery.subscribe(&worker);
        let state = recovery.state.lock().unwrap();
        assert_eq!(state.subscribers.len(), 1);
        assert!(state.subscribers[0].upgrade().is_some());
        drop(state);
        recovery.report(false, true);
        assert_eq!(replacement.observed(), 1);
    }
}
