use super::super::*;
use super::*;
use crate::signal::{Direction, PortDescriptor};

fn ep(node: &str, port: &str) -> Endpoint {
    Endpoint {
        node_id: node.into(),
        port_id: port.into(),
    }
}
fn contract() -> PortContract {
    PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 100,
        fallback: None,
    }
}
fn value(value: bool) -> SignalSample {
    SignalSample::scalar(PortValue::Boolean(value))
}
fn node(id: &str, source: &str, port: &str, operation: Operation) -> Node {
    Node {
        id: id.into(),
        inputs: vec![InputBinding {
            port_id: "input".into(),
            from: ep(source, port),
            contract: contract(),
            edge_id: format!("edge-{id}"),
        }],
        outputs: vec![PortDescriptor {
            id: "value".into(),
            name: "value".into(),
            direction: Direction::Output,
            contract: contract(),
        }],
        operation,
    }
}
fn program(script: bool) -> Program {
    let mut nodes = vec![
        node("mapped", "game", "x", Operation::Identity {}),
        node(
            "pulse",
            "game",
            "x",
            Operation::Builtin {
                rule: crate::rules::Rule::Pulse {
                    duration_us: 1_000,
                    repeat_us: 0,
                },
                inputs: vec!["input".into()],
            },
        ),
        node("other", "game", "z", Operation::Identity {}),
        node(
            "pad-pulse",
            "pad",
            "value",
            Operation::Builtin {
                rule: crate::rules::Rule::Pulse {
                    duration_us: 1_000,
                    repeat_us: 0,
                },
                inputs: vec!["input".into()],
            },
        ),
    ];
    if script {
        nodes.push(node(
            "script",
            "game",
            "x",
            Operation::Script {
                source: "fn on_start(ctx) { ctx } fn on_event(ctx) { ctx }".into(),
                parameters: BTreeMap::new(),
            },
        ));
    }
    Program {
        sources: [
            ep("game", "x"),
            ep("game", "y"),
            ep("game", "z"),
            ep("pad", "value"),
        ]
        .into_iter()
        .map(|endpoint| SourcePort {
            endpoint,
            contract: contract(),
        })
        .collect(),
        sinks: nodes
            .iter()
            .map(|node| Sink {
                id: format!("{}-sink", node.id),
                endpoint: ep("destination", &node.id),
                from: ep(&node.id, "value"),
                contract: contract(),
                kind: SinkKind::Resource {
                    adapter: ResourceAdapter::CompanionFeedback,
                },
            })
            .collect(),
        nodes,
        controllers: vec![],
    }
}
fn phase(script: bool, policy: NativePolicy) -> Phase {
    let mut phase = Phase::new(
        program(script),
        Counter(1),
        "b3738156-6794-42dc-978b-2376dce0c6c6".into(),
    )
    .unwrap();
    phase.pin_native_clock("clock").unwrap();
    phase
        .register_native_source(NativeSourceRegistration {
            source: "game".into(),
            clock_id: "clock".into(),
            ports: [("x", "bios"), ("y", "bios"), ("z", "ownship")]
                .into_iter()
                .map(|(port, family)| NativePortRegistration {
                    port_id: port.into(),
                    family: family.into(),
                    policy,
                })
                .collect(),
        })
        .unwrap();
    phase.drain();
    phase
}
fn capture(sequence: u64, at: u64) -> NativeCapture {
    NativeCapture {
        producer_id: "producer".into(),
        generation: Counter(1),
        sequence: Counter(sequence),
        byte_offset: None,
        captured_us: Counter(at),
    }
}
fn frame(
    sequence: u64,
    changes: &[(&str, NativeChangeKind, u64, Option<bool>, u64, u64)],
) -> NativeFrame {
    let mut frame = NativeFrame {
        source: "game".into(),
        generation: Counter(1),
        sequence: Counter(sequence),
        clock_id: "clock".into(),
        captures: vec![],
        leases: vec![],
        changes: vec![],
    };
    for (port, kind, continuity, sample, sequence, at) in changes {
        let contributors = sample
            .map(|_| {
                let capture = capture(*sequence, *at);
                let index = frame
                    .captures
                    .iter()
                    .position(|existing| existing == &capture)
                    .unwrap_or_else(|| {
                        frame.captures.push(capture);
                        frame.captures.len() - 1
                    });
                vec![NativeContributor {
                    input: NativeInput::Channel((*port).into()),
                    capture: index as u32,
                    validity: NativeValidity::Sampled {
                        valid_until_us: Counter(at + 100_000),
                    },
                }]
            })
            .unwrap_or_default();
        frame.changes.push(NativeChange {
            port_id: (*port).into(),
            continuity: Counter(*continuity),
            kind: *kind,
            sample: sample.map(value).unwrap_or(SignalSample::Unknown),
            contributors,
        });
    }
    frame
}
fn baseline(sequence: u64, at: u64, pressed: bool) -> NativeFrame {
    frame(
        sequence,
        &[
            (
                "x",
                NativeChangeKind::Baseline,
                1,
                Some(pressed),
                sequence,
                at,
            ),
            ("y", NativeChangeKind::Baseline, 1, None, sequence, at),
        ],
    )
}
fn update(sequence: u64, at: u64, pressed: bool) -> NativeFrame {
    frame(
        sequence,
        &[(
            "x",
            NativeChangeKind::Update,
            1,
            Some(pressed),
            sequence,
            at,
        )],
    )
}
fn admit(phase: &mut Phase, frame: NativeFrame, now: u64) {
    phase
        .accept_admissions([frame.into()], Counter(now))
        .unwrap();
}
fn direct(sequence: u64, reason: Reason, pressed: Option<bool>, at: u64) -> Admission {
    Frame {
        source: "pad".into(),
        generation: Counter(1),
        sequence: Counter(sequence),
        captured_us: Counter(at),
        reason,
        changes: pressed
            .map(|v| {
                vec![Change {
                    port_id: "value".into(),
                    sample: value(v),
                }]
            })
            .unwrap_or_default(),
    }
    .into()
}
fn latched(
    mut frame: NativeFrame,
    observation_sequence: u64,
    at: u64,
    deadline: u64,
) -> NativeFrame {
    let observation = capture(observation_sequence, at);
    let index = frame
        .captures
        .iter()
        .position(|existing| existing == &observation)
        .unwrap_or_else(|| {
            frame.captures.push(observation);
            frame.captures.len() - 1
        });
    frame.leases.push(NativeLease {
        domain_id: "lease".into(),
        generation: Counter(1),
        observation: index as u32,
        valid_until_us: Counter(deadline),
    });
    for change in &mut frame.changes {
        for contributor in &mut change.contributors {
            contributor.validity = NativeValidity::Latched { lease: 0 };
        }
    }
    frame
}

#[test]
fn complete_family_baseline_registers_absent_members_and_pins_clock_policy() {
    let mut phase = phase(false, NativePolicy::Sampled);
    assert!(phase.pin_native_clock("other").is_err());
    assert!(phase
        .accept_admissions(
            [frame(
                1,
                &[("x", NativeChangeKind::Baseline, 1, Some(false), 1, 1)]
            )
            .into()],
            Counter(1)
        )
        .is_err());
    admit(&mut phase, baseline(1, 1, false), 1);
    assert_eq!(phase.sample(&ep("game", "y")), Some(&SignalSample::Unknown));
    assert_eq!(phase.sample(&ep("mapped", "value")), Some(&value(false)));
    assert_eq!(phase.sample(&ep("game", "z")), Some(&SignalSample::Unknown));
    assert!(phase
        .observe_liveness("game", Counter(1), Counter(2))
        .is_err());
    let mut registration = phase.native.registrations["game"].clone();
    registration.ports[0].policy = NativePolicy::Latched;
    assert!(phase.register_native_source(registration).is_err());
}

#[test]
fn pulse_survives_ordinary_release_and_equal_updates_but_rebase_cancels() {
    let mut phase = phase(false, NativePolicy::Sampled);
    admit(&mut phase, baseline(1, 1, false), 1);
    admit(&mut phase, update(2, 10, true), 10);
    let token = phase.token("pulse").unwrap();
    admit(&mut phase, update(3, 11, false), 11);
    admit(&mut phase, update(4, 12, false), 12);
    assert_eq!(phase.token("pulse").unwrap(), token);
    assert_eq!(phase.sample(&ep("pulse", "value")), Some(&value(true)));
    assert!(!phase.endpoint_has_current_native_level(&ep("pulse", "value")));
    phase.advance(Counter(1010)).unwrap();
    assert_eq!(phase.sample(&ep("pulse", "value")), Some(&value(false)));
    admit(&mut phase, update(5, 1020, true), 1020);
    let rebase = frame(
        6,
        &[
            ("x", NativeChangeKind::Baseline, 2, Some(true), 6, 1030),
            ("y", NativeChangeKind::Baseline, 2, None, 6, 1030),
        ],
    );
    admit(&mut phase, rebase, 1030);
    assert_ne!(phase.token("pulse").unwrap(), token);
    assert_eq!(phase.sample(&ep("pulse", "value")), Some(&value(false)));
    assert!(phase.states["pulse"].rule.next().is_none());
}

#[test]
fn both_mixed_drain_orders_revoke_native_and_direct_before_due_timers() {
    for reverse in [false, true] {
        let mut phase = phase(false, NativePolicy::Sampled);
        admit(&mut phase, baseline(1, 1, false), 1);
        phase
            .accept_admissions([direct(1, Reason::Baseline, Some(false), 1)], Counter(1))
            .unwrap();
        admit(&mut phase, update(2, 10, true), 10);
        phase
            .accept_admissions([direct(2, Reason::Change, Some(true), 10)], Counter(10))
            .unwrap();
        phase.drain();
        let mut drain = vec![
            direct(3, Reason::Disconnect, None, 11),
            frame(
                3,
                &[
                    ("x", NativeChangeKind::Withdraw, 1, None, 0, 0),
                    ("y", NativeChangeKind::Withdraw, 1, None, 0, 0),
                ],
            )
            .into(),
        ];
        if reverse {
            drain.reverse();
        }
        phase.accept_admissions(drain, Counter(1010)).unwrap();
        assert!(phase
            .drain()
            .iter()
            .all(|effect| effect.reason != ReportReason::Timer));
        assert!(phase.states["pulse"].rule.next().is_none());
        assert!(phase.states["pad-pulse"].rule.next().is_none());
    }
}

#[test]
fn lease_only_genuine_renewal_retains_original_capture_without_callback_or_rearm() {
    let mut phase = phase(true, NativePolicy::Latched);
    admit(&mut phase, latched(baseline(1, 1, false), 1, 1, 200_000), 1);
    let request = phase.script_requests().pop().unwrap();
    phase
        .script_reply("script", &request.ticket, ScriptTransaction::default())
        .unwrap();
    let original = phase.values[&ep("game", "x")].captured;
    let token = phase.token("pulse").unwrap();
    phase.drain();
    let renewal = latched(frame(2, &[]), 2, 90_000, 250_000);
    admit(&mut phase, renewal, 90_000);
    assert_eq!(phase.values[&ep("game", "x")].captured, original);
    assert_eq!(phase.token("pulse").unwrap(), token);
    assert!(phase.script_requests().is_empty());
    assert!(phase.drain().is_empty());
    assert_eq!(phase.next_deadline_us(), Some(Counter(190_000)));
    let retained = latched(
        frame(3, &[("x", NativeChangeKind::Update, 1, Some(false), 1, 1)]),
        3,
        95_000,
        260_000,
    );
    admit(&mut phase, retained, 95_000);
    assert!(phase.script_requests().is_empty());
    assert!(phase.drain().is_empty());
    assert_eq!(phase.values[&ep("game", "x")].captured, 1);
    phase.advance(Counter(100_001)).unwrap();
    assert_eq!(phase.sample(&ep("mapped", "value")), Some(&value(false)));
}

#[path = "causal_tests.rs"]
mod causal;
#[path = "recovery_tests.rs"]
mod recovery;
#[path = "retained_bytes_tests.rs"]
mod retained_bytes;
#[path = "validation_tests.rs"]
mod validation;

#[path = "desktop_tests.rs"]
mod desktop_cases;

#[path = "quality_tests.rs"]
mod quality;
