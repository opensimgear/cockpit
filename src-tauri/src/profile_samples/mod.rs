//! Lazy value-only views. Only the worker encodes or publishes; routing signals
//! value changes without inspecting subscribers or waiting for a consumer.
mod registration;
mod worker;
#[cfg(test)]
mod tests;

use crate::profile_session::{inspection::samples::{CapturedSamples, NodePlan}, RunOwner};
use osg_streams::{Broker, NativeWake, Publisher};
use std::{collections::BTreeMap, sync::{Arc, Condvar, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering}}, thread::JoinHandle};

pub(crate) const MAX_TOPICS: usize = 64;
pub(crate) const MAX_PORTS: usize = 1024;
type Capture = dyn Fn(&[Arc<NodePlan>]) -> Vec<Result<CapturedSamples, String>> + Send + Sync;

struct Slot {
    id: u64,
    plan: Arc<NodePlan>,
    pins: usize,
    ready: Option<Result<(), String>>,
    publisher: Option<Publisher>,
    last: Option<Arc<CapturedSamples>>,
}
#[derive(Default)]
struct Registry { slots: BTreeMap<String, Slot>, next_id: u64 }
struct Inner {
    registry: Mutex<Registry>,
    changed: Condvar,
    wake: Arc<NativeWake>,
    interested: AtomicUsize,
    closed: AtomicBool,
    broker: OnceLock<Broker>,
}
impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(|error| error.into_inner())
    }
    fn current(&self, topic: &str, id: u64) -> bool {
        !self.closed.load(Ordering::Acquire) && self.lock().slots.get(topic)
            .is_some_and(|slot| slot.id == id && !matches!(slot.ready, Some(Err(_))))
    }
    fn retire(&self, publishers: Vec<Publisher>) {
        for publisher in publishers {
            if !self.broker.get().is_some_and(|broker| broker.retire(&publisher)) {
                publisher.close();
            }
        }
    }
}

/// The live data path performs only atomic notification. Lifecycle retirement
/// additionally closes exact passive publishers, without encoding or callbacks.
#[derive(Clone)]
pub(crate) struct Signal { inner: Arc<Inner> }
impl Default for Signal {
    fn default() -> Self {
        Self { inner: Arc::new(Inner { registry: Mutex::new(Registry::default()),
            changed: Condvar::new(), wake: Arc::new(NativeWake::default()),
            interested: AtomicUsize::new(0), closed: AtomicBool::new(false), broker: OnceLock::new() }) }
    }
}
impl Signal {
    pub(crate) fn notify(&self) {
        if self.inner.interested.load(Ordering::Acquire) > 0
            && !self.inner.closed.load(Ordering::Acquire) { self.inner.wake.notify(); }
    }
    pub(crate) fn retire(&self, owner: &RunOwner) {
        let publishers = {
            let mut state = self.inner.lock();
            let topics = state.slots.iter().filter(|(_, slot)| slot.plan.owner == *owner)
                .map(|(topic, _)| topic.clone()).collect::<Vec<_>>();
            let publishers = topics.into_iter().filter_map(|topic| state.slots.remove(&topic)
                .and_then(|slot| slot.publisher)).collect();
            self.inner.interested.store(state.slots.len(), Ordering::Release);
            publishers
        };
        self.inner.changed.notify_all();
        self.inner.retire(publishers);
        self.inner.wake.notify();
    }
}

pub(crate) struct Runtime { signal: Signal, worker: Mutex<Option<JoinHandle<()>>> }
impl Runtime {
    pub(crate) fn new(broker: Broker, signal: Signal, capture: Arc<Capture>, clock: String) -> Self {
        let _ = signal.inner.broker.set(broker.clone());
        let inner = signal.inner.clone();
        let worker = std::thread::Builder::new().name("profile-samples".into())
            .spawn(move || worker::run(inner, broker, capture, clock));
        let handle = match worker {
            Ok(worker) => {
                let _ = signal.inner.wake.bind_thread(worker.thread().clone());
                Some(worker)
            }
            Err(_) => { signal.inner.closed.store(true, Ordering::Release); None }
        };
        Self { signal, worker: Mutex::new(handle) }
    }
    pub(crate) fn readers_changed(&self) { self.signal.notify(); }
    pub(crate) fn reader_wake(&self) -> Arc<NativeWake> { self.signal.inner.wake.clone() }
    pub(crate) fn shutdown(&self) {
        self.signal.inner.closed.store(true, Ordering::Release);
        let publishers = {
            let mut state = self.signal.inner.lock();
            self.signal.inner.interested.store(0, Ordering::Release);
            std::mem::take(&mut state.slots).into_values().filter_map(|slot| slot.publisher).collect()
        };
        self.signal.inner.changed.notify_all();
        self.signal.inner.retire(publishers);
        self.signal.inner.wake.notify();
        if let Some(worker) = self.worker.lock().unwrap_or_else(|error| error.into_inner()).take() {
            if worker.thread().id() != std::thread::current().id() { let _ = worker.join(); }
        }
    }
}
impl Drop for Runtime { fn drop(&mut self) { self.shutdown(); } }
