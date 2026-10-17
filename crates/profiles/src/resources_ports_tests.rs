use super::*;
use serde_json::json;

fn action(parameters: Vec<ActionParameter>) -> PortDescriptor {
    PortDescriptor {
        id: "display.set".into(),
        name: "Display".into(),
        direction: Direction::Input,
        contract: PortContract::Action {
            parameters,
            semantics: ActionSemantics::Setter,
            endpoint: "display".into(),
            readable: true,
            display: None,
            ownership: Some(OwnershipCapability {
                scope: "endpoint".into(),
                max_lease_ms: 5000,
                restore_policy: None,
            }),
        },
    }
}
fn parameter(id: &str, schema: ScalarSchema) -> ActionParameter {
    ActionParameter {
        id: id.into(),
        name: id.into(),
        schema,
    }
}

#[test]
fn multi_argument_actions_round_trip_losslessly_and_validate_every_argument() {
    let port = action(vec![
        parameter("on", ScalarSchema::Boolean {}),
        parameter(
            "level",
            ScalarSchema::Integer {
                min: -10,
                max: 10,
                unit: Some("steps".into()),
            },
        ),
        parameter(
            "mode",
            ScalarSchema::Enum {
                values: vec!["a".into(), "b".into()],
            },
        ),
        parameter("color", ScalarSchema::Rgb {}),
    ]);
    port.validate().unwrap();
    let restored: PortDescriptor =
        serde_json::from_value(serde_json::to_value(&port).unwrap()).unwrap();
    assert_eq!(restored, port);
    let values = vec![
        PortValue::Boolean(true),
        PortValue::Number(10.0),
        PortValue::Text("b".into()),
        PortValue::Text("a0FF00".into()),
    ];
    port.validate_arguments(&values).unwrap();
    assert!(port.validate_arguments(&values[..3]).is_err());
    for (index, bad) in [
        (0, PortValue::Number(1.0)),
        (1, PortValue::Number(10.5)),
        (2, PortValue::Text("other".into())),
        (3, PortValue::Text("GG0000".into())),
    ] {
        let mut wrong = values.clone();
        wrong[index] = bad;
        assert!(port.validate_arguments(&wrong).is_err());
    }
    let mut reordered = port.clone();
    if let PortContract::Action { parameters, .. } = &mut reordered.contract {
        parameters.swap(0, 1);
    }
    assert_ne!(
        port.contract_hash().unwrap(),
        reordered.contract_hash().unwrap()
    );
    let zero = action(vec![]);
    zero.validate_arguments(&[]).unwrap();
}

#[test]
fn display_constraints_preserve_byte_character_and_encoding_limits() {
    let mut port = action(vec![parameter(
        "text",
        ScalarSchema::Text {
            max_bytes: 8,
            encoding: TextEncoding::Utf8,
        },
    )]);
    if let PortContract::Action { display, .. } = &mut port.contract {
        *display = Some(DisplayConstraints {
            columns: 4,
            rows: 1,
            encoding: TextEncoding::Utf8,
        });
    }
    port.validate_arguments(&[PortValue::Text("éééé".into())])
        .unwrap();
    assert!(port
        .validate_arguments(&[PortValue::Text("12345".into())])
        .is_err());
    assert!(port
        .validate_arguments(&[PortValue::Text("ééééé".into())])
        .is_err());
    assert!(port
        .validate_arguments(&[PortValue::Text("bad\n".into())])
        .is_err());
    let ascii = ScalarSchema::Text {
        max_bytes: 8,
        encoding: TextEncoding::Ascii,
    };
    assert!(!ascii.accepts(&PortValue::Text("é".into())));
    if let PortContract::Action { display, .. } = &mut port.contract {
        display.as_mut().unwrap().encoding = TextEncoding::Ascii;
    }
    assert!(port.validate().is_err());
}

#[test]
fn state_event_and_scalar_contracts_are_bounded_and_strict() {
    for schema in [
        ScalarSchema::Integer {
            min: 2,
            max: 1,
            unit: None,
        },
        ScalarSchema::Integer {
            min: 0,
            max: crate::MAX_REVISION as i64 + 1,
            unit: None,
        },
        ScalarSchema::Number {
            min: f64::NAN,
            max: 1.0,
            unit: None,
        },
        ScalarSchema::Text {
            max_bytes: 0,
            encoding: TextEncoding::Utf8,
        },
        ScalarSchema::Enum { values: vec![] },
        ScalarSchema::Enum {
            values: vec!["a".into(), "a".into()],
        },
    ] {
        assert!(schema.validate().is_err());
    }
    let number = ScalarSchema::Number {
        min: -1.0,
        max: 1.0,
        unit: Some("ratio".into()),
    };
    number.validate().unwrap();
    assert!(number.accepts(&PortValue::Number(0.5)));
    assert!(!number.accepts(&PortValue::Number(f64::INFINITY)));
    let mut port = PortDescriptor {
        id: "x".into(),
        name: "X".into(),
        direction: Direction::Output,
        contract: PortContract::State {
            schema: number,
            freshness_ms: 1000,
            fallback: None,
        },
    };
    port.validate().unwrap();
    assert!(port.validate_arguments(&[]).is_err());
    if let PortContract::State { fallback, .. } = &mut port.contract {
        *fallback = Some(PortValue::Number(2.0));
    }
    assert!(port.validate().is_err());
    port.contract = PortContract::Event {
        schema: ScalarSchema::Boolean {},
    };
    port.validate().unwrap();
    let mut value = serde_json::to_value(&port).unwrap();
    value["contract"]["secret"] = json!(true);
    assert!(serde_json::from_value::<PortDescriptor>(value).is_err());
    let mut value = serde_json::to_value(ScalarSchema::Number {
        min: 0.0,
        max: 1.0,
        unit: None,
    })
    .unwrap();
    value.as_object_mut().unwrap().remove("unit");
    assert!(serde_json::from_value::<ScalarSchema>(value).is_err());
}

#[test]
fn action_descriptors_reject_duplicate_parameters_and_false_capabilities() {
    let boolean = parameter("a", ScalarSchema::Boolean {});
    assert!(action(vec![boolean.clone(), boolean.clone()])
        .validate()
        .is_err());
    assert!(action(vec![boolean; 5]).validate().is_err());
    let mut port = action(vec![]);
    port.direction = Direction::Output;
    assert!(port.validate().is_err());
    port.direction = Direction::Input;
    if let PortContract::Action { ownership, .. } = &mut port.contract {
        ownership.as_mut().unwrap().scope = "device".into();
    }
    assert!(port.validate().is_err());
    let mut port = action(vec![parameter(
        "text",
        ScalarSchema::Text {
            max_bytes: 8,
            encoding: TextEncoding::Ascii,
        },
    )]);
    if let PortContract::Action { display, .. } = &mut port.contract {
        *display = Some(DisplayConstraints {
            columns: 63,
            rows: 4,
            encoding: TextEncoding::Ascii,
        });
    }
    assert!(port.validate().is_err());
    if let PortContract::Action {
        display, semantics, ..
    } = &mut port.contract
    {
        *display = Some(DisplayConstraints {
            columns: 4,
            rows: 1,
            encoding: TextEncoding::Ascii,
        });
        *semantics = ActionSemantics::OneShot;
    }
    assert!(port.validate().is_err());
}

#[test]
fn catalogue_hash_ignores_names_order_and_refuses_lossy_port_bindings() {
    let boolean = PortDescriptor {
        id: "b".into(),
        name: "B".into(),
        direction: Direction::Output,
        contract: PortContract::Event {
            schema: ScalarSchema::Boolean {},
        },
    };
    let mut numeric = boolean.clone();
    numeric.id = "n".into();
    numeric.contract = PortContract::Event {
        schema: ScalarSchema::Integer {
            min: 0,
            max: 10,
            unit: Some("steps".into()),
        },
    };
    let expected = catalogue_hash(&[boolean.clone(), numeric.clone()]).unwrap();
    let mut renamed = boolean.clone();
    renamed.name = "Renamed".into();
    assert_eq!(
        expected,
        catalogue_hash(&[numeric.clone(), renamed]).unwrap()
    );
    assert!(catalogue_hash(&[boolean.clone(), boolean.clone()]).is_err());
    let mut graph = Port {
        id: "logical".into(),
        name: "Logical".into(),
        direction: Direction::Output,
        contract: boolean.contract.clone(),
    };
    assert!(graph_compatible(&boolean, &graph));
    graph.direction = Direction::Input;
    assert!(!graph_compatible(&boolean, &graph));
    graph.direction = Direction::Output;
    graph.contract = numeric.contract.clone();
    assert!(graph_compatible(&numeric, &graph));
    let constrained = action(vec![parameter(
        "text",
        ScalarSchema::Text {
            max_bytes: 8,
            encoding: TextEncoding::Utf8,
        },
    )]);
    graph.direction = Direction::Input;
    graph.contract = constrained.contract.clone();
    assert!(graph_compatible(&constrained, &graph));
    if let PortContract::Action { parameters, .. } = &mut graph.contract {
        parameters[0].name = "Cosmetic portable label".into();
    }
    assert!(graph_compatible(&constrained, &graph));
    if let PortContract::Action { parameters, .. } = &mut graph.contract {
        parameters[0].schema = ScalarSchema::Text {
            max_bytes: 9,
            encoding: TextEncoding::Utf8,
        };
    }
    assert!(!graph_compatible(&constrained, &graph));
    let mut update = PortUpdate::complete(vec![boolean]).unwrap();
    update.total = 2;
    assert!(update.validate().is_err());
    update.complete = false;
    update.validate().unwrap();
    update.offset = 3;
    assert!(update.validate().is_err());
}
