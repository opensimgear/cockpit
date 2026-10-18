use super::*;
use crate::profile::{AxisRange, OutputControl};
use crate::signal::{ActionParameter, Direction, PortDescriptor, TextEncoding};

fn endpoint(node: &str, port: &str) -> Endpoint {
    Endpoint {
        node_id: node.into(),
        port_id: port.into(),
    }
}
fn boolean_contract() -> PortContract {
    state(ScalarSchema::Boolean {})
}
fn number_contract(min: f64, max: f64, unit: Option<&str>) -> PortContract {
    state(ScalarSchema::Number {
        min,
        max,
        unit: unit.map(str::to_owned),
    })
}
fn integer_contract(min: i64, max: i64) -> PortContract {
    state(ScalarSchema::Integer {
        min,
        max,
        unit: None,
    })
}
fn state(schema: ScalarSchema) -> PortContract {
    PortContract::State {
        schema,
        freshness_ms: 1000,
        fallback: None,
    }
}
fn event() -> PortContract {
    PortContract::Event {
        schema: ScalarSchema::Boolean {},
    }
}
fn descriptor(id: &str, direction: Direction, contract: PortContract) -> PortDescriptor {
    PortDescriptor {
        id: id.into(),
        name: id.into(),
        direction,
        contract,
    }
}
fn input(id: &str, node: &str, port: &str, contract: PortContract) -> InputBinding {
    InputBinding {
        port_id: id.into(),
        from: endpoint(node, port),
        contract,
        edge_id: format!("edge-{node}-{port}-{id}"),
    }
}
fn source(node: &str, port: &str, contract: PortContract) -> SourcePort {
    SourcePort {
        endpoint: endpoint(node, port),
        contract,
    }
}
fn resource_sink(id: &str, from: Endpoint, contract: PortContract) -> Sink {
    Sink {
        id: id.into(),
        endpoint: endpoint("destination", id),
        from,
        contract,
        kind: SinkKind::Resource {
            adapter: ResourceAdapter::CompanionFeedback,
        },
    }
}
fn unary(operation: Operation, from: PortContract, to: PortContract) -> Program {
    Program {
        sources: vec![source("source", "value", from.clone())],
        nodes: vec![Node {
            id: "processor".into(),
            inputs: vec![input("input", "source", "value", from)],
            outputs: vec![descriptor("output", Direction::Output, to.clone())],
            operation,
        }],
        sinks: vec![resource_sink("sink", endpoint("processor", "output"), to)],
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
fn sample(value: f64) -> SignalSample {
    SignalSample::scalar(PortValue::Number(value))
}
fn pressed(value: bool) -> SignalSample {
    SignalSample::scalar(PortValue::Boolean(value))
}
fn frame(
    node: &str,
    generation: u64,
    sequence: u64,
    at: u64,
    reason: Reason,
    changes: Vec<(&str, SignalSample)>,
) -> Frame {
    Frame {
        source: node.into(),
        generation: Counter(generation),
        sequence: Counter(sequence),
        captured_us: Counter(at),
        reason,
        changes: changes
            .into_iter()
            .map(|(port, sample)| Change {
                port_id: port.into(),
                sample,
            })
            .collect(),
    }
}
fn feed(engine: &mut Phase, sequence: u64, at: u64, value: SignalSample) {
    engine
        .accept(frame(
            "source",
            1,
            sequence,
            at,
            if sequence == 1 {
                Reason::Baseline
            } else {
                Reason::Change
            },
            vec![("value", value)],
        ))
        .unwrap();
}
fn output(engine: &Phase) -> SignalSample {
    engine
        .sample(&endpoint("processor", "output"))
        .unwrap()
        .clone()
}
fn scalar_output(engine: &Phase) -> f64 {
    number(&output(engine)).unwrap()
}

#[test]
fn desktop_projection_resets_rule_state_prunes_unrelated_scripts_and_keeps_native_frame_shape() {
    let mut program = unary(
        Operation::Builtin {
            rule: crate::rules::Rule::Toggle,
            inputs: vec!["input".into()],
        },
        boolean_contract(),
        boolean_contract(),
    );
    program
        .sources
        .push(source("source", "other", boolean_contract()));
    program.nodes.push(Node {
        id: "unrelated-script".into(),
        inputs: vec![input("input", "source", "other", boolean_contract())],
        outputs: vec![descriptor("output", Direction::Output, boolean_contract())],
        operation: Operation::Script {
            source: "fn on_start(ctx) { transaction(#{output: scalar(true)}, [], #{}) }".into(),
            parameters: BTreeMap::new(),
        },
    });
    let action = crate::desktop::Action::KeyHold {
        key: crate::desktop::Key(4),
    };
    program.sinks.push(Sink {
        id: "desktop".into(),
        endpoint: endpoint("keyboard", "action"),
        from: endpoint("processor", "output"),
        contract: action.contract(),
        kind: SinkKind::Desktop { action },
    });
    let mut canonical = phase(program);
    canonical
        .accept(frame(
            "source",
            1,
            1,
            10,
            Reason::Baseline,
            vec![("value", pressed(false)), ("other", pressed(false))],
        ))
        .unwrap();
    canonical
        .accept(frame(
            "source",
            1,
            2,
            20,
            Reason::Change,
            vec![("value", pressed(true))],
        ))
        .unwrap();
    assert_eq!(output(&canonical), pressed(true));
    let mut desktop = canonical.fork_desktop().unwrap();
    assert_eq!(desktop.run_token(), canonical.run_token());
    assert_eq!(desktop.at_us(), canonical.at_us());
    assert_eq!(desktop.program().sources, canonical.program().sources);
    assert_eq!(desktop.program().nodes.len(), 1);
    assert_eq!(desktop.program().nodes[0].id, "processor");
    assert_eq!(desktop.program().sinks.len(), 1);
    assert_eq!(output(&desktop), pressed(false));
    assert!(desktop.drain().is_empty());
    let original_baseline = frame(
        "source",
        1,
        3,
        30,
        Reason::Baseline,
        vec![("value", pressed(false)), ("other", pressed(true))],
    );
    canonical.accept(original_baseline.clone()).unwrap();
    desktop.accept_batch([original_baseline]).unwrap();
    assert_eq!(
        desktop.sample(&endpoint("source", "other")),
        Some(&pressed(true))
    );
    let producer = desktop
        .endpoint_token(&endpoint("processor", "output"))
        .unwrap();
    let observations = desktop.source_observations(&producer);
    assert_eq!(observations[0].at_us, Counter(30));
    assert_eq!(observations[0].sequence, Counter(3));
}
#[test]
fn desktop_return_cannot_seed_hysteresis_from_cached_or_delayed_prefocus_axis_values() {
    let mut program = unary(
        Operation::Hysteresis { press: 0.75, release: 0.25, comparison: Comparison::GreaterEqual },
        number_contract(0.0, 1.0, None), boolean_contract(),
    );
    let action = crate::desktop::Action::KeyHold { key: crate::desktop::Key(4) };
    program.sinks.push(Sink {
        id: "desktop".into(), endpoint: endpoint("keyboard", "action"), from: endpoint("processor", "output"),
        contract: action.contract(), kind: SinkKind::Desktop { action },
    });
    let mut canonical = phase(program);
    feed(&mut canonical, 1, 1, sample(1.0));
    canonical.advance(Counter(200)).unwrap();
    assert_eq!(output(&canonical), pressed(true));
    let mut desktop = canonical.fork_desktop().unwrap();
    desktop.require_desktop_neutral_after(Counter(200)).unwrap();
    feed(&mut canonical, 2, 150, sample(1.0));
    feed(&mut desktop, 2, 150, sample(1.0));
    assert_eq!(output(&desktop), SignalSample::Unknown);
    feed(&mut canonical, 3, 201, sample(0.5));
    feed(&mut desktop, 3, 201, sample(0.5));
    assert_eq!(output(&canonical), pressed(true));
    assert_eq!(output(&desktop), pressed(false), "Fresh midrange input must use cold hysteresis state");
    feed(&mut desktop, 4, 202, sample(1.0));
    assert_eq!(output(&desktop), pressed(true), "A fresh threshold crossing still activates normally");
}

#[test]
fn live_constructor_validates_its_clock_before_evaluating_startup() {
    let program = unary(Operation::Not {}, boolean_contract(), boolean_contract());
    let session = uuid::Uuid::new_v4().to_string();
    for invalid in [0, u64::MAX - 600_000_000 + 1, u64::MAX] {
        let result = Phase::new_at(
            program.clone(),
            Counter(1),
            session.clone(),
            Counter(invalid),
        );
        assert!(
            matches!(result, Err(error) if error == "A live typed mapping needs a nonzero bounded capture clock.")
        );
    }
    for now in [1, u64::MAX - 600_000_000] {
        let mut engine =
            Phase::new_at(program.clone(), Counter(1), session.clone(), Counter(now)).unwrap();
        assert_eq!(engine.at_us(), Counter(now));
        assert!(engine
            .drain()
            .iter()
            .all(|effect| effect.at_us == Counter(now)));
        assert_eq!(
            engine.inspected_sample(&endpoint("source", "value")),
            Some((&SignalSample::Unknown, None))
        );
    }
    assert_eq!(phase(program).at_us(), Counter(0));
}

#[test]
fn live_constructor_preserves_actual_source_age_including_zero_time() {
    let mut engine = Phase::new_at(
        unary(
            Operation::Identity {},
            boolean_contract(),
            boolean_contract(),
        ),
        Counter(1),
        uuid::Uuid::new_v4().to_string(),
        Counter(2_000_000),
    )
    .unwrap();
    engine.drain();
    feed(&mut engine, 1, 0, pressed(true));
    assert_eq!(engine.at_us(), Counter(2_000_000));
    assert_eq!(
        engine.inspected_sample(&endpoint("source", "value")),
        Some((&SignalSample::Stale, Some(Counter(0))))
    );
    assert_eq!(output(&engine), SignalSample::Stale);
    assert!(engine
        .drain()
        .iter()
        .all(|effect| !matches!(effect.sample, SignalSample::Known(_))));
    engine
        .accept(frame(
            "source",
            1,
            2,
            1_999_990,
            Reason::Baseline,
            vec![("value", pressed(false))],
        ))
        .unwrap();
    assert_eq!(output(&engine), pressed(false));
    engine.advance(Counter(2_000_005)).unwrap();
    assert_eq!(
        engine.inspected_sample(&endpoint("source", "value")),
        Some((&pressed(false), Some(Counter(1_999_990))))
    );
    assert!(engine
        .accept(frame(
            "source",
            1,
            3,
            u64::MAX,
            Reason::Change,
            vec![("value", pressed(true))]
        ))
        .is_err());
    assert_eq!(engine.at_us(), Counter(2_000_005));
    assert_eq!(output(&engine), SignalSample::Unknown);
}

#[test]
fn inspected_samples_keep_unobserved_and_zero_time_distinct_without_mutating_execution() {
    let mut engine = Phase::new(
        unary(Operation::Not {}, boolean_contract(), boolean_contract()),
        Counter(1),
        uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    assert_eq!(
        engine.inspected_sample(&endpoint("source", "value")),
        Some((&SignalSample::Unknown, None))
    );
    assert!(engine
        .inspected_sample(&endpoint("missing", "port"))
        .is_none());
    feed(
        &mut engine,
        1,
        0,
        SignalSample::scalar(PortValue::Boolean(true)),
    );
    let before = (
        engine.clock,
        engine.effects.len(),
        engine.reports.len(),
        engine.sequence,
    );
    for _ in 0..20 {
        assert_eq!(
            engine.inspected_sample(&endpoint("source", "value")),
            Some((
                &SignalSample::scalar(PortValue::Boolean(true)),
                Some(Counter(0))
            ))
        );
    }
    assert_eq!(
        (
            engine.clock,
            engine.effects.len(),
            engine.reports.len(),
            engine.sequence
        ),
        before
    );
}
fn invocation(parameters: Vec<ActionParameter>, semantics: ActionSemantics) -> PortContract {
    PortContract::Invocation {
        parameters,
        semantics,
    }
}
fn action(parameters: Vec<ActionParameter>, semantics: ActionSemantics) -> PortContract {
    PortContract::Action {
        parameters,
        semantics,
        endpoint: "advertised.command".into(),
        readable: false,
        display: None,
        ownership: None,
    }
}
fn parameter(id: &str, schema: ScalarSchema) -> ActionParameter {
    ActionParameter {
        id: id.into(),
        name: id.into(),
        schema,
    }
}
fn packed(values: &[(&str, PortValue)]) -> SignalValue {
    SignalValue::ActionPack(ActionPack {
        arguments: values
            .iter()
            .map(|(id, value)| NamedArgument {
                id: (*id).into(),
                value: value.clone(),
            })
            .collect(),
    })
}

#[test]
fn unit_conversion_hysteresis_and_missing_status_replay() {
    let ft = number_contract(0.0, 12000.0, Some("ft"));
    let metres = number_contract(0.0, 3657.6, Some("m"));
    let mut program = unary(
        Operation::UnitConvert {
            from: "ft".into(),
            to: "m".into(),
            rounding: Rounding::None,
        },
        ft,
        metres.clone(),
    );
    program.nodes.push(Node {
        id: "threshold".into(),
        inputs: vec![input("input", "processor", "output", metres)],
        outputs: vec![descriptor("output", Direction::Output, boolean_contract())],
        operation: Operation::Hysteresis {
            press: 3000.0,
            release: 2900.0,
            comparison: Comparison::GreaterEqual,
        },
    });
    program.sinks.push(resource_sink(
        "threshold-result",
        endpoint("threshold", "output"),
        boolean_contract(),
    ));
    let mut engine = phase(program);
    feed(&mut engine, 1, 0, sample(10000.0));
    assert!((scalar_output(&engine) - 3048.0).abs() < 1e-9);
    assert_eq!(
        engine.sample(&endpoint("threshold", "output")),
        Some(&pressed(true))
    );
    feed(&mut engine, 2, 10, sample(2900.0 / 0.3048));
    assert_eq!(
        engine.sample(&endpoint("threshold", "output")),
        Some(&pressed(false))
    );
    engine.advance(Counter(1_000_010)).unwrap();
    assert_eq!(output(&engine), SignalSample::Stale);
    feed(&mut engine, 3, 1_000_011, SignalSample::Unknown);
    assert_eq!(output(&engine), SignalSample::Unknown);
    engine
        .accept(frame("source", 1, 4, 1_000_012, Reason::Disconnect, vec![]))
        .unwrap();
    assert_eq!(output(&engine), SignalSample::Unavailable);
}

#[test]
fn declared_raw_quantity_keeps_values_and_missing_status_through_explicit_conversion() {
    let raw = integer_contract(0, 65535);
    let feet = state(ScalarSchema::Integer {
        min: 0,
        max: 65535,
        unit: Some("ft".into()),
    });
    let metres = number_contract(0.0, 65535.0 * 0.3048, Some("m"));
    let mut program = unary(
        Operation::DeclareQuantity {
            quantity: "DCS-BIOS reported MSL altitude".into(),
            unit: "ft".into(),
        },
        raw,
        feet.clone(),
    );
    program.nodes.push(Node {
        id: "metres".into(),
        inputs: vec![input("input", "processor", "output", feet)],
        outputs: vec![descriptor("output", Direction::Output, metres.clone())],
        operation: Operation::UnitConvert {
            from: "ft".into(),
            to: "m".into(),
            rounding: Rounding::None,
        },
    });
    program.nodes.push(Node {
        id: "threshold".into(),
        inputs: vec![input("input", "metres", "output", metres)],
        outputs: vec![descriptor("output", Direction::Output, boolean_contract())],
        operation: Operation::Hysteresis {
            comparison: Comparison::GreaterEqual,
            press: 3000.0,
            release: 2900.0,
        },
    });
    let mut engine = phase(program);
    feed(&mut engine, 1, 0, SignalSample::Unknown);
    assert_eq!(output(&engine), SignalSample::Unknown);
    for (sequence, raw, expected) in [(2, 10000.0, true), (3, 9600.0, true), (4, 9500.0, false)] {
        feed(&mut engine, sequence, sequence, sample(raw));
        assert_eq!(scalar_output(&engine), raw);
        assert_eq!(
            engine.sample(&endpoint("metres", "output")),
            Some(&sample(raw * 0.3048))
        );
        assert_eq!(
            engine.sample(&endpoint("threshold", "output")),
            Some(&pressed(expected))
        );
    }
    engine.advance(Counter(1_000_005)).unwrap();
    for id in ["processor", "metres", "threshold"] {
        assert_eq!(
            engine.sample(&endpoint(id, "output")),
            Some(&SignalSample::Stale)
        );
    }
    feed(&mut engine, 5, 1_000_006, SignalSample::Unknown);
    assert_eq!(output(&engine), SignalSample::Unknown);
    engine
        .accept(frame("source", 1, 6, 1_000_007, Reason::Disconnect, vec![]))
        .unwrap();
    for id in ["processor", "metres", "threshold"] {
        assert_eq!(
            engine.sample(&endpoint(id, "output")),
            Some(&SignalSample::Unavailable)
        );
    }
}

#[test]
fn quantity_declarations_cannot_hide_scaling_type_changes_or_existing_units() {
    let raw = integer_contract(0, 65535);
    let feet = state(ScalarSchema::Integer {
        min: 0,
        max: 65535,
        unit: Some("ft".into()),
    });
    let declaration = Operation::DeclareQuantity {
        quantity: "Reported MSL altitude".into(),
        unit: "ft".into(),
    };
    assert!(operation_valid(
        declaration.clone(),
        vec![raw.clone()],
        feet.clone()
    ));
    assert!(operation_valid(
        declaration.clone(),
        vec![number_contract(-100.0, 100.0, None)],
        number_contract(-100.0, 100.0, Some("ft"))
    ));
    for invalid in [
        feet.clone(),
        boolean_contract(),
        number_contract(0.0, 65535.0, None),
    ] {
        assert!(!operation_valid(
            declaration.clone(),
            vec![invalid],
            feet.clone()
        ));
    }
    for invalid in [
        integer_contract(0, 65535),
        state(ScalarSchema::Integer {
            min: 0,
            max: 65534,
            unit: Some("ft".into()),
        }),
        state(ScalarSchema::Integer {
            min: 0,
            max: 65535,
            unit: Some("m".into()),
        }),
        number_contract(0.0, 65535.0, Some("ft")),
    ] {
        assert!(!operation_valid(
            declaration.clone(),
            vec![raw.clone()],
            invalid
        ));
    }
    for text in [
        "".to_owned(),
        " ".into(),
        " padded ".into(),
        "line\nbreak".into(),
        "x".repeat(129),
    ] {
        assert!(!operation_valid(
            Operation::DeclareQuantity {
                quantity: text.clone(),
                unit: "ft".into()
            },
            vec![raw.clone()],
            feet.clone()
        ));
        assert!(!operation_valid(
            Operation::DeclareQuantity {
                quantity: "Altitude".into(),
                unit: text
            },
            vec![raw.clone()],
            feet.clone()
        ));
    }
    assert!(!operation_valid(
        Operation::Scale {
            factor: 1.0,
            offset: 0.0
        },
        vec![raw.clone()],
        feet.clone()
    ));
    assert!(!operation_valid(
        Operation::RangeMap {
            input_min: 0.0,
            input_max: 65535.0,
            output_min: 0.0,
            output_max: 65535.0,
            rounding: Rounding::None
        },
        vec![raw],
        feet
    ));
    let original = unary(
        declaration,
        integer_contract(0, 65535),
        state(ScalarSchema::Integer {
            min: 0,
            max: 65535,
            unit: Some("ft".into()),
        }),
    );
    let mut relabelled = original.clone();
    if let Operation::DeclareQuantity { quantity, .. } = &mut relabelled.nodes[0].operation {
        *quantity = "Reported AGL altitude".into();
    }
    assert_ne!(
        original.compile().unwrap().hash,
        relabelled.compile().unwrap().hash
    );
}

#[test]
fn raw_quantity_events_preserve_occurrences_without_replaying_baselines() {
    let raw = PortContract::Event {
        schema: ScalarSchema::Integer {
            min: 0,
            max: 65535,
            unit: None,
        },
    };
    let feet = PortContract::Event {
        schema: ScalarSchema::Integer {
            min: 0,
            max: 65535,
            unit: Some("ft".into()),
        },
    };
    let mut engine = phase(unary(
        Operation::DeclareQuantity {
            quantity: "Reported altitude".into(),
            unit: "ft".into(),
        },
        raw,
        feet,
    ));
    engine.drain();
    feed(&mut engine, 1, 0, sample(10000.0));
    assert!(engine.drain().is_empty());
    for sequence in 2..=3 {
        feed(&mut engine, sequence, sequence, sample(10000.0));
        let effects = engine.drain();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].sample, sample(10000.0));
    }
}

#[test]
fn raw_numeric_ranges_and_multiple_virtual_layouts_are_preserved() {
    let raw = integer_contract(0, 65535);
    let axis = integer_contract(0, 32767);
    let mut program = unary(
        Operation::RangeMap {
            input_min: 0.0,
            input_max: 65535.0,
            output_min: 0.0,
            output_max: 32767.0,
            rounding: Rounding::Nearest,
        },
        raw,
        axis.clone(),
    );
    for id in ["vjoy-a", "vjoy-b"] {
        program.controllers.push(VirtualController {
            id: id.into(),
            layout: ControllerLayout {
                buttons: vec![],
                axes: vec![ControllerAxis {
                    port_id: "throttle".into(),
                    range: AxisRange::Unipolar,
                    fallback: 42,
                }],
                hats: vec![],
            },
        });
        program.sinks.push(Sink {
            id: format!("to-{id}"),
            endpoint: endpoint(id, "throttle"),
            from: endpoint("processor", "output"),
            contract: axis.clone(),
            kind: SinkKind::Virtual {
                controller_id: id.into(),
                control: OutputControl::Axis { index: 0 },
            },
        });
    }
    let mut engine = phase(program);
    let initial = engine.drain_reports();
    assert_eq!(
        initial
            .iter()
            .map(|report| (report.target.as_str(), report.sequence.0))
            .collect::<Vec<_>>(),
        vec![("vjoy-a", 1), ("vjoy-b", 2)]
    );
    let mut effects = engine.drain();
    feed(&mut engine, 1, 0, sample(32768.0));
    assert_eq!(scalar_output(&engine), 16384.0);
    let reports = engine.drain_reports();
    assert_eq!(reports.len(), 2);
    assert_eq!(
        reports
            .iter()
            .map(|report| report.sequence.0)
            .collect::<Vec<_>>(),
        vec![3, 4]
    );
    assert!(reports.iter().all(|report| report.state.axes == [16384]));
    effects.extend(engine.drain());
    engine.advance(Counter(1)).unwrap();
    assert!(engine.drain_reports().is_empty());
    assert!(engine.drain().is_empty());
    engine.stop();
    let stopped = engine.drain_reports();
    assert_eq!(
        stopped
            .iter()
            .map(|report| report.sequence.0)
            .collect::<Vec<_>>(),
        vec![5, 6]
    );
    assert!(stopped.iter().all(|report| report.state.axes == [42]));
    effects.extend(engine.drain());
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.sequence.0)
            .collect::<Vec<_>>(),
        (1..=9).collect::<Vec<_>>()
    );
    assert!(effects
        .iter()
        .any(|effect| effect.sample == SignalSample::Unavailable));
}

#[test]
fn any_and_all_keep_other_source_facts_on_disconnect() {
    for (operation, held, expected) in [
        (Operation::Any {}, true, true),
        (Operation::All {}, false, false),
    ] {
        let contract = boolean_contract();
        let mut program = unary(operation, contract.clone(), contract.clone());
        program
            .sources
            .push(source("other", "value", contract.clone()));
        program.nodes[0]
            .inputs
            .push(input("other", "other", "value", contract));
        let mut engine = phase(program);
        feed(&mut engine, 1, 0, pressed(held));
        engine
            .accept(frame(
                "other",
                1,
                1,
                1,
                Reason::Baseline,
                vec![("value", pressed(!held))],
            ))
            .unwrap();
        engine
            .accept(frame("other", 1, 2, 2, Reason::Disconnect, vec![]))
            .unwrap();
        assert_eq!(output(&engine), pressed(expected));
    }
}

#[test]
fn numeric_operations_replay_at_endpoints_and_interiors() {
    let number = number_contract(-10.0, 10.0, None);
    let cases = [
        (
            Operation::Scale {
                factor: 0.5,
                offset: 1.0,
            },
            vec![(-10.0, -4.0), (0.0, 1.0), (10.0, 6.0)],
        ),
        (
            Operation::Invert {},
            vec![(-10.0, 10.0), (2.5, -2.5), (10.0, -10.0)],
        ),
        (
            Operation::Deadzone {
                center: 0.0,
                radius: 2.0,
                rounding: Rounding::None,
            },
            vec![
                (-10.0, -10.0),
                (-2.0, 0.0),
                (2.0, 0.0),
                (6.0, 5.0),
                (10.0, 10.0),
            ],
        ),
        (
            Operation::Curve {
                points: vec![
                    CurvePoint {
                        input: -10.0,
                        output: -10.0,
                    },
                    CurvePoint {
                        input: 0.0,
                        output: 5.0,
                    },
                    CurvePoint {
                        input: 10.0,
                        output: 10.0,
                    },
                ],
                rounding: Rounding::None,
            },
            vec![(-10.0, -10.0), (-5.0, -2.5), (0.0, 5.0), (10.0, 10.0)],
        ),
    ];
    for (operation, values) in cases {
        let mut engine = phase(unary(operation, number.clone(), number.clone()));
        for (i, (value, expected)) in values.into_iter().enumerate() {
            feed(&mut engine, i as u64 + 1, i as u64, sample(value));
            assert!((scalar_output(&engine) - expected).abs() < 1e-9);
        }
    }
    let mut engine = phase(unary(
        Operation::Deadzone {
            center: 0.0,
            radius: 2.0,
            rounding: Rounding::Nearest,
        },
        integer_contract(0, 10),
        integer_contract(0, 10),
    ));
    feed(&mut engine, 1, 0, sample(6.0));
    assert_eq!(scalar_output(&engine), 5.0);
}

#[test]
fn range_map_property_is_monotonic_bounded_and_explicitly_rounded() {
    let mut engine = phase(unary(
        Operation::RangeMap {
            input_min: 0.0,
            input_max: 255.0,
            output_min: 0.0,
            output_max: 32767.0,
            rounding: Rounding::Floor,
        },
        integer_contract(0, 255),
        integer_contract(0, 32767),
    ));
    let mut last = -1.0;
    for value in 0..=255 {
        feed(&mut engine, value + 1, value, sample(value as f64));
        let actual = scalar_output(&engine);
        assert!(actual >= last && actual <= 32767.0 && actual.fract() == 0.0);
        last = actual;
    }
    assert_eq!(last, 32767.0);
}

#[test]
fn comparison_select_not_and_explicit_fallback_cover_every_status() {
    for comparison in [
        Comparison::Greater,
        Comparison::GreaterEqual,
        Comparison::Less,
        Comparison::LessEqual,
        Comparison::Equal,
        Comparison::NotEqual,
    ] {
        let mut engine = phase(unary(
            Operation::Compare {
                comparison,
                threshold: 5.0,
            },
            number_contract(0.0, 10.0, None),
            boolean_contract(),
        ));
        feed(&mut engine, 1, 0, sample(5.0));
        assert_eq!(output(&engine), pressed(compare(5.0, 5.0, comparison)));
    }
    let mut engine = phase(unary(
        Operation::Not {},
        boolean_contract(),
        boolean_contract(),
    ));
    feed(&mut engine, 1, 0, pressed(true));
    assert_eq!(output(&engine), pressed(false));
    let mut engine = phase(unary(
        Operation::Select {
            when_true: PortValue::Number(8.0),
            when_false: PortValue::Number(2.0),
        },
        boolean_contract(),
        integer_contract(0, 10),
    ));
    feed(&mut engine, 1, 0, pressed(false));
    assert_eq!(scalar_output(&engine), 2.0);
    feed(&mut engine, 2, 1, pressed(true));
    assert_eq!(scalar_output(&engine), 8.0);
    let mut engine = phase(unary(
        Operation::Fallback {
            unknown: PortValue::Number(1.0),
            stale: PortValue::Number(2.0),
            unavailable: PortValue::Number(3.0),
        },
        integer_contract(0, 10),
        integer_contract(0, 10),
    ));
    for (i, (status, expected)) in [
        (SignalSample::Unknown, 1.0),
        (SignalSample::Stale, 2.0),
        (SignalSample::Unavailable, 3.0),
        (sample(8.0), 8.0),
    ]
    .into_iter()
    .enumerate()
    {
        feed(&mut engine, i as u64 + 1, i as u64, status);
        assert_eq!(scalar_output(&engine), expected)
    }
}

#[test]
fn priority_is_explicit_and_port_declaration_reorder_does_not_change_hash() {
    let contract = integer_contract(0, 10);
    let mut program = unary(
        Operation::Priority {
            order: vec!["secondary".into(), "input".into()],
        },
        contract.clone(),
        contract.clone(),
    );
    program
        .sources
        .push(source("second", "value", contract.clone()));
    program.nodes[0]
        .inputs
        .push(input("secondary", "second", "value", contract));
    let hash = program.compile().unwrap().hash;
    let mut reordered = program.clone();
    reordered.sources.reverse();
    reordered.nodes[0].inputs.reverse();
    assert_eq!(hash, reordered.compile().unwrap().hash);
    let mut engine = phase(reordered);
    feed(&mut engine, 1, 0, sample(2.0));
    engine
        .accept(frame(
            "second",
            1,
            1,
            1,
            Reason::Baseline,
            vec![("value", sample(8.0))],
        ))
        .unwrap();
    assert_eq!(scalar_output(&engine), 8.0);
    engine
        .accept(frame("second", 1, 2, 2, Reason::Disconnect, vec![]))
        .unwrap();
    assert_eq!(scalar_output(&engine), 2.0);
    if let Operation::Priority { order } = &mut program.nodes[0].operation {
        order.reverse()
    }
    assert_ne!(hash, program.compile().unwrap().hash);
}

#[test]
fn format_has_explicit_rounding_utf8_boundary_and_failure_policy() {
    let text = state(ScalarSchema::Text {
        max_bytes: 5,
        encoding: TextEncoding::Utf8,
    });
    let mut engine = phase(unary(
        Operation::Format {
            decimals: 1,
            prefix: "é".into(),
            suffix: "!".into(),
            rounding: Rounding::Floor,
            overflow: TextOverflow::Truncate,
        },
        number_contract(0.0, 100.0, None),
        text,
    ));
    feed(&mut engine, 1, 0, sample(12.39));
    assert_eq!(
        output(&engine),
        SignalSample::scalar(PortValue::Text("é12.".into()))
    );
    let text = state(ScalarSchema::Text {
        max_bytes: 2,
        encoding: TextEncoding::Ascii,
    });
    let mut engine = phase(unary(
        Operation::Format {
            decimals: 0,
            prefix: "".into(),
            suffix: "".into(),
            rounding: Rounding::Nearest,
            overflow: TextOverflow::Reject,
        },
        number_contract(0.0, 100.0, None),
        text,
    ));
    assert!(engine
        .accept(frame(
            "source",
            1,
            1,
            0,
            Reason::Baseline,
            vec![("value", sample(100.0))]
        ))
        .is_err());
    assert!(!engine.active());
    assert!(engine.fault().unwrap().contains("text budget"));
}

fn action_program(setter: bool, args: bool) -> Program {
    let parameters = if args {
        vec![
            parameter(
                "number",
                ScalarSchema::Integer {
                    min: 0,
                    max: 10,
                    unit: None,
                },
            ),
            parameter("enabled", ScalarSchema::Boolean {}),
        ]
    } else {
        vec![]
    };
    let semantics = if setter {
        ActionSemantics::Setter
    } else {
        ActionSemantics::OneShot
    };
    let mut sources = vec![source("source", "trigger", event())];
    let mut inputs = vec![];
    let arguments = parameters.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    if !setter {
        inputs.push(input("trigger", "source", "trigger", event()));
    }
    for p in &parameters {
        let contract = state(p.schema.clone());
        sources.push(source("source", &p.id, contract.clone()));
        inputs.push(input(&p.id, "source", &p.id, contract));
    }
    if setter {
        sources.remove(0);
    }
    Program {
        sources,
        nodes: vec![Node {
            id: "processor".into(),
            inputs,
            outputs: vec![descriptor(
                "output",
                Direction::Output,
                invocation(parameters.clone(), semantics),
            )],
            operation: if setter {
                Operation::SetOnChange { arguments }
            } else {
                Operation::ActionPack { arguments }
            },
        }],
        sinks: vec![resource_sink(
            "action",
            endpoint("processor", "output"),
            action(parameters, semantics),
        )],
        controllers: vec![],
    }
}

#[test]
fn constant_setter_emits_its_initial_tuple_once_without_tick_replay() {
    let contract = boolean_contract();
    let parameters = vec![parameter("enabled", ScalarSchema::Boolean {})];
    let mut engine = phase(Program {
        sources: vec![],
        nodes: vec![
            Node {
                id: "constant".into(),
                inputs: vec![],
                outputs: vec![descriptor("value", Direction::Output, contract.clone())],
                operation: Operation::Constant { value: PortValue::Boolean(true) },
            },
            Node {
                id: "setter".into(),
                inputs: vec![input("enabled", "constant", "value", contract)],
                outputs: vec![descriptor("invoke", Direction::Output,
                    invocation(parameters.clone(), ActionSemantics::Setter))],
                operation: Operation::SetOnChange { arguments: vec!["enabled".into()] },
            },
        ],
        sinks: vec![resource_sink("action", endpoint("setter", "invoke"),
            action(parameters, ActionSemantics::Setter))],
        controllers: vec![],
    });
    let effects = engine.drain();
    assert_eq!(effects.len(), 1, "The initial known setter tuple must reach its sink");
    assert_eq!(effects[0].reason, ReportReason::Start);
    assert_eq!(effects[0].sample, SignalSample::Known(packed(&[("enabled", PortValue::Boolean(true))])));
    for at in [1, 1_000_000, 5_000_000] {
        engine.advance(Counter(at)).unwrap();
        assert!(engine.drain().is_empty(), "An unchanged constant must not replay its setter");
    }
}

#[test]
fn constant_held_edge_does_not_emit_a_startup_one_shot() {
    let contract = boolean_contract();
    let mut program = action_program(false, false);
    program.sources.clear();
    program.nodes[0].inputs[0].from = endpoint("edge", "event");
    program.nodes.splice(0..0, [
        Node {
            id: "constant".into(),
            inputs: vec![],
            outputs: vec![descriptor("value", Direction::Output, contract.clone())],
            operation: Operation::Constant { value: PortValue::Boolean(true) },
        },
        Node {
            id: "edge".into(),
            inputs: vec![input("input", "constant", "value", contract)],
            outputs: vec![descriptor("event", Direction::Output, event())],
            operation: Operation::EventEdge { edge: Edge::Rising },
        },
    ]);
    let mut engine = phase(program);
    assert!(engine.drain().iter().all(|effect| !matches!(effect.sample, SignalSample::Known(_))));
    for at in [1, 1_000_000, 5_000_000] {
        engine.advance(Counter(at)).unwrap();
        assert!(engine.drain().is_empty(), "A held startup edge must not replay a one-shot");
    }
}

#[test]
fn action_pack_is_atomic_named_ordered_and_only_fires_on_event_occurrence() {
    let mut engine = phase(action_program(false, true));
    engine.drain();
    engine
        .accept(frame(
            "source",
            1,
            1,
            0,
            Reason::Baseline,
            vec![
                ("trigger", pressed(true)),
                ("number", sample(3.0)),
                ("enabled", pressed(false)),
            ],
        ))
        .unwrap();
    assert!(engine
        .drain()
        .iter()
        .all(|e| !matches!(e.sample, SignalSample::Known(_))));
    engine
        .accept(frame(
            "source",
            1,
            2,
            1,
            Reason::Change,
            vec![("number", sample(4.0))],
        ))
        .unwrap();
    assert!(engine.drain().is_empty());
    for sequence in [3, 4] {
        engine
            .accept(frame(
                "source",
                1,
                sequence,
                sequence,
                Reason::Change,
                vec![("trigger", pressed(true))],
            ))
            .unwrap();
        let effects = engine.drain();
        assert_eq!(effects.len(), 1);
        assert_eq!(
            effects[0].sample,
            SignalSample::Known(packed(&[
                ("number", PortValue::Number(4.0)),
                ("enabled", PortValue::Boolean(false))
            ]))
        );
    }
    engine
        .accept(frame(
            "source",
            1,
            5,
            5,
            Reason::Change,
            vec![
                ("enabled", SignalSample::Unknown),
                ("trigger", pressed(true)),
            ],
        ))
        .unwrap();
    assert!(engine
        .drain()
        .iter()
        .all(|e| !matches!(e.sample, SignalSample::Known(_))));
}

#[test]
fn zero_argument_one_shot_and_setter_tuple_changes_are_not_replayed() {
    let mut engine = phase(action_program(false, false));
    engine.drain();
    engine
        .accept(frame(
            "source",
            1,
            1,
            0,
            Reason::Baseline,
            vec![("trigger", pressed(true))],
        ))
        .unwrap();
    assert!(engine.drain().is_empty());
    engine
        .accept(frame(
            "source",
            1,
            2,
            1,
            Reason::Change,
            vec![("trigger", pressed(false))],
        ))
        .unwrap();
    assert_eq!(engine.drain()[0].sample, SignalSample::Known(packed(&[])));
    let mut engine = phase(action_program(true, true));
    engine.drain();
    engine
        .accept(frame(
            "source",
            1,
            1,
            0,
            Reason::Baseline,
            vec![("number", sample(5.0)), ("enabled", pressed(true))],
        ))
        .unwrap();
    assert_eq!(engine.drain().len(), 1);
    engine
        .accept(frame(
            "source",
            1,
            2,
            1,
            Reason::Change,
            vec![("number", sample(5.0))],
        ))
        .unwrap();
    assert!(engine.drain().is_empty());
    engine
        .accept(frame(
            "source",
            1,
            3,
            2,
            Reason::Change,
            vec![("enabled", pressed(false))],
        ))
        .unwrap();
    assert_eq!(engine.drain().len(), 1);
}

#[test]
fn source_sequences_gaps_generations_and_old_baselines_fail_closed() {
    let mut engine = phase(unary(
        Operation::Identity {},
        boolean_contract(),
        boolean_contract(),
    ));
    feed(&mut engine, 1, 10, pressed(true));
    assert!(engine
        .accept(frame(
            "source",
            1,
            3,
            11,
            Reason::Change,
            vec![("value", pressed(false))]
        ))
        .is_err());
    assert_eq!(output(&engine), SignalSample::Unknown);
    assert!(engine
        .accept(frame(
            "source",
            1,
            1,
            12,
            Reason::Baseline,
            vec![("value", pressed(true))]
        ))
        .is_err());
    engine
        .accept(frame(
            "source",
            1,
            4,
            13,
            Reason::Baseline,
            vec![("value", pressed(false))],
        ))
        .unwrap();
    engine
        .accept(frame("source", 1, 5, 14, Reason::Disconnect, vec![]))
        .unwrap();
    assert!(engine
        .accept(frame(
            "source",
            1,
            4,
            15,
            Reason::Baseline,
            vec![("value", pressed(true))]
        ))
        .is_err());
    engine
        .accept(frame(
            "source",
            2,
            1,
            16,
            Reason::Baseline,
            vec![("value", pressed(true))],
        ))
        .unwrap();
    assert!(engine
        .accept(frame(
            "source",
            1,
            10,
            17,
            Reason::Baseline,
            vec![("value", pressed(false))]
        ))
        .is_err());
    assert!(engine
        .accept(frame(
            "source",
            2,
            2,
            15,
            Reason::Change,
            vec![("value", pressed(false))]
        ))
        .is_err());
    assert_eq!(output(&engine), SignalSample::Unknown);
    engine
        .accept(frame(
            "source",
            2,
            u64::MAX,
            18,
            Reason::Baseline,
            vec![("value", pressed(true))],
        ))
        .unwrap();
    assert!(engine
        .accept(frame(
            "source",
            2,
            u64::MAX,
            19,
            Reason::Change,
            vec![("value", pressed(false))]
        ))
        .is_err());
}

#[test]
fn invalid_frames_are_atomic_and_complete_baselines_are_required() {
    let mut program = unary(
        Operation::Identity {},
        boolean_contract(),
        boolean_contract(),
    );
    program
        .sources
        .push(source("source", "extra", boolean_contract()));
    let mut engine = phase(program);
    assert!(engine
        .accept(frame(
            "source",
            1,
            1,
            0,
            Reason::Baseline,
            vec![("value", pressed(true))]
        ))
        .is_err());
    assert!(engine
        .accept(frame(
            "source",
            1,
            1,
            1,
            Reason::Baseline,
            vec![("value", pressed(true)), ("extra", sample(1.0))]
        ))
        .is_err());
    assert_eq!(
        engine.sample(&endpoint("source", "value")),
        Some(&SignalSample::Unknown)
    );
    assert!(engine
        .accept(frame(
            "source",
            1,
            1,
            2,
            Reason::Baseline,
            vec![("value", pressed(true)), ("value", pressed(false))]
        ))
        .is_err());
    assert!(engine
        .accept(frame("not-source", 1, 1, 2, Reason::Baseline, vec![]))
        .is_err());
}

#[test]
fn freshness_tokens_cancel_timers_even_when_same_generation_becomes_known_again() {
    let mut engine = phase(unary(
        Operation::Identity {},
        boolean_contract(),
        boolean_contract(),
    ));
    feed(&mut engine, 1, 0, pressed(false));
    let token = engine.token("processor").unwrap();
    engine
        .schedule(
            token.clone(),
            "later".into(),
            Counter(1_500_000),
            BTreeMap::from([(
                "output".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            )]),
        )
        .unwrap();
    engine.advance(Counter(1_000_000)).unwrap();
    assert_eq!(output(&engine), SignalSample::Stale);
    assert!(engine.timers.is_empty());
    feed(&mut engine, 2, 1_000_001, pressed(false));
    assert_ne!(token, engine.token("processor").unwrap());
    assert!(engine
        .schedule(token, "old".into(), Counter(1_100_000), BTreeMap::new())
        .is_err());
    engine.advance(Counter(1_500_000)).unwrap();
    assert_eq!(output(&engine), pressed(false));
    assert!(engine.advance(Counter(1)).is_err());
    assert!(engine.advance(Counter(u64::MAX)).is_err());
}

fn script_program(event_input: bool, output_contract: PortContract) -> Program {
    unary(
        Operation::Script {
            source: "fn on_event(ctx) { ctx }".into(),
            parameters: BTreeMap::new(),
        },
        if event_input {
            event()
        } else {
            boolean_contract()
        },
        output_contract,
    )
}
fn script_request(engine: &mut Phase) -> ScriptInvocation {
    let requests = engine.script_requests();
    assert_eq!(requests.len(), 1);
    requests.into_iter().next().unwrap()
}
fn scalar_transaction(value: bool) -> ScriptTransaction {
    ScriptTransaction {
        outputs: BTreeMap::from([(
            "output".into(),
            SignalValue::Scalar(PortValue::Boolean(value)),
        )]),
        ..Default::default()
    }
}

#[test]
fn queued_script_callbacks_observe_previous_committed_state() {
    let mut engine = phase(script_program(true, boolean_contract()));
    feed(&mut engine, 1, 0, pressed(false));
    let first = script_request(&mut engine);
    feed(&mut engine, 2, 1, pressed(true));
    feed(&mut engine, 3, 2, pressed(true));
    engine
        .script_reply(
            "processor",
            &first.ticket,
            ScriptTransaction {
                state: BTreeMap::from([("count".into(), PortValue::Number(1.0))]),
                ..Default::default()
            },
        )
        .unwrap();
    let second = script_request(&mut engine);
    assert_eq!(second.state["count"], PortValue::Number(1.0));
    assert!(second.event_trigger);
    engine
        .script_reply(
            "processor",
            &second.ticket,
            ScriptTransaction {
                state: BTreeMap::from([("count".into(), PortValue::Number(2.0))]),
                ..Default::default()
            },
        )
        .unwrap();
    let third = script_request(&mut engine);
    assert_eq!(third.state["count"], PortValue::Number(2.0));
    assert_eq!(third.at_us, Counter(2));
}

#[test]
fn state_only_and_baseline_scripts_cannot_invoke_one_shots() {
    for event_input in [false, true] {
        let mut program = script_program(event_input, invocation(vec![], ActionSemantics::OneShot));
        program.sinks[0].contract = action(vec![], ActionSemantics::OneShot);
        let mut engine = phase(program);
        feed(&mut engine, 1, 0, pressed(false));
        let request = script_request(&mut engine);
        assert!(!request.event_trigger);
        assert!(engine
            .script_reply(
                "processor",
                &request.ticket,
                ScriptTransaction {
                    outputs: BTreeMap::from([("output".into(), packed(&[]))]),
                    ..Default::default()
                }
            )
            .is_err());
        assert!(engine.states["processor"].fault);
    }
    let mut program = script_program(false, invocation(vec![], ActionSemantics::OneShot));
    program.sinks[0].contract = action(vec![], ActionSemantics::OneShot);
    let mut engine = phase(program);
    feed(&mut engine, 1, 0, pressed(false));
    let request = script_request(&mut engine);
    engine
        .script_reply("processor", &request.ticket, Default::default())
        .unwrap();
    feed(&mut engine, 2, 1, pressed(true));
    let request = script_request(&mut engine);
    assert!(!request.event_trigger);
    assert!(engine
        .script_reply(
            "processor",
            &request.ticket,
            ScriptTransaction {
                outputs: BTreeMap::from([("output".into(), packed(&[]))]),
                ..Default::default()
            }
        )
        .is_err());
}

#[test]
fn scripts_and_timers_cannot_launder_baseline_into_event_trigger() {
    let mut program = script_program(false, event());
    program.sinks.clear();
    program.nodes.push(Node {
        id: "pack".into(),
        inputs: vec![input("trigger", "processor", "output", event())],
        outputs: vec![descriptor(
            "call",
            Direction::Output,
            invocation(vec![], ActionSemantics::OneShot),
        )],
        operation: Operation::ActionPack { arguments: vec![] },
    });
    program.sinks.push(resource_sink(
        "call",
        endpoint("pack", "call"),
        action(vec![], ActionSemantics::OneShot),
    ));
    let mut engine = phase(program);
    engine.drain();
    feed(&mut engine, 1, 0, pressed(false));
    let request = script_request(&mut engine);
    engine
        .script_reply(
            "processor",
            &request.ticket,
            ScriptTransaction {
                outputs: BTreeMap::from([(
                    "output".into(),
                    SignalValue::Scalar(PortValue::Boolean(true)),
                )]),
                timers: vec![ScriptTimer {
                    id: "later".into(),
                    after_us: Counter(100),
                    outputs: BTreeMap::from([(
                        "output".into(),
                        SignalValue::Scalar(PortValue::Boolean(true)),
                    )]),
                }],
                state: BTreeMap::new(),
            },
        )
        .unwrap();
    assert!(engine
        .drain()
        .iter()
        .all(|e| !matches!(e.sample, SignalSample::Known(_))));
    engine.advance(Counter(100)).unwrap();
    assert!(engine
        .drain()
        .iter()
        .all(|e| !matches!(e.sample, SignalSample::Known(_))));
}

#[test]
fn actual_event_script_may_emit_one_atomic_call_and_validated_timer() {
    let mut program = script_program(true, invocation(vec![], ActionSemantics::OneShot));
    program.sinks[0].contract = action(vec![], ActionSemantics::OneShot);
    let mut engine = phase(program);
    engine.drain();
    feed(&mut engine, 1, 0, pressed(false));
    let request = script_request(&mut engine);
    engine
        .script_reply("processor", &request.ticket, Default::default())
        .unwrap();
    feed(&mut engine, 2, 1, pressed(true));
    let request = script_request(&mut engine);
    assert!(request.event_trigger);
    engine
        .script_reply(
            "processor",
            &request.ticket,
            ScriptTransaction {
                outputs: BTreeMap::from([("output".into(), packed(&[]))]),
                timers: vec![ScriptTimer {
                    id: "later".into(),
                    after_us: Counter(100),
                    outputs: BTreeMap::from([("output".into(), packed(&[]))]),
                }],
                state: BTreeMap::new(),
            },
        )
        .unwrap();
    assert_eq!(
        engine
            .drain()
            .iter()
            .filter(|e| matches!(e.sample, SignalSample::Known(_)))
            .count(),
        1
    );
    engine.advance(Counter(101)).unwrap();
    assert_eq!(engine.drain().len(), 1);
}

#[test]
fn intermediate_gate_validity_invalidates_script_ticket_and_timer() {
    let mut program = script_program(false, boolean_contract());
    program
        .sources
        .push(source("gate", "open", boolean_contract()));
    program.nodes.insert(
        0,
        Node {
            id: "gate-node".into(),
            inputs: vec![
                input("gate", "gate", "open", boolean_contract()),
                input("value", "source", "value", boolean_contract()),
            ],
            outputs: vec![descriptor("gated", Direction::Output, boolean_contract())],
            operation: Operation::Gate {},
        },
    );
    program.nodes[1].inputs[0].from = endpoint("gate-node", "gated");
    let mut engine = phase(program);
    feed(&mut engine, 1, 0, pressed(true));
    engine
        .accept(frame(
            "gate",
            1,
            1,
            0,
            Reason::Baseline,
            vec![("open", pressed(true))],
        ))
        .unwrap();
    let request = script_request(&mut engine);
    let token = request.ticket.producer.clone();
    engine
        .schedule(
            token.clone(),
            "later".into(),
            Counter(100),
            BTreeMap::from([(
                "output".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            )]),
        )
        .unwrap();
    engine
        .accept(frame(
            "gate",
            1,
            2,
            1,
            Reason::Change,
            vec![("open", pressed(false))],
        ))
        .unwrap();
    engine
        .accept(frame(
            "gate",
            1,
            3,
            2,
            Reason::Change,
            vec![("open", pressed(true))],
        ))
        .unwrap();
    assert_ne!(token, engine.token("processor").unwrap());
    assert!(engine
        .script_reply("processor", &request.ticket, scalar_transaction(true))
        .is_err());
    assert!(engine.timers.is_empty());
}

#[test]
fn script_transactions_validate_all_fields_before_committing() {
    let mut engine = phase(script_program(false, boolean_contract()));
    feed(&mut engine, 1, 0, pressed(false));
    let request = script_request(&mut engine);
    assert!(engine.script_ticket_current(&request.ticket));
    let mut forged = request.ticket.clone();
    forged.sequence.0 += 1;
    assert!(!engine.script_ticket_current(&forged));
    let invalid = ScriptTransaction {
        outputs: BTreeMap::from([(
            "output".into(),
            SignalValue::Scalar(PortValue::Boolean(true)),
        )]),
        timers: vec![ScriptTimer {
            id: "invalid".into(),
            after_us: Counter(0),
            outputs: BTreeMap::new(),
        }],
        state: BTreeMap::from([("counter".into(), PortValue::Number(1.0))]),
    };
    assert!(engine
        .script_reply("processor", &request.ticket, invalid)
        .is_err());
    assert!(engine.timers.is_empty());
    assert!(engine.states["processor"].script_state.is_empty());
    assert!(!engine.script_ticket_current(&request.ticket));
    assert_eq!(output(&engine), SignalSample::Unknown);
    feed(&mut engine, 2, 1, pressed(true));
    assert!(engine.script_requests().is_empty());
}

#[test]
fn script_and_timer_queues_have_shared_bounds_and_stop_invalidates_work() {
    let mut engine = phase(script_program(true, boolean_contract()));
    feed(&mut engine, 1, 0, pressed(false));
    let request = script_request(&mut engine);
    assert!(engine.script_ticket_current(&request.ticket));
    for sequence in 2..=34 {
        feed(&mut engine, sequence, sequence, pressed(true));
    }
    assert!(engine.states["processor"].fault);
    assert!(!engine.script_ticket_current(&request.ticket));
    assert!(engine.script_requests().is_empty());
    assert!(engine
        .script_reply("processor", &request.ticket, Default::default())
        .is_err());
    let mut engine = phase(unary(
        Operation::Identity {},
        boolean_contract(),
        boolean_contract(),
    ));
    feed(&mut engine, 1, 0, pressed(false));
    let token = engine.token("processor").unwrap();
    assert!(engine
        .schedule(
            token.clone(),
            "too-late".into(),
            Counter(60_000_001),
            BTreeMap::new()
        )
        .is_err());
    for i in 0..MAX_TIMERS {
        engine
            .schedule(
                token.clone(),
                format!("timer-{i}"),
                Counter(1),
                BTreeMap::new(),
            )
            .unwrap();
    }
    assert!(engine
        .schedule(
            token.clone(),
            "overflow".into(),
            Counter(1),
            BTreeMap::new()
        )
        .is_err());
    engine.stop();
    assert!(engine.timers.is_empty());
    assert!(engine
        .schedule(token, "old".into(), Counter(1), BTreeMap::new())
        .is_err());
    assert!(engine
        .accept(frame("source", 1, 2, 1, Reason::Change, vec![]))
        .is_err());
}

#[test]
fn builtin_pulse_reuses_existing_timing_and_stop_cancels_release() {
    let mut engine = phase(unary(
        Operation::Builtin {
            rule: crate::rules::Rule::Pulse {
                duration_us: 1000,
                repeat_us: 0,
            },
            inputs: vec!["input".into()],
        },
        boolean_contract(),
        boolean_contract(),
    ));
    feed(&mut engine, 1, 0, pressed(false));
    feed(&mut engine, 2, 1, pressed(true));
    assert_eq!(output(&engine), pressed(true));
    engine.advance(Counter(1001)).unwrap();
    assert_eq!(output(&engine), pressed(false));
    feed(&mut engine, 3, 1002, pressed(false));
    feed(&mut engine, 4, 1003, pressed(true));
    engine.stop();
    assert!(engine.advance(Counter(2003)).is_err());
    assert_eq!(output(&engine), SignalSample::Unavailable);
}

#[test]
fn event_edges_do_not_replay_baselines_and_event_to_state_is_explicit() {
    for edge in [Edge::Rising, Edge::Falling, Edge::Change] {
        let mut engine = phase(unary(
            Operation::EventEdge { edge },
            boolean_contract(),
            event(),
        ));
        engine.drain();
        feed(&mut engine, 1, 0, pressed(false));
        assert!(engine.drain().is_empty());
        feed(&mut engine, 2, 1, pressed(true));
        assert_eq!(engine.drain().len(), usize::from(edge != Edge::Falling));
        feed(&mut engine, 3, 2, pressed(false));
        assert_eq!(engine.drain().len(), usize::from(edge != Edge::Rising));
    }
    let mut engine = phase(unary(
        Operation::EventToState {},
        event(),
        boolean_contract(),
    ));
    feed(&mut engine, 1, 0, pressed(true));
    assert_eq!(output(&engine), pressed(true));
}

#[test]
fn runtime_candidate_validation_preserves_current_run_and_replacement_cancels_it() {
    let program = unary(
        Operation::Identity {},
        boolean_contract(),
        boolean_contract(),
    );
    let mut runtime = crate::runtime::Runtime::default();
    let old = runtime.start_program(program.clone()).unwrap();
    runtime
        .accept_signal_for(
            &old,
            frame(
                "source",
                1,
                1,
                0,
                Reason::Baseline,
                vec![("value", pressed(true))],
            ),
        )
        .unwrap();
    let mut invalid = program.clone();
    invalid.nodes[0].inputs[0].from = endpoint("missing", "value");
    assert!(runtime.start_program(invalid).is_err());
    assert_eq!(runtime.token().unwrap(), old);
    let next = runtime.start_program(program).unwrap();
    assert_ne!(next, old);
    assert!(runtime
        .accept_signal_for(&old, frame("source", 1, 2, 1, Reason::Change, vec![]))
        .is_err());
    assert!(runtime.engine().is_none());
    runtime.advance_for(&next, Counter(1)).unwrap();
    runtime.stop();
    assert!(runtime.token().is_err());
}

fn operation_valid(operation: Operation, inputs: Vec<PortContract>, output: PortContract) -> bool {
    let inputs = inputs
        .into_iter()
        .enumerate()
        .map(|(i, contract)| descriptor(&format!("input-{i}"), Direction::Input, contract))
        .collect::<Vec<_>>();
    validate_operation(
        &operation,
        &inputs,
        &[descriptor("output", Direction::Output, output)],
    )
    .is_ok()
}

#[test]
fn operation_contracts_reject_lossy_rounding_units_and_implicit_policies() {
    let raw = integer_contract(0, 65535);
    let output = integer_contract(0, 32767);
    assert!(!operation_valid(
        Operation::Identity {},
        vec![raw.clone()],
        output.clone()
    ));
    assert!(!operation_valid(
        Operation::RangeMap {
            input_min: 0.0,
            input_max: 65535.0,
            output_min: 0.0,
            output_max: 32767.0,
            rounding: Rounding::None
        },
        vec![raw.clone()],
        output.clone()
    ));
    assert!(!operation_valid(
        Operation::RangeMap {
            input_min: 1.0,
            input_max: 65535.0,
            output_min: 0.0,
            output_max: 32767.0,
            rounding: Rounding::Nearest
        },
        vec![raw],
        output
    ));
    assert!(!operation_valid(
        Operation::Scale {
            factor: 0.5,
            offset: 0.0
        },
        vec![integer_contract(0, 10)],
        integer_contract(0, 10)
    ));
    assert!(!operation_valid(
        Operation::Scale {
            factor: f64::NAN,
            offset: 0.0
        },
        vec![integer_contract(0, 10)],
        integer_contract(0, 10)
    ));
    assert!(!operation_valid(
        Operation::UnitConvert {
            from: "ft".into(),
            to: "m".into(),
            rounding: Rounding::None
        },
        vec![number_contract(0.0, 10.0, Some("m"))],
        number_contract(0.0, 10.0, Some("m"))
    ));
    assert!(!operation_valid(
        Operation::UnitConvert {
            from: "ft".into(),
            to: "kg".into(),
            rounding: Rounding::None
        },
        vec![number_contract(0.0, 10.0, Some("ft"))],
        number_contract(0.0, 10.0, Some("kg"))
    ));
    assert!(!operation_valid(
        Operation::Deadzone {
            center: 0.0,
            radius: 1.0,
            rounding: Rounding::None
        },
        vec![integer_contract(-10, 10)],
        integer_contract(-10, 10)
    ));
    assert!(!operation_valid(
        Operation::Curve {
            points: vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.0
                },
                CurvePoint {
                    input: 10.0,
                    output: 10.0
                }
            ],
            rounding: Rounding::None
        },
        vec![integer_contract(0, 10)],
        integer_contract(0, 10)
    ));
    assert!(!operation_valid(
        Operation::Hysteresis {
            press: 1.0,
            release: 2.0,
            comparison: Comparison::GreaterEqual
        },
        vec![number_contract(0.0, 10.0, None)],
        boolean_contract()
    ));
    assert!(!operation_valid(
        Operation::Builtin {
            rule: crate::rules::Rule::Threshold {
                press: 100,
                release: 90
            },
            inputs: vec!["input-0".into()]
        },
        vec![integer_contract(0, 65535)],
        boolean_contract()
    ));
    assert!(!operation_valid(
        Operation::Script {
            source: "x".repeat(16385),
            parameters: BTreeMap::new()
        },
        vec![boolean_contract()],
        boolean_contract()
    ));
    assert!(!operation_valid(
        Operation::Priority {
            order: vec!["input-0".into(), "input-0".into()]
        },
        vec![boolean_contract(), boolean_contract()],
        boolean_contract()
    ));
    assert!(!operation_valid(
        Operation::Identity {},
        vec![boolean_contract()],
        event()
    ));
    let input = descriptor("same", Direction::Input, boolean_contract());
    let output = descriptor("same", Direction::Output, boolean_contract());
    assert!(validate_operation(&Operation::Identity {}, &[input], &[output]).is_err());
}

#[test]
fn atomic_action_validation_rejects_duplicate_missing_reordered_and_incompatible_arguments() {
    let program = action_program(false, true);
    assert!(program.compile().is_ok());
    let mut bad = program.clone();
    if let Operation::ActionPack { arguments } = &mut bad.nodes[0].operation {
        arguments.reverse()
    }
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    bad.nodes[0].inputs.remove(1);
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    bad.nodes[0].outputs[0].contract = invocation(
        vec![
            parameter("number", ScalarSchema::Boolean {}),
            parameter("number", ScalarSchema::Boolean {}),
        ],
        ActionSemantics::OneShot,
    );
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    bad.nodes[0].operation = Operation::SetOnChange {
        arguments: vec!["number".into(), "enabled".into()],
    };
    bad.nodes[0].inputs.remove(0);
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    if let PortContract::Action { semantics, .. } = &mut bad.sinks[0].contract {
        *semantics = ActionSemantics::Setter
    }
    assert!(bad.compile().is_err());
    let contract = action(
        vec![parameter(
            "value",
            ScalarSchema::Integer {
                min: 0,
                max: 10,
                unit: None,
            },
        )],
        ActionSemantics::Setter,
    );
    assert!(!accepts(
        &contract,
        &packed(&[("other", PortValue::Number(1.0))])
    ));
    assert!(!accepts(&contract, &packed(&[])));
    assert!(!accepts(
        &contract,
        &SignalValue::Scalar(PortValue::Number(1.0))
    ));
}

#[test]
fn layouts_are_named_complete_bounded_and_have_native_hat_neutral() {
    let layout = ControllerLayout {
        buttons: vec!["button".into()],
        axes: vec![ControllerAxis {
            port_id: "axis".into(),
            range: AxisRange::Bipolar,
            fallback: -100,
        }],
        hats: vec!["hat".into()],
    };
    layout.validate().unwrap();
    assert_eq!(
        layout.virtual_port_contract("button"),
        Some(PortContract::State {
            schema: ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: Some(PortValue::Boolean(false))
        })
    );
    assert_eq!(
        schema(&layout.virtual_port_contract("axis").unwrap()),
        Some(&ScalarSchema::Integer {
            min: -32768,
            max: 32767,
            unit: None
        })
    );
    assert_eq!(
        layout.virtual_port_contract("hat"),
        Some(PortContract::State {
            schema: ScalarSchema::Integer {
                min: -1,
                max: 7,
                unit: None
            },
            freshness_ms: 1000,
            fallback: Some(PortValue::Number(-1.0))
        })
    );
    assert_eq!(layout.virtual_port_contract("absent"), None);
    let mut bad = layout.clone();
    bad.buttons.push("axis".into());
    assert!(bad.validate().is_err());
    let mut bad = layout.clone();
    bad.axes[0].fallback = -32769;
    assert!(bad.validate().is_err());
    let bad = ControllerLayout {
        buttons: vec![],
        axes: vec![],
        hats: vec![],
    };
    assert!(bad.validate().is_err());
    let mut program = unary(
        Operation::Identity {},
        integer_contract(-1, 7),
        integer_contract(-1, 7),
    );
    program.controllers.push(VirtualController {
        id: "virtual".into(),
        layout: ControllerLayout {
            buttons: vec![],
            axes: vec![],
            hats: vec!["hat".into()],
        },
    });
    program.sinks.push(Sink {
        id: "to-hat".into(),
        endpoint: endpoint("virtual", "hat"),
        from: endpoint("processor", "output"),
        contract: integer_contract(-1, 7),
        kind: SinkKind::Virtual {
            controller_id: "virtual".into(),
            control: OutputControl::Hat { index: 0 },
        },
    });
    let mut engine = phase(program);
    feed(&mut engine, 1, 0, sample(-1.0));
    assert_eq!(engine.drain_reports().last().unwrap().state.hats, [None]);
    feed(&mut engine, 2, 1, sample(7.0));
    assert_eq!(engine.drain_reports()[0].state.hats, [Some(7)]);
}

#[test]
fn compiled_program_rejects_cycles_aliases_bad_edges_and_layout_mismatches() {
    let program = unary(
        Operation::Identity {},
        boolean_contract(),
        boolean_contract(),
    );
    let mut bad = program.clone();
    bad.nodes[0].inputs[0].from = endpoint("processor", "output");
    assert!(bad.compile().unwrap_err().contains("cycle"));
    let mut bad = program.clone();
    bad.sources.push(bad.sources[0].clone());
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    bad.nodes[0].id = "source".into();
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    let mut sink = bad.sinks[0].clone();
    sink.id = "different-edge".into();
    bad.sinks.push(sink);
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    bad.nodes[0].inputs[0].edge_id = bad.sinks[0].id.clone();
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    bad.nodes[0].inputs[0].contract = event();
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    bad.sinks[0].from = endpoint("missing", "output");
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    bad.sinks[0].endpoint.port_id = "".into();
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    bad.sinks[0].kind = SinkKind::Virtual {
        controller_id: "missing".into(),
        control: OutputControl::Button { index: 0 },
    };
    assert!(bad.compile().is_err());
    let mut bad = program.clone();
    bad.controllers.push(VirtualController {
        id: "virtual".into(),
        layout: ControllerLayout {
            buttons: vec!["button".into()],
            axes: vec![],
            hats: vec![],
        },
    });
    bad.sinks[0].endpoint = endpoint("virtual", "button");
    bad.sinks[0].kind = SinkKind::Virtual {
        controller_id: "virtual".into(),
        control: OutputControl::Button { index: 1 },
    };
    assert!(bad.compile().is_err());
}

#[test]
fn bounded_program_never_truncates_source_target_port_or_binding_counts() {
    let base = unary(
        Operation::Identity {},
        boolean_contract(),
        boolean_contract(),
    );
    let mut bad = base.clone();
    for i in 1..=MAX_SOURCES {
        bad.sources
            .push(source(&format!("source-{i}"), "value", boolean_contract()));
    }
    assert!(bad.compile().unwrap_err().contains("16 source"));
    let mut bad = base.clone();
    for i in 0..=MAX_TARGETS {
        bad.controllers.push(VirtualController {
            id: format!("virtual-{i}"),
            layout: ControllerLayout {
                buttons: vec!["button".into()],
                axes: vec![],
                hats: vec![],
            },
        });
    }
    assert!(bad.compile().is_err());
    let mut bad = base.clone();
    for i in 0..MAX_PORTS {
        bad.sources
            .push(source("source", &format!("unused-{i}"), boolean_contract()));
    }
    assert!(bad.compile().is_err());
    let mut bad = base.clone();
    for i in 1..=MAX_BINDINGS {
        let mut sink = bad.sinks[0].clone();
        sink.id = format!("sink-{i}");
        sink.endpoint.port_id = sink.id.clone();
        bad.sinks.push(sink);
    }
    assert!(bad.compile().is_err());
    let mut bad = base.clone();
    bad.nodes[0].operation = Operation::Script {
        source: "x".repeat(MAX_PROGRAM_BYTES),
        parameters: BTreeMap::new(),
    };
    assert!(bad.compile().is_err());
    assert!(Phase::new(
        base.clone(),
        Counter(0),
        "b3738156-6794-42dc-978b-2376dce0c6c6".into()
    )
    .is_err());
    assert!(Phase::new(base, Counter(1), "not-a-session".into()).is_err());
}

#[test]
fn constant_values_do_not_expire_but_consumers_still_stop_safely() {
    let program = Program {
        sources: vec![],
        nodes: vec![Node {
            id: "processor".into(),
            inputs: vec![],
            outputs: vec![descriptor("output", Direction::Output, boolean_contract())],
            operation: Operation::Constant {
                value: PortValue::Boolean(true),
            },
        }],
        sinks: vec![resource_sink(
            "constant",
            endpoint("processor", "output"),
            boolean_contract(),
        )],
        controllers: vec![],
    };
    let mut engine = phase(program);
    assert_eq!(output(&engine), pressed(true));
    engine.advance(Counter(5_000_000)).unwrap();
    assert_eq!(output(&engine), pressed(true));
    engine.stop();
    assert_eq!(output(&engine), SignalSample::Unavailable);
}

#[test]
fn output_backpressure_faults_and_releases_to_unavailable() {
    let mut engine = phase(unary(Operation::Identity {}, event(), event()));
    feed(&mut engine, 1, 0, pressed(false));
    for sequence in 2..=MAX_REPORTS as u64 + 2 {
        if !engine.active() {
            break;
        }
        engine
            .accept(frame(
                "source",
                1,
                sequence,
                sequence,
                Reason::Change,
                vec![("value", pressed(true))],
            ))
            .unwrap();
    }
    assert!(!engine.active());
    assert!(engine.fault().unwrap().contains("consumer fell behind"));
    assert!(engine
        .drain()
        .iter()
        .all(|effect| effect.sample == SignalSample::Unavailable));
    assert!(engine
        .states
        .values()
        .all(|state| state.pending.is_empty() && state.inflight.is_none()));
}

#[test]
fn script_state_scalars_and_rebound_request_budget_are_validated() {
    let mut engine = phase(script_program(false, boolean_contract()));
    feed(&mut engine, 1, 0, pressed(false));
    let request = script_request(&mut engine);
    assert!(engine
        .script_reply(
            "processor",
            &request.ticket,
            ScriptTransaction {
                state: BTreeMap::from([("invalid".into(), PortValue::Number(f64::NAN))]),
                ..Default::default()
            }
        )
        .is_err());
    let mut engine = phase(script_program(false, boolean_contract()));
    feed(&mut engine, 1, 0, pressed(false));
    let request = script_request(&mut engine);
    assert!(engine
        .script_reply(
            "processor",
            &request.ticket,
            ScriptTransaction {
                state: BTreeMap::from([("x".repeat(81), PortValue::Boolean(true))]),
                ..Default::default()
            }
        )
        .is_err());
    // A queued callback's captured inputs can fit alone while the newly committed state makes its envelope exceed IPC_LIMIT.
    let text = state(ScalarSchema::Text {
        max_bytes: 4096,
        encoding: TextEncoding::Utf8,
    });
    let mut program = unary(
        Operation::Script {
            source: "fn on_event(ctx) {ctx}".into(),
            parameters: BTreeMap::new(),
        },
        text.clone(),
        boolean_contract(),
    );
    for i in 1..6 {
        let id = format!("text-{i}");
        program.sources.push(source("source", &id, text.clone()));
        program.nodes[0]
            .inputs
            .push(input(&id, "source", &id, text.clone()));
    }
    let mut engine = phase(program);
    let changes = engine
        .program()
        .sources
        .iter()
        .map(|source| Change {
            port_id: source.endpoint.port_id.clone(),
            sample: SignalSample::scalar(PortValue::Text("x".repeat(4000))),
        })
        .collect();
    engine
        .accept(Frame {
            source: "source".into(),
            generation: Counter(1),
            sequence: Counter(1),
            captured_us: Counter(0),
            reason: Reason::Baseline,
            changes,
        })
        .unwrap();
    let request = script_request(&mut engine);
    feed(
        &mut engine,
        2,
        1,
        SignalSample::scalar(PortValue::Text("y".repeat(4000))),
    );
    engine
        .script_reply(
            "processor",
            &request.ticket,
            ScriptTransaction {
                state: BTreeMap::from([("state".into(), PortValue::Text("z".repeat(12000)))]),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(engine.script_requests().is_empty());
    assert!(engine.states["processor"].fault);
}

#[test]
fn source_loss_and_new_baseline_cancel_due_script_actions_before_advancing_time() {
    for reason in [Reason::Disconnect, Reason::Gap, Reason::Baseline] {
        let mut program = script_program(true, invocation(vec![], ActionSemantics::OneShot));
        program.sinks[0].contract = action(vec![], ActionSemantics::OneShot);
        let mut engine = phase(program);
        feed(&mut engine, 1, 0, pressed(false));
        let request = script_request(&mut engine);
        engine
            .script_reply("processor", &request.ticket, Default::default())
            .unwrap();
        feed(&mut engine, 2, 1, pressed(true));
        let request = script_request(&mut engine);
        engine
            .script_reply(
                "processor",
                &request.ticket,
                ScriptTransaction {
                    timers: vec![ScriptTimer {
                        id: "old-action".into(),
                        after_us: Counter(100),
                        outputs: BTreeMap::from([("output".into(), packed(&[]))]),
                    }],
                    ..Default::default()
                },
            )
            .unwrap();
        engine.drain();
        engine
            .accept(frame(
                "source",
                if reason == Reason::Baseline { 2 } else { 1 },
                3,
                1000,
                reason,
                if reason == Reason::Baseline {
                    vec![("value", pressed(true))]
                } else {
                    vec![]
                },
            ))
            .unwrap();
        assert!(
            engine
                .drain()
                .iter()
                .all(|effect| !matches!(effect.sample, SignalSample::Known(_))),
            "{reason:?}"
        );
        assert!(engine.timers.is_empty());
    }
}

#[test]
fn typed_wire_samples_reject_hidden_fields_and_preserve_statuses() {
    for sample in [
        pressed(true),
        SignalSample::Unknown,
        SignalSample::Stale,
        SignalSample::Unavailable,
    ] {
        let encoded = serde_json::to_vec(&sample).unwrap();
        assert_eq!(
            serde_json::from_slice::<SignalSample>(&encoded).unwrap(),
            sample
        )
    }
    assert!(serde_json::from_value::<SignalSample>(
        serde_json::json!({"status":"unknown","unexpected":true})
    )
    .is_err());
    assert!(serde_json::from_value::<SignalValue>(
        serde_json::json!({"kind":"scalar","value":true,"unexpected":true})
    )
    .is_err());
}

#[test]
fn script_callbacks_replace_their_whole_output_and_timer_set() {
    let mut program = script_program(false, boolean_contract());
    program.nodes[0].outputs.push(descriptor(
        "other-output",
        Direction::Output,
        boolean_contract(),
    ));
    program.sinks.push(resource_sink(
        "other-sink",
        endpoint("processor", "other-output"),
        boolean_contract(),
    ));
    let mut engine = phase(program);
    feed(&mut engine, 1, 0, pressed(false));
    let request = script_request(&mut engine);
    engine
        .script_reply(
            "processor",
            &request.ticket,
            ScriptTransaction {
                outputs: BTreeMap::from([
                    (
                        "output".into(),
                        SignalValue::Scalar(PortValue::Boolean(true)),
                    ),
                    (
                        "other-output".into(),
                        SignalValue::Scalar(PortValue::Boolean(true)),
                    ),
                ]),
                timers: vec![ScriptTimer {
                    id: "old".into(),
                    after_us: Counter(100),
                    outputs: BTreeMap::from([(
                        "output".into(),
                        SignalValue::Scalar(PortValue::Boolean(true)),
                    )]),
                }],
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(output(&engine), pressed(true));
    feed(&mut engine, 2, 1, pressed(true));
    let request = script_request(&mut engine);
    engine
        .script_reply("processor", &request.ticket, scalar_transaction(false))
        .unwrap();
    assert_eq!(output(&engine), pressed(false));
    assert_eq!(
        engine.sample(&endpoint("processor", "other-output")),
        Some(&SignalSample::Unknown)
    );
    assert!(engine.timers.is_empty());
    engine.advance(Counter(100)).unwrap();
    assert_eq!(output(&engine), pressed(false));
}

#[test]
fn timer_identity_replacement_is_bounded_at_capacity_and_script_sets_replace_atomically() {
    let mut engine = phase(script_program(false, boolean_contract()));
    feed(&mut engine, 1, 0, pressed(false));
    let request = script_request(&mut engine);
    let token = request.ticket.producer.clone();
    for i in 0..MAX_TIMERS {
        engine
            .schedule(
                token.clone(),
                format!("timer-{i}"),
                Counter(100),
                BTreeMap::new(),
            )
            .unwrap();
    }
    engine
        .schedule(
            token,
            "timer-0".into(),
            Counter(200),
            BTreeMap::from([(
                "output".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            )]),
        )
        .unwrap();
    assert_eq!(engine.timers.len(), MAX_TIMERS);
    assert!(engine
        .timers
        .keys()
        .any(|(at, _, id)| *at == 200 && id == "timer-0"));
    assert!(!engine
        .timers
        .keys()
        .any(|(at, _, id)| *at == 100 && id == "timer-0"));
    let timers = (0..MAX_TIMERS)
        .map(|i| ScriptTimer {
            id: format!("replacement-{i}"),
            after_us: Counter(300),
            outputs: BTreeMap::new(),
        })
        .collect();
    engine
        .script_reply(
            "processor",
            &request.ticket,
            ScriptTransaction {
                timers,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(engine.timers.len(), MAX_TIMERS);
    assert!(engine
        .timers
        .keys()
        .all(|(at, _, id)| *at == 300 && id.starts_with("replacement-")));
}

#[test]
fn invalid_matching_callback_cannot_partially_replace_or_release_other_producers() {
    let mut program = script_program(false, boolean_contract());
    program.nodes.push(Node {
        id: "unrelated".into(),
        inputs: vec![],
        outputs: vec![descriptor(
            "constant",
            Direction::Output,
            boolean_contract(),
        )],
        operation: Operation::Constant {
            value: PortValue::Boolean(true),
        },
    });
    program.sinks.push(resource_sink(
        "unrelated-sink",
        endpoint("unrelated", "constant"),
        boolean_contract(),
    ));
    let mut engine = phase(program);
    feed(&mut engine, 1, 0, pressed(false));
    let request = script_request(&mut engine);
    engine
        .script_reply("processor", &request.ticket, scalar_transaction(true))
        .unwrap();
    feed(&mut engine, 2, 1, pressed(true));
    let request = script_request(&mut engine);
    let timer = ScriptTimer {
        id: "duplicate".into(),
        after_us: Counter(100),
        outputs: BTreeMap::new(),
    };
    assert!(engine
        .script_reply(
            "processor",
            &request.ticket,
            ScriptTransaction {
                outputs: BTreeMap::from([(
                    "output".into(),
                    SignalValue::Scalar(PortValue::Boolean(false))
                )]),
                timers: vec![timer.clone(), timer],
                state: BTreeMap::from([("partial".into(), PortValue::Boolean(true))])
            }
        )
        .is_err());
    assert_eq!(output(&engine), SignalSample::Unknown);
    assert_eq!(
        engine.sample(&endpoint("unrelated", "constant")),
        Some(&pressed(true))
    );
    assert!(engine.states["processor"].script_state.is_empty());
    assert!(engine.timers.is_empty());
}

#[test]
fn native_argument_named_trigger_is_preserved_with_arbitrary_event_port() {
    let mut program = action_program(false, true);
    program.sources[0].endpoint.port_id = "fire".into();
    program.sources[1].endpoint.port_id = "trigger".into();
    program.nodes[0].inputs[0].port_id = "fire".into();
    program.nodes[0].inputs[0].from.port_id = "fire".into();
    program.nodes[0].inputs[1].port_id = "trigger".into();
    program.nodes[0].inputs[1].from.port_id = "trigger".into();
    if let Operation::ActionPack { arguments } = &mut program.nodes[0].operation {
        arguments[0] = "trigger".into();
    }
    if let PortContract::Invocation { parameters, .. } = &mut program.nodes[0].outputs[0].contract {
        parameters[0].id = "trigger".into();
    }
    if let PortContract::Action { parameters, .. } = &mut program.sinks[0].contract {
        parameters[0].id = "trigger".into();
    }
    let mut engine = phase(program);
    engine.drain();
    engine
        .accept(frame(
            "source",
            1,
            1,
            0,
            Reason::Baseline,
            vec![
                ("fire", pressed(true)),
                ("trigger", sample(3.0)),
                ("enabled", pressed(true)),
            ],
        ))
        .unwrap();
    assert!(engine
        .drain()
        .iter()
        .all(|effect| !matches!(effect.sample, SignalSample::Known(_))));
    engine
        .accept(frame(
            "source",
            1,
            2,
            1,
            Reason::Change,
            vec![("trigger", sample(6.0))],
        ))
        .unwrap();
    assert!(engine.drain().is_empty());
    engine
        .accept(frame(
            "source",
            1,
            3,
            2,
            Reason::Change,
            vec![("fire", pressed(true))],
        ))
        .unwrap();
    let effects = engine.drain();
    assert_eq!(effects.len(), 1);
    assert_eq!(
        effects[0].sample,
        SignalSample::Known(packed(&[
            ("trigger", PortValue::Number(6.0)),
            ("enabled", PortValue::Boolean(true))
        ]))
    );
}

#[test]
fn loss_metadata_fences_late_baseline_time_sequence_and_generation() {
    for reason in [Reason::Disconnect, Reason::Gap] {
        let mut engine = phase(unary(
            Operation::Identity {},
            boolean_contract(),
            boolean_contract(),
        ));
        feed(&mut engine, 1, 10, pressed(true));
        engine
            .accept(frame("source", 1, 2, 100, reason, vec![]))
            .unwrap();
        assert!(engine
            .accept(frame(
                "source",
                1,
                3,
                50,
                Reason::Baseline,
                vec![("value", pressed(true))]
            ))
            .is_err());
        assert_ne!(output(&engine), pressed(true));
        assert!(engine
            .accept(frame(
                "source",
                1,
                2,
                101,
                Reason::Baseline,
                vec![("value", pressed(true))]
            ))
            .is_err());
        engine
            .accept(frame(
                "source",
                1,
                3,
                102,
                Reason::Baseline,
                vec![("value", pressed(false))],
            ))
            .unwrap();
        assert_eq!(output(&engine), pressed(false));
        assert!(engine
            .accept(frame(
                "source",
                2,
                1,
                103,
                Reason::Change,
                vec![("value", pressed(true))]
            ))
            .is_err());
        assert_eq!(output(&engine), SignalSample::Unknown);
        assert!(engine
            .accept(frame(
                "source",
                1,
                100,
                104,
                Reason::Baseline,
                vec![("value", pressed(true))]
            ))
            .is_err());
        assert!(engine
            .accept(frame(
                "source",
                2,
                1,
                104,
                Reason::Baseline,
                vec![("value", pressed(true))]
            ))
            .is_err());
        engine
            .accept(frame(
                "source",
                2,
                2,
                105,
                Reason::Baseline,
                vec![("value", pressed(false))],
            ))
            .unwrap();
        assert_eq!(output(&engine), pressed(false));
    }
}

#[test]
fn freshness_at_timer_deadline_cancels_without_panic_or_popping_later_timer() {
    let program = Program {
        sources: vec![source("source", "value", boolean_contract())],
        nodes: vec![
            Node {
                id: "processor".into(),
                inputs: vec![input("input", "source", "value", boolean_contract())],
                outputs: vec![descriptor("output", Direction::Output, boolean_contract())],
                operation: Operation::Identity {},
            },
            Node {
                id: "constant".into(),
                inputs: vec![],
                outputs: vec![descriptor("output", Direction::Output, boolean_contract())],
                operation: Operation::Constant {
                    value: PortValue::Boolean(false),
                },
            },
        ],
        sinks: vec![
            resource_sink("sink", endpoint("processor", "output"), boolean_contract()),
            resource_sink(
                "constant-sink",
                endpoint("constant", "output"),
                boolean_contract(),
            ),
        ],
        controllers: vec![],
    };
    let mut engine = phase(program);
    feed(&mut engine, 1, 0, pressed(false));
    let token = engine.token("processor").unwrap();
    engine
        .schedule(
            token,
            "expires".into(),
            Counter(1_000_000),
            BTreeMap::from([(
                "output".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            )]),
        )
        .unwrap();
    let token = engine.token("constant").unwrap();
    engine
        .schedule(
            token,
            "later".into(),
            Counter(1_000_100),
            BTreeMap::from([(
                "output".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            )]),
        )
        .unwrap();
    engine.advance(Counter(1_000_000)).unwrap();
    assert_eq!(output(&engine), SignalSample::Stale);
    assert_eq!(
        engine.sample(&endpoint("constant", "output")),
        Some(&pressed(false))
    );
    assert_eq!(engine.timers.len(), 1);
    assert_eq!(engine.timers.first_key_value().unwrap().0 .0, 1_000_100);
    engine.advance(Counter(1_000_100)).unwrap();
    assert!(engine.timers.is_empty());
    // Also cover the revoked timer being the final queue entry.
    let mut engine = phase(unary(
        Operation::Identity {},
        boolean_contract(),
        boolean_contract(),
    ));
    feed(&mut engine, 1, 0, pressed(false));
    let token = engine.token("processor").unwrap();
    engine
        .schedule(token, "only".into(), Counter(1_000_000), BTreeMap::new())
        .unwrap();
    engine.advance(Counter(1_000_000)).unwrap();
    assert!(engine.timers.is_empty());
}

#[test]
fn typed_scripts_keep_retained_worker_input_source_and_parameter_byte_bounds() {
    let valid = Operation::Script {
        source: "fn on_event(ctx) { ctx }".into(),
        parameters: BTreeMap::new(),
    };
    assert!(!operation_valid(valid.clone(), vec![], boolean_contract()));
    assert!(operation_valid(
        valid.clone(),
        vec![boolean_contract(); 32],
        boolean_contract()
    ));
    assert!(!operation_valid(
        valid,
        vec![boolean_contract(); 33],
        boolean_contract()
    ));
    for value in [
        PortValue::Number(f64::NAN),
        PortValue::Number(f64::INFINITY),
        PortValue::Number(crate::signal::MAX_SAFE_INTEGER as f64 + 1.0),
    ] {
        assert!(!operation_valid(
            Operation::Script {
                source: "x".into(),
                parameters: BTreeMap::from([("value".into(), value)])
            },
            vec![boolean_contract()],
            boolean_contract()
        ));
    }
    // UTF-8 byte lengths are authoritative for both code and the complete JSON parameter envelope.
    assert!(!operation_valid(
        Operation::Script {
            source: "é".repeat(8193),
            parameters: BTreeMap::new()
        },
        vec![boolean_contract()],
        boolean_contract()
    ));
    assert!(operation_valid(
        Operation::Script {
            source: "x".into(),
            parameters: BTreeMap::from([("value".into(), PortValue::Text("é".repeat(2000)))])
        },
        vec![boolean_contract()],
        boolean_contract()
    ));
    assert!(!operation_valid(
        Operation::Script {
            source: "x".into(),
            parameters: BTreeMap::from([("value".into(), PortValue::Text("é".repeat(2048)))])
        },
        vec![boolean_contract()],
        boolean_contract()
    ));
}

fn mixed_timers() -> Phase {
    let mut program = Program {
        sources: vec![],
        nodes: vec![],
        sinks: vec![],
        controllers: vec![],
    };
    for name in ["hid", "panel", "game"] {
        let mut branch = script_program(true, invocation(vec![], ActionSemantics::OneShot));
        let producer = format!("{name}-script");
        branch.sources[0].endpoint = endpoint(name, "value");
        branch.nodes[0].id = producer.clone();
        branch.nodes[0].inputs[0].from = endpoint(name, "value");
        branch.nodes[0].inputs[0].edge_id = format!("edge-{name}");
        branch.sinks[0].id = format!("{name}-sink");
        branch.sinks[0].from = endpoint(&producer, "output");
        branch.sinks[0].endpoint = endpoint(&format!("device-{name}"), "call");
        branch.sinks[0].contract = action(vec![], ActionSemantics::OneShot);
        program.sources.extend(branch.sources);
        program.nodes.extend(branch.nodes);
        program.sinks.extend(branch.sinks);
    }
    let mut engine = phase(program);
    for name in ["hid", "panel", "game"] {
        engine
            .accept(frame(
                name,
                1,
                1,
                0,
                Reason::Baseline,
                vec![("value", pressed(false))],
            ))
            .unwrap();
    }
    for _ in 0..8 {
        let requests = engine.script_requests();
        if requests.is_empty() {
            break;
        }
        for request in requests {
            engine
                .script_reply(&request.node_id, &request.ticket, Default::default())
                .unwrap();
        }
    }
    for name in ["hid", "panel", "game"] {
        engine
            .accept(frame(
                name,
                1,
                2,
                1,
                Reason::Change,
                vec![("value", pressed(true))],
            ))
            .unwrap();
    }
    for request in engine.script_requests() {
        engine
            .script_reply(
                &request.node_id,
                &request.ticket,
                ScriptTransaction {
                    timers: vec![ScriptTimer {
                        id: "scheduled-call".into(),
                        after_us: Counter(49),
                        outputs: BTreeMap::from([("output".into(), packed(&[]))]),
                    }],
                    ..Default::default()
                },
            )
            .unwrap();
    }
    assert_eq!(engine.timers.len(), 3);
    engine.drain();
    engine.drain_reports();
    engine
}
#[test]
fn batch_pre_revokes_every_panel_game_loss_before_hid_or_first_loss_can_advance_timers() {
    for order in [0, 1, 2] {
        let mut engine = mixed_timers();
        let hid = frame(
            "hid",
            1,
            3,
            100,
            Reason::Change,
            vec![("value", pressed(false))],
        );
        let panel = frame("panel", 1, 3, 100, Reason::Gap, vec![]);
        let game = frame("game", 1, 3, 100, Reason::Disconnect, vec![]);
        let frames = match order {
            0 => vec![hid, panel, game],
            1 => vec![panel, game, hid],
            _ => vec![game, hid, panel],
        };
        let diagnostics = engine.accept_batch(frames).unwrap();
        assert_eq!(diagnostics.len(), 2);
        let calls = engine
            .drain()
            .into_iter()
            .filter(|effect| matches!(effect.sample, SignalSample::Known(_)))
            .map(|effect| effect.sink_id)
            .collect::<Vec<_>>();
        assert_eq!(calls, vec!["hid-sink"]);
        assert_eq!(
            engine.sample(&endpoint("panel", "value")),
            Some(&SignalSample::Unknown)
        );
        assert_eq!(
            engine.sample(&endpoint("game", "value")),
            Some(&SignalSample::Unavailable)
        );
        assert!(engine.timers.is_empty());
        assert!(engine.active());
    }
}
#[test]
fn batch_loss_removes_already_queued_dependent_actions_but_keeps_independent_ones() {
    let mut engine = mixed_timers();
    engine.advance(Counter(50)).unwrap();
    assert_eq!(
        engine
            .effects
            .iter()
            .filter(|effect| matches!(effect.sample, SignalSample::Known(_)))
            .count(),
        3
    );
    engine
        .accept_batch([
            frame("panel", 1, 3, 100, Reason::Gap, vec![]),
            frame("game", 1, 3, 100, Reason::Disconnect, vec![]),
        ])
        .unwrap();
    let calls = engine
        .drain()
        .into_iter()
        .filter(|effect| matches!(effect.sample, SignalSample::Known(_)))
        .map(|effect| effect.sink_id)
        .collect::<Vec<_>>();
    assert_eq!(calls, vec!["hid-sink"]);
}
#[test]
fn batch_source_gap_discards_earlier_and_unbaselined_events_then_admits_only_fresh_occurrence() {
    let mut engine = phase(action_program(false, false));
    engine
        .accept(frame(
            "source",
            1,
            1,
            0,
            Reason::Baseline,
            vec![("trigger", pressed(false))],
        ))
        .unwrap();
    engine.drain();
    let diagnostics = engine
        .accept_batch([
            frame(
                "source",
                1,
                2,
                10,
                Reason::Change,
                vec![("trigger", pressed(true))],
            ),
            frame("source", 1, 3, 20, Reason::Gap, vec![]),
            frame(
                "source",
                1,
                4,
                21,
                Reason::Change,
                vec![("trigger", pressed(true))],
            ),
            frame(
                "source",
                1,
                5,
                22,
                Reason::Baseline,
                vec![("trigger", pressed(true))],
            ),
            frame(
                "source",
                1,
                6,
                23,
                Reason::Change,
                vec![("trigger", pressed(true))],
            ),
        ])
        .unwrap();
    assert_eq!(diagnostics.len(), 1);
    let calls = engine
        .drain()
        .into_iter()
        .filter(|effect| matches!(effect.sample, SignalSample::Known(_)))
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].at_us, Counter(23));
    assert_eq!(engine.sources["source"].sequence, 6);
}
#[test]
fn batch_generation_and_sequence_gaps_fence_recovery_and_do_not_stop_unrelated_timers() {
    for generation_gap in [false, true] {
        let mut engine = mixed_timers();
        let (generation, sequence) = if generation_gap { (2, 1) } else { (1, 9) };
        engine
            .accept_batch([
                frame(
                    "panel",
                    generation,
                    sequence,
                    20,
                    Reason::Change,
                    vec![("value", pressed(true))],
                ),
                frame(
                    "panel",
                    generation,
                    sequence + 1,
                    30,
                    Reason::Change,
                    vec![("value", pressed(false))],
                ),
                frame("game", 1, 3, 100, Reason::Gap, vec![]),
            ])
            .unwrap();
        assert!(!engine.sources["panel"].available);
        assert_eq!(engine.sources["panel"].generation, generation);
        assert_eq!(engine.sources["panel"].sequence, sequence + 1);
        assert_eq!(
            engine
                .drain()
                .into_iter()
                .filter(|effect| matches!(effect.sample, SignalSample::Known(_)))
                .map(|effect| effect.sink_id)
                .collect::<Vec<_>>(),
            vec!["hid-sink"]
        );
        assert!(engine
            .accept_batch([frame(
                "panel",
                generation,
                sequence + 1,
                101,
                Reason::Baseline,
                vec![("value", pressed(true))]
            )])
            .is_err());
        engine
            .accept_batch([frame(
                "panel",
                generation,
                sequence + 2,
                102,
                Reason::Baseline,
                vec![("value", pressed(true))],
            )])
            .unwrap();
        assert!(engine
            .drain()
            .iter()
            .all(|effect| !matches!(effect.sample, SignalSample::Known(_))));
        assert!(engine.sources["panel"].available);
    }
}
#[test]
fn batch_nonknown_port_samples_cancel_old_tickets_and_suppress_only_that_ports_prior_events() {
    for missing in [
        SignalSample::Unknown,
        SignalSample::Stale,
        SignalSample::Unavailable,
    ] {
        let mut engine = mixed_timers();
        engine
            .accept_batch([
                frame(
                    "hid",
                    1,
                    3,
                    100,
                    Reason::Change,
                    vec![("value", pressed(false))],
                ),
                frame(
                    "panel",
                    1,
                    3,
                    100,
                    Reason::Change,
                    vec![("value", missing.clone())],
                ),
                frame(
                    "game",
                    1,
                    3,
                    100,
                    Reason::Change,
                    vec![("value", missing.clone())],
                ),
            ])
            .unwrap();
        assert_eq!(
            engine
                .drain()
                .into_iter()
                .filter(|effect| matches!(effect.sample, SignalSample::Known(_)))
                .map(|effect| effect.sink_id)
                .collect::<Vec<_>>(),
            vec!["hid-sink"]
        );
        assert_eq!(engine.sample(&endpoint("panel", "value")), Some(&missing));
    }
    let mut program = Program {
        sources: vec![
            source("panel", "left", event()),
            source("panel", "right", event()),
        ],
        nodes: vec![],
        sinks: vec![
            resource_sink("left", endpoint("panel", "left"), event()),
            resource_sink("right", endpoint("panel", "right"), event()),
        ],
        controllers: vec![],
    };
    let mut engine = phase(program.clone());
    engine
        .accept(frame(
            "panel",
            1,
            1,
            0,
            Reason::Baseline,
            vec![("left", pressed(false)), ("right", pressed(false))],
        ))
        .unwrap();
    engine.drain();
    engine
        .accept_batch([
            frame(
                "panel",
                1,
                2,
                10,
                Reason::Change,
                vec![("left", pressed(true))],
            ),
            frame(
                "panel",
                1,
                3,
                11,
                Reason::Change,
                vec![("right", pressed(true))],
            ),
            frame(
                "panel",
                1,
                4,
                12,
                Reason::Change,
                vec![("left", SignalSample::Unavailable)],
            ),
            frame(
                "panel",
                1,
                5,
                13,
                Reason::Change,
                vec![("right", pressed(false))],
            ),
        ])
        .unwrap();
    let known = engine
        .drain()
        .into_iter()
        .filter(|effect| matches!(effect.sample, SignalSample::Known(_)))
        .collect::<Vec<_>>();
    assert_eq!(
        known
            .iter()
            .map(|effect| effect.sink_id.as_str())
            .collect::<Vec<_>>(),
        vec!["right", "right"]
    );
    // A complete baseline earlier in this same batch stays complete when its
    // subsequently invalidated port is conservatively seeded as non-Known.
    program.sources[0].contract = boolean_contract();
    program.sinks[0].contract = boolean_contract();
    let mut engine = phase(program);
    engine
        .accept_batch([
            frame(
                "panel",
                1,
                1,
                0,
                Reason::Baseline,
                vec![("left", pressed(true)), ("right", pressed(false))],
            ),
            frame(
                "panel",
                1,
                2,
                10,
                Reason::Change,
                vec![("left", SignalSample::Unknown)],
            ),
        ])
        .unwrap();
    assert_eq!(
        engine.sample(&endpoint("panel", "left")),
        Some(&SignalSample::Unknown)
    );
}
#[test]
fn batch_revokes_virtual_slots_without_erasing_other_controls_queued_pulse() {
    let b = boolean_contract();
    let axis = integer_contract(0, 32767);
    let hat = integer_contract(-1, 7);
    let mut program = Program {
        sources: vec![
            source("panel", "button", b.clone()),
            source("panel", "axis", axis.clone()),
            source("panel", "hat", hat.clone()),
            source("hid", "button", b.clone()),
        ],
        nodes: vec![],
        sinks: vec![],
        controllers: vec![VirtualController {
            id: "target".into(),
            layout: ControllerLayout {
                buttons: vec!["lost".into(), "healthy".into()],
                axes: vec![ControllerAxis {
                    port_id: "axis".into(),
                    range: AxisRange::Unipolar,
                    fallback: 42,
                }],
                hats: vec!["hat".into()],
            },
        }],
    };
    for (id, node, port, contract, control) in [
        (
            "lost",
            "panel",
            "button",
            b.clone(),
            OutputControl::Button { index: 0 },
        ),
        (
            "healthy",
            "hid",
            "button",
            b,
            OutputControl::Button { index: 1 },
        ),
        (
            "axis",
            "panel",
            "axis",
            axis,
            OutputControl::Axis { index: 0 },
        ),
        ("hat", "panel", "hat", hat, OutputControl::Hat { index: 0 }),
    ] {
        program.sinks.push(Sink {
            id: id.into(),
            endpoint: endpoint("target", id),
            from: endpoint(node, port),
            contract,
            kind: SinkKind::Virtual {
                controller_id: "target".into(),
                control,
            },
        });
    }
    let mut engine = phase(program);
    engine
        .accept(frame(
            "panel",
            1,
            1,
            0,
            Reason::Baseline,
            vec![
                ("button", pressed(true)),
                ("axis", sample(100.0)),
                ("hat", sample(2.0)),
            ],
        ))
        .unwrap();
    engine
        .accept(frame(
            "hid",
            1,
            1,
            0,
            Reason::Baseline,
            vec![("button", pressed(false))],
        ))
        .unwrap();
    engine.drain_reports();
    engine.drain();
    engine
        .accept(frame(
            "panel",
            1,
            2,
            1,
            Reason::Change,
            vec![("button", pressed(false))],
        ))
        .unwrap();
    engine
        .accept(frame(
            "panel",
            1,
            3,
            2,
            Reason::Change,
            vec![("button", pressed(true))],
        ))
        .unwrap();
    engine
        .accept(frame(
            "hid",
            1,
            2,
            3,
            Reason::Change,
            vec![("button", pressed(true))],
        ))
        .unwrap();
    engine
        .accept(frame(
            "hid",
            1,
            3,
            4,
            Reason::Change,
            vec![("button", pressed(false))],
        ))
        .unwrap();
    engine
        .accept_batch([frame("panel", 1, 4, 100, Reason::Gap, vec![])])
        .unwrap();
    let reports = engine.drain_reports();
    assert!(reports.iter().all(|report| !report.state.buttons[0]
        && report.state.axes == vec![42]
        && report.state.hats == vec![None]));
    assert!(reports.iter().any(|report| report.state.buttons[1]));
    assert!(!reports.last().unwrap().state.buttons[1]);
    assert!(engine
        .drain()
        .iter()
        .filter(|effect| effect.sink_id == "lost"
            || effect.sink_id == "axis"
            || effect.sink_id == "hat")
        .all(|effect| !matches!(effect.sample, SignalSample::Known(_))));
}
#[test]
fn malformed_and_oversized_batches_cannot_advance_or_replay_queued_source_work() {
    let mut engine = mixed_timers();
    let mut invalid = frame(
        "panel",
        1,
        3,
        100,
        Reason::Change,
        vec![("value", sample(1.0))],
    );
    assert!(engine
        .accept_batch([
            frame(
                "hid",
                1,
                3,
                100,
                Reason::Change,
                vec![("value", pressed(true))]
            ),
            invalid.clone(),
            frame("game", 1, 3, 100, Reason::Gap, vec![])
        ])
        .is_err());
    assert_eq!(engine.at_us(), Counter(1));
    assert_eq!(engine.timers.len(), 1);
    assert!(engine
        .effects
        .iter()
        .all(|effect| !matches!(effect.sample, SignalSample::Known(_))));
    invalid.source = "unknown".into();
    assert!(engine.accept_batch([invalid]).is_err());
    let mut engine = mixed_timers();
    engine.advance(Counter(50)).unwrap();
    let frame = frame("hid", 1, 3, 100, Reason::Change, vec![]);
    assert!(engine
        .accept_batch(std::iter::repeat(frame).take(MAX_REPORTS + 1))
        .is_err());
    assert!(!engine.active());
    assert!(engine.fault().unwrap().contains("budget"));
    assert!(engine
        .drain()
        .iter()
        .all(|effect| !matches!(effect.sample, SignalSample::Known(_))));
    assert!(engine.accept_batch([]).is_err());
}
