use super::*;
use crate::typed_test_support::*;
use osg_mapping::{
    engine::{typed::ScriptTimer, Counter},
    program::{ActionPack, NamedArgument},
    signal::*,
};
use serde_json::json;
#[test]
fn declaration_pins_ports_and_exact_typed_wire_roundtrips() {
    let definition = definition();
    definition.validate().unwrap();
    let mut request = request(true);
    for status in [
        SignalSample::Unknown,
        SignalSample::Stale,
        SignalSample::Unavailable,
        SignalSample::scalar(PortValue::Boolean(false)),
    ] {
        request.inputs.insert("input".into(), status);
        definition.validate_invocation(&request).unwrap();
        let bytes = serde_json::to_vec(&request).unwrap();
        let parsed: Invocation = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.inputs, request.inputs);
        assert_eq!(parsed.ticket, request.ticket);
    }
    let mut value = serde_json::to_value(&request).unwrap();
    value["hiddenRoute"] = json!("wrong");
    assert!(serde_json::from_value::<Invocation>(value).is_err());
    let mut value = serde_json::to_value(&definition).unwrap();
    value["secrets"] = json!([]);
    assert!(serde_json::from_value::<Definition>(value).is_err());
    let mut node = node("transaction(#{},#{},[])");
    node.operation = Operation::Identity {};
    assert!(Definition::from_node(&node).is_err());
    let mut bad = definition.clone();
    bad.node_id = "bad node".into();
    assert!(bad.validate().is_err());
    let mut bad = definition.clone();
    bad.inputs.push(bad.inputs[0].clone());
    assert!(bad.validate().is_err());
    let mut huge = definition.clone();
    huge.outputs[0].name = "x".repeat(40000);
    assert!(huge.validate().is_err());
}
#[test]
fn undeclared_inputs_forged_identity_or_unsafe_state_never_enter_context() {
    let definition = definition();
    let original = request(true);
    for mutate in [
        (|r: &mut Invocation| r.node_id = "other".into()) as fn(&mut Invocation),
        |r| r.ticket.producer.node_id = "other".into(),
        |r| r.callback = "on_timer".into(),
        |r| {
            r.parameters
                .insert("hidden".into(), PortValue::Boolean(true))
                .map(|_| ())
                .unwrap_or(())
        },
        |r| {
            r.inputs.insert("other".into(), SignalSample::Unknown);
        },
        |r| {
            r.inputs.insert(
                "input".into(),
                SignalSample::scalar(PortValue::Text("bad".into())),
            );
        },
        |r| r.ticket.sequence = Counter(0),
        |r| r.ticket.producer.run.generation = Counter(0),
        |r| r.ticket.producer.run.session = "forged".into(),
        |r| r.at_us = Counter(u64::MAX),
        |r| {
            r.state
                .insert("bad".into(), PortValue::Number(f64::INFINITY));
        },
        |r| {
            r.state.insert("bad\n".into(), PortValue::Boolean(true));
        },
        |r| {
            r.state
                .insert("large".into(), PortValue::Text("x".repeat(17000)));
        },
    ] {
        let mut bad = original.clone();
        mutate(&mut bad);
        assert!(definition.validate_invocation(&bad).is_err());
    }
    let mut start = request(false);
    start.event_trigger = true;
    assert!(definition.validate_invocation(&start).is_err());
}
fn pack() -> SignalValue {
    SignalValue::ActionPack(ActionPack {
        arguments: vec![
            NamedArgument {
                id: "trigger".into(),
                value: PortValue::Boolean(true),
            },
            NamedArgument {
                id: "gain:0".into(),
                value: PortValue::Number(3.0),
            },
        ],
    })
}
#[test]
fn action_tuple_semantics_and_whole_timer_transactions_are_checked() {
    let definition = definition();
    let event = request(true);
    let transaction = ScriptTransaction {
        outputs: BTreeMap::from([("call".into(), pack())]),
        timers: vec![ScriptTimer {
            id: "later".into(),
            after_us: Counter(1000),
            outputs: BTreeMap::from([(
                "lamp".into(),
                SignalValue::Scalar(PortValue::Boolean(false)),
            )]),
        }],
        state: BTreeMap::from([("count".into(), PortValue::Number(1.0))]),
    };
    definition
        .validate_transaction(&event, &transaction)
        .unwrap();
    assert!(definition
        .validate_transaction(&request(false), &transaction)
        .is_err());
    for mutate in [
        (|t: &mut ScriptTransaction| {
            t.outputs.insert(
                "unknown".into(),
                SignalValue::Scalar(PortValue::Boolean(true)),
            );
        }) as fn(&mut ScriptTransaction),
        |t| {
            if let SignalValue::ActionPack(pack) = t.outputs.get_mut("call").unwrap() {
                pack.arguments.reverse();
            }
        },
        |t| {
            if let SignalValue::ActionPack(pack) = t.outputs.get_mut("call").unwrap() {
                pack.arguments[1].value = PortValue::Number(100.0);
            }
        },
        |t| t.timers[0].after_us = Counter(0),
        |t| t.timers[0].after_us = Counter(60000001),
        |t| t.timers[0].id = "bad\n".into(),
        |t| t.timers.push(t.timers[0].clone()),
        |t| {
            t.timers[0]
                .outputs
                .insert("bad".into(), SignalValue::Scalar(PortValue::Boolean(true)));
        },
        |t| t.timers = vec![t.timers[0].clone(); 65],
    ] {
        let mut bad = transaction.clone();
        mutate(&mut bad);
        assert!(definition.validate_transaction(&event, &bad).is_err());
    }
    let mut declaration = definition.clone();
    declaration.source = "x".repeat(16384);
    declaration.inputs = vec![descriptor(
        "input",
        Direction::Input,
        PortContract::Event {
            schema: ScalarSchema::Enum {
                values: (0..256).map(|i| format!("{i}{}", "x".repeat(60))).collect(),
            },
        },
    )];
    assert!(declaration.validate().is_err());
}
