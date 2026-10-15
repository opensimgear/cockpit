use super::*;
fn invocation(callback: &str) -> Invocation {
    serde_json::from_value(serde_json::json!({"ticket":{"producer":{"session":"a78d48b8-3958-4de5-a8b3-d3b52b1e24e7","runGeneration":"1","node":"logic:result","sources":[]},"epoch":"0","sequence":"1"},"callback":callback,"atUs":"1","inputs":{"left":{"trigger":{"kind":"button","value":false},"x":{"kind":"axis","value":0}},"right":{"trigger":{"kind":"button","value":false}}},"event":{"timer":"hold"},"parameters":{},"state":{}})).unwrap()
}
fn source(body: &str) -> String {
    format!(
        "fn on_start(ctx) {{ {body} }} fn on_event(ctx) {{ {body} }} fn on_timer(ctx) {{ {body} }}"
    )
}
#[test]
fn editor_examples_compile_and_evaluate_without_native_services() {
    for code in [
        include_str!("../../../src/lib/script-examples/shift-layer.rhai"),
        include_str!("../../../src/lib/script-examples/two-controls.rhai"),
        include_str!("../../../src/lib/script-examples/threshold.rhai"),
        include_str!("../../../src/lib/script-examples/short-long.rhai"),
        include_str!("../../../src/lib/script-examples/pulse.rhai"),
    ] {
        let program = Program::compile(code).unwrap();
        let mut request = invocation("on_start");
        let start = program.call(&request).unwrap();
        request.state = start.state;
        request.callback = "on_event".into();
        let released = program.call(&request).unwrap();
        request.state = released.state;
        request.at_us.0 = 10;
        request
            .inputs
            .get_mut("left")
            .unwrap()
            .get_mut("trigger")
            .unwrap()
            .clone_from(&osg_mapping::profile::Value::Button(true));
        request
            .inputs
            .get_mut("right")
            .unwrap()
            .get_mut("trigger")
            .unwrap()
            .clone_from(&osg_mapping::profile::Value::Button(true));
        request
            .inputs
            .get_mut("left")
            .unwrap()
            .get_mut("x")
            .unwrap()
            .clone_from(&osg_mapping::profile::Value::Axis(20000));
        let result = program.call(&request).unwrap();
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            serde_json::to_value(program.call(&request).unwrap()).unwrap()
        );
        request.state = result.state;
        request.callback = "on_timer".into();
        assert!(program.call(&request).is_ok());
    }
}
#[test]
fn syntax_import_dynamic_eval_and_native_apis_are_unavailable() {
    for code in [
        "",
        "let = ;",
        "fn on_event(ctx) {}",
        "import \"secret\" as x;",
        &"x".repeat(16385),
    ] {
        assert!(Program::compile(code).is_err());
    }
    for expression in [
        "read_file(\"secret\")",
        "eval(\"1\")",
        "timestamp()",
        "print(\"x\")",
        "random()",
        "write_device(1)",
    ] {
        let code = source(expression);
        assert!(
            Program::compile(&code)
                .and_then(|p| p.call(&invocation("on_start")))
                .is_err(),
            "{expression}"
        );
    }
    let p = Program::compile(&source("transaction(#{},[],[]) ")).unwrap();
    assert!(p.call(&invocation("on_stop")).is_err());
}
#[test]
fn loops_recursion_collection_growth_and_malformed_replies_fail_bounded() {
    for body in [
        "loop {}",
        "let a=[]; loop { a.push(1); }",
        "let a=\"x\"; loop { a+=a; }",
        "#{wrong: true}",
    ] {
        let p = Program::compile(&source(body)).unwrap();
        let start = Instant::now();
        assert!(p.call(&invocation("on_start")).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
    let p =
        Program::compile(&(format!("fn recurse(){{recurse()}} {}", source("recurse()")))).unwrap();
    assert!(p.call(&invocation("on_event")).is_err());
    let p = Program::compile(&source(
        "transaction(#{}, [axis(\"x\", 123),hat(\"h\",-1)], [timer(\"t\",5000)])",
    ))
    .unwrap();
    let result = p.call(&invocation("on_start")).unwrap();
    assert_eq!(result.outputs.len(), 2);
    assert_eq!(result.timers[0].after_us, 5000);
}
