//! Emergency inhibition never acquires automatic settings or storage locks.
use super::{AutomaticChange, AutomaticObserver};
use std::sync::{atomic::{AtomicU64, Ordering}, OnceLock};

pub(crate) struct Emergency {
    state: AtomicU64,
    observer: OnceLock<AutomaticObserver>,
}
impl Emergency {
    pub(crate) fn new(value: bool) -> Self {
        Self { state: AtomicU64::new(u64::from(value)), observer: OnceLock::new() }
    }
    pub(crate) fn load(&self, order: Ordering) -> bool { self.state.load(order) & 1 != 0 }
    pub(crate) fn store(&self, value: bool, _: Ordering) {
        // The revision and flag share one linearization point. A delayed queue
        // sender cannot overwrite a newer emergency notification in the worker.
        let mut old = self.state.load(Ordering::Acquire);
        loop {
            if old & 1 == u64::from(value) { return; }
            let Some(revision) = (old >> 1).checked_add(1).filter(|revision| *revision < (u64::MAX >> 1)) else {
                // Counter exhaustion must remain inhibited.
                self.state.store(u64::MAX, Ordering::Release);
                self.notify(u64::MAX);
                return;
            };
            let next = (revision << 1) | u64::from(value);
            match self.state.compare_exchange_weak(old, next, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => { self.notify(next); return; }
                Err(current) => old = current,
            }
        }
    }
    fn notify(&self, state: u64) {
        if let Some(observer) = self.observer.get() {
            observer(AutomaticChange::Emergency { revision: state >> 1, enabled: state & 1 != 0 });
        }
    }
    pub(crate) fn observe(&self, observer: AutomaticObserver) {
        let _ = self.observer.set(observer);
        self.notify(self.state.load(Ordering::Acquire));
    }
}
