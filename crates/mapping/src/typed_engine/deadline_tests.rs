use super::super::*;
use crate::signal::{Direction, PortDescriptor};

#[path = "constant_tests.rs"]
mod constants;

#[path = "clock_tests.rs"]
mod clocks;

fn endpoint(node: &str) -> Endpoint {
    Endpoint {
        node_id: node.into(),
        port_id: "value".into(),
    }
}
fn contract() -> PortContract {
    PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 1000,
        fallback: None,
    }
}
fn known(value: bool) -> SignalSample {
    SignalSample::scalar(PortValue::Boolean(value))
}
fn engine(pulse: bool) -> Phase {
    let operation = if pulse {
        Operation::Builtin {
            rule: crate::rules::Rule::Pulse {
                duration_us: 1000,
                repeat_us: 0,
            },
            inputs: vec!["input".into()],
        }
    } else {
        Operation::Identity {}
    };
    let node = |id: &str, inputs, operation| Node {
        id: id.into(),
        inputs,
        outputs: vec![PortDescriptor {
            id: "value".into(),
            name: "value".into(),
            direction: Direction::Output,
            contract: contract(),
        }],
        operation,
    };
    Phase::new(
        Program {
            sources: vec![SourcePort {
                endpoint: endpoint("source"),
                contract: contract(),
            }],
            nodes: vec![
                node(
                    "processor",
                    vec![InputBinding {
                        port_id: "input".into(),
                        from: endpoint("source"),
                        contract: contract(),
                        edge_id: "edge".into(),
                    }],
                    operation,
                ),
                node(
                    "constant",
                    vec![],
                    Operation::Constant {
                        value: PortValue::Boolean(false),
                    },
                ),
            ],
            sinks: vec![Sink {
                id: "sink".into(),
                endpoint: endpoint("destination"),
                from: endpoint("processor"),
                contract: contract(),
                kind: SinkKind::Resource {
                    adapter: ResourceAdapter::CompanionFeedback,
                },
            }],
            controllers: vec![],
        },
        Counter(1),
        "b3738156-6794-42dc-978b-2376dce0c6c6".into(),
    )
    .unwrap()
}
fn feed(engine: &mut Phase, sequence: u64, at: u64, reason: Reason, sample: SignalSample) {
    { let admitted_frame = Frame {
            source: "source".into(),
            generation: Counter(1),
            sequence: Counter(sequence),
            captured_us: Counter(at),
            changes: if matches!(reason, Reason::Gap | Reason::Disconnect) {
                vec![]
            } else {
                vec![Change {
                    port_id: "value".into(),
                    sample,
                }]
            },
            reason,
        }; let admitted_now = Counter(engine.at_us().0.max(admitted_frame.captured_us.0)); engine.accept(admitted_frame, admitted_now) }
        .unwrap();
}
fn timer(engine: &mut Phase, node: &str, id: &str, at: u64) {
    let token = engine.token(node).unwrap();
    engine
        .schedule(
            token,
            id.into(),
            Counter(at),
            BTreeMap::from([(
                "value".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            )]),
        )
        .unwrap();
}

#[test]
fn next_deadline_selects_external_internal_then_freshness_inclusively() {
    let mut engine = engine(true);
    assert_eq!(engine.next_deadline_us(), None);
    feed(&mut engine, 1, 0, Reason::Baseline, known(false));
    assert_eq!(engine.next_deadline_us(), Some(Counter(1_000_000)));
    feed(&mut engine, 2, 1, Reason::Change, known(true));
    assert_eq!(engine.next_deadline_us(), Some(Counter(1001)));
    timer(&mut engine, "constant", "early", 501);
    assert_eq!(engine.next_deadline_us(), Some(Counter(501)));
    engine.advance(Counter(500)).unwrap();
    assert_eq!(engine.next_deadline_us(), Some(Counter(501)));
    engine.advance(Counter(501)).unwrap();
    assert!(engine.timers.is_empty());
    assert_eq!(engine.next_deadline_us(), Some(Counter(1001)));
    engine.advance(Counter(1001)).unwrap();
    assert_eq!(engine.sample(&endpoint("processor")), Some(&known(false)));
    // The unchanged source expires before the pulse output's newer freshness.
    assert_eq!(engine.next_deadline_us(), Some(Counter(1_000_001)));
    engine.advance(Counter(1_000_000)).unwrap();
    assert_eq!(engine.next_deadline_us(), Some(Counter(1_000_001)));
    engine.advance(Counter(1_000_001)).unwrap();
    assert_eq!(
        engine.sample(&endpoint("processor")),
        Some(&SignalSample::Stale)
    );
    assert_eq!(engine.next_deadline_us(), None);
}

#[test]
fn source_and_liveness_refresh_deadlines_without_replacing_capture_or_emitting_reports() {
    let mut engine = engine(false);
    feed(&mut engine, 1, 10, Reason::Baseline, known(false));
    feed(&mut engine, 2, 20, Reason::Change, known(true));
    assert_eq!(engine.next_deadline_us(), Some(Counter(1_000_020)));
    engine.drain_reports();
    engine.drain();
    let provenance = engine
        .values
        .iter()
        .map(|(id, state)| (id.clone(), (state.captured, state.captured_sequence)))
        .collect::<BTreeMap<_, _>>();
    engine
        .observe_liveness("source", Counter(1), Counter(50))
        .unwrap();
    assert_eq!(engine.next_deadline_us(), Some(Counter(1_000_050)));
    assert_eq!(engine.at_us(), Counter(20));
    assert!(engine.drain_reports().is_empty());
    assert!(engine.drain().is_empty());
    for (id, state) in &engine.values {
        assert_eq!((state.captured, state.captured_sequence), provenance[id]);
    }
    // Repeated scheduling queries have no clock or report side effects.
    assert_eq!(engine.next_deadline_us(), Some(Counter(1_000_050)));
    assert!(engine.drain_reports().is_empty());
}

#[test]
fn revocation_cancels_dependent_deadlines_but_keeps_independent_timer() {
    let mut engine = engine(true);
    feed(&mut engine, 1, 0, Reason::Baseline, known(false));
    feed(&mut engine, 2, 1, Reason::Change, known(true));
    timer(&mut engine, "processor", "dependent", 500);
    timer(&mut engine, "constant", "independent", 2000);
    feed(
        &mut engine,
        3,
        2,
        Reason::Disconnect,
        SignalSample::Unavailable,
    );
    assert_eq!(engine.next_deadline_us(), Some(Counter(2000)));
    engine.advance(Counter(1001)).unwrap();
    assert_eq!(engine.next_deadline_us(), Some(Counter(2000)));
    engine.advance(Counter(2000)).unwrap();
    assert_eq!(engine.next_deadline_us(), None);
    assert!(engine.timers.is_empty());
}

#[test]
fn expiry_cancels_same_time_timer_without_removing_future_timer() {
    let mut engine = engine(false);
    feed(&mut engine, 1, 0, Reason::Baseline, known(false));
    timer(&mut engine, "processor", "expires", 1_000_000);
    timer(&mut engine, "constant", "future", 1_000_100);
    assert_eq!(engine.next_deadline_us(), Some(Counter(1_000_000)));
    engine.advance(Counter(1_000_000)).unwrap();
    assert_eq!(
        engine.sample(&endpoint("processor")),
        Some(&SignalSample::Stale)
    );
    assert_eq!(engine.sample(&endpoint("constant")), Some(&known(false)));
    assert_eq!(engine.next_deadline_us(), Some(Counter(1_000_100)));
    engine.advance(Counter(1_000_100)).unwrap();
    assert!(engine.timers.is_empty());
    assert_eq!(engine.next_deadline_us(), None);
}

#[test]
fn stopped_or_faulted_phase_has_no_schedulable_deadline() {
    let mut engine = engine(false);
    feed(&mut engine, 1, 0, Reason::Baseline, known(false));
    timer(&mut engine, "constant", "future", 2000);
    engine.fault = Some("injected runtime fault".into());
    assert_eq!(engine.next_deadline_us(), None);
    engine.fault = None;
    assert_eq!(engine.next_deadline_us(), Some(Counter(2000)));
    engine.stop();
    assert_eq!(engine.next_deadline_us(), None);
}

#[test]
fn empty_batch_preserves_clock_provenance_queued_reports_and_pending_work() {
    let mut engine = engine(true);
    feed(&mut engine, 1, 0, Reason::Baseline, known(false));
    feed(&mut engine, 2, 1, Reason::Change, known(true));
    timer(&mut engine, "constant", "future", 2000);
    let values = engine
        .values
        .iter()
        .map(|(id, value)| {
            (
                id.clone(),
                (
                    value.sample.clone(),
                    value.epoch,
                    value.observed,
                    value.captured,
                    value.captured_sequence,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let reports = serde_json::to_value(&engine.reports).unwrap();
    let effects = serde_json::to_value(&engine.effects).unwrap();
    let deadline = engine.next_deadline_us();
    assert!({ let admitted_frames: Vec<Frame> = ([]).into_iter().collect(); let admitted_now = Counter(engine.at_us().0.max(admitted_frames.iter().map(|frame| frame.captured_us.0).max().unwrap_or(0))); engine.accept_batch(admitted_frames, admitted_now) }.unwrap().is_empty());
    assert_eq!(engine.at_us(), Counter(1));
    assert_eq!(engine.next_deadline_us(), deadline);
    assert_eq!(engine.timers.len(), 1);
    assert_eq!(serde_json::to_value(&engine.reports).unwrap(), reports);
    assert_eq!(serde_json::to_value(&engine.effects).unwrap(), effects);
    for (id, value) in &engine.values {
        assert_eq!(
            &(
                value.sample.clone(),
                value.epoch,
                value.observed,
                value.captured,
                value.captured_sequence
            ),
            &values[id]
        );
    }
    engine.stop();
    assert_eq!(
        { let admitted_frames: Vec<Frame> = ([]).into_iter().collect(); let admitted_now = Counter(engine.at_us().0.max(admitted_frames.iter().map(|frame| frame.captured_us.0).max().unwrap_or(0))); engine.accept_batch(admitted_frames, admitted_now) }.unwrap_err(),
        "Typed mapping is stopped."
    );
}

#[test]
fn empty_batch_leaves_inflight_and_queued_script_callbacks_untouched() {
    let mut program = engine(false).program().clone();
    program.nodes[0].operation = Operation::Script {
        source: "fn on_event(ctx) { ctx }".into(),
        parameters: BTreeMap::new(),
    };
    let mut engine = Phase::new(
        program,
        Counter(1),
        "b3738156-6794-42dc-978b-2376dce0c6c6".into(),
    )
    .unwrap();
    feed(&mut engine, 1, 0, Reason::Baseline, known(false));
    let request = engine.script_requests().pop().unwrap();
    feed(&mut engine, 2, 1, Reason::Change, known(true));
    let pending = serde_json::to_value(&engine.states["processor"].pending).unwrap();
    assert!(!engine.states["processor"].pending.is_empty());
    assert!({ let admitted_frames: Vec<Frame> = ([]).into_iter().collect(); let admitted_now = Counter(engine.at_us().0.max(admitted_frames.iter().map(|frame| frame.captured_us.0).max().unwrap_or(0))); engine.accept_batch(admitted_frames, admitted_now) }.unwrap().is_empty());
    assert!(engine.script_ticket_current(&request.ticket));
    assert_eq!(
        serde_json::to_value(&engine.states["processor"].pending).unwrap(),
        pending
    );
    assert!(engine.script_requests().is_empty());
}
