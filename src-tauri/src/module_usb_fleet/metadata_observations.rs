//! Passive latest-only projection of the shared service metadata cache.
use super::*;
use osg_streams::{Broker, NativeWake, Publisher, TopicSpec};
use sha2::{Digest, Sha256};
use std::{panic::{catch_unwind, AssertUnwindSafe}, thread::{self, JoinHandle}};

const MAX_METADATA_BRIDGES: usize = 4;
const CLOCK: &str = "usb-metadata";

pub(crate) fn topic(key: &str) -> String {
    format!("usb.metadata.{:x}", Sha256::digest(key.as_bytes()))
}

pub(super) struct Entry {
    bridge: Arc<Bridge>,
    worker: Option<JoinHandle<()>>,
}
impl Entry {
    pub(super) fn stop(&mut self) {
        self.bridge.stop.store(true, Ordering::Release);
        self.bridge.wake.notify();
        if let Some(worker) = self.worker.take() {
            if worker.thread().id() != thread::current().id() { let _ = worker.join(); }
        }
        self.bridge.broker.retire(&self.bridge.publisher);
    }
    #[cfg(test)]
    pub(super) fn idle_probe(&self) -> std::sync::mpsc::Receiver<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        *self.bridge.probe.lock().unwrap_or_else(|p|p.into_inner()) = Some(tx);
        self.bridge.wake.notify();
        rx
    }
}
struct Bridge {
    key: String,
    wake: Arc<NativeWake>,
    stop: AtomicBool,
    armed: AtomicBool,
    running: AtomicBool,
    admission: Mutex<()>,
    publisher: Publisher,
    broker: Broker,
    #[cfg(test)]
    probe: Mutex<Option<std::sync::mpsc::Sender<()>>>,
}
// A panicking subscriber callback must still permit last-reader retirement.
struct AdmissionCleanup(Arc<Bridge>);
impl Drop for AdmissionCleanup {
    fn drop(&mut self) {
        self.0.armed.store(true,Ordering::Release);
        self.0.wake.notify();
    }
}
struct Cached {
    service: Option<Arc<dyn Endpoint>>,
    route: Option<UsbRouteToken>,
    connected: bool,
    payload: Value,
    at: u64,
}
impl Cached {
    fn same_source(&self, other: &Self) -> bool {
        let service = match (&self.service, &other.service) {
            (Some(a), Some(b)) => Arc::ptr_eq(a,b),
            (None, None) => true,
            _ => false,
        };
        let route = match (&self.route, &other.route) {
            (Some(a),Some(b)) => a.connection_generation == b.connection_generation
                && a.boot == b.boot && a.module_id == b.module_id && a.serial == b.serial
                && a.configuration_generation == b.configuration_generation,
            (None,None) => true,
            _ => false,
        };
        service && route && self.connected == other.connected
    }
}

impl Fleet {
    /// Admission pins registration and the baseline until the reader exists.
    pub(crate) fn with_metadata_observation<T>(
        self: &Arc<Self>, topic_name: &str, broker: Broker,
        before_subscribe: impl FnOnce(Arc<NativeWake>),
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let key = self.known_metadata_key(topic_name)?;
        let mut before = Some(before_subscribe);
        let mut action = Some(action);
        loop {
            let bridge = self.metadata_bridge(&key,broker.clone())?;
            let admission = bridge.admission.lock().unwrap_or_else(|p|p.into_inner());
            if bridge.stop.load(Ordering::Acquire) {
                drop(admission); self.retire_metadata_bridge(&key,&bridge); continue;
            }
            let cleanup=AdmissionCleanup(bridge.clone());
            before.take().expect("metadata admission callback")(bridge.wake.clone());
            let result = action.take().expect("metadata subscribe action")();
            let retire = !bridge.publisher.has_readers();
            if retire { bridge.stop.store(true,Ordering::Release); }
            bridge.armed.store(true,Ordering::Release);
            bridge.wake.notify(); drop(cleanup); drop(admission);
            if retire { self.retire_metadata_bridge(&key,&bridge); }
            return result;
        }
    }
    fn known_metadata_key(&self, requested: &str) -> Result<String,String> {
        let suffix = requested.strip_prefix("usb.metadata.").ok_or("Invalid USB metadata topic.")?;
        if suffix.len()!=64 || !suffix.bytes().all(|c|c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
            return Err("Invalid USB metadata topic hash.".into());
        }
        self.state.lock().unwrap_or_else(|p|p.into_inner()).slots.keys()
            .find(|key|topic(key)==requested).cloned().ok_or_else(||"Unknown USB metadata endpoint topic.".into())
    }
    fn metadata_cached(&self,key:&str) -> Result<Cached,String> {
        let state = self.state.lock().unwrap_or_else(|p|p.into_inner());
        let slot = state.slots.get(key).ok_or("Unknown USB metadata endpoint.")?;
        let current = snapshot_projection(key,slot,state.diagnostic.as_deref(),true).snapshot;
        let at = current.observed_at_ms.unwrap_or(0).saturating_mul(1000);
        let payload = json!({"key":key,"connected":current.connected,"port":current.port,
            "identity":current.identity,"route":current.route,"config":current.config,
            "observedAtMs":current.observed_at_ms,"error":current.error});
        Ok(Cached { service:slot.service.clone(),route:current.route,connected:current.connected,payload,at })
    }
    fn metadata_bridge(self:&Arc<Self>,key:&str,broker:Broker) -> Result<Arc<Bridge>,String> {
        let mut bridges = self.metadata_bridges.lock().unwrap_or_else(|p|p.into_inner());
        if self.closed.load(Ordering::Acquire) { return Err("USB metadata fleet is closed.".into()); }
        if let Some(entry) = bridges.get(key) {
            if entry.bridge.running.load(Ordering::Acquire) && !entry.bridge.stop.load(Ordering::Acquire) {
                return Ok(entry.bridge.clone());
            }
        }
        if let Some(mut entry) = bridges.remove(key) {
            drop(bridges); entry.stop(); return self.metadata_bridge(key,broker);
        }
        if bridges.len()>=MAX_METADATA_BRIDGES { return Err("USB metadata observation bridge limit reached.".into()); }
        let wake = Arc::new(NativeWake::default());
        self.watch_inputs(key,wake.clone())?;
        let initial = self.metadata_cached(key)?;
        let publisher = broker.register(TopicSpec { topic:topic(key),source:CLOCK.into(),
            audiences:vec!["ui".into(),"browser".into(),"mcp".into()] },
            initial.payload.clone(),initial.at,CLOCK.into()).map_err(|e|e.to_string())?;
        let bridge = Arc::new(Bridge { key:key.into(),wake,stop:AtomicBool::new(false),
            armed:AtomicBool::new(false),running:AtomicBool::new(true),admission:Mutex::new(()),
            publisher,broker,
            #[cfg(test)]
            probe:Mutex::new(None),
        });
        let weak = Arc::downgrade(self); let cleanup = weak.clone(); let worker_bridge = bridge.clone();
        let worker = thread::Builder::new().name("usb-metadata-observation".into()).spawn(move|| {
            let _ = catch_unwind(AssertUnwindSafe(||run(weak,worker_bridge.clone(),initial)));
            worker_bridge.stop.store(true,Ordering::Release);
            worker_bridge.broker.retire(&worker_bridge.publisher);
            worker_bridge.running.store(false,Ordering::Release);
            if let Some(fleet)=cleanup.upgrade() { fleet.retire_metadata_bridge(&worker_bridge.key,&worker_bridge); }
        }).map_err(|error| {
            bridge.broker.retire(&bridge.publisher);
            format!("Cannot start USB metadata observation bridge: {error}")
        })?;
        bridges.insert(key.into(),Entry { bridge:bridge.clone(),worker:Some(worker) });
        Ok(bridge)
    }
    fn retire_metadata_bridge(&self,key:&str,bridge:&Arc<Bridge>) {
        let entry = {
            let mut bridges = self.metadata_bridges.lock().unwrap_or_else(|p|p.into_inner());
            if bridges.get(key).is_some_and(|entry|Arc::ptr_eq(&entry.bridge,bridge)) { bridges.remove(key) } else { None }
        };
        if let Some(mut entry)=entry { entry.stop(); }
    }
}

fn run(fleet:Weak<Fleet>,bridge:Arc<Bridge>,mut previous:Cached) {
    let _ = bridge.wake.bind_thread(thread::current());
    let mut generation = 1u64;
    loop {
        let observed = bridge.wake.observed();
        if bridge.stop.load(Ordering::Acquire) { break; }
        let Some(fleet)=fleet.upgrade() else { break; };
        if fleet.closed.load(Ordering::Acquire) { break; }
        let Ok(mut current)=fleet.metadata_cached(&bridge.key) else { break; };
        if !previous.same_source(&current) {
            if fleet.watch_inputs(&bridge.key,bridge.wake.clone()).is_err(){break;}
            let Ok(registered)=fleet.metadata_cached(&bridge.key)else{break;};
            current=registered;
            let Some(next)=generation.checked_add(1) else { break; };
            if bridge.publisher.replace_generation(next,current.payload.clone(),current.at,CLOCK).is_err() { break; }
            generation=next;
        } else if current.payload!=previous.payload
            && bridge.publisher.publish_latest_state(generation,current.payload.clone(),current.at,CLOCK).is_err() { break; }
        previous=current;
        drop(fleet);
        let admission=bridge.admission.lock().unwrap_or_else(|p|p.into_inner());
        let retire=bridge.armed.load(Ordering::Acquire)&&!bridge.publisher.has_readers();
        if retire { bridge.stop.store(true,Ordering::Release); }
        drop(admission);
        if retire { break; }
        #[cfg(test)]
        if let Some(probe)=bridge.probe.lock().unwrap_or_else(|p|p.into_inner()).take() { let _=probe.send(()); }
        bridge.wake.wait_scheduled(observed,None);
    }
}
