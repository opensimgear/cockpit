use super::*;
use osg_desktop_output::{
    coordinator::Channel,
    protocol::{native_count, Ack, Adapter, NativeEvent, Request},
    worker::Worker,
};
use osg_mapping::{
    desktop::{Action, Key, PointerAxis},
    engine::{
        typed::{Change, Frame},
        Counter, Reason,
    },
    program::{Endpoint, Program, SignalSample, Sink, SinkKind, SourcePort},
    signal::PortValue,
};
use std::sync::{Arc, Mutex};

struct RecordingAdapter(Arc<Mutex<Vec<NativeEvent>>>);
impl Adapter for RecordingAdapter {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> {
        Ok(true)
    }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.0.lock().unwrap().extend_from_slice(events);
        Ok(native_count(events))
    }
}

struct RecordingChannel(Worker<RecordingAdapter>);
impl Channel for RecordingChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        Ok(self.0.receive(request.clone(), 10))
    }
}

fn clock() -> u64 {
    10
}

fn endpoint(node: &str, port: &str) -> Endpoint {
    Endpoint {
        node_id: node.into(),
        port_id: port.into(),
    }
}

#[test]
fn flush_preserves_pointer_and_key_barriers_then_batches_adjacent_motion() {
    let x = Action::Pointer {
        axis: PointerAxis::X,
        speed: 1000,
        curve: 1.0,
    };
    let x2 = Action::Pointer {
        axis: PointerAxis::X,
        speed: 1000,
        curve: 1.0,
    };
    let y = Action::Pointer {
        axis: PointerAxis::Y,
        speed: 1000,
        curve: 1.0,
    };
    let key = Action::KeyHold { key: Key(4) };
    let source = endpoint("source", "value");
    let button = endpoint("source", "button");
    let program = Program {
        sources: vec![
            SourcePort {
                endpoint: source.clone(),
                contract: x.contract(),
            },
            SourcePort {
                endpoint: button.clone(),
                contract: key.contract(),
            },
        ],
        nodes: vec![],
        sinks: vec![
            Sink {
                id: "x".into(),
                endpoint: endpoint("desktop", "x"),
                from: source.clone(),
                contract: x.contract(),
                kind: SinkKind::Desktop { action: x },
            },
            Sink {
                id: "y".into(),
                endpoint: endpoint("desktop", "y"),
                from: source.clone(),
                contract: y.contract(),
                kind: SinkKind::Desktop { action: y },
            },
            Sink {
                id: "x2".into(),
                endpoint: endpoint("desktop", "x2"),
                from: source.clone(),
                contract: x2.contract(),
                kind: SinkKind::Desktop { action: x2 },
            },
            Sink {
                id: "key".into(),
                endpoint: endpoint("desktop", "key"),
                from: button.clone(),
                contract: key.contract(),
                kind: SinkKind::Desktop { action: key },
            },
        ],
        controllers: vec![],
    };
    let run = "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12";
    let mut phase = Phase::new(program.clone(), Counter(1), run.into()).unwrap();
    phase.drain();
    let events = Arc::new(Mutex::new(vec![]));
    let output = Coordinator::with_channel(
        Box::new(RecordingChannel(Worker::new(RecordingAdapter(
            events.clone(),
        )))),
        clock,
    );
    let run_token = phase.run_token();
    let mut desktop = Desktop {
        output,
        scheduler: osg_mapping::desktop::Scheduler::from_program(run_token.clone(), &program)
            .unwrap(),
        run: run_token,
        scope: Some(Scope::DesktopWide),
        armed: BTreeMap::new(),
        focused: false,
        started: false,
        registered: false,
        last_error: None,
        outcomes: VecDeque::new(),
        dropped_outcomes: 0,
        diagnostics: VecDeque::new(),
        scope_error: None,
        focus_boundary: None,
        waiting_neutral: BTreeSet::new(),
        rearm_evidence: BTreeMap::new(),
        shadow: None,
        audit_shadow: 0,
    };
    phase
        .accept(Frame {
            source: "source".into(),
            generation: Counter(7),
            sequence: Counter(1),
            captured_us: Counter(1),
            reason: Reason::Baseline,
            changes: vec![
                Change {
                    port_id: "value".into(),
                    sample: SignalSample::scalar(PortValue::Number(0.0)),
                },
                Change {
                    port_id: "button".into(),
                    sample: SignalSample::scalar(PortValue::Boolean(false)),
                },
            ],
        })
        .unwrap();
    desktop.start_focused(&phase, true).unwrap();

    let producer = phase.endpoint_token(&source).unwrap();
    let button_producer = phase.endpoint_token(&button).unwrap();
    desktop
        .scheduler
        .accept(
            "x",
            producer.clone(),
            SignalSample::scalar(PortValue::Number(0.5)),
            Counter(1),
            false,
        )
        .unwrap();
    desktop.scheduler.advance(Counter(8_001)).unwrap();
    desktop
        .scheduler
        .accept(
            "key",
            button_producer,
            SignalSample::scalar(PortValue::Boolean(true)),
            Counter(8_001),
            false,
        )
        .unwrap();
    desktop
        .scheduler
        .accept(
            "x",
            producer.clone(),
            SignalSample::scalar(PortValue::Number(0.0)),
            Counter(8_001),
            false,
        )
        .unwrap();
    desktop
        .scheduler
        .accept(
            "y",
            producer.clone(),
            SignalSample::scalar(PortValue::Number(0.5)),
            Counter(8_001),
            false,
        )
        .unwrap();
    desktop.scheduler.advance(Counter(16_001)).unwrap();
    desktop
        .scheduler
        .accept(
            "x2",
            producer,
            SignalSample::scalar(PortValue::Number(0.5)),
            Counter(16_001),
            false,
        )
        .unwrap();
    desktop.scheduler.advance(Counter(24_001)).unwrap();
    desktop.flush(24_001).unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            NativeEvent::Pointer { dx: 4, dy: 0 },
            NativeEvent::Key {
                usage: 4,
                down: true,
                repeat: false
            },
            NativeEvent::Pointer { dx: 0, dy: 4 },
            NativeEvent::Pointer { dx: 4, dy: 4 },
        ],
    );
}

struct BoundedRecordingChannel {
    worker: Worker<RecordingAdapter>,
    groups: Arc<Mutex<Vec<usize>>>,
}
impl Channel for BoundedRecordingChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        use osg_desktop_output::protocol::{Command, FRAME_LIMIT};
        if serde_json::to_vec(request).unwrap().len() > FRAME_LIMIT {
            return Err("Desktop parent frame exceeds 64 KiB.".into());
        }
        match &request.command {
            Command::DeliverPointerBatch { deliveries } => {
                self.groups.lock().unwrap().push(deliveries.len())
            }
            Command::Deliver { delivery }
                if matches!(
                    delivery.intent,
                    osg_mapping::desktop::Intent::Pointer { .. }
                ) =>
            {
                self.groups.lock().unwrap().push(1);
            }
            _ => {}
        }
        Ok(self.worker.receive(request.clone(), 10))
    }
}

#[test]
fn large_valid_tokens_split_before_transport_limit_without_losing_motion() {
    use osg_mapping::{
        desktop::ActionDefinition,
        engine::{
            typed::{Dependency, Token},
            RunToken,
        },
    };
    for (widths, expected_groups) in [
        (vec![128, 128, 256], vec![2, 1]),
        (vec![256, 256], vec![1, 1]),
    ] {
        let run = RunToken {
            session: "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12".into(),
            generation: Counter(1),
        };
        let tokens = widths
            .iter()
            .enumerate()
            .map(|(index, width)| Token {
                run: run.clone(),
                node_id: format!("source-{index}"),
                dependencies: (0..64)
                    .map(|dependency| Dependency {
                        endpoint: Endpoint {
                            node_id: format!("{dependency:02}{}", "n".repeat(width - 2)),
                            port_id: "p".repeat(*width),
                        },
                        generation: Counter(1),
                        epoch: Counter(1),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let definitions = widths
            .iter()
            .enumerate()
            .map(|(index, _)| ActionDefinition {
                id: format!("axis-{index}"),
                action: Action::Pointer {
                    axis: if index == 1 {
                        PointerAxis::Y
                    } else {
                        PointerAxis::X
                    },
                    speed: 1000,
                    curve: 1.0,
                },
            })
            .collect();
        let events = Arc::new(Mutex::new(vec![]));
        let groups = Arc::new(Mutex::new(vec![]));
        let mut output = Coordinator::with_channel(
            Box::new(BoundedRecordingChannel {
                worker: Worker::new(RecordingAdapter(events.clone())),
                groups: groups.clone(),
            }),
            clock,
        );
        output.activate(run.clone(), Scope::DesktopWide).unwrap();
        let mut desktop = Desktop {
            output,
            scheduler: Scheduler::new(run.clone(), definitions).unwrap(),
            run,
            scope: Some(Scope::DesktopWide),
            armed: BTreeMap::new(),
            focused: true,
            started: true,
            registered: true,
            last_error: None,
            outcomes: VecDeque::new(),
            dropped_outcomes: 0,
            diagnostics: VecDeque::new(),
            scope_error: None,
            focus_boundary: None,
            waiting_neutral: BTreeSet::new(),
            rearm_evidence: BTreeMap::new(),
            shadow: None,
            audit_shadow: 0,
        };
        for (index, token) in tokens.iter().enumerate() {
            desktop
                .scheduler
                .accept(
                    &format!("axis-{index}"),
                    token.clone(),
                    SignalSample::scalar(PortValue::Number(0.0)),
                    Counter(0),
                    true,
                )
                .unwrap();
        }
        desktop.scheduler.drain();
        for (index, token) in tokens.iter().enumerate() {
            desktop
                .scheduler
                .accept(
                    &format!("axis-{index}"),
                    token.clone(),
                    SignalSample::scalar(PortValue::Number(0.5)),
                    Counter(1),
                    false,
                )
                .unwrap();
        }
        desktop.scheduler.advance(Counter(8_001)).unwrap();
        let unsplit = osg_desktop_output::protocol::Request {
            sequence: Counter(1),
            deadline_us: None,
            command: osg_desktop_output::protocol::Command::DeliverPointerBatch {
                deliveries: desktop.scheduler.pending_deliveries().cloned().collect(),
            },
        };
        assert!(
            serde_json::to_vec(&unsplit).unwrap().len() > osg_desktop_output::protocol::FRAME_LIMIT
        );
        desktop.flush(8_001).unwrap();
        assert_eq!(*groups.lock().unwrap(), expected_groups);
        let expected = if widths.len() == 3 {
            vec![
                NativeEvent::Pointer { dx: 4, dy: 4 },
                NativeEvent::Pointer { dx: 4, dy: 0 },
            ]
        } else {
            vec![
                NativeEvent::Pointer { dx: 4, dy: 0 },
                NativeEvent::Pointer { dx: 0, dy: 4 },
            ]
        };
        assert_eq!(*events.lock().unwrap(), expected);
        assert_eq!(desktop.outcomes.len(), widths.len());
        for (outcome, token) in desktop.outcomes.iter().zip(tokens) {
            assert_eq!(outcome.delivery.producer, token);
        }
        let snapshot = desktop.output.snapshot();
        assert!(!snapshot.uncertain);
        assert!(serde_json::to_vec(&snapshot).unwrap().len() < 128 * 1024);
    }
}
