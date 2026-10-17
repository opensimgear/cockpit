use super::*;
use serde_json::json;
#[test]
fn actual_and_prospective_companion_contracts_preserve_native_ranges_and_declared_feedback() {
    use osg_companion_service::{FeedbackDescriptor, FeedbackKind, FeedbackRange};
    use osg_mapping::profile::{AxisRange, Kind, Value as Input};
    use osg_mapping_host::companions::{ControlDefinition, Definition};
    let mut definition = Definition {
        source_id: "instance".into(),
        name: "Panel".into(),
        publication_hash: format!("sha256:{}", "a".repeat(64)),
        controls: vec![
            ControlDefinition {
                id: "button".into(),
                kind: Kind::Button,
                fallback: Input::Button(false),
            },
            ControlDefinition {
                id: "axis".into(),
                kind: Kind::Axis {
                    range: AxisRange::Bipolar,
                },
                fallback: Input::Axis(0),
            },
            ControlDefinition {
                id: "hat".into(),
                kind: Kind::Hat,
                fallback: Input::Hat(None),
            },
        ],
    };
    let mut feedback = vec![
        FeedbackDescriptor {
            channel_id: "active".into(),
            output_ids: vec!["indicator".into()],
            kind: FeedbackKind::Boolean,
            unit: None,
            range: None,
        },
        FeedbackDescriptor {
            channel_id: "label".into(),
            output_ids: vec!["text".into()],
            kind: FeedbackKind::Text,
            unit: None,
            range: None,
        },
        FeedbackDescriptor {
            channel_id: "height".into(),
            output_ids: vec!["gauge".into()],
            kind: FeedbackKind::Number,
            unit: Some("m".into()),
            range: Some(FeedbackRange {
                min: -100.0,
                max: 10000.0,
            }),
        },
    ];
    let ports = companion(&definition, &feedback).unwrap();
    assert_eq!(ports.len(), 6);
    assert!(matches!(
        ports[1].contract,
        PortContract::State {
            schema: ScalarSchema::Integer {
                min: -32768,
                max: 32767,
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        ports[2].contract,
        PortContract::State {
            schema: ScalarSchema::Integer {
                min: -1,
                max: 7,
                ..
            },
            ..
        }
    ));
    assert_eq!(ports[5].id, "feedback:height:gauge");
    assert!(
        matches!(&ports[5].contract,PortContract::State{schema:ScalarSchema::Number{unit:Some(unit),..},fallback:None,..} if unit=="m")
    );
    feedback[2].range = None;
    assert!(companion(&definition, &feedback).is_err());
    feedback.pop();
    feedback.push(feedback[0].clone());
    assert!(companion(&definition, &feedback).is_err());
    feedback.pop();
    definition.controls[0].fallback = Input::Axis(2);
    assert!(companion(&definition, &feedback).is_err());
}
#[test]
fn read_only_catalogues_keep_real_units_and_never_invent_commands() {
    for catalogue in [
        crate::xplane::catalogue(),
        crate::game_telemetry::catalogue(),
    ] {
        let ports = integration(&catalogue).unwrap();
        assert!(!ports.is_empty());
        assert!(ports.iter().all(|p| p.direction == Direction::Output));
        assert!(ports
            .iter()
            .all(|p| matches!(p.contract, PortContract::State { fallback: None, .. })));
    }
}
#[test]
fn module_actions_preserve_atomic_arguments_and_display_limits() {
    let cache = json!({"actions":[{"id":"color","label":"Color","endpoint":"led","semantics":"setter","readable":true,
            "parameters":[{"id":"index","label":"Index","type":"integer","minimum":0,"maximum":3},{"id":"rgb","label":"RGB","type":"rgb"}]}]});
    let ports = actions(&cache).unwrap();
    ports[0]
        .validate_arguments(&[PortValue::Number(2.0), PortValue::Text("FF00AA".into())])
        .unwrap();
    assert!(ports[0]
        .validate_arguments(&[PortValue::Number(2.0)])
        .is_err());
    assert!(ports[0]
        .validate_arguments(&[PortValue::Number(4.0), PortValue::Text("FF00AA".into())])
        .is_err());
}

#[test]
fn host_controls_preserve_axis_ranges_hat_neutral_and_filter_diagnostics() {
    use osg_controller_input::registry::Control;
    let button = Control::new(9, 1, 0, 0, 0);
    let mut axis = Control::new(1, 0x30, 0, 0, 1);
    axis.min = -32768;
    axis.max = 32767;
    let hat = Control::new(1, 0x39, 0, 0, 2);
    let diagnostic = Control::new(0xff00, 1, 0, 0, 3);
    let ports = controls(&[button.clone(), axis.clone(), hat.clone(), diagnostic]);
    assert_eq!(
        ports.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        vec![button.id, axis.id, hat.id]
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert!(matches!(
        &ports[0].contract,
        PortContract::State {
            schema: ScalarSchema::Boolean {},
            fallback: Some(PortValue::Boolean(false)),
            ..
        }
    ));
    assert!(matches!(
        &ports[1].contract,
        PortContract::State {
            schema: ScalarSchema::Integer {
                min: -32768,
                max: 32767,
                unit: None
            },
            fallback: None,
            ..
        }
    ));
    assert!(
        matches!(&ports[2].contract,PortContract::State { schema:ScalarSchema::Integer {min:-1,max:7,..}, fallback:Some(PortValue::Number(n)), .. } if *n == -1.0)
    );
}
fn event_state() -> Value {
    json!({"ok":true,"schemaVersion":1,"stream":7,"head":0,"oldest":1,"overflow":0,"buttonCount":2,"held":[false,true],"encoderButtons":[],
        "contactButtonCount":2,"maintainedButtons":[1],"selectorButtons":[],"axisCount":1,"axes":[40000],"axisNeutral":[32767],"axisUsage":[48],"axisAvailable":[true],"sampledAtMs":5})
}
#[test]
fn event_contracts_expose_real_buttons_and_axes_with_no_synthetic_hats() {
    let ports = events(&event_state()).unwrap();
    assert_eq!(
        ports.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        vec![
            "button:0",
            "button-event:0",
            "button:1",
            "button-event:1",
            "axis:0"
        ]
    );
    assert!(matches!(
        &ports[1].contract,
        PortContract::Event {
            schema: ScalarSchema::Boolean {}
        }
    ));
    assert!(
        matches!(&ports[4].contract,PortContract::State {schema:ScalarSchema::Integer{min:0,max:65535,..},fallback:Some(PortValue::Number(n)),..} if *n==32767.0)
    );
    let mut bad = event_state();
    bad["held"] = json!([true]);
    assert!(events(&bad).is_err());
    assert!(events(&json!({"route":{},"status":"unsupported"})).is_err());
    assert!(events(&json!({"ok":true,"schemaVersion":1,"stream":1,"head":0,"oldest":1,"overflow":0,"buttonCount":0,"held":[],"encoderButtons":[]})).unwrap().is_empty());
}
fn action(id: &str, semantics: &str, readable: bool, parameters: Value) -> Value {
    json!({"id":id,"label":id,"endpoint":id,"semantics":semantics,"readable":readable,"parameters":parameters})
}
#[test]
fn action_translation_preserves_atomic_typed_arguments_and_scopes_ownership_to_readable_setters() {
    let setter = action(
        "color",
        "setter",
        true,
        json!([
        {"id":"enabled","label":"Enabled","type":"boolean"},
        {"id":"level","label":"Level","type":"integer","minimum":-3,"maximum":8},
        {"id":"mode","label":"Mode","type":"enum","options":["solid","blink"]},
        {"id":"rgb","label":"RGB","type":"rgb"}]),
    );
    let shot = action("pulse", "oneShot", false, json!([]));
    let cache = json!({"ownership":{"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5000},"actions":[setter,shot]});
    let ports = actions(&cache).unwrap();
    ports[0]
        .validate_arguments(&[
            PortValue::Boolean(true),
            PortValue::Number(-3.0),
            PortValue::Text("blink".into()),
            PortValue::Text("00ffAA".into()),
        ])
        .unwrap();
    assert!(ports[0]
        .validate_arguments(&[
            PortValue::Boolean(true),
            PortValue::Number(9.0),
            PortValue::Text("blink".into()),
            PortValue::Text("00ffAA".into())
        ])
        .is_err());
    match &ports[0].contract {
        PortContract::Action {
            ownership: Some(owner),
            semantics,
            readable,
            parameters,
            ..
        } => {
            assert_eq!(owner.scope, "endpoint");
            assert_eq!(owner.max_lease_ms, 5000);
            assert_eq!(*semantics, ActionSemantics::Setter);
            assert!(*readable);
            assert_eq!(parameters.len(), 4);
        }
        _ => panic!("Expected a typed owned setter"),
    }
    assert!(matches!(
        &ports[1].contract,
        PortContract::Action {
            ownership: None,
            semantics: ActionSemantics::OneShot,
            ..
        }
    ));
    let mut bad = cache;
    bad["ownership"]["maxLeaseMs"] = json!(999999);
    assert!(actions(&bad).is_err());
}
#[test]
fn display_geometry_encoding_and_byte_limits_survive_provider_translation() {
    for encoding in ["ascii", "utf8"] {
        let mut screen = action(
            "screen",
            "setter",
            true,
            json!([{"id":"text","label":"Text","type":"string","maxBytes":16}]),
        );
        screen["display"] = json!({"columns":4,"rows":2,"encoding":encoding});
        let port = actions(&json!({"actions":[screen]})).unwrap().remove(0);
        port.validate_arguments(&[PortValue::Text("12345678".into())])
            .unwrap();
        assert!(port
            .validate_arguments(&[PortValue::Text("123456789".into())])
            .is_err());
        assert_eq!(
            port.validate_arguments(&[PortValue::Text("éééééééé".into())])
                .is_ok(),
            encoding == "utf8"
        );
        if let PortContract::Action {
            display: Some(display),
            parameters,
            ..
        } = port.contract
        {
            assert_eq!((display.columns, display.rows), (4, 2));
            assert!(matches!(
                parameters[0].schema,
                ScalarSchema::Text { max_bytes: 16, .. }
            ));
        } else {
            panic!("Display constraints were lost")
        }
    }
    let mut malformed = action(
        "bad",
        "setter",
        true,
        json!([{"id":"value","label":"Value","type":"unknown"}]),
    );
    assert!(actions(&json!({"actions":[malformed.clone()]})).is_err());
    malformed["parameters"] = json!([{"id":"value","label":"Value","type":"string","maxBytes":4}]);
    malformed["display"] = json!({"columns":8,"rows":2,"encoding":"ascii"});
    assert!(actions(&json!({"actions":[malformed]})).is_err());
}
#[test]
fn integration_channels_keep_units_and_dcs_commands_keep_their_actual_interfaces() {
    let catalogue = json!({"channels":[
        {"id":"altitude","label":"Altitude","type":"number","minimum":-1000,"maximum":60000,"unit":"ft"},
        {"id":"heading","type":"integer","minimum":0,"maximum":359,"unit":"deg"},
        {"id":"powered","type":"boolean"},{"id":"callsign","type":"string"}],
        "controls":[{"id":"RADIO","label":"Radio","outputs":[{"index":0,"type":"string","maxLength":24},{"index":1,"type":"integer","maxValue":65535}],
            "inputs":[{"interface":"set_state","maxValue":65535},{"interface":"variable_step","maxValue":42},{"interface":"fixed_step"},{"interface":"unimplemented"}]}]});
    let ports = integration(&catalogue).unwrap();
    assert_eq!(ports.len(), 9);
    assert!(
        matches!(&ports[0].contract,PortContract::State {schema:ScalarSchema::Number {unit:Some(unit),min,max},fallback:None,..} if unit=="ft" && *min == -1000.0 && *max==60000.0)
    );
    assert!(
        matches!(&ports[1].contract,PortContract::State {schema:ScalarSchema::Integer {unit:Some(unit),min:0,max:359},..} if unit=="deg")
    );
    let variable = ports
        .iter()
        .find(|p| p.id == "command:RADIO:variable_step")
        .unwrap();
    variable
        .validate_arguments(&[PortValue::Number(-42.0)])
        .unwrap();
    assert!(variable
        .validate_arguments(&[PortValue::Number(-43.0)])
        .is_err());
    let step = ports
        .iter()
        .find(|p| p.id == "command:RADIO:fixed_step")
        .unwrap();
    step.validate_arguments(&[PortValue::Text("INC".into())])
        .unwrap();
    assert!(step
        .validate_arguments(&[PortValue::Text("UP".into())])
        .is_err());
    assert!(ports.iter().all(|p| !p.id.ends_with(":unimplemented")));
}
#[test]
fn malformed_integration_catalogues_fail_as_a_whole_instead_of_publishing_partial_ports() {
    for catalogue in [
        json!({"channels":[{"id":"good","type":"boolean"},{"id":"bad","type":"integer","minimum":0.5,"maximum":3}]}),
        json!({"channels":[{"type":"boolean"}]}),
        json!({"channels":[{"id":"x","type":"unknown"}]}),
        json!({"channels":[{"id":"x","type":"number","maximum":5}]}),
        json!({"controls":[{"id":"x","outputs":[{"index":0,"type":"string"}]}]}),
        json!({"controls":[{"id":"x","inputs":[{"interface":"set_state"}]}]}),
    ] {
        assert!(integration(&catalogue).is_err(), "{catalogue}");
    }
}

#[test]
fn exact_guarded_restoration_survives_action_projection_and_legacy_remains_visible() {
    let setter = action(
        "led",
        "setter",
        true,
        json!([{"id":"on","label":"On","type":"boolean"}]),
    );
    let mut cache = json!({"ownership":{"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5000},"actions":[setter]});
    let legacy = actions(&cache).unwrap();
    assert_eq!(legacy.len(), 1);
    assert!(!legacy[0].contract.supports_guarded_setter());
    cache["ownership"]["restorePolicy"] = json!("if-unchanged");
    let guarded = actions(&cache).unwrap();
    assert!(guarded[0].contract.supports_guarded_setter());
    assert_ne!(
        legacy[0].contract_hash().unwrap(),
        guarded[0].contract_hash().unwrap()
    );
    assert_eq!(
        serde_json::to_value(&guarded[0]).unwrap()["contract"]["ownership"]["restorePolicy"],
        "if-unchanged"
    );
    cache["actions"][0]["readable"] = json!(false);
    assert!(!actions(&cache).unwrap()[0]
        .contract
        .supports_guarded_setter());
    cache["ownership"]["restorePolicy"] = json!("always");
    assert!(actions(&cache).is_err());
}

#[test]
fn exact_atomic_fallback_capability_is_preserved_and_never_inferred() {
    let setter = action(
        "led",
        "setter",
        true,
        json!([{"id":"on","label":"On","type":"boolean"}]),
    );
    let mut cache = json!({"ownership":{"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5000,"restorePolicy":"if-unchanged"},"actions":[setter]});
    let old = actions(&cache).unwrap();
    assert!(!old[0].contract.supports_captured_setter());
    cache["ownership"]["fallbackPolicy"] = json!("capture-at-claim");
    let captured = actions(&cache).unwrap();
    assert!(captured[0].contract.supports_captured_setter());
    assert_ne!(
        old[0].contract_hash().unwrap(),
        captured[0].contract_hash().unwrap()
    );
    assert_eq!(
        serde_json::to_value(&captured[0]).unwrap()["contract"]["ownership"]["fallbackPolicy"],
        "capture-at-claim"
    );
    cache["ownership"]["fallbackPolicy"] = Value::Null;
    assert!(actions(&cache).is_err());
}
