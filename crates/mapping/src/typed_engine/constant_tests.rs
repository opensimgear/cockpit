use super::*;

fn scripted(constant_input: bool) -> Phase {
    let mut program = engine(false).program().clone();
    program.nodes[0].operation = Operation::Script {
        source: "fn on_start(ctx) { ctx } fn on_event(ctx) { ctx }".into(),
        parameters: BTreeMap::new(),
    };
    if constant_input {
        program.nodes[0].inputs[0].from = endpoint("constant");
    }
    let mut independent = program.nodes[1].clone();
    independent.id = "independent".into();
    program.nodes.push(independent);
    program.sinks.clear();
    Phase::new(
        program,
        Counter(1),
        "b3738156-6794-42dc-978b-2376dce0c6c6".into(),
    )
    .unwrap()
}

fn reply(engine: &mut Phase, request: &ScriptInvocation) {
    engine
        .script_reply(
            "processor",
            &request.ticket,
            ScriptTransaction {
                outputs: BTreeMap::from([(
                    "value".into(),
                    SignalValue::Scalar(PortValue::Boolean(true)),
                )]),
                ..ScriptTransaction::default()
            },
        )
        .unwrap();
}

#[test]
fn unchanged_constant_cannot_reinvoke_script_on_reply_or_any_expiry() {
    let mut engine = scripted(true);
    let initial = engine.script_requests();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].callback, "on_start");
    let constant = engine.values[&endpoint("constant")].clone();
    feed(&mut engine, 1, 0, Reason::Baseline, known(false));
    engine.advance(Counter(100)).unwrap();
    reply(&mut engine, &initial[0]);
    assert!(engine.script_requests().is_empty());
    assert_eq!(engine.sample(&endpoint("processor")), Some(&known(true)));
    timer(&mut engine, "independent", "future", 1_000_200);

    // Expiring an unrelated admitted source is not a new constant input.
    engine.advance(Counter(1_000_000)).unwrap();
    assert!(engine.script_requests().is_empty());
    assert_eq!(engine.sample(&endpoint("processor")), Some(&known(true)));
    assert_eq!(engine.next_deadline_us(), Some(Counter(1_000_100)));
    engine.advance(Counter(1_000_099)).unwrap();
    assert_eq!(engine.sample(&endpoint("processor")), Some(&known(true)));

    // The script's output really expires, without a fabricated on_event that
    // could continually renew its own contribution.
    engine.advance(Counter(1_000_100)).unwrap();
    assert_eq!(
        engine.sample(&endpoint("processor")),
        Some(&SignalSample::Stale)
    );
    assert!(engine.script_requests().is_empty());
    assert_eq!(engine.next_deadline_us(), Some(Counter(1_000_200)));
    assert_eq!(engine.timers.len(), 1);
    engine.advance(Counter(1_000_200)).unwrap();
    assert!(engine.timers.is_empty());
    assert!(engine.script_requests().is_empty());
    assert_eq!(engine.next_deadline_us(), None);
    let current = &engine.values[&endpoint("constant")];
    assert_eq!(current.sample, known(false));
    assert_eq!(
        (
            current.observed,
            current.captured,
            current.captured_sequence,
            current.epoch
        ),
        (
            constant.observed,
            constant.captured,
            constant.captured_sequence,
            constant.epoch
        )
    );
}

#[test]
fn real_source_observations_still_invoke_script_including_unchanged_values() {
    let mut engine = scripted(false);
    feed(&mut engine, 1, 0, Reason::Baseline, known(false));
    let start = engine.script_requests().pop().unwrap();
    assert_eq!(start.callback, "on_start");
    reply(&mut engine, &start);
    assert!(engine.script_requests().is_empty());

    for (sequence, value) in [(2, true), (3, true)] {
        feed(
            &mut engine,
            sequence,
            sequence,
            Reason::Change,
            known(value),
        );
        let requests = engine.script_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].callback, "on_event");
        assert_eq!(requests[0].inputs["input"], known(value));
        assert_eq!(requests[0].at_us, Counter(sequence));
        reply(&mut engine, &requests[0]);
        assert!(engine.script_requests().is_empty());
    }
    assert_eq!(engine.values[&endpoint("source")].captured, 3);
    assert_eq!(engine.values[&endpoint("source")].captured_sequence, 3);
}
