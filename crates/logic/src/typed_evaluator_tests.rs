use super::*;
use crate::typed_test_support::*;
use osg_mapping::{
    program::{SignalSample, SignalValue},
    signal::*,
};
#[test]
fn typed_context_preserves_status_and_atomic_ordered_arguments_without_authority() {
    let first_node = node(
        r#"if ctx.inputs.input.status != "known" { return transaction(#{},#{lamp:scalar(false)},[]); } if ctx.eventTrigger { transaction(#{count:1},#{lamp:scalar(true),"call":action([argument("trigger",true),argument("gain:0",3)])},[after("release",1000,#{lamp:scalar(false)})]) } else { transaction(#{},#{},[]) }"#,
    );
    let program = Program::compile(Definition::from_node(&first_node).unwrap()).unwrap();
    let result = program.call(&request(true)).unwrap();
    assert_eq!(result.outputs.len(), 2);
    assert_eq!(result.timers[0].after_us.0, 1000);
    assert_eq!(result.state["count"], PortValue::Number(1.0));
    let SignalValue::ActionPack(pack) = &result.outputs["call"] else {
        panic!()
    };
    assert_eq!(pack.arguments[0].id, "trigger");
    assert_eq!(pack.arguments[1].id, "gain:0");
    for sample in [
        SignalSample::Unknown,
        SignalSample::Stale,
        SignalSample::Unavailable,
    ] {
        let mut r = request(true);
        r.inputs.insert("input".into(), sample);
        assert_eq!(
            program.call(&r).unwrap().outputs["lamp"],
            SignalValue::Scalar(PortValue::Boolean(false))
        );
    }
    let node = node(
        r#"if "ticket" in ctx || "nodeId" in ctx || "outputs" in ctx { loop {} } transaction(#{},#{lamp:scalar(true)},[])"#,
    );
    assert!(Program::compile(Definition::from_node(&node).unwrap())
        .unwrap()
        .call(&request(true))
        .is_ok());
}
#[test]
fn typed_interpreter_rejects_native_apis_undeclared_ports_invalid_tuples_and_state() {
    for body in [
        r#"read_file("secret")"#,
        r#"eval("1")"#,
        "loop {}",
        r#"transaction(#{},#{wrong:scalar(true)},[])"#,
        r#"transaction(#{},#{"call":action([argument("gain:0",3),argument("trigger",true)])},[])"#,
        r#"transaction(#{nested:#{bad:true}},#{},[])"#,
        r#"transaction(#{},#{lamp:scalar(1.5)},[])"#,
        r#"#{outputs:#{},state:#{},timers:[],hidden:true}"#,
    ] {
        let definition = Definition::from_node(&node(body)).unwrap();
        assert!(
            Program::compile(definition)
                .and_then(|program| program.call(&request(true)))
                .is_err(),
            "{body}"
        );
    }
    let mut definition = definition();
    definition.source = "fn on_start(ctx){}".into();
    assert!(Program::compile(definition).is_err());
}
#[test]
fn typed_scalar_helpers_preserve_all_scalar_domains() {
    let mut node = node(
        r#"transaction(#{},#{lamp:scalar(false),number:scalar(2),fraction:scalar(2.5),text:scalar("hello")},[])"#,
    );
    node.outputs.retain(|port| port.id == "lamp");
    for (id, schema, fallback) in [
        (
            "number",
            ScalarSchema::Integer {
                min: 0,
                max: 10,
                unit: None,
            },
            PortValue::Number(0.0),
        ),
        (
            "fraction",
            ScalarSchema::Number {
                min: 0.0,
                max: 10.0,
                unit: None,
            },
            PortValue::Number(0.0),
        ),
        (
            "text",
            ScalarSchema::Text {
                max_bytes: 32,
                encoding: TextEncoding::Utf8,
            },
            PortValue::Text("".into()),
        ),
    ] {
        node.outputs.push(descriptor(
            id,
            Direction::Output,
            PortContract::State {
                schema,
                freshness_ms: 1000,
                fallback: Some(fallback),
            },
        ));
    }
    let result = Program::compile(Definition::from_node(&node).unwrap())
        .unwrap()
        .call(&request(true))
        .unwrap();
    assert_eq!(result.outputs.len(), 4);
    assert_eq!(
        result.outputs["fraction"],
        SignalValue::Scalar(PortValue::Number(2.5))
    );
}
