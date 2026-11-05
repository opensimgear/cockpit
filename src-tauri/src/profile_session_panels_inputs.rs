//! Bounded profile-owned readers use the existing USB/CAN workers. Host
//! polling only drains typed frames; revocation never waits for serial I/O.
use crate::{
    gateways::ProfilePanelIo,
    module_usb::{UsbRequest, UsbRouteToken, UsbSnapshot},
    module_usb_fleet::SlotSnapshot,
};
use osg_mapping::{
    engine::{
        typed::{Change, Frame},
        Counter, Reason,
    },
    program::SignalSample,
};
use osg_profiles::{
    compiler_models::{ExecutionPlan, PlanReadiness},
    resources::{ResourceToken, Route, Transport},
    resources_ports::PortContract,
    PortValue,
};
use serde_json::{json, Value};
use osg_streams::NativeWake;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, Weak,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
mod target;
mod source;
mod preparation;
#[cfg(test)]
mod can;
mod can_stream;
mod usb;
mod usb_source;
mod admission;
mod control;
const MAX_QUEUE: usize = 512;
#[cfg(test)]
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
struct ReadPermit;
#[cfg(test)]
impl ReadPermit {
    fn acquire() -> Option<Self> {
        IN_FLIGHT
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 16).then_some(n + 1)
            })
            .ok()
            .map(|_| Self)
    }
}
#[cfg(test)]
impl Drop for ReadPermit {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}
trait Io: Send + Sync {
    #[cfg(test)]
    fn uses_polled_test_fixture(&self) -> bool { false }
    fn can_capture(&self, _gateway: &str, _module: &str) -> Result<Arc<crate::stm32_panel_capture::Capture>, String> {
        Err("This gateway has no native panel stream.".into())
    }
    #[cfg(test)]
    fn can(&self, request: Value) -> Value;
    fn can_cached(&self, gateway: &str) -> Result<Value, String>;
    fn usb(&self, key: &str, request: UsbRequest) -> Result<UsbSnapshot, String>;
    fn usb_capture(&self, _key: &str) -> Result<Arc<crate::module_usb::stream::Capture>, String> {
        Err("The USB endpoint has no native input capture.".into())
    }
    fn snapshots(&self) -> Vec<SlotSnapshot>;
}
impl Io for ProfilePanelIo {
    fn can_capture(&self, gateway: &str, module: &str) -> Result<Arc<crate::stm32_panel_capture::Capture>, String> {
        ProfilePanelIo::can_capture(self, gateway, module)
    }
    #[cfg(test)]
    fn can(&self, r: Value) -> Value {
        self.can(r)
    }
    fn can_cached(&self, gateway: &str) -> Result<Value, String> {
        self.can_snapshot(gateway)
    }
    fn usb(&self, key: &str, r: UsbRequest) -> Result<UsbSnapshot, String> {
        self.usb(key, r)
    }
    fn snapshots(&self) -> Vec<SlotSnapshot> {
        self.usb_snapshots()
    }
    fn usb_capture(&self, key: &str) -> Result<Arc<crate::module_usb::stream::Capture>, String> {
        ProfilePanelIo::usb_capture(self, key)
    }
}
#[derive(Clone, Debug, PartialEq)]
enum Target {
    Usb {
        key: String,
        route: UsbRouteToken,
    },
    Can {
        gateway: String,
        module: String,
        epoch: Value,
        node: Value,
        lease: Value,
        boot: Value,
        configuration: Value,
        stream: Value,
    },
}

#[derive(Clone)]
struct Port {
    logical: String,
    observed: String,
    contract: PortContract,
    resource_id: String,
    contract_hash: String,
}
#[derive(Clone)]
struct Spec {
    logical: String,
    target: Target,
    ports: Vec<Port>,
}
#[derive(Clone)]
struct Source {
    spec: Spec,
    cursor: Option<(u32, u32)>,
    held: Vec<bool>,
    sequence: u64,
    at: u64,
    retry: Instant,
    generation: u64,
    sampled_at: Option<u32>,
}

#[derive(Default)]
struct Mailbox {
    frames: VecDeque<Frame>,
    baselines: BTreeMap<String, Frame>,
    diagnostics: BTreeMap<String, String>,
    blocked: BTreeSet<String>,
    loss_epoch: u64,
    liveness: BTreeMap<String,(u64,u64,u64)>,
    wake: Option<Arc<NativeWake>>,
    baseline_wake: Weak<NativeWake>,
    controls: Vec<Weak<NativeWake>>,
}
#[derive(Default)]
pub(crate) struct PanelPoll {
    pub frames: Vec<Frame>,
    pub diagnostics: Vec<String>,
    pub liveness: Vec<(String,Counter,Vec<String>)>,
}
pub(crate) struct PanelInputs {
    specs: Vec<Spec>,
    io: Arc<dyn Io>,
    mailbox: Arc<Mutex<Mailbox>>,
    revoked: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    admitted: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
    tokens: BTreeMap<String, Vec<ResourceToken>>,
    accepted: BTreeMap<String, (u64, u64, u64)>,
    loss_epoch: u64,
    clock: Option<(Instant, u64)>,
    bound: bool,
}
impl PanelInputs {
    pub(crate) fn watch_baseline(&self, wake: &Arc<NativeWake>) {
        self.mailbox.lock().unwrap_or_else(|p| p.into_inner()).baseline_wake = Arc::downgrade(wake);
    }

    pub(crate) fn set_wake(&self, wake: Option<Arc<NativeWake>>) {
        let mut state = self.mailbox.lock().unwrap_or_else(|p| p.into_inner());
        state.wake = wake;
        state.notify();
    }
    pub(crate) fn capture_receipts(&self) -> Vec<crate::profile_session::Receipt> {
        vec![]
    }
    pub(crate) fn acquire(&mut self, clock_origin_us: u64) -> Result<(), String> {
        if self.revoked.load(Ordering::Acquire) || self.clock.is_some() {
            return Err(
                "This panel reader was already acquired or revoked; prepare a fresh owner.".into(),
            );
        }
        let started = Instant::now();
        self.clock = Some((started, clock_origin_us));
        for spec in self.specs.clone() {
            let io = self.io.clone();
            let mailbox = self.mailbox.clone();
            let revoked = self.revoked.clone();
            let active = self.active.clone();
            let admitted = self.admitted.clone();
            let wake = Arc::new(NativeWake::default());
            self.mailbox.lock().unwrap_or_else(|p| p.into_inner()).controls.push(Arc::downgrade(&wake));
            let worker = thread::Builder::new()
                .name("profile-panel-inputs".into())
                .spawn(move || {
                    if matches!(&spec.target, Target::Usb { .. }) {
                        usb::run(spec, io, mailbox, revoked, active, admitted, started, clock_origin_us, wake);
                    } else {
                        #[cfg(test)]
                        if io.uses_polled_test_fixture() {
                            return can::run(spec, io, mailbox, revoked, active, admitted, started, clock_origin_us);
                        }
                        can_stream::run(spec, io, mailbox, revoked, active, admitted, started, clock_origin_us, wake);
                    }
                })
                .map_err(|_| "Cannot start a bounded native panel reader.")?;
            self.workers.push(worker);
        }
        Ok(())
    }
    pub(crate) fn wait_baseline(
        &mut self,
        delivery: &ExecutionPlan,
        timeout: Duration,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
        cancelled: impl Fn() -> bool,
    ) -> Result<Vec<Frame>, String> {
        let deadline = Instant::now() + timeout.min(Duration::from_secs(3));
        loop {
            if cancelled() {
                return Err("Native panel baseline acquisition was cancelled.".into());
            }
            if let Some(frames) = self.baseline(delivery, &validate)? {
                return Ok(frames);
            }
            if Instant::now() >= deadline {
                return Err(
                    "Waiting for actual native panel baselines; no retained events were replayed."
                        .into(),
                );
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
    pub(crate) fn start(&mut self) -> Result<(), String> {
        self.activate(true)
    }
    pub(crate) fn begin_live_baseline(&mut self) -> Result<(), String> {
        self.activate(false)
    }
    fn activate(&mut self, admitted: bool) -> Result<(), String> {
        if !self.bound
            || self.revoked.load(Ordering::Acquire)
            || self.active.load(Ordering::Acquire)
        {
            return Err("Admit real panel baselines before starting this reader.".into());
        }
        // Publish the activation fence under the same lock as worker results.
        // The Host must consume a new live baseline before exposing outputs.
        let mut state = self.mailbox.lock().unwrap_or_else(|p| p.into_inner());
        state.frames.clear();
        state.baselines.clear();
        state.liveness.clear();
        self.admitted.store(admitted, Ordering::Release);
        self.active.store(true, Ordering::Release);
        state.control();
        Ok(())
    }
    pub(crate) fn poll(
        &mut self,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
    ) -> Result<PanelPoll, String> {
        if self.revoked.load(Ordering::Acquire) {
            return Ok(PanelPoll::default());
        }
        if !self.bound {
            return Err("Admit native panel baselines before polling.".into());
        }
        let at = self
            .clock
            .map(|(start, origin)| {
                origin
                    .saturating_add(u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX))
            })
            .unwrap_or(0);
        let mut result = PanelPoll::default();
        let mut state = self.mailbox.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(logical) = state.frames.iter().find(|frame| {
            frame.reason != Reason::Gap && at.saturating_sub(frame.captured_us.0) > 250_000
        }).map(|frame| frame.source.clone()) {
            state.loss(&logical, "Panel input expired before Host admission; stale transitions were discarded.")?;
        }
        let mut gapped = BTreeSet::new();
        if state.loss_epoch != self.loss_epoch {
            self.loss_epoch = state.loss_epoch;
            for spec in &self.specs {
                let (generation, sequence, previous) = self
                    .accepted
                    .get(&spec.logical)
                    .copied()
                    .unwrap_or((1, 0, 0));
                gapped.insert(spec.logical.clone());
                result.frames.push(Frame {
                    source: spec.logical.clone(),
                    generation: Counter(generation),
                    sequence: Counter(
                        sequence
                            .checked_add(1)
                            .ok_or("Panel admission sequence exhausted.")?,
                    ),
                    captured_us: Counter(previous),
                    reason: Reason::Gap,
                    changes: vec![],
                });
            }
        }
        for (spec, tokens) in &self.tokens {
            if !state.blocked.contains(spec) && tokens.iter().any(|token| validate(token).is_err())
            {
                state.blocked.insert(spec.clone());
                state.control();
                let (generation, seq, previous) =
                    self.accepted.get(spec).copied().unwrap_or((1, 0, 0));
                let sequence = seq
                    .checked_add(1)
                    .ok_or("Panel admission sequence exhausted.")?;
                if !gapped.contains(spec) {
                    result.frames.push(Frame {
                        source: spec.clone(),
                        generation: Counter(generation),
                        sequence: Counter(sequence),
                        captured_us: Counter(at.max(previous)),
                        reason: Reason::Gap,
                        changes: vec![],
                    });
                }
                state.diagnostics.insert(
                    spec.clone(),
                    "The panel inventory route expired; explicitly acquire a fresh profile route."
                        .into(),
                );
            }
        }
        while let Some(frame) = state.frames.pop_front() {
            if !state.blocked.contains(&frame.source) {
                result.frames.push(frame);
            }
        }
        result.diagnostics = state.diagnostics.values().cloned().collect();
        drop(state);
        if self.revoked.load(Ordering::Acquire) {
            return Ok(PanelPoll::default());
        }
        for frame in &result.frames {
            self.accepted.insert(
                frame.source.clone(),
                (frame.generation.0, frame.sequence.0, frame.captured_us.0),
            );
        }
        let state=self.mailbox.lock().unwrap_or_else(|p|p.into_inner());
        if state.loss_epoch==self.loss_epoch && !self.revoked.load(Ordering::Acquire) {
            for spec in &self.specs {
                if state.blocked.contains(&spec.logical) || state.diagnostics.contains_key(&spec.logical) {continue;}
                if let Some(&(generation,sequence,verified))=state.liveness.get(&spec.logical) {
                    if at.saturating_sub(verified)>250_000 || self.accepted.get(&spec.logical).is_none_or(|accepted|accepted.0!=generation||accepted.1!=sequence) {continue;}
                    // Button levels are latched by the verified event journal.
                    // Axes retain their sampledAt freshness and events retain
                    // every original edge; neither is made from this marker.
                    let ports=spec.ports.iter().filter(|port|port.observed.starts_with("button:"))
                        .map(|port|port.logical.clone()).collect::<Vec<_>>();
                    if !ports.is_empty(){result.liveness.push((spec.logical.clone(),Counter(generation),ports));}
                }
            }
        }
        Ok(result)
    }
    pub(crate) fn stop(&mut self) {
        self.revoked.store(true, Ordering::Release);
        self.active.store(false, Ordering::Release);
        self.admitted.store(false,Ordering::Release);
        self.bound = false;
        let mut state = self.mailbox.lock().unwrap_or_else(|p| p.into_inner());
        state.frames.clear();
        state.baselines.clear();
        state.liveness.clear();
        state.control();
        drop(state);
        for worker in self.workers.drain(..) {
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}
impl Drop for PanelInputs {
    fn drop(&mut self) {
        self.stop();
    }
}
#[cfg(test)]
#[path = "profile_session_panels_inputs_tests.rs"]
mod tests;
