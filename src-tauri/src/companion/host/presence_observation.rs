use super::Runtime;
use crate::observations;
use osg_companion_service::Hub;
use osg_streams::{NativeWake, TopicSpec};
use serde_json::json;
use std::{
    sync::{atomic::{AtomicBool, Ordering}, Arc},
    thread::{self, JoinHandle},
};

const TOPIC: &str = "companion.presence";

pub(super) struct Observation {
    wake: Arc<NativeWake>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Runtime {
    pub(crate) fn observe_presence(&self, runtime: &observations::Runtime) -> Result<(), String> {
        let mut current = self.presence_observation.lock().unwrap_or_else(|p| p.into_inner());
        let broker = runtime.broker();
        let publisher = broker.register(TopicSpec {
            topic: TOPIC.into(),
            source: "companion-host".into(),
            audiences: vec!["ui".into(), "browser".into(), "mcp".into()],
        }, json!({}), runtime.captured_us(), TOPIC.into()).map_err(|error| error.to_string())?;
        let wake = Arc::new(NativeWake::default());
        let stop = Arc::new(AtomicBool::new(false));
        let runtime = runtime.clone();
        let signal = wake.clone();
        let cancelled = stop.clone();
        let active = publisher.clone();
        let worker_broker = broker.clone();
        let worker = thread::Builder::new().name("companion-presence".into()).spawn(move || {
            let _ = signal.bind_thread(thread::current());
            let mut delivered = 0;
            while !cancelled.load(Ordering::Acquire) {
                let observed = signal.observed();
                if delivered != observed {
                    if active.publish_latest_state(active.generation(), json!({}), runtime.captured_us(), TOPIC).is_err() {
                        break;
                    }
                    delivered = observed;
                }
                signal.wait_scheduled(observed, None);
            }
            worker_broker.retire(&active);
        }).map_err(|error| {
            broker.retire(&publisher);
            error.to_string()
        })?;
        if let Some(metadata) = self.metadata.get() {
            metadata.hub.observe_presence(wake.clone());
        }
        *current = Some(Observation { wake, stop, worker: Some(worker) });
        Ok(())
    }

    pub(super) fn register_presence_wake(&self, hub: &Hub) {
        let observation = self.presence_observation.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(observation) = observation.as_ref() {
            hub.observe_presence(observation.wake.clone());
        }
    }
}

impl Drop for Observation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.wake.notify();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
