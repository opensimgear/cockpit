use super::*;
use osg_profiles::{
    compiler_models::{BackendCapabilities, CompileSnapshot, SourcePlan},
    resources::*,
    resources_ports::*,
};
use std::sync::Condvar;

#[test]
fn activation_requires_a_new_live_read_and_consumes_its_baseline_exactly_once() {
    for can in [false, true] {
        let (plan, io) = fixture(can, 1);
        io.automatic_clock.store(true, Ordering::Release);
        let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
        inputs.acquire(0).unwrap();
        let original = eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());

        let inactive = Arc::new(Block::new());
        *io.block.lock().unwrap() = Some(("panel0".into(), inactive.clone()));
        inactive.entered();
        // These edges predate activation and must never become occurrences.
        io.press(true);
        io.press(false);
        inputs.begin_live_baseline().unwrap();
        assert!(inputs.live_baseline(&plan, |_| Ok(())).unwrap().is_none());

        let live = Arc::new(Block::new());
        *io.block.lock().unwrap() = Some(("panel0".into(), live.clone()));
        inactive.release();
        live.entered();
        assert!(
            inputs.live_baseline(&plan, |_| Ok(())).unwrap().is_none(),
            "A late inactive reply cannot acknowledge live activation"
        );
        assert!(inputs.mailbox.lock().unwrap().frames.is_empty());

        live.release();
        let baseline = eventually(|| inputs.live_baseline(&plan, |_| Ok(())).unwrap());
        assert_eq!(baseline.len(), 1);
        assert_eq!(baseline[0].reason, Reason::Baseline);
        assert!(baseline[0].sequence > original[0].sequence);
        assert_eq!(
            sample(&baseline[0], "button-event:0"),
            Some(&SignalSample::Unknown)
        );
        let pending = inputs.poll(|_| Ok(())).unwrap();
        assert!(
            pending
                .frames
                .iter()
                .all(|frame| frame.reason != Reason::Baseline),
            "The baseline already admitted by the Host cannot reset its live reducer"
        );
        io.press(true);
        let changes = eventually(|| {
            let poll = inputs.poll(|_| Ok(())).unwrap();
            poll.frames
                .iter()
                .any(|frame| {
                    sample(frame, "button-event:0")
                        == Some(&SignalSample::scalar(PortValue::Boolean(true)))
                })
                .then_some(poll.frames)
        });
        assert!(changes
            .iter()
            .all(|frame| frame.sequence > baseline[0].sequence));
        inputs.stop();
    }
}
#[test]
fn verified_panel_reads_keep_latched_buttons_fresh_for_five_seconds_without_inventing_edges() {
    use osg_mapping::{
        engine::typed::Phase,
        program::{Program, SourcePort},
    };
    let (plan, io) = fixture(false, 1);
    io.press(true);
    let sampled_at = io.data.lock().unwrap()["sampledAtMs"].clone();
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    inputs.begin_live_baseline().unwrap();
    let baseline = eventually(|| inputs.live_baseline(&plan, |_| Ok(())).unwrap());
    let program = Program {
        sources: plan.sources.iter().map(|source| SourcePort {
            endpoint: osg_mapping::program::Endpoint {
                node_id: source.endpoint.node_id.clone(),
                port_id: source.endpoint.port_id.clone(),
            },
            contract: source.contract.clone(),
        }).collect(),
        nodes: vec![], sinks: vec![], controllers: vec![],
    };
    let mut phase = Phase::new(program, Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
    for frame in baseline { phase.accept(frame).unwrap(); }
    phase.drain();
    let button = osg_mapping::program::Endpoint {
        node_id: "logical0".into(), port_id: "button:0".into(),
    };
    let mut first_verified = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(Instant::now() < deadline, "Verified read clock did not advance across five seconds");
        // Await an actual verified read rather than requiring a polling rate
        // from a shared CI runner. Frozen sampledAt and held state must produce
        // liveness markers only, without synthetic state/event frames.
        let (poll, verified) = eventually(|| {
            let poll = inputs.poll(|_| Ok(())).unwrap();
            assert!(poll.frames.is_empty(), "Stationary panel control must not manufacture edges");
            if poll.liveness.is_empty() { return None; }
            let verified = inputs.mailbox.lock().unwrap().liveness["logical0"].2;
            Some((poll, verified))
        });
        let (epoch, origin) = inputs.clock.unwrap();
        let now = Counter(origin + epoch.elapsed().as_micros() as u64);
        for (source, generation, ports) in poll.liveness {
            assert_eq!(ports, vec!["button:0"]);
            phase.observe_port_liveness(&source, generation, now, &ports).unwrap();
        }
        phase.advance(now).unwrap();
        assert_eq!(phase.sample(&button), Some(&SignalSample::scalar(PortValue::Boolean(true))));
        let first = *first_verified.get_or_insert(verified);
        if verified.saturating_sub(first) > 5_000_000 { break; }
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(io.data.lock().unwrap()["sampledAtMs"], sampled_at);
    io.fail.store(true, Ordering::Release);
    let loss = eventually(|| {
        let poll = inputs.poll(|_| Ok(())).unwrap();
        poll.frames.iter().any(|frame| frame.reason == Reason::Gap).then_some(poll)
    });
    assert!(loss.liveness.is_empty());
    phase.accept_batch(loss.frames).unwrap();
    assert!(!matches!(phase.sample(&button), Some(SignalSample::Known(_))));
    inputs.stop();
}

#[test]
fn delayed_live_peer_keeps_other_baselines_fresh_and_admits_only_postbaseline_changes() {
    let (plan, io) = fixture(true, 2);
    io.automatic_clock.store(true, Ordering::Release);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    let fast = inputs
        .specs
        .iter()
        .find(|spec| matches!(&spec.target,Target::Can{gateway,..} if gateway=="panel1"))
        .unwrap()
        .logical
        .clone();
    let slow = Arc::new(Block::new());
    *io.block.lock().unwrap() = Some(("panel0".into(), slow.clone()));
    inputs.begin_live_baseline().unwrap();
    {
        let mut mailbox = inputs.mailbox.lock().unwrap();
        mailbox.loss_epoch += 1;
        mailbox.baselines.clear();
    }
    slow.entered();
    eventually(|| {
        inputs
            .mailbox
            .lock()
            .unwrap()
            .baselines
            .contains_key(&fast)
            .then_some(())
    });
    thread::sleep(Duration::from_millis(320));
    assert!(inputs.live_baseline(&plan, |_| Ok(())).unwrap().is_none());
    assert!(
        inputs.mailbox.lock().unwrap().frames.is_empty(),
        "Unadmitted snapshots cannot queue event delivery"
    );

    let in_flight = Arc::new(Block::new());
    *io.block.lock().unwrap() = Some(("panel1".into(), in_flight.clone()));
    in_flight.entered();
    slow.release();
    let baseline = eventually(|| inputs.live_baseline(&plan, |_| Ok(())).unwrap());
    assert_eq!(baseline.len(), 2);
    let accepted = baseline
        .iter()
        .find(|frame| frame.source == fast)
        .unwrap()
        .clone();
    let first = inputs.poll(|_| Ok(())).unwrap();
    assert!(
        first
            .frames
            .iter()
            .all(|frame| !matches!(frame.reason, Reason::Baseline | Reason::Gap)),
        "The admitted newer generation cannot immediately gap again"
    );
    io.press(true);
    in_flight.release();
    let frames = eventually(|| {
        let poll = inputs.poll(|_| Ok(())).unwrap();
        poll.frames
            .iter()
            .any(|frame| {
                frame.source == fast
                    && sample(frame, "button-event:0")
                        == Some(&SignalSample::scalar(PortValue::Boolean(true)))
            })
            .then_some(poll.frames)
    });
    assert!(
        frames
            .iter()
            .filter(|frame| frame.source == fast)
            .all(|frame| frame.sequence > accepted.sequence
                && frame.generation == accepted.generation)
    );
    inputs.stop();
}

fn events() -> Value {
    json!({"ok":true,"schemaVersion":1,"stream":7,"head":0,"oldest":1,"overflow":0,
        "buttonCount":1,"held":[false],"contactButtonCount":1,"maintainedButtons":[0],"encoderButtons":[],"selectorButtons":[],
        "axisCount":1,"axes":[65535],"axisNeutral":[32767],"axisUsage":[48],"axisAvailable":[true],"sampledAtMs":10})
}
fn usb_token() -> UsbRouteToken {
    UsbRouteToken {
        module_id: "4f53474400112233445566778899aabb".into(),
        serial: "00112233445566778899aabb".into(),
        connection_generation: 1,
        boot: 2,
        configuration_generation: 3,
    }
}
fn native_route(can: bool, key: &str) -> Route {
    Route {
        id: if can {
            format!("can:{key}:module")
        } else {
            format!("usb:{key}")
        },
        transport: if can { Transport::Can } else { Transport::Usb },
        provider_generation: if can {
            json!([1, 2, 3, 4, 7]).to_string()
        } else {
            "1:2:3".into()
        },
        attributes: if can {
            BTreeMap::from([
                ("gatewayId".into(), key.into()),
                ("moduleId".into(), "module".into()),
                ("node".into(), "5".into()),
                ("boot".into(), "2".into()),
            ])
        } else {
            BTreeMap::new()
        },
        baseline_ready: true,
        writable: false,
        priority: 100,
    }
}
struct Block {
    entered: Mutex<bool>,
    released: Mutex<bool>,
    cv: Condvar,
}
impl Block {
    fn new() -> Self {
        Self {
            entered: Mutex::new(false),
            released: Mutex::new(false),
            cv: Condvar::new(),
        }
    }
    fn wait(&self) {
        *self.entered.lock().unwrap() = true;
        self.cv.notify_all();
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.cv.wait(released).unwrap();
        }
    }
    fn entered(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !*self.entered.lock().unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.cv.notify_all();
    }
}
struct Fake {
    data: Mutex<Value>,
    journal: Mutex<Vec<Value>>,
    routes: Mutex<BTreeMap<String, Route>>,
    usb: Mutex<UsbRouteToken>,
    calls: AtomicUsize,
    automatic_clock: AtomicBool,
    block: Mutex<Option<(String, Arc<Block>)>>,
    fail: AtomicBool,
}
impl Fake {
    fn new() -> Self {
        Self {
            data: Mutex::new(events()),
            journal: Mutex::new(vec![]),
            routes: Mutex::new(BTreeMap::new()),
            usb: Mutex::new(usb_token()),
            calls: AtomicUsize::new(0),
            automatic_clock: AtomicBool::new(false),
            block: Mutex::new(None),
            fail: AtomicBool::new(false),
        }
    }
    fn press(&self, pressed: bool) {
        let mut data = self.data.lock().unwrap();
        let mut history = self.journal.lock().unwrap();
        let sequence = data["head"].as_u64().unwrap() + 1;
        history.push(json!({"sequence":sequence,"control":0,"pressed":pressed}));
        data["head"] = json!(sequence);
        data["held"][0] = json!(pressed);
        let time = data["sampledAtMs"].as_u64().unwrap() + 1;
        data["sampledAtMs"] = json!(time);
    }
    fn reply(&self, cursor: Option<(u32, u32)>) -> Value {
        let mut state = self.data.lock().unwrap();
        if self.automatic_clock.load(Ordering::Acquire) {
            let n = state["sampledAtMs"].as_u64().unwrap() + 1;
            state["sampledAtMs"] = json!(n);
        }
        let mut data = state.clone();
        drop(state);
        if let Some((_, after)) = cursor {
            let entries = self
                .journal
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e["sequence"].as_u64().unwrap() > after as u64)
                .take(16)
                .cloned()
                .collect::<Vec<_>>();
            let next = entries
                .last()
                .map_or(after as u64, |e| e["sequence"].as_u64().unwrap());
            data["after"] = json!(after);
            data["events"] = json!(entries);
            data["next"] = json!(next);
            data["more"] = json!(next < data["head"].as_u64().unwrap());
        }
        data
    }
    fn state(&self, key: &str, events: Value) -> Value {
        let route = self.routes.lock().unwrap().get(key).unwrap().clone();
        let values: Vec<Value> = serde_json::from_str(&route.provider_generation).unwrap();
        let node: Value = serde_json::from_str(&route.attributes["node"]).unwrap();
        let mut events = events;
        events["route"] = json!([values[0], node, values[2], values[1]]);
        events["configurationGeneration"] = values[3].clone();
        json!({"connected":true,"epoch":values[0],"modules":[{"id":"module","online":true,"node":node,"lease":values[2],"boot":values[1],"configurationGeneration":values[3]}],"moduleEvents":{"module":events}})
    }
    fn before(&self, key: &str) {
        self.calls.fetch_add(1, Ordering::AcqRel);
        let block = self
            .block
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(blocked, _)| blocked == key)
            .map(|(_, b)| b.clone());
        if let Some(block) = block {
            block.wait();
        }
    }
}
impl Io for Fake {
    fn can(&self, r: Value) -> Value {
        let key = r["gatewayId"].as_str().unwrap();
        self.before(key);
        if self.fail.load(Ordering::Acquire) {
            return json!({"ok":false,"error":"device read failed\nprivate"});
        }
        let cursor = r["after"]
            .as_u64()
            .map(|after| (r["stream"].as_u64().unwrap() as u32, after as u32));
        json!({"ok":true,"state":self.state(key,self.reply(cursor))})
    }
    fn can_cached(&self, key: &str) -> Result<Value, String> {
        if !self.routes.lock().unwrap().contains_key(key) {
            return Err("missing gateway".into());
        }
        Ok(self.state(key, self.data.lock().unwrap().clone()))
    }
    fn usb(&self, key: &str, r: UsbRequest) -> Result<UsbSnapshot, String> {
        self.before(key);
        if self.fail.load(Ordering::Acquire) {
            return Err("USB read failed".into());
        }
        let cursor = match r {
            UsbRequest::EventsState { .. } => None,
            UsbRequest::EventsRead { stream, after, .. } => Some((stream, after)),
            _ => panic!("read-only worker attempted mutation"),
        };
        let route = self.usb.lock().unwrap().clone();
        let mut events = self.reply(cursor);
        events["route"] = json!(route);
        Ok(UsbSnapshot {
            connected: true,
            route: Some(route),
            events: Some(events),
            ..Default::default()
        })
    }
    fn snapshots(&self) -> Vec<SlotSnapshot> {
        let route = self.usb.lock().unwrap().clone();
        self.routes
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, r)| r.transport == Transport::Usb)
            .map(|(key, _)| SlotSnapshot {
                key: key.clone(),
                port: crate::transport::PortInfo {
                    path: "COM4".into(),
                    serial_number: Some(route.serial.clone()),
                    vendor_id: None,
                    product_id: None,
                    manufacturer: None,
                    friendly_name: None,
                },
                present: true,
                automatic: false,
                admissible: true,
                snapshot: UsbSnapshot {
                    connected: true,
                    route: Some(route.clone()),
                    ..Default::default()
                },
                diagnostics: vec![],
            })
            .collect()
    }
}
fn empty_plan() -> ExecutionPlan {
    osg_profiles::compiler::compile(
        &osg_profiles::default_document(),
        &CompileSnapshot {
            inventory_session: "inventory".into(),
            inventory_revision: "1".into(),
            assignments: vec![],
            resources: vec![],
            game_context: None,
            backend: BackendCapabilities::default(),
            inventory_diagnostics: vec![],
            read_only: false,
        },
    )
    .plan
    .unwrap()
}
fn fixture(can: bool, count: usize) -> (ExecutionPlan, Arc<Fake>) {
    let io = Arc::new(Fake::new());
    let mut registry = Registry::new();
    let descriptors = crate::profile_resource_ports::events(&events()).unwrap();
    for n in 0..count {
        let key = format!("panel{n}");
        let route = native_route(can, &key);
        io.routes.lock().unwrap().insert(key.clone(), route.clone());
        registry
            .reconcile(ProviderBatch {
                scope: format!("panel:{n}"),
                session: format!("native:{n}"),
                revision: 1,
                complete: true,
                observations: vec![Observation {
                    key,
                    name: "Panel".into(),
                    resource_type: ResourceType::OsgModule,
                    evidence: vec![],
                    attributes: BTreeMap::new(),
                    diagnostics: vec![],
                    route: Some(route),
                    ports: Some(PortUpdate::complete(descriptors.clone()).unwrap()),
                }],
            })
            .unwrap();
    }
    let mut plan = empty_plan();
    for (n, resource) in registry.list(0, 128).unwrap().resources.iter().enumerate() {
        let details = registry.inspect(&resource.id, 0, 128).unwrap();
        for port in &details.ports {
            let preparation = registry
                .prepare_port(
                    &resource.id,
                    &port.descriptor.id,
                    &port.contract_hash,
                    Access::Read,
                )
                .unwrap();
            let route = preparation.resolved().route.clone();
            plan.sources.push(SourcePlan {
                endpoint: osg_profiles::Endpoint {
                    node_id: format!("logical{n}"),
                    port_id: port.descriptor.id.clone(),
                },
                inventory_resource_id: Some(resource.id.clone()),
                observed_port_id: Some(port.descriptor.id.clone()),
                contract: port.descriptor.contract.clone(),
                token: Some(
                    registry
                        .token(&resource.id, &port.descriptor.id, Access::Read)
                        .unwrap(),
                ),
                preparation: Some(preparation),
                route: Some(route),
                readiness: PlanReadiness::Ready,
            });
        }
    }
    (plan, io)
}
fn make_source(spec: Spec) -> Source {
    Source {
        spec,
        cursor: None,
        held: vec![],
        sequence: 0,
        at: 0,
        retry: Instant::now(),
        generation: 1,
        sampled_at: None,
    }
}
fn sample<'a>(frame: &'a Frame, id: &str) -> Option<&'a SignalSample> {
    frame
        .changes
        .iter()
        .find(|c| c.port_id == id)
        .map(|c| &c.sample)
}
fn eventually<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(value) = f() {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "bounded condition did not become ready"
        );
        thread::sleep(Duration::from_millis(3));
    }
}
#[test]
fn native_baseline_keeps_raw_axis_and_event_occurrences_unknown_until_actual_sample_advances() {
    for can in [false, true] {
        let (plan, io) = fixture(can, 1);
        let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
        let mut source = make_source(inputs.specs[0].clone());
        let first = source.baseline(io.as_ref(), 10).unwrap();
        assert_eq!(sample(&first, "axis:0"), Some(&SignalSample::Unknown));
        assert_eq!(
            sample(&first, "button-event:0"),
            Some(&SignalSample::Unknown)
        );
        io.data.lock().unwrap()["sampledAtMs"] = json!(11);
        let next = source.baseline(io.as_ref(), 20).unwrap();
        assert_eq!(
            sample(&next, "axis:0"),
            Some(&SignalSample::scalar(PortValue::Number(65535.0)))
        );
        assert!(
            source.page(io.as_ref(), 30).unwrap().is_empty(),
            "frozen sample cannot refresh state freshness"
        );
        io.data.lock().unwrap()["axisAvailable"][0] = json!(false);
        let loss = source.page(io.as_ref(), 40).unwrap();
        assert_eq!(sample(&loss[0], "axis:0"), Some(&SignalSample::Unavailable));
    }
}
#[test]
fn short_press_journal_is_ordered_without_replay_or_axis_normalization() {
    let (plan, io) = fixture(true, 1);
    let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    source.baseline(io.as_ref(), 10).unwrap();
    io.press(true);
    io.press(false);
    let frames = source.page(io.as_ref(), 20).unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(
        sample(&frames[0], "button-event:0"),
        Some(&SignalSample::scalar(PortValue::Boolean(true)))
    );
    assert_eq!(
        sample(&frames[1], "button-event:0"),
        Some(&SignalSample::scalar(PortValue::Boolean(false)))
    );
    assert_eq!(frames[0].captured_us, frames[1].captured_us);
    assert!(frames[0].sequence.0 < frames[1].sequence.0);
    assert!(source.page(io.as_ref(), 30).unwrap().is_empty());
    io.press(true);
    let _ = source.page(io.as_ref(), 40).unwrap();
    io.data.lock().unwrap()["sampledAtMs"] = json!(50);
    let held = source.page(io.as_ref(), 50).unwrap();
    assert_eq!(
        sample(&held[0], "button:0"),
        Some(&SignalSample::scalar(PortValue::Boolean(true)))
    );
    assert_eq!(sample(&held[0], "button-event:0"), None);
}
#[test]
fn partial_pages_do_not_jump_state_ahead_and_corrupt_journals_admit_no_partial_frame() {
    let (plan, io) = fixture(true, 1);
    let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    source.baseline(io.as_ref(), 1).unwrap();
    for n in 0..19 {
        io.press(n % 2 == 0);
    }
    let page = source.page(io.as_ref(), 2).unwrap();
    assert_eq!(page.len(), 16);
    assert!(page.iter().all(|frame| sample(frame, "axis:0").is_none()));
    assert_eq!(source.cursor, Some((7, 16)));
    let tail = source.page(io.as_ref(), 3).unwrap();
    assert_eq!(tail.len(), 4);
    io.press(false);
    io.data.lock().unwrap()["held"][0] = json!(true);
    let sequence = source.sequence;
    assert!(source.page(io.as_ref(), 4).is_err());
    assert_eq!(source.sequence, sequence);
    assert_eq!(source.cursor, Some((7, 19)));
    io.data.lock().unwrap()["oldest"] = json!(21);
    assert!(source.page(io.as_ref(), 5).is_err());
}
#[test]
fn sample_clock_wrap_is_accepted_but_backwards_clock_contract_and_identity_changes_are_rejected() {
    let (plan, io) = fixture(true, 1);
    let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    io.data.lock().unwrap()["sampledAtMs"] = json!(u32::MAX);
    source.baseline(io.as_ref(), 1).unwrap();
    io.data.lock().unwrap()["sampledAtMs"] = json!(0);
    assert!(source.baseline(io.as_ref(), 2).is_ok());
    io.data.lock().unwrap()["sampledAtMs"] = json!(u32::MAX);
    assert!(source.baseline(io.as_ref(), 3).is_err());
    assert_eq!(
        source.sampled_at,
        Some(0),
        "a rejected old clock cannot move the sample fence"
    );
    io.data.lock().unwrap()["sampledAtMs"] = json!(1);
    io.data.lock().unwrap()["axisNeutral"][0] = json!(0);
    assert!(source.baseline(io.as_ref(), 4).is_err());
    io.routes
        .lock()
        .unwrap()
        .get_mut("panel0")
        .unwrap()
        .provider_generation = json!([1, 2, 3, 5, 7]).to_string();
    assert!(Target::from_route(plan.sources[0].route.as_ref().unwrap(), io.as_ref()).is_err());
    let (plan, io) = fixture(false, 1);
    let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    io.usb.lock().unwrap().connection_generation = 2;
    assert!(source.baseline(io.as_ref(), 1).is_err());
    assert!(Target::from_route(plan.sources[0].route.as_ref().unwrap(), io.as_ref()).is_err());
}
#[test]
fn preparation_and_delivery_require_exact_native_port_scope_and_no_invoke_capability() {
    let (plan, io) = fixture(true, 1);
    let mut wrong = plan.clone();
    wrong.sources[0].preparation = None;
    assert!(PanelInputs::prepare_io(&wrong, io.clone()).is_err());
    wrong = plan.clone();
    wrong.sources[0].route.as_mut().unwrap().provider_generation =
        json!([2, 2, 3, 4, 7]).to_string();
    assert!(PanelInputs::prepare_io(&wrong, io.clone()).is_err());
    wrong = plan.clone();
    wrong.sources[0].contract = PortContract::Event {
        schema: ScalarSchema::Boolean {},
    };
    assert!(PanelInputs::prepare_io(&wrong, io.clone()).is_err());
    wrong = plan.clone();
    wrong.sources.push(wrong.sources[0].clone());
    assert!(PanelInputs::prepare_io(&wrong, io.clone()).is_err());
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    assert!(inputs.start().is_err());
    assert!(inputs.poll(|_| Ok(())).is_err());
    assert!(inputs.baseline(&plan, |_| Ok(())).is_err());
    inputs.acquire(100).unwrap();
    assert!(inputs.acquire(100).is_err());
    eventually(|| {
        let ready = inputs.mailbox.lock().unwrap().baselines.len() == 1;
        ready.then_some(())
    });
    for field in ["access", "resource", "port", "hash", "generation"] {
        let mut wrong = plan.clone();
        let token = wrong.sources[0].token.as_mut().unwrap();
        match field {
            "access" => token.access = Access::Invoke,
            "resource" => token.resource_id = "other".into(),
            "port" => token.port_id = "other".into(),
            "hash" => token.contract_hash = "bad".into(),
            _ => token.provider_generation = "old".into(),
        };
        assert!(inputs.baseline(&wrong, |_| Ok(())).is_err(), "{field}");
    }
    assert!(inputs.baseline(&plan, |_| Err("revoked".into())).is_err());
    let frames = inputs.baseline(&plan, |_| Ok(())).unwrap().unwrap();
    assert_eq!(frames.len(), 1);
    inputs.start().unwrap();
    assert!(inputs.baseline(&plan, |_| Ok(())).is_err());
    inputs.stop();
    assert!(inputs.capture_receipts().is_empty());
}
#[test]
fn nonblocking_poll_and_stop_discard_late_device_reply() {
    let (plan, io) = fixture(true, 1);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    inputs.start().unwrap();
    let block = Arc::new(Block::new());
    *io.block.lock().unwrap() = Some(("panel0".into(), block.clone()));
    block.entered();
    let start = Instant::now();
    let _ = inputs.poll(|_| Ok(())).unwrap();
    assert!(start.elapsed() < Duration::from_millis(50));
    let start = Instant::now();
    inputs.stop();
    assert!(start.elapsed() < Duration::from_millis(50));
    block.release();
    eventually(|| (IN_FLIGHT.load(Ordering::Acquire) == 0).then_some(()));
    assert!(inputs.poll(|_| Ok(())).unwrap().frames.is_empty());
    assert!(inputs.mailbox.lock().unwrap().baselines.is_empty());
}
#[test]
fn one_slow_device_does_not_prevent_other_native_baselines_and_wait_is_cancellable() {
    let (plan, io) = fixture(true, 2);
    let block = Arc::new(Block::new());
    *io.block.lock().unwrap() = Some(("panel0".into(), block.clone()));
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    block.entered();
    eventually(|| (inputs.mailbox.lock().unwrap().baselines.len() == 1).then_some(()));
    assert!(inputs.baseline(&plan, |_| Ok(())).unwrap().is_none());
    assert!(inputs
        .wait_baseline(&plan, Duration::from_secs(3), |_| Ok(()), || true)
        .unwrap_err()
        .contains("cancelled"));
    assert!(inputs
        .wait_baseline(&plan, Duration::ZERO, |_| Ok(()), || false)
        .is_err());
    block.release();
    let frames = inputs
        .wait_baseline(&plan, Duration::from_secs(2), |_| Ok(()), || false)
        .unwrap();
    assert_eq!(frames.len(), 2);
    inputs.stop();
}
#[test]
fn start_discards_pre_activation_history_and_revocation_isolated_to_one_source() {
    let (plan, io) = fixture(true, 2);
    io.automatic_clock.store(true, Ordering::Release);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    io.press(true);
    io.press(false);
    inputs.start().unwrap();
    let mut initial = vec![];
    eventually(|| {
        initial.extend(inputs.poll(|_| Ok(())).unwrap().frames);
        (initial
            .iter()
            .filter(|f| f.reason == Reason::Baseline)
            .count()
            >= 2)
            .then_some(())
    });
    assert!(initial
        .iter()
        .flat_map(|f| &f.changes)
        .filter(|c| c.port_id == "button-event:0")
        .all(|c| c.sample == SignalSample::Unknown));
    let revoked = plan.sources[0].token.as_ref().unwrap().resource_id.clone();
    let result = inputs
        .poll(|token| {
            if token.resource_id == revoked {
                Err("removed".into())
            } else {
                Ok(())
            }
        })
        .unwrap();
    assert_eq!(
        result
            .frames
            .iter()
            .filter(|f| f.reason == Reason::Gap)
            .count(),
        1
    );
    io.press(true);
    let frames = eventually(|| {
        let result = inputs.poll(|_| Ok(())).unwrap();
        result
            .frames
            .iter()
            .any(|f| f.reason == Reason::Change)
            .then_some(result.frames)
    });
    assert!(frames.iter().all(|f| f.source != "logical0"));
    inputs.stop();
}
#[test]
fn queue_loss_precedes_fresh_generation_without_replaying_missing_occurrences() {
    let (plan, io) = fixture(true, 1);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    inputs.start().unwrap();
    eventually(|| {
        let result = inputs.poll(|_| Ok(())).unwrap();
        result
            .frames
            .iter()
            .any(|f| f.reason == Reason::Baseline)
            .then_some(())
    });
    for n in 0..650 {
        io.press(n % 2 == 0);
    }
    eventually(|| (inputs.mailbox.lock().unwrap().loss_epoch > 0).then_some(()));
    eventually(|| {
        inputs
            .mailbox
            .lock()
            .unwrap()
            .frames
            .iter()
            .any(|f| f.reason == Reason::Baseline && f.generation.0 > 1)
            .then_some(())
    });
    let result = inputs.poll(|_| Ok(())).unwrap();
    assert_eq!(result.frames[0].reason, Reason::Gap);
    assert!(result.frames.iter().skip(1).all(|f| f.generation.0 > 1));
    assert!(result
        .frames
        .windows(2)
        .all(|pair| pair[0].captured_us.0 <= pair[1].captured_us.0));
    assert!(result
        .frames
        .iter()
        .flat_map(|f| &f.changes)
        .filter(|c| c.port_id == "button-event:0")
        .all(|c| c.sample == SignalSample::Unknown));
    assert!(result.frames.len() <= MAX_QUEUE + 1);
    inputs.stop();
}
#[test]
fn empty_reader_and_stale_cached_baseline_are_bounded_and_idempotent() {
    let io = Arc::new(Fake::new());
    let plan = empty_plan();
    let mut empty = PanelInputs::prepare_io(&plan, io).unwrap();
    empty.acquire(0).unwrap();
    assert!(empty
        .baseline(&plan, |_| Ok(()))
        .unwrap()
        .unwrap()
        .is_empty());
    empty.start().unwrap();
    assert!(empty.poll(|_| Ok(())).unwrap().frames.is_empty());
    empty.stop();
    empty.stop();
    let (plan, io) = fixture(true, 1);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    inputs.stop();
    inputs.revoked.store(false, Ordering::Release);
    inputs.clock = Some((Instant::now(), 1_000_000));
    let frame = Frame {
        source: "logical0".into(),
        generation: Counter(1),
        sequence: Counter(1),
        captured_us: Counter(1),
        reason: Reason::Baseline,
        changes: vec![],
    };
    inputs
        .mailbox
        .lock()
        .unwrap()
        .baselines
        .insert("logical0".into(), frame);
    assert!(inputs.baseline(&plan, |_| Ok(())).unwrap().is_none());
    inputs.stop();
}

#[test]
fn actual_panel_frames_feed_typed_reducer_and_baseline_gap_stop_cannot_replay_occurrences() {
    use osg_mapping::{
        engine::typed::Phase,
        program::{Endpoint, Program, SourcePort},
    };
    let (plan, io) = fixture(true, 1);
    let inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    let mut source = make_source(inputs.specs[0].clone());
    let program = Program {
        sources: plan
            .sources
            .iter()
            .map(|s| SourcePort {
                endpoint: Endpoint {
                    node_id: s.endpoint.node_id.clone(),
                    port_id: s.endpoint.port_id.clone(),
                },
                contract: s.contract.clone(),
            })
            .collect(),
        nodes: vec![],
        sinks: vec![],
        controllers: vec![],
    };
    let mut phase = Phase::new(
        program,
        Counter(1),
        "11111111-1111-4111-8111-111111111111".into(),
    )
    .unwrap();
    let event = Endpoint {
        node_id: "logical0".into(),
        port_id: "button-event:0".into(),
    };
    phase
        .accept(source.baseline(io.as_ref(), 10).unwrap())
        .unwrap();
    assert_eq!(phase.sample(&event), Some(&SignalSample::Unknown));
    io.press(true);
    io.press(false);
    let frames = source.page(io.as_ref(), 20).unwrap();
    phase.accept(frames[0].clone()).unwrap();
    assert_eq!(
        phase.sample(&event),
        Some(&SignalSample::scalar(PortValue::Boolean(true)))
    );
    phase.accept(frames[1].clone()).unwrap();
    assert_eq!(
        phase.sample(&event),
        Some(&SignalSample::scalar(PortValue::Boolean(false)))
    );
    phase
        .accept(source.frame(30, Reason::Gap, vec![]).unwrap())
        .unwrap();
    phase
        .accept(source.baseline(io.as_ref(), 40).unwrap())
        .unwrap();
    assert_eq!(phase.sample(&event), Some(&SignalSample::Unknown));
    phase.stop();
    io.press(true);
    for frame in source.page(io.as_ref(), 50).unwrap() {
        assert!(phase.accept(frame).is_err());
    }
    assert!(!phase.active());
    assert!(phase.drain().is_empty());
}
#[test]
fn invalid_native_generations_and_acquisition_budgets_fail_before_any_device_request() {
    let (plan, io) = fixture(true, 1);
    let initial = io.calls.load(Ordering::Acquire);
    for generation in ["bad", "[]", "[1,0,3,4,7]", "[1,2,3,4,8]"] {
        let mut route = plan.sources[0].route.clone().unwrap();
        route.provider_generation = generation.into();
        assert!(Target::from_route(&route, io.as_ref()).is_err());
    }
    let mut wrong = plan.sources[0].route.clone().unwrap();
    wrong.attributes.remove("moduleId");
    assert!(Target::from_route(&wrong, io.as_ref()).is_err());
    wrong = plan.sources[0].route.clone().unwrap();
    wrong.attributes.insert("node".into(), "bad".into());
    assert!(Target::from_route(&wrong, io.as_ref()).is_err());
    assert_eq!(io.calls.load(Ordering::Acquire), initial);
    let (oversized, large) = fixture(true, 17);
    assert!(PanelInputs::prepare_io(&oversized, large).is_err());
    let mut dormant = plan.clone();
    dormant.sources[0].readiness = PlanReadiness::Dormant;
    dormant.sources[0].preparation = None;
    assert!(PanelInputs::prepare_io(&dormant, io.clone()).is_ok());
    let mut route = native_route(false, "missing");
    assert!(Target::from_route(&route, io.as_ref()).is_err());
    route.transport = Transport::Hid;
    assert!(Target::from_route(&route, io.as_ref()).is_err());
}
#[test]
fn source_errors_rebaseline_without_reconstructing_missing_transitions() {
    let (plan, io) = fixture(true, 1);
    let mut inputs = PanelInputs::prepare_io(&plan, io.clone()).unwrap();
    inputs.acquire(0).unwrap();
    eventually(|| inputs.baseline(&plan, |_| Ok(())).unwrap());
    inputs.start().unwrap();
    eventually(|| {
        inputs
            .poll(|_| Ok(()))
            .unwrap()
            .frames
            .iter()
            .any(|f| f.reason == Reason::Baseline)
            .then_some(())
    });
    io.fail.store(true, Ordering::Release);
    let error = eventually(|| {
        let result = inputs.poll(|_| Ok(())).unwrap();
        result
            .frames
            .iter()
            .any(|f| f.reason == Reason::Gap)
            .then_some(result)
    });
    assert!(error.diagnostics.iter().all(|s| !s.contains('\n')));
    io.press(true);
    io.press(false);
    io.fail.store(false, Ordering::Release);
    let frames = eventually(|| {
        let result = inputs.poll(|_| Ok(())).unwrap();
        result
            .frames
            .iter()
            .any(|f| f.reason == Reason::Baseline)
            .then_some(result.frames)
    });
    assert!(frames
        .iter()
        .flat_map(|f| &f.changes)
        .filter(|c| c.port_id == "button-event:0")
        .all(|c| c.sample == SignalSample::Unknown));
    inputs.stop();
}

#[test]
fn native_usb_companion_routes_belong_to_the_companion_adapter() {
    let (mut plan, io) = fixture(false, 1);
    for source in &mut plan.sources {
        source.route.as_mut().unwrap().id = "companion:standalone-panel".into();
        source.preparation = None;
    }
    let inputs = PanelInputs::prepare_io(&plan, io).unwrap();
    assert!(inputs.specs.is_empty());
}
