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
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
const MAX_QUEUE: usize = 512;
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
struct ReadPermit;
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
impl Drop for ReadPermit {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}
trait Io: Send + Sync {
    fn can(&self, request: Value) -> Value;
    fn can_cached(&self, gateway: &str) -> Result<Value, String>;
    fn usb(&self, key: &str, request: UsbRequest) -> Result<UsbSnapshot, String>;
    fn snapshots(&self) -> Vec<SlotSnapshot>;
}
impl Io for ProfilePanelIo {
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
impl Target {
    fn from_route(route: &Route, io: &dyn Io) -> Result<Self, String> {
        match route.transport {
            Transport::Usb => {
                let key = route
                    .id
                    .strip_prefix("usb:")
                    .ok_or("The USB route has no exact native endpoint.")?;
                let slots = io.snapshots();
                let slot = slots
                    .iter()
                    .find(|slot| {
                        slot.key == key
                            && slot.present
                            && slot.admissible
                            && slot.snapshot.connected
                    })
                    .ok_or("The selected USB endpoint is no longer admissible.")?;
                let token = slot
                    .snapshot
                    .route
                    .as_ref()
                    .ok_or("The USB endpoint has no current native route.")?;
                if route.provider_generation
                    != format!(
                        "{}:{}:{}",
                        token.connection_generation, token.boot, token.configuration_generation
                    )
                {
                    return Err("The USB route changed during preparation.".into());
                }
                Ok(Self::Usb {
                    key: key.into(),
                    route: token.clone(),
                })
            }
            Transport::Can => {
                let values: Vec<Value> = serde_json::from_str(&route.provider_generation)
                    .map_err(|_| "The CAN route has no exact native generation.")?;
                if values.len() != 5 {
                    return Err("The CAN route generation is incomplete.".into());
                }
                let field = |key: &str| {
                    route
                        .attributes
                        .get(key)
                        .cloned()
                        .ok_or_else(|| format!("Missing native CAN {key}."))
                };
                let gateway = field("gatewayId")?;
                let module = field("moduleId")?;
                let node: Value = serde_json::from_str(&field("node")?)
                    .map_err(|_| "Invalid native CAN node.")?;
                let boot: Value = serde_json::from_str(&field("boot")?)
                    .map_err(|_| "Invalid native CAN boot.")?;
                if boot != values[1]
                    || boot.as_u64().is_none_or(|n| n == 0)
                    || node.as_u64().is_none()
                {
                    return Err("The CAN route boot/node changed.".into());
                }
                let target = Self::Can {
                    gateway: gateway.clone(),
                    module,
                    epoch: values[0].clone(),
                    node,
                    lease: values[2].clone(),
                    boot,
                    configuration: values[3].clone(),
                    stream: values[4].clone(),
                };
                target.check_can(&io.can_cached(&gateway)?)?;
                Ok(target)
            }
            _ => Err("Only native USB/CAN panel routes belong to this reader.".into()),
        }
    }
    fn check_can(&self, state: &Value) -> Result<(), String> {
        let Self::Can {
            module,
            epoch,
            node,
            lease,
            boot,
            configuration,
            stream,
            ..
        } = self
        else {
            return Ok(());
        };
        let current = state["modules"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|m| m["id"] == *module && m["online"] == true)
            })
            .ok_or("The native CAN module is offline.")?;
        if state["connected"] != true
            || state["epoch"] != *epoch
            || current["node"] != *node
            || current["lease"] != *lease
            || current["boot"] != *boot
            || current["configurationGeneration"] != *configuration
        {
            return Err("CAN epoch, node, lease, boot or configuration changed.".into());
        }
        let events = &state["moduleEvents"][module];
        if events["route"] != json!([epoch, node, lease, boot])
            || events["configurationGeneration"] != *configuration
            || events["stream"] != *stream
        {
            return Err("The CAN input stream does not match its selected route.".into());
        }
        Ok(())
    }
    fn events(&self, io: &dyn Io, cursor: Option<(u32, u32)>) -> Result<Value, String> {
        let events = match self {
            Self::Usb { key, route } => {
                let request = cursor.map_or_else(
                    || UsbRequest::EventsState {
                        route: route.clone(),
                    },
                    |(stream, after)| UsbRequest::EventsRead {
                        route: route.clone(),
                        stream,
                        after,
                    },
                );
                let snapshot = io.usb(key, request)?;
                if !snapshot.connected || snapshot.route.as_ref() != Some(route) {
                    return Err("USB connection, boot or configuration changed.".into());
                }
                snapshot
                    .events
                    .filter(|e| e["route"] == json!(route))
                    .ok_or("USB input reply is not from the selected native route.")?
            }
            Self::Can {
                gateway,
                module,
                boot,
                ..
            } => {
                let mut request = json!({"command":if cursor.is_some(){"events_read"}else{"events_state"},"gatewayId":gateway,"moduleId":module,"boot":boot,"clientId":"profile-panel-reader"});
                if let Some((stream, after)) = cursor {
                    request["stream"] = json!(stream);
                    request["after"] = json!(after);
                }
                let reply = io.can(request);
                if reply["ok"] != true {
                    return Err(reply["error"]
                        .as_str()
                        .unwrap_or("The CAN input read failed.")
                        .into());
                }
                let state = &reply["state"];
                self.check_can(state)?;
                let events = &state["moduleEvents"][module];
                events.clone()
            }
        };
        crate::panel_events::validate(&events, cursor)?;
        Ok(events)
    }
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
impl Source {
    fn frame(&mut self, at: u64, reason: Reason, changes: Vec<Change>) -> Result<Frame, String> {
        if at < self.at {
            return Err("Native panel clock moved backwards.".into());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Panel admission sequence exhausted.")?;
        self.at = at;
        Ok(Frame {
            source: self.spec.logical.clone(),
            generation: Counter(self.generation),
            sequence: Counter(self.sequence),
            captured_us: Counter(at),
            reason,
            changes,
        })
    }
    fn verify(&self, events: &Value) -> Result<(), String> {
        let descriptors = crate::profile_resource_ports::events(events)?;
        for port in &self.spec.ports {
            if descriptors
                .iter()
                .find(|p| p.id == port.observed)
                .is_none_or(|p| p.contract != port.contract)
            {
                return Err(
                    "The selected panel input contract changed; reconcile its exact port.".into(),
                );
            }
        }
        Ok(())
    }
    fn state_changes(
        &self,
        events: &Value,
        baseline: bool,
        sample_advanced: bool,
    ) -> Result<Vec<Change>, String> {
        self.spec
            .ports
            .iter()
            .filter_map(|port| {
                let sample = if let Some(index) = port.observed.strip_prefix("button:") {
                    if !baseline && !sample_advanced && events.get("sampledAtMs").is_some() {
                        return None;
                    }
                    let index = index
                        .parse::<usize>()
                        .map_err(|_| "Invalid selected button index.".to_owned());
                    Some(index.and_then(|index| {
                        events["held"][index]
                            .as_bool()
                            .map(|v| SignalSample::scalar(PortValue::Boolean(v)))
                            .ok_or_else(|| "The selected button disappeared.".into())
                    }))
                } else if let Some(index) = port.observed.strip_prefix("axis:") {
                    if !baseline
                        && !sample_advanced
                        && events["axisAvailable"][index.parse::<usize>().unwrap_or(usize::MAX)]
                            != false
                    {
                        return None;
                    }
                    Some(
                        index
                            .parse::<usize>()
                            .map_err(|_| "Invalid selected axis index.".into())
                            .and_then(|index| {
                                if events["axisAvailable"][index] == false {
                                    Ok(SignalSample::Unavailable)
                                } else if !sample_advanced {
                                    Ok(SignalSample::Unknown)
                                } else {
                                    events["axes"][index]
                                        .as_u64()
                                        .map(|v| SignalSample::scalar(PortValue::Number(v as f64)))
                                        .ok_or_else(|| "The selected raw axis disappeared.".into())
                                }
                            }),
                    )
                } else if port.observed.starts_with("button-event:") {
                    baseline.then_some(Ok(SignalSample::Unknown))
                } else {
                    Some(Err("The selected input has no native panel decoder.".into()))
                };
                sample.map(|sample| {
                    sample.map(|sample| Change {
                        port_id: port.logical.clone(),
                        sample,
                    })
                })
            })
            .collect()
    }
    fn sample_advanced(&mut self, events: &Value) -> Result<bool, String> {
        let Some(current) = events["sampledAtMs"].as_u64() else {
            return Ok(false);
        };
        let current = current as u32;
        let previous = self.sampled_at;
        match previous {
            None => {
                self.sampled_at = Some(current);
                Ok(false)
            }
            Some(previous) => {
                let delta = current.wrapping_sub(previous);
                if delta > i32::MAX as u32 {
                    Err("Native panel sample clock moved backwards.".into())
                } else {
                    self.sampled_at = Some(current);
                    Ok(delta > 0)
                }
            }
        }
    }
    fn baseline(&mut self, io: &dyn Io, at: u64) -> Result<Frame, String> {
        let events = self.spec.target.events(io, None)?;
        self.verify(&events)?;
        let advanced = self.sample_advanced(&events)?;
        let changes = self.state_changes(&events, true, advanced)?;
        self.held = events["held"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_bool().unwrap())
            .collect();
        self.cursor = Some((
            events["stream"].as_u64().unwrap() as u32,
            events["head"].as_u64().unwrap() as u32,
        ));
        self.frame(at, Reason::Baseline, changes)
    }
    fn page(&mut self, io: &dyn Io, at: u64) -> Result<Vec<Frame>, String> {
        let cursor = self
            .cursor
            .ok_or("Establish a native baseline before reading panel events.")?;
        let events = self.spec.target.events(io, Some(cursor))?;
        self.verify(&events)?;
        let entries = events["events"].as_array().unwrap();
        let mut next_held = self.held.clone();
        for event in entries {
            let index = event["control"].as_u64().unwrap() as usize;
            if index >= next_held.len() {
                return Err(crate::panel_events::INVALID.into());
            }
            next_held[index] = event["pressed"].as_bool().unwrap();
        }
        if events["more"] == false
            && next_held.iter().map(|v| json!(v)).collect::<Vec<_>>()
                != *events["held"].as_array().unwrap()
        {
            return Err("The panel journal does not reconstruct its reported state; establish a new baseline.".into());
        }
        let mut frames = vec![];
        for event in entries {
            let index = event["control"].as_u64().unwrap();
            let pressed = event["pressed"].as_bool().unwrap();
            let changes = self
                .spec
                .ports
                .iter()
                .filter(|port| {
                    port.observed == format!("button:{index}")
                        || port.observed == format!("button-event:{index}")
                })
                .map(|port| Change {
                    port_id: port.logical.clone(),
                    sample: SignalSample::scalar(PortValue::Boolean(pressed)),
                })
                .collect::<Vec<_>>();
            if !changes.is_empty() {
                frames.push(self.frame(at, Reason::Change, changes)?);
            }
        }
        self.held = next_held;
        self.cursor = Some((cursor.0, events["next"].as_u64().unwrap() as u32));
        if events["more"] == false {
            // State is sampled at head; never move axes ahead of queued edges.
            let advanced = self.sample_advanced(&events)?;
            let changes = self.state_changes(&events, false, advanced)?;
            if !changes.is_empty() {
                frames.push(self.frame(at, Reason::Change, changes)?);
            }
        }
        Ok(frames)
    }
}
#[derive(Default)]
struct Mailbox {
    frames: VecDeque<Frame>,
    baselines: BTreeMap<String, Frame>,
    diagnostics: BTreeMap<String, String>,
    blocked: BTreeSet<String>,
    loss_epoch: u64,
}
#[derive(Default)]
pub(crate) struct PanelPoll {
    pub frames: Vec<Frame>,
    pub diagnostics: Vec<String>,
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
    pub(crate) fn prepare(plan: &ExecutionPlan, io: Arc<ProfilePanelIo>) -> Result<Self, String> {
        Self::prepare_io(plan, io)
    }
    fn prepare_io(plan: &ExecutionPlan, io: Arc<dyn Io>) -> Result<Self, String> {
        let mut specs: Vec<Spec> = vec![];
        for source in &plan.sources {
            if source.readiness != PlanReadiness::Ready
                || !source
                    .route
                    .as_ref()
                    .is_some_and(|r| matches!(r.transport, Transport::Usb | Transport::Can))
            {
                continue;
            }
            if source
                .route
                .as_ref()
                .is_some_and(|route| route.id.starts_with("companion:"))
            {
                // USB is a transport shared with companions, whose native
                // publication and input admission belong to their own adapter.
                continue;
            }
            let pin = source
                .preparation
                .as_ref()
                .ok_or("Prepare the exact native panel route before acquiring inputs.")?;
            if pin.access() != osg_profiles::resources::Access::Read {
                return Err("Panel input preparation cannot grant output access.".into());
            }
            let observed = source
                .observed_port_id
                .as_ref()
                .ok_or("Select the exact native panel input port.")?;
            if pin.resolved().port.id != *observed
                || pin.resolved().port.contract != source.contract
                || source.inventory_resource_id.as_ref() != Some(&pin.resolved().resource_id)
                || source.route.as_ref() != Some(&pin.resolved().route)
            {
                return Err(
                    "The panel preparation no longer matches its selected contract.".into(),
                );
            }
            let target = Target::from_route(source.route.as_ref().unwrap(), io.as_ref())?;
            let port = Port {
                logical: source.endpoint.port_id.clone(),
                observed: observed.clone(),
                contract: source.contract.clone(),
                resource_id: pin.resolved().resource_id.clone(),
                contract_hash: pin.resolved().port.contract_hash()?,
            };
            if let Some(spec) = specs
                .iter_mut()
                .find(|s| s.logical == source.endpoint.node_id)
            {
                if spec.target != target || spec.ports.iter().any(|p| p.logical == port.logical) {
                    return Err(
                        "A logical panel source cannot mix routes or duplicate logical ports."
                            .into(),
                    );
                }
                spec.ports.push(port);
            } else {
                specs.push(Spec {
                    logical: source.endpoint.node_id.clone(),
                    target,
                    ports: vec![port],
                });
            }
        }
        if specs.len() > 16 || specs.iter().map(|s| s.ports.len()).sum::<usize>() > 1024 {
            return Err(
                "Panel source acquisition exceeds its bounded resource/port budget.".into(),
            );
        }
        Ok(Self {
            specs,
            io,
            mailbox: Arc::new(Mutex::new(Mailbox::default())),
            revoked: Arc::new(AtomicBool::new(false)),
            active: Arc::new(AtomicBool::new(false)),
            admitted: Arc::new(AtomicBool::new(false)),
            workers: vec![],
            tokens: BTreeMap::new(),
            accepted: BTreeMap::new(),
            loss_epoch: 0,
            clock: None,
            bound: false,
        })
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
            let worker = thread::Builder::new()
                .name("profile-panel-inputs".into())
                .spawn(move || {
                    let mut source = Source {
                        spec, cursor: None, held: vec![], sequence: 0, at: clock_origin_us,
                        retry: Instant::now(), generation: 1, sampled_at: None,
                    };
                    let mut was_active = false;
                    let mut was_admitted = false;
                    let mut startup:Option<Source>=None;
                    let mut epoch = 0;
                    let now = || clock_origin_us.saturating_add(
                        u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
                    while !revoked.load(Ordering::Acquire) {
                        let running = active.load(Ordering::Acquire);
                        let delivering = admitted.load(Ordering::Acquire);
                        if running && !was_active {
                            // Events that predate activation are not replayable.
                            source.cursor = None;
                        }
                        was_active = running;
                        if delivering&&!was_admitted {
                            // Restore the exact snapshot accepted by the Host,
                            // discarding any later unadmitted read's cursor.
                            if let Some(checkpoint)=startup.take(){source=checkpoint;}
                        }
                        was_admitted=delivering;
                        {
                            let state = mailbox.lock().unwrap_or_else(|p| p.into_inner());
                            if state.blocked.contains(&source.spec.logical) { break; }
                            if state.loss_epoch != epoch {
                                epoch = state.loss_epoch;
                                source.cursor = None;
                                source.generation = source.generation.saturating_add(1);
                            }
                        }
                        if Instant::now() < source.retry {
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        let Some(_permit) = ReadPermit::acquire() else {
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        };
                        let baseline = !running || !delivering || source.cursor.is_none();
                        let result = if baseline {
                            source.baseline(io.as_ref(), now()).map(|frame| vec![frame])
                        } else {
                            source.page(io.as_ref(), now())
                        };
                        let after = now();
                        source.at = after;
                        if revoked.load(Ordering::Acquire) { break; }
                        let mut state = mailbox.lock().unwrap_or_else(|p| p.into_inner());
                        // Check again under the publication lock so Stop cannot
                        // clear the mailbox immediately before a late append.
                        if revoked.load(Ordering::Acquire) || state.blocked.contains(&source.spec.logical) { break; }
                        // An inactive read already in flight when Start armed
                        // the reader cannot supply the first live baseline.
                        if running != active.load(Ordering::Acquire) { continue; }
                        if delivering != admitted.load(Ordering::Acquire) { continue; }
                        if state.loss_epoch != epoch { continue; }
                        match result {
                            Ok(mut frames) => {
                                for frame in &mut frames { frame.captured_us = Counter(after); }
                                state.diagnostics.remove(&source.spec.logical);
                                if baseline {
                                    if let Some(frame) = frames.first() {
                                        state.baselines.insert(source.spec.logical.clone(), frame.clone());
                                    }
                                    if running&&!delivering {startup=Some(source.clone());}
                                }
                                if running&&delivering {
                                    if state.frames.len() + frames.len() > MAX_QUEUE {
                                        state.frames.clear();
                                        state.baselines.clear();
                                        state.loss_epoch = state.loss_epoch.saturating_add(1);
                                        state.diagnostics.insert(source.spec.logical.clone(), "Panel frame queue overflowed; missing events were discarded and fresh baselines are required.".into());
                                    } else { state.frames.extend(frames); }
                                }
                                source.retry = Instant::now() + Duration::from_millis(if running {10} else {100});
                            }
                            Err(error) => {
                                source.cursor = None;
                                source.retry = Instant::now() + Duration::from_millis(100);
                                state.baselines.remove(&source.spec.logical);
                                if running&&delivering {
                                    if let Ok(gap) = source.frame(after, Reason::Gap, vec![]) {
                                        if state.frames.len() == MAX_QUEUE {
                                            state.frames.clear();
                                            state.baselines.clear();
                                            state.loss_epoch = state.loss_epoch.saturating_add(1);
                                        } else { state.frames.push_back(gap); }
                                    }
                                }
                                state.diagnostics.insert(source.spec.logical.clone(), error.chars().filter(|c| !c.is_control()).take(320).collect());
                            }
                        }
                    }
                })
                .map_err(|_| "Cannot start a bounded native panel reader.")?;
            self.workers.push(worker);
        }
        Ok(())
    }
    /// None means actual native baseline acquisition is still pending. It never
    /// substitutes retained values or manufactures a token/sample.
    pub(crate) fn baseline(
        &mut self,
        delivery: &ExecutionPlan,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
    ) -> Result<Option<Vec<Frame>>, String> {
        self.capture_baseline(delivery, validate, false)
    }
    /// Capture the actual first live snapshot before the Host arms its reducer.
    /// This consumes only duplicate baseline frames, retaining later changes.
    pub(crate) fn live_baseline(
        &mut self,
        delivery: &ExecutionPlan,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
    ) -> Result<Option<Vec<Frame>>, String> {
        self.capture_baseline(delivery, validate, true)
    }
    fn capture_baseline(
        &mut self,
        delivery: &ExecutionPlan,
        validate: impl Fn(&ResourceToken) -> Result<(), String>,
        live: bool,
    ) -> Result<Option<Vec<Frame>>, String> {
        if self.revoked.load(Ordering::Acquire)
            || self.clock.is_none()
            || self.active.load(Ordering::Acquire) != live
            || live && self.admitted.load(Ordering::Acquire)
        {
            return Err("The native panel reader is not acquired.".into());
        }
        let mut state = self.mailbox.lock().unwrap_or_else(|p| p.into_inner());
        if self
            .specs
            .iter()
            .any(|s| !state.baselines.contains_key(&s.logical))
        {
            return Ok(None);
        }
        let now = self
            .clock
            .map(|(start, origin)| {
                origin
                    .saturating_add(u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX))
            })
            .unwrap_or(0);
        if state
            .baselines
            .values()
            .any(|frame| now.saturating_sub(frame.captured_us.0) > 250_000)
        {
            return Ok(None);
        }
        let mut tokens: BTreeMap<String, Vec<ResourceToken>> = BTreeMap::new();
        for spec in &self.specs {
            for port in &spec.ports {
                let source = delivery
                    .sources
                    .iter()
                    .find(|s| {
                        s.endpoint.node_id == spec.logical
                            && s.endpoint.port_id == port.logical
                            && s.readiness == PlanReadiness::Ready
                    })
                    .ok_or("The delivered panel source disappeared.")?;
                let token = source.token.as_ref().ok_or(
                    "Acquire an actual native read token before admitting a panel baseline.",
                )?;
                if token.resource_id != port.resource_id
                    || token.port_id != port.observed
                    || token.contract_hash != port.contract_hash
                    || source.inventory_resource_id.as_ref() != Some(&port.resource_id)
                    || source.route.as_ref().is_none_or(|route| {
                        token.route_id != route.id
                            || token.provider_generation != route.provider_generation
                    })
                    || token.access != osg_profiles::resources::Access::Read
                    || source.observed_port_id.as_ref() != Some(&port.observed)
                    || source.contract != port.contract
                    || source.route.as_ref().is_none_or(|route| {
                        Target::from_route(route, self.io.as_ref()).ok().as_ref()
                            != Some(&spec.target)
                    })
                {
                    return Err(
                        "The native panel baseline does not match the selected route/contract."
                            .into(),
                    );
                }
                validate(token)?;
                tokens
                    .entry(spec.logical.clone())
                    .or_default()
                    .push(token.clone());
            }
        }
        let frames = self
            .specs
            .iter()
            .map(|s| state.baselines[&s.logical].clone())
            .collect::<Vec<_>>();
        if live {
            state.frames.retain(|frame| {
                !frames.iter().any(|baseline| {
                    baseline.source == frame.source
                        && (frame.generation < baseline.generation
                            || frame.generation == baseline.generation
                                && frame.sequence <= baseline.sequence)
                })
            });
            // No page/event queue is published before this common admission.
            // All sources now use exactly the baselines captured under this lock.
            self.loss_epoch = state.loss_epoch;
            self.admitted.store(true, Ordering::Release);
        }
        drop(state);
        for frame in &frames {
            self.accepted.insert(
                frame.source.clone(),
                (frame.generation.0, frame.sequence.0, frame.captured_us.0),
            );
        }
        self.tokens = tokens;
        self.bound = true;
        Ok(Some(frames))
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
        self.admitted.store(admitted, Ordering::Release);
        self.active.store(true, Ordering::Release);
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
