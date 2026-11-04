use super::*;
use osg_mapping::{engine::{Counter, Reason, typed::{Change, Frame, Phase}}, program::SignalSample};
use osg_profiles::{Binding, Direction, Endpoint, Port, PortContract, PortValue, ProcessingNode, Resource, ResourceKind,
    compiler_models::{BackendCapabilities, CompileSnapshot}};
use serde_json::json;
use std::{sync::{mpsc, atomic::AtomicU64}, time::{Duration, Instant}};

fn fixture() -> (Arc<NodePlan>, Arc<Mutex<Phase>>, crate::profile_session::OwnedRun) {
    let mut document = osg_profiles::default_document();
    document.resources.push(Resource { id: "input".into(), name: "Input".into(),
        kind: ResourceKind::Companion, required: false, ports: vec![Port {
            id: "button".into(), name: "Button".into(), direction: Direction::Output,
            contract: PortContract::State { schema: osg_profiles::resources_ports::ScalarSchema::Boolean {},
                freshness_ms: 1000, fallback: None },
        }, Port { id: "indicator".into(), name: "Indicator".into(), direction: Direction::Input,
            contract: PortContract::State { schema: osg_profiles::resources_ports::ScalarSchema::Boolean {},
                freshness_ms: 1000, fallback: None } }] });
    let bool_port = |id: &str, direction| Port { id: id.into(), name: id.into(), direction,
        contract: PortContract::State { schema: osg_profiles::resources_ports::ScalarSchema::Boolean {},
            freshness_ms: 1000, fallback: None } };
    document.processing_nodes.push(ProcessingNode { draft: None, id: "invert".into(), name: "Invert".into(),
        operation: osg_mapping::program::Operation::Not {}, inputs: vec![bool_port("input", Direction::Input)],
        outputs: vec![bool_port("output", Direction::Output)] });
    document.bindings = vec![
        Binding { id: "in".into(), from: Endpoint { node_id: "input".into(), port_id: "button".into() },
            to: Endpoint { node_id: "invert".into(), port_id: "input".into() } },
        Binding { id: "out".into(), from: Endpoint { node_id: "invert".into(), port_id: "output".into() },
            to: Endpoint { node_id: "input".into(), port_id: "indicator".into() } },
    ];
    let report = osg_profiles::compiler::compile(&document, &CompileSnapshot {
        inventory_session: "fixture".into(), inventory_revision: "1".into(), assignments: vec![], resources: vec![],
        game_context: None, backend: BackendCapabilities::default(), inventory_diagnostics: vec![], read_only: false,
    });
    let session = uuid::Uuid::new_v4().to_string();
    let phase = Phase::new(report.plan.unwrap().program, Counter(1), session.clone()).unwrap();
    let run = crate::profile_session::OwnedRun { owner: RunOwner { session, generation: 1,
        profile_id: document.id.clone(), profile_revision: document.revision, game_id: None },
        snapshot: osg_profiles::session::AppliedSnapshot::capture(&document).unwrap().summary().clone(),
        program_hash: report.program_hash.unwrap(), capture_fingerprint: report.capture_fingerprint,
        receipts: vec![], attempted: vec![] };
    let plan = NodePlan::prepare(&document, &document, &run,
        &crate::profile_session::inspection::samples::topic(&run.owner, "input")).unwrap();
    (plan, Arc::new(Mutex::new(phase)), run)
}
fn runtime(broker: Broker, signal: Signal, phase: Arc<Mutex<Phase>>, run: crate::profile_session::OwnedRun,
    count: Arc<AtomicU64>) -> Runtime {
    Runtime::new(broker, signal, Arc::new(move |plans| {
        count.fetch_add(1, Ordering::Relaxed);
        plans.iter().map(|plan| plan.capture(&phase.lock().unwrap(), &run)).collect()
    }), format!("profile-samples:{}", uuid::Uuid::new_v4()))
}
fn feed(phase: &Mutex<Phase>, sequence: u64, value: bool) {
    crate::profile_admission_test_clock::frame(&mut phase.lock().unwrap(), Frame {
        source: "input".into(), generation: Counter(1), sequence: Counter(sequence), captured_us: Counter(sequence),
        reason: if sequence == 1 { Reason::Baseline } else { Reason::Change },
        changes: vec![Change { port_id: "button".into(), sample: SignalSample::scalar(PortValue::Boolean(value)) }],
    }).unwrap();
}
fn until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready() {
        assert!(Instant::now() < deadline, "Sample worker did not complete its notified work");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn receive(reader: &mut osg_streams::Reader) -> osg_streams::WireFrame {
    let mut frame = None;
    until(|| { frame = reader.try_recv().unwrap(); frame.is_some() });
    frame.unwrap()
}

fn plans_with_ports(nodes: usize, ports_per_node: usize) -> (Vec<Arc<NodePlan>>, Arc<Mutex<Phase>>, crate::profile_session::OwnedRun) {
    let mut document = osg_profiles::default_document();
    for node in 0..nodes {
        document.resources.push(Resource { id: format!("input-{node}"), name: format!("Input {node}"),
            kind: ResourceKind::Companion, required: false,
            ports: (0..ports_per_node).map(|port| Port { id: format!("button-{port}"), name: format!("Button {port}"),
                direction: Direction::Output,
                contract: PortContract::State { schema: osg_profiles::resources_ports::ScalarSchema::Boolean {},
                    freshness_ms: 1000, fallback: None } }).collect() });
    }
    let report = osg_profiles::compiler::compile(&document, &CompileSnapshot {
        inventory_session: "fixture".into(), inventory_revision: "1".into(), assignments: vec![], resources: vec![],
        game_context: None, backend: BackendCapabilities::default(), inventory_diagnostics: vec![], read_only: false,
    });
    let session = uuid::Uuid::new_v4().to_string();
    let phase = Arc::new(Mutex::new(Phase::new(report.plan.unwrap().program, Counter(1), session.clone()).unwrap()));
    let run = crate::profile_session::OwnedRun { owner: RunOwner { session, generation: 1,
        profile_id: document.id.clone(), profile_revision: document.revision, game_id: None },
        snapshot: osg_profiles::session::AppliedSnapshot::capture(&document).unwrap().summary().clone(),
        program_hash: report.program_hash.unwrap(), capture_fingerprint: report.capture_fingerprint,
        receipts: vec![], attempted: vec![] };
    let plans = (0..nodes).map(|node| NodePlan::prepare(&document, &document, &run,
        &crate::profile_session::inspection::samples::topic(&run.owner, &format!("input-{node}"))).unwrap()).collect();
    (plans, phase, run)
}

#[test]
fn lazy_snapshot_value_changes_and_stop_are_passive_and_idle_is_quiet() {
    let (plan, phase, run) = fixture();
    let broker = Broker::new(uuid::Uuid::new_v4().to_string()).unwrap();
    let signal = Signal::default();
    let count = Arc::new(AtomicU64::new(0));
    let runtime = runtime(broker.clone(), signal.clone(), phase.clone(), run.clone(), count.clone());
    std::thread::sleep(Duration::from_millis(10));
    assert_eq!(count.load(Ordering::Relaxed), 0);
    let pin = runtime.pin(plan.clone()).unwrap();
    pin.wait_ready().unwrap();
    let mut reader = broker.subscribe("fixture", "ui", &plan.topic).unwrap();
    drop(pin);
    let first = receive(&mut reader);
    assert_eq!(first.payload.unwrap()["ports"][0]["sample"]["status"], "unknown");
    until(|| count.load(Ordering::Relaxed) >= 1);
    // Pin release may request one idle review; settle it before the idle proof.
    std::thread::sleep(Duration::from_millis(20));
    let before = count.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(count.load(Ordering::Relaxed), before);
    feed(&phase, 1, false); signal.notify();
    let frame = receive(&mut reader);
    assert_eq!(frame.payload.unwrap()["ports"].as_array().unwrap().iter().find(|port| port["endpoint"]["portId"] == "button").unwrap()["sample"]["value"]["value"], false);
    signal.retire(&run.owner);
    let loss = receive(&mut reader);
    assert_eq!(loss.kind, "loss");
    assert!(runtime.signal.inner.lock().slots.is_empty());
    assert!(phase.lock().unwrap().active(), "Observation retirement cannot stop routing");
    runtime.shutdown();
}

#[test]
fn retiring_during_blocked_capture_cannot_publish_a_late_baseline() {
    let (plan, phase, run) = fixture();
    let broker = Broker::new(uuid::Uuid::new_v4().to_string()).unwrap();
    let signal = Signal::default();
    let (entered, arrived) = mpsc::channel();
    let (release, resumed) = mpsc::channel();
    let resumed = Mutex::new(resumed);
    let captured_run = run.clone();
    let runtime = Runtime::new(broker.clone(), signal.clone(), Arc::new(move |plans| {
        entered.send(()).unwrap(); resumed.lock().unwrap().recv().unwrap();
        plans.iter().map(|plan| plan.capture(&phase.lock().unwrap(), &captured_run)).collect()
    }), "profile-samples:fixture".into());
    let pin = runtime.pin(plan.clone()).unwrap();
    arrived.recv_timeout(Duration::from_secs(1)).unwrap();
    let before = Instant::now();
    signal.retire(&run.owner);
    assert!(before.elapsed() < Duration::from_millis(100));
    assert!(pin.wait_ready().is_err());
    release.send(()).unwrap();
    drop(pin);
    runtime.shutdown();
    assert!(broker.subscribe("fixture", "ui", &plan.topic).is_err());
}

#[test]
fn slow_preview_keeps_the_latest_state_and_does_not_backlog_or_fault_routing() {
    let (plan, phase, run) = fixture();
    let broker = Broker::new(uuid::Uuid::new_v4().to_string()).unwrap();
    let signal = Signal::default();
    let runtime = runtime(broker.clone(), signal.clone(), phase.clone(), run, Arc::new(AtomicU64::new(0)));
    let pin = runtime.pin(plan.clone()).unwrap(); pin.wait_ready().unwrap();
    let mut reader = broker.subscribe("slow", "ui", &plan.topic).unwrap(); drop(pin);
    for sequence in 1..=1000 { feed(&phase, sequence, sequence % 2 == 1); signal.notify(); }
    // Coalescing the same final value need not republish its later revision.
    let expected = phase.lock().unwrap().sample(&osg_mapping::program::Endpoint {
        node_id: "input".into(), port_id: "button".into() }).unwrap().clone();
    until(|| runtime.signal.inner.lock().slots[&plan.topic].last.as_ref()
        .is_some_and(|last| last.samples[0] == expected));
    let frame = receive(&mut reader);
    assert_eq!(frame.kind, "snapshot");
    assert_eq!(frame.payload.unwrap()["ports"].as_array().unwrap().iter().find(|port| port["endpoint"]["portId"] == "button").unwrap()["sample"]["value"]["value"], false);
    assert!(reader.try_recv().unwrap().is_none());
    assert!(phase.lock().unwrap().fault().is_none());
    drop(reader); runtime.readers_changed();
    until(|| runtime.signal.inner.lock().slots.is_empty());
    runtime.shutdown();
}

#[test]
fn topic_pin_topic_count_and_total_port_quotas_are_enforced() {
    let (plans, phase, run) = plans_with_ports(33, 1);
    let broker = Broker::new(uuid::Uuid::new_v4().to_string()).unwrap();
    let signal = Signal::default();
    let capture_phase = phase.clone(); let capture_run = run.clone();
    let runtime = Runtime::new(broker, signal, Arc::new(move |plans| plans.iter()
        .map(|plan| plan.capture(&capture_phase.lock().unwrap(), &capture_run)).collect()), "profile-samples:quotas".into());
    let topic_pins = (0..64).map(|_| runtime.pin(plans[0].clone()).unwrap()).collect::<Vec<_>>();
    assert!(runtime.pin(plans[0].clone()).err().unwrap().contains("subscription capacity"));
    drop(topic_pins);
    let topic_pins = (0..32).map(|index| runtime.pin(plans[index].clone()).unwrap()).collect::<Vec<_>>();
    assert!(runtime.pin(plans[32].clone()).err().unwrap().contains("capacity is full"));
    drop(topic_pins);
    runtime.shutdown();

    let (wide_plans, wide_phase, wide_run) = plans_with_ports(8, 128);
    let (extra, extra_phase, extra_run) = plans_with_ports(1, 1);
    let broker = Broker::new(uuid::Uuid::new_v4().to_string()).unwrap();
    let signal = Signal::default();
    let runtime = Runtime::new(broker, signal, Arc::new(move |plans| plans.iter().map(|plan|
        if plan.owner == wide_run.owner { plan.capture(&wide_phase.lock().unwrap(), &wide_run) }
        else { plan.capture(&extra_phase.lock().unwrap(), &extra_run) }).collect()), "profile-samples:port-quota".into());
    let wide_pins = wide_plans.iter().map(|plan| runtime.pin(plan.clone()).unwrap()).collect::<Vec<_>>();
    assert!(runtime.pin(extra[0].clone()).err().unwrap().contains("capacity is full"));
    drop(wide_pins);
    runtime.shutdown();
}

#[test]
fn exhausted_query_ids_fail_without_registering_a_slot() {
    let (plan, _, _) = fixture();
    let broker = Broker::new(uuid::Uuid::new_v4().to_string()).unwrap();
    let signal = Signal::default();
    let runtime = Runtime::new(broker, signal.clone(), Arc::new(|_| vec![]), "profile-samples:ids".into());
    signal.inner.lock().next_id = u64::MAX;
    assert!(runtime.pin(plan).err().unwrap().contains("counter exhausted"));
    assert!(signal.inner.lock().slots.is_empty());
    runtime.shutdown();
}

#[test]
fn failed_capture_and_encode_wake_waiters_without_faulting_the_reducer() {
    for encode_failure in [false, true] {
        let (plan, phase, _) = fixture();
        let broker = Broker::new(uuid::Uuid::new_v4().to_string()).unwrap();
        let signal = Signal::default();
        let capture = if encode_failure {
            Arc::new(|_: &[Arc<NodePlan>]| vec![Ok(crate::profile_session::inspection::samples::CapturedSamples {
                revision: 1, captured_us: 0, samples: vec![], observed_us: vec![] })]) as Arc<Capture>
        } else { Arc::new(|_: &[Arc<NodePlan>]| vec![Err("baseline unavailable".into())]) as Arc<Capture> };
        let runtime = Runtime::new(broker, signal, capture, "profile-samples:failure".into());
        let pin = runtime.pin(plan).unwrap();
        let error = pin.wait_ready().unwrap_err();
        assert!(!error.is_empty());
        assert!(phase.lock().unwrap().active());
        assert!(phase.lock().unwrap().fault().is_none());
        runtime.shutdown();
    }
}

#[test]
fn shutdown_unblocks_a_baseline_waiter_while_capture_is_blocked() {
    let (plan, _, _) = fixture();
    let broker = Broker::new(uuid::Uuid::new_v4().to_string()).unwrap();
    let signal = Signal::default();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let runtime = Arc::new(Runtime::new(broker, signal, Arc::new(move |_| {
        entered_tx.send(()).unwrap(); release_rx.lock().unwrap().recv().unwrap(); vec![]
    }), "profile-samples:shutdown".into()));
    let pin = runtime.pin(plan).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let (waiter_tx, waiter_rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || waiter_tx.send(pin.wait_ready()).unwrap());
    let shutdown_runtime = runtime.clone();
    let shutdown = std::thread::spawn(move || shutdown_runtime.shutdown());
    assert!(waiter_rx.recv_timeout(Duration::from_secs(1)).unwrap().is_err());
    release_tx.send(()).unwrap();
    waiter.join().unwrap();
    shutdown.join().unwrap();
}
