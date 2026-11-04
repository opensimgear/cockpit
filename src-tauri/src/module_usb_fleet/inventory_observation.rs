use super::Fleet;
use crate::observations;
use osg_streams::{NativeWake, TopicSpec};
use serde_json::{json, Value};
use std::{
    sync::{atomic::{AtomicBool, Ordering}, Arc},
    thread::{self, JoinHandle},
};

const TOPIC: &str = "usb.inventory";

pub(super) struct Observation {
    stop: Arc<AtomicBool>,
    wake: Arc<NativeWake>,
    worker: Option<JoinHandle<()>>,
}

impl Fleet {
    pub(crate) fn observe_inventory(self: &Arc<Self>, runtime: &observations::Runtime) -> Result<(), String> {
        let mut slot = self.inventory_observation.lock().unwrap_or_else(|p| p.into_inner());
        if self.closed.load(Ordering::Acquire) { return Err("USB module fleet has stopped.".into()); }
        let broker = runtime.broker();
        let publisher = broker.register(TopicSpec {
            topic: TOPIC.into(), source: "usb-fleet".into(),
            audiences: vec!["ui".into(), "browser".into(), "mcp".into()],
        }, json!({}), runtime.captured_us(), TOPIC.into()).map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let wake = self.inventory_wake.clone();
        let cancelled = stop.clone();
        let signal = wake.clone();
        let initial = inventory_signature(self);
        let fleet = Arc::downgrade(self);
        let runtime = runtime.clone();
        let active = publisher.clone();
        let worker_broker = broker.clone();
        let worker = thread::Builder::new().name("usb-inventory-observation".into()).spawn(move || {
            let _ = signal.bind_thread(thread::current());
            let mut previous = initial;
            while !cancelled.load(Ordering::Acquire) {
                let observed = signal.observed();
                let Some(fleet) = fleet.upgrade() else { break; };
                let rows = inventory_signature(&fleet);
                drop(fleet);
                if previous != rows {
                    if active.publish_latest_state(active.generation(), json!({}), runtime.captured_us(), TOPIC).is_err() { break; }
                    previous = rows;
                }
                signal.wait_scheduled(observed, None);
            }
            worker_broker.retire(&active);
        }).map_err(|error| {
            broker.retire(&publisher);
            error.to_string()
        })?;
        *slot = Some(Observation { stop, wake, worker: Some(worker) });
        Ok(())
    }
}

fn inventory_signature(fleet: &Fleet) -> Vec<Value> {
    fleet.snapshots().iter().map(|slot| {
        let mut row = slot.summary_value();
        // A timestamp alone must not wake every Devices consumer.
        row["snapshot"].as_object_mut().expect("summary snapshot").remove("observedAtMs");
        row
    }).collect()
}

impl Drop for Observation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.wake.notify();
        if let Some(worker) = self.worker.take() {
            if worker.thread().id() != thread::current().id() { let _ = worker.join(); }
        }
    }
}
