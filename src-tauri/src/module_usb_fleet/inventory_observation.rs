use super::{Fleet, SlotSnapshot};
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
    pub(crate) fn observe_inventory(
        self: &Arc<Self>,
        runtime: &observations::Runtime,
        profile_wake: Option<&Arc<NativeWake>>,
        system_changed: impl Fn() + Send + 'static,
    ) -> Result<(), String> {
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
        let revision = self.inventory_revision.clone();
        let profile_wake = profile_wake.map(Arc::downgrade);
        let initial = self.snapshots();
        let mut previous = inventory_signature(&initial);
        let mut previous_profiles = profile_wake.as_ref().map(|_| profile_signature(&initial));
        let mut previous_stamps = profile_wake.as_ref().map(|_| stamps(&initial));
        let fleet = Arc::downgrade(self);
        let runtime = runtime.clone();
        let active = publisher.clone();
        let worker_broker = broker.clone();
        let worker = thread::Builder::new().name("usb-inventory-observation".into()).spawn(move || {
            let _ = signal.bind_thread(thread::current());
            while !cancelled.load(Ordering::Acquire) {
                let observed = signal.observed();
                let Some(fleet) = fleet.upgrade() else { break; };
                let slots = fleet.snapshots();
                drop(fleet);
                let rows = inventory_signature(&slots);
                if let Some(wake) = profile_wake.as_ref().and_then(|wake| wake.upgrade()) {
                    let profiles = profile_signature(&slots);
                    let current_stamps = stamps(&slots);
                    let now = crate::profile_resource_sources::hid_publication::now_ms();
                    let restored = previous_stamps.as_ref().is_some_and(|prior| {
                        prior.iter().zip(&current_stamps).any(|(&before, &after)| {
                            crate::profile_resource_sources::hid_publication::freshness_restored(before, after, now)
                        })
                    });
                    previous_stamps = Some(current_stamps);
                    if restored || previous != rows || previous_profiles.as_ref() != Some(&profiles) {
                        previous_profiles = Some(profiles);
                        wake.notify();
                    }
                }
                if previous != rows {
                    revision.fetch_add(1, Ordering::AcqRel);
                    system_changed();
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

fn inventory_signature(slots: &[SlotSnapshot]) -> Vec<Value> {
    slots.iter().map(|slot| {
        let mut row = slot.summary_value();
        // A timestamp alone must not wake every Devices consumer.
        row["snapshot"].as_object_mut().expect("summary snapshot").remove("observedAtMs");
        row
    }).collect()
}

fn profile_signature(slots: &[SlotSnapshot]) -> Vec<Value> {
    slots.iter().cloned().map(crate::profile_resource_sources::usb_inventory_entry).collect()
}

fn stamps(slots: &[SlotSnapshot]) -> Vec<Option<u64>> {
    slots.iter().map(|slot| slot.snapshot.observed_at_ms).collect()
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
