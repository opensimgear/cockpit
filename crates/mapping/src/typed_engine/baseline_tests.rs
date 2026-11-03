//! Source-local rebasing must not disturb independent branches.
use super::*;
use crate::signal::PortDescriptor;

fn ep(node: &str, port: &str) -> Endpoint {
    Endpoint {
        node_id: node.into(),
        port_id: port.into(),
    }
}
fn bool_contract() -> PortContract {
    PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 10_000,
        fallback: None,
    }
}
fn value(v: bool) -> SignalSample {
    SignalSample::scalar(PortValue::Boolean(v))
}
fn desc(id: &str, contract: PortContract) -> PortDescriptor {
    PortDescriptor {
        id: id.into(),
        name: id.into(),
        direction: crate::signal::Direction::Output,
        contract,
    }
}
fn binding(port: &str, source: &str) -> InputBinding {
    InputBinding {
        port_id: "input".into(),
        from: ep(source, "value"),
        contract: bool_contract(),
        edge_id: format!("edge-{port}"),
    }
}
fn rule_node(id: &str, source: &str, rule: crate::rules::Rule) -> Node {
    Node {
        id: id.into(),
        inputs: vec![binding(id, source)],
        outputs: vec![desc("value", bool_contract())],
        operation: Operation::Builtin {
            rule,
            inputs: vec!["input".into()],
        },
    }
}
fn sink(id: &str, node: &str) -> Sink {
    Sink {
        id: id.into(),
        endpoint: ep("destination", id),
        from: ep(node, "value"),
        contract: bool_contract(),
        kind: SinkKind::Resource {
            adapter: ResourceAdapter::CompanionFeedback,
        },
    }
}
fn frame(
    source: &str,
    generation: u64,
    sequence: u64,
    at: u64,
    reason: Reason,
    change: Option<bool>,
) -> Frame {
    Frame {
        source: source.into(),
        generation: Counter(generation),
        sequence: Counter(sequence),
        captured_us: Counter(at),
        reason,
        changes: change
            .map(|pressed| {
                vec![Change {
                    port_id: "value".into(),
                    sample: value(pressed),
                }]
            })
            .unwrap_or_default(),
    }
}
fn accept(
    phase: &mut Phase,
    source: &str,
    generation: u64,
    sequence: u64,
    at: u64,
    reason: Reason,
    change: Option<bool>,
) {
    phase
        .accept(
            frame(source, generation, sequence, at, reason, change),
            Counter(at),
        )
        .unwrap();
}
fn bool_sink(phase: &Phase, id: &str) -> Option<bool> {
    let node = id.strip_suffix("-sink").unwrap_or(id);
    phase
        .sample(&ep(node, "value"))
        .and_then(|sample| boolean(sample))
}
fn script_program() -> Program {
    let a = rule_node(
        "a-pulse",
        "a",
        crate::rules::Rule::Pulse {
            duration_us: 1_000,
            repeat_us: 0,
        },
    );
    let toggle = rule_node("a-toggle", "a", crate::rules::Rule::Toggle);
    let script = Node {
        id: "a-script".into(),
        inputs: vec![binding("script", "a")],
        outputs: vec![desc("value", bool_contract())],
        operation: Operation::Script {
            source: "fn on_start(ctx) { ctx } fn on_event(ctx) { ctx }".into(),
            parameters: BTreeMap::new(),
        },
    };
    Program {
        sources: vec![
            SourcePort {
                endpoint: ep("a", "value"),
                contract: bool_contract(),
            },
            SourcePort {
                endpoint: ep("b", "value"),
                contract: bool_contract(),
            },
        ],
        nodes: vec![
            a,
            toggle,
            script,
            rule_node(
                "b-identity",
                "b",
                crate::rules::Rule::Pulse {
                    duration_us: 1_000,
                    repeat_us: 0,
                },
            ),
        ],
        sinks: vec![
            sink("a-pulse-sink", "a-pulse"),
            sink("a-toggle-sink", "a-toggle"),
            sink("a-script-sink", "a-script"),
            sink("b-sink", "b-identity"),
        ],
        controllers: vec![],
    }
}
fn phase(program: Program) -> Phase {
    Phase::new(
        program,
        Counter(1),
        "b3738156-6794-42dc-978b-2376dce0c6c6".into(),
    )
    .unwrap()
}

#[test]
fn another_sources_baseline_loss_and_recovery_preserve_script_ticket_and_rule_state() {
    let mut phase = phase(script_program());
    accept(&mut phase, "a", 1, 1, 1, Reason::Baseline, Some(false));
    let start = phase.script_requests().pop().unwrap();
    assert_eq!(start.callback, "on_start");
    phase
        .script_reply("a-script", &start.ticket, ScriptTransaction::default())
        .unwrap();
    accept(&mut phase, "b", 1, 1, 2, Reason::Baseline, Some(false));
    assert!(phase.script_requests().is_empty(), "B baseline must not enqueue an A callback");

    accept(&mut phase, "a", 1, 2, 10, Reason::Change, Some(true));
    assert_eq!(bool_sink(&phase, "a-pulse-sink"), Some(true));
    assert_eq!(bool_sink(&phase, "a-toggle-sink"), Some(true));
    let inflight = phase.script_requests().pop().unwrap();
    assert_eq!(inflight.callback, "on_event");
    assert_eq!(inflight.inputs["input"], value(true));
    assert!(phase.script_ticket_current(&inflight.ticket));

    // A full baseline, gap, disconnect and replacement baseline on B are all
    // scoped to B. They cannot clear A's queued callback or its latches.
    accept(&mut phase, "b", 1, 2, 20, Reason::Baseline, Some(true));
    assert!(phase.script_requests().is_empty());
    assert!(phase.script_ticket_current(&inflight.ticket));
    assert_eq!(bool_sink(&phase, "a-toggle-sink"), Some(true));
    phase
        .schedule(
            phase.token("b-identity").unwrap(),
            "b-dependent".into(),
            Counter(25),
            BTreeMap::from([(
                "value".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            )]),
        )
        .unwrap();
    accept(&mut phase, "b", 1, 3, 30, Reason::Gap, None);
    assert!(
        phase.timers.is_empty(),
        "B continuity loss must cancel B-owned pending work"
    );
    assert!(phase.script_requests().is_empty());
    assert!(phase.script_ticket_current(&inflight.ticket));
    assert_eq!(bool_sink(&phase, "a-toggle-sink"), Some(true));
    accept(&mut phase, "b", 1, 4, 40, Reason::Baseline, Some(false));
    accept(&mut phase, "b", 1, 5, 50, Reason::Disconnect, None);
    accept(&mut phase, "b", 2, 1, 60, Reason::Baseline, Some(true));
    assert!(phase.script_requests().is_empty());
    assert!(phase.script_ticket_current(&inflight.ticket));
    assert_eq!(bool_sink(&phase, "a-toggle-sink"), Some(true));
    let mut malformed = frame("b", 2, 2, 70, Reason::Change, Some(false));
    malformed.changes.push(malformed.changes[0].clone());
    assert!(phase.accept(malformed, Counter(70)).is_err());
    assert!(phase.script_ticket_current(&inflight.ticket));
    assert_eq!(bool_sink(&phase, "a-pulse-sink"), Some(true));
    assert_eq!(bool_sink(&phase, "a-toggle-sink"), Some(true));
    assert_eq!(phase.next_deadline_us(), Some(Counter(1_010)));
    phase.script_reply("a-script", &inflight.ticket, ScriptTransaction::default()).unwrap();
    assert!(phase.script_requests().is_empty(), "Unrelated recovery must not queue work behind A's inflight request");
}

fn fallback_program() -> Program {
    let mut nodes = vec![Node {
        id: "fallback".into(),
        inputs: vec![binding("fallback", "source")],
        outputs: vec![desc("value", bool_contract())],
        operation: Operation::Fallback {
            unknown: PortValue::Boolean(false),
            stale: PortValue::Boolean(false),
            unavailable: PortValue::Boolean(false),
        },
    }];
    let pulse = rule_node(
        "pulse",
        "fallback",
        crate::rules::Rule::Pulse {
            duration_us: 10_000,
            repeat_us: 0,
        },
    );
    let toggle = rule_node("toggle", "fallback", crate::rules::Rule::Toggle);
    let macro_rule = rule_node(
        "macro",
        "fallback",
        crate::rules::Rule::Macro {
            steps: vec![
                crate::rules::Step {
                    after_us: 0,
                    pressed: true,
                },
                crate::rules::Step {
                    after_us: 1_000,
                    pressed: false,
                },
            ],
        },
    );
    // The rule helpers point to source.value; these inputs are intentionally
    // redirected through the explicit false fallback gate.
    let mut pulse = pulse;
    let mut toggle = toggle;
    let mut macro_rule = macro_rule;
    for node in [&mut pulse, &mut toggle, &mut macro_rule] {
        node.inputs[0].from = ep("fallback", "value");
    }
    nodes.extend([pulse, toggle, macro_rule]);
    Program {
        sources: vec![SourcePort {
            endpoint: ep("source", "value"),
            contract: bool_contract(),
        }],
        nodes,
        sinks: vec![
            sink("pulse-sink", "pulse"),
            sink("toggle-sink", "toggle"),
            sink("macro-sink", "macro"),
        ],
        controllers: vec![],
    }
}

#[test]
fn fallback_false_suppresses_held_baselines_until_neutral_baseline_then_real_press() {
    let mut phase = phase(fallback_program());
    // Startup Unknown is mapped to false. A held first baseline must not be
    // interpreted as a fresh press by any stateful rule.
    accept(&mut phase, "source", 1, 1, 1, Reason::Baseline, Some(true));
    assert_eq!(bool_sink(&phase, "pulse-sink"), Some(false));
    assert_eq!(bool_sink(&phase, "toggle-sink"), Some(false));
    assert_eq!(bool_sink(&phase, "macro-sink"), Some(false));

    accept(&mut phase, "source", 1, 2, 2, Reason::Disconnect, None);
    accept(&mut phase, "source", 2, 1, 3, Reason::Baseline, Some(true));
    assert_eq!(bool_sink(&phase, "pulse-sink"), Some(false));
    assert_eq!(bool_sink(&phase, "toggle-sink"), Some(false));
    assert_eq!(bool_sink(&phase, "macro-sink"), Some(false));

    // A neutral baseline is explicit rearming evidence. Only a subsequent
    // Change can produce the press action.
    accept(&mut phase, "source", 2, 2, 4, Reason::Baseline, Some(false));
    accept(&mut phase, "source", 2, 3, 5, Reason::Change, Some(true));
    assert_eq!(bool_sink(&phase, "pulse-sink"), Some(true));
    assert_eq!(bool_sink(&phase, "toggle-sink"), Some(true));
    assert_eq!(bool_sink(&phase, "macro-sink"), Some(true));
    phase.advance(Counter(1_005)).unwrap();
    assert_eq!(bool_sink(&phase, "macro-sink"), Some(false));
}

#[test]
fn independent_pulse_keeps_its_original_deadline_across_other_source_baselines_and_loss() {
    let mut phase = phase(script_program());
    accept(&mut phase, "a", 1, 1, 1, Reason::Baseline, Some(false));
    accept(&mut phase, "b", 1, 1, 2, Reason::Baseline, Some(false));
    accept(&mut phase, "a", 1, 2, 100, Reason::Change, Some(true));
    assert_eq!(bool_sink(&phase, "a-pulse-sink"), Some(true));
    assert_eq!(phase.next_deadline_us(), Some(Counter(1_100)));

    accept(&mut phase, "b", 1, 2, 200, Reason::Baseline, Some(true));
    assert_eq!(phase.next_deadline_us(), Some(Counter(1_100)));
    accept(&mut phase, "b", 1, 3, 300, Reason::Disconnect, None);
    assert_eq!(phase.next_deadline_us(), Some(Counter(1_100)));
    accept(&mut phase, "b", 2, 1, 400, Reason::Baseline, Some(false));
    assert_eq!(phase.next_deadline_us(), Some(Counter(1_100)));
    phase.advance(Counter(1_099)).unwrap();
    assert_eq!(bool_sink(&phase, "a-pulse-sink"), Some(true));
    phase.advance(Counter(1_100)).unwrap();
    assert_eq!(bool_sink(&phase, "a-pulse-sink"), Some(false));
}
