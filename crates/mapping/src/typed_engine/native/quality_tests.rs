use super::*;

#[test]
fn decisive_boolean_inputs_remain_usable_when_other_native_inputs_are_missing() {
    for (operation, expected) in [(Operation::Any {}, true), (Operation::All {}, false)] {
        let mut program = program(true);
        let mapped = program
            .nodes
            .iter_mut()
            .find(|node| node.id == "mapped")
            .unwrap();
        mapped.inputs.push(InputBinding {
            port_id: "other".into(),
            from: ep("game", "z"),
            contract: contract(),
            edge_id: "other-decisive-input".into(),
        });
        mapped.operation = operation;
        program
            .nodes
            .iter_mut()
            .find(|node| node.id == "script")
            .unwrap()
            .inputs[0]
            .from = ep("mapped", "value");
        let mut reducer = Phase::new(
            program,
            Counter(1),
            "b3738156-6794-42dc-978b-2376dce0c6c6".into(),
        )
        .unwrap();
        reducer.pin_native_clock("clock").unwrap();
        reducer
            .register_native_source(
                super::phase(false, NativePolicy::Sampled)
                    .native
                    .registrations["game"]
                    .clone(),
            )
            .unwrap();
        admit(
            &mut reducer,
            frame(
                1,
                &[
                    ("x", NativeChangeKind::Baseline, 1, None, 0, 0),
                    ("y", NativeChangeKind::Baseline, 1, None, 0, 0),
                    ("z", NativeChangeKind::Baseline, 1, Some(expected), 1, 1),
                ],
            ),
            1,
        );
        assert_eq!(
            reducer.sample(&ep("mapped", "value")),
            Some(&value(expected))
        );
        assert!(reducer.endpoint_has_current_native_level(&ep("mapped", "value")));
        assert_eq!(reducer.script_requests().len(), 1);
    }
}

#[test]
fn fallback_survives_unrelated_native_expiry() {
    let mut p = program(false);
    p.nodes
        .iter_mut()
        .find(|n| n.id == "mapped")
        .unwrap()
        .outputs[0]
        .contract = PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 1000,
        fallback: None,
    };
    p.sinks
        .iter_mut()
        .find(|s| s.id == "mapped-sink")
        .unwrap()
        .contract = PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 1000,
        fallback: None,
    };
    p.nodes
        .iter_mut()
        .find(|n| n.id == "mapped")
        .unwrap()
        .operation = Operation::Fallback {
        unknown: PortValue::Boolean(false),
        stale: PortValue::Boolean(false),
        unavailable: PortValue::Boolean(false),
    };
    let mut phase =
        Phase::new(p, Counter(1), "b3738156-6794-42dc-978b-2376dce0c6c6".into()).unwrap();
    phase.pin_native_clock("clock").unwrap();
    phase
        .register_native_source(
            super::phase(false, NativePolicy::Sampled)
                .native
                .registrations["game"]
                .clone(),
        )
        .unwrap();
    admit(
        &mut phase,
        frame(
            1,
            &[
                ("x", NativeChangeKind::Baseline, 1, None, 0, 0),
                ("y", NativeChangeKind::Baseline, 1, None, 0, 0),
                ("z", NativeChangeKind::Baseline, 1, Some(false), 1, 1),
            ],
        ),
        1,
    );
    assert_eq!(phase.sample(&ep("mapped", "value")), Some(&value(false)));
    phase.advance(Counter(100_001)).unwrap();
    assert_eq!(phase.sample(&ep("mapped", "value")), Some(&value(false)));
}

#[test]
fn priority_selected_healthy_input_can_invoke_script() {
    let mut p = program(true);
    let mapped = p.nodes.iter_mut().find(|n| n.id == "mapped").unwrap();
    mapped.inputs.push(InputBinding {
        port_id: "other".into(),
        from: ep("game", "z"),
        contract: contract(),
        edge_id: "other-edge".into(),
    });
    mapped.operation = Operation::Priority {
        order: vec!["other".into(), "input".into()],
    };
    p.nodes
        .iter_mut()
        .find(|n| n.id == "script")
        .unwrap()
        .inputs[0]
        .from = ep("mapped", "value");
    let mut phase =
        Phase::new(p, Counter(1), "b3738156-6794-42dc-978b-2376dce0c6c6".into()).unwrap();
    phase.pin_native_clock("clock").unwrap();
    phase
        .register_native_source(
            super::phase(false, NativePolicy::Sampled)
                .native
                .registrations["game"]
                .clone(),
        )
        .unwrap();
    admit(
        &mut phase,
        frame(
            1,
            &[
                ("x", NativeChangeKind::Baseline, 1, None, 0, 0),
                ("y", NativeChangeKind::Baseline, 1, None, 0, 0),
                ("z", NativeChangeKind::Baseline, 1, Some(false), 1, 1),
            ],
        ),
        1,
    );
    assert_eq!(phase.sample(&ep("mapped", "value")), Some(&value(false)));
    assert_eq!(
        phase.script_requests().len(),
        1,
        "A healthy selected native input must retain its callback"
    );
}

#[test]
fn indirect_authored_fallback_never_claims_native_neutral() {
    let mut p = program(true);
    let fallback = node(
        "quality-fallback",
        "mapped",
        "value",
        Operation::Fallback {
            unknown: PortValue::Boolean(false),
            stale: PortValue::Boolean(false),
            unavailable: PortValue::Boolean(false),
        },
    );
    p.nodes.push(fallback);
    p.nodes
        .iter_mut()
        .find(|n| n.id == "script")
        .unwrap()
        .inputs[0]
        .from = ep("quality-fallback", "value");
    let mut phase =
        Phase::new(p, Counter(1), "b3738156-6794-42dc-978b-2376dce0c6c6".into()).unwrap();
    phase.pin_native_clock("clock").unwrap();
    phase
        .register_native_source(
            super::phase(false, NativePolicy::Sampled)
                .native
                .registrations["game"]
                .clone(),
        )
        .unwrap();
    admit(
        &mut phase,
        frame(
            1,
            &[
                ("x", NativeChangeKind::Baseline, 1, None, 0, 0),
                ("y", NativeChangeKind::Baseline, 1, None, 0, 0),
            ],
        ),
        1,
    );
    assert_eq!(
        phase.sample(&ep("quality-fallback", "value")),
        Some(&value(false))
    );
    assert!(!phase.endpoint_has_current_native_level(&ep("quality-fallback", "value")));
    let request = phase.script_requests().pop().unwrap();
    phase
        .script_reply(
            "script",
            &request.ticket,
            ScriptTransaction {
                outputs: BTreeMap::from([(
                    "value".into(),
                    SignalValue::Scalar(PortValue::Boolean(false)),
                )]),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(phase.sample(&ep("script", "value")), Some(&value(false)));
    assert!(!phase.endpoint_has_current_native_level(&ep("script", "value")));
}
