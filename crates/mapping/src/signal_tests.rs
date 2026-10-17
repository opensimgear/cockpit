use super::*;
use serde_json::{json, Value};

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
            max: MAX_SAFE_INTEGER as i64 + 1,
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
    let mut update = PortUpdate::complete(vec![boolean]).unwrap();
    update.total = 2;
    assert!(update.validate().is_err());
    update.complete = false;
    update.validate().unwrap();
    update.offset = 3;
    assert!(update.validate().is_err());
}

#[test]
fn invocation_is_an_explicit_graph_tuple_never_a_provider_capability() {
    let parameters = vec![
        parameter("left", ScalarSchema::Boolean {}),
        parameter(
            "right",
            ScalarSchema::Integer {
                min: 0,
                max: 10,
                unit: None,
            },
        ),
    ];
    let invocation = PortContract::Invocation {
        parameters: parameters.clone(),
        semantics: ActionSemantics::OneShot,
    };
    invocation.validate_graph(Direction::Output).unwrap();
    assert!(invocation.validate_graph(Direction::Input).is_err());
    let provider = PortDescriptor {
        id: "action".into(),
        name: "Action".into(),
        direction: Direction::Output,
        contract: invocation.clone(),
    };
    assert!(provider.validate().is_err());
    assert!(provider.contract_hash().is_err());
    assert!(provider.validate_arguments(&[]).is_err());
    let mut destination = action(parameters.clone()).contract;
    assert!(!invocation.can_bind_to(&destination));
    if let PortContract::Action { semantics, .. } = &mut destination {
        *semantics = ActionSemantics::OneShot;
    }
    assert!(invocation.can_bind_to(&destination));
    if let PortContract::Action { parameters, .. } = &mut destination {
        parameters.reverse();
    }
    assert!(!invocation.can_bind_to(&destination));
    let duplicate = PortContract::Invocation {
        parameters: vec![parameters[0].clone(), parameters[0].clone()],
        semantics: ActionSemantics::Setter,
    };
    assert!(duplicate.validate_graph(Direction::Output).is_err());
    let excess = PortContract::Invocation {
        parameters: vec![parameters[0].clone(); 5],
        semantics: ActionSemantics::Setter,
    };
    assert!(excess.validate_graph(Direction::Output).is_err());
    let mut value = serde_json::to_value(invocation).unwrap();
    value.as_object_mut().unwrap().remove("semantics");
    assert!(serde_json::from_value::<PortContract>(value).is_err());
}

#[test]
fn direct_bindings_preserve_delivery_numeric_units_and_constrained_text() {
    let integer = |min, max, unit: Option<&str>| ScalarSchema::Integer {
        min,
        max,
        unit: unit.map(str::to_owned),
    };
    let number = |min, max, unit: Option<&str>| ScalarSchema::Number {
        min,
        max,
        unit: unit.map(str::to_owned),
    };
    let pairs = [
        (integer(0, 10, None), integer(-1, 20, None), true),
        (integer(0, 10, Some("m")), integer(0, 10, Some("ft")), false),
        (number(0.0, 10.0, None), number(0.0, 20.0, None), true),
        (number(0.0, 30.0, None), number(0.0, 20.0, None), false),
        (integer(0, 10, None), number(0.0, 10.0, None), false),
        (
            ScalarSchema::Text {
                max_bytes: 4,
                encoding: TextEncoding::Ascii,
            },
            ScalarSchema::Text {
                max_bytes: 8,
                encoding: TextEncoding::Utf8,
            },
            true,
        ),
        (
            ScalarSchema::Text {
                max_bytes: 4,
                encoding: TextEncoding::Utf8,
            },
            ScalarSchema::Text {
                max_bytes: 8,
                encoding: TextEncoding::Ascii,
            },
            false,
        ),
        (
            ScalarSchema::Text {
                max_bytes: 9,
                encoding: TextEncoding::Utf8,
            },
            ScalarSchema::Text {
                max_bytes: 8,
                encoding: TextEncoding::Utf8,
            },
            false,
        ),
        (
            ScalarSchema::Enum {
                values: vec!["up".into()],
            },
            ScalarSchema::Enum {
                values: vec!["down".into(), "up".into()],
            },
            true,
        ),
        (
            ScalarSchema::Enum {
                values: vec!["missing".into()],
            },
            ScalarSchema::Enum {
                values: vec!["up".into()],
            },
            false,
        ),
        (ScalarSchema::Boolean {}, ScalarSchema::Boolean {}, true),
        (ScalarSchema::Rgb {}, ScalarSchema::Boolean {}, false),
    ];
    for (from, to, accepted) in pairs {
        assert_eq!(from.fits(&to), accepted);
        let state = PortContract::State {
            schema: from.clone(),
            freshness_ms: 1,
            fallback: None,
        };
        let target = PortContract::State {
            schema: to.clone(),
            freshness_ms: 600000,
            fallback: None,
        };
        assert_eq!(state.can_bind_to(&target), accepted);
        assert!(!state.can_bind_to(&PortContract::Event { schema: to.clone() }));
        assert_eq!(
            PortContract::Event { schema: from }.can_bind_to(&PortContract::Event { schema: to }),
            accepted
        );
        assert!(state.scalar_schema().is_some());
    }
    assert!(action(vec![]).contract.scalar_schema().is_none());
}

#[test]
fn bounded_pages_and_exact_contracts_never_admit_oversized_or_malformed_metadata() {
    for value in ["", "bad\n"] {
        assert!(text(value, 80).is_err());
    }
    assert!(text(&"x".repeat(81), 80).is_err());
    let boolean = PortDescriptor {
        id: "button".into(),
        name: "Button".into(),
        direction: Direction::Output,
        contract: PortContract::State {
            schema: ScalarSchema::Boolean {},
            freshness_ms: 1,
            fallback: None,
        },
    };
    assert!(validate_ports(&vec![boolean.clone(); MAX_CATALOGUE_PORTS + 1]).is_err());
    let page = PortUpdate {
        revision: "revision".into(),
        total: 20000,
        offset: 0,
        complete: false,
        ports: (0..MAX_PORT_PAGE + 1)
            .map(|i| PortDescriptor {
                id: format!("p{i}"),
                ..boolean.clone()
            })
            .collect(),
    };
    assert!(page.validate().is_err());
    for schema in [
        ScalarSchema::Integer {
            min: -(MAX_SAFE_INTEGER as i64) - 1,
            max: 0,
            unit: None,
        },
        ScalarSchema::Number {
            min: 0.0,
            max: MAX_SAFE_INTEGER as f64 + 1.0,
            unit: None,
        },
        ScalarSchema::Number {
            min: 0.0,
            max: 1.0,
            unit: Some("".into()),
        },
        ScalarSchema::Text {
            max_bytes: 4097,
            encoding: TextEncoding::Utf8,
        },
        ScalarSchema::Enum {
            values: vec!["".into()],
        },
    ] {
        assert!(schema.validate().is_err());
    }
    let mut invalid = boolean.clone();
    if let PortContract::State { freshness_ms, .. } = &mut invalid.contract {
        *freshness_ms = 0;
    }
    assert!(invalid.validate().is_err());
    let mut invalid = action(vec![]);
    if let PortContract::Action { ownership, .. } = &mut invalid.contract {
        ownership.as_mut().unwrap().max_lease_ms = 0;
    }
    assert!(invalid.validate().is_err());
}

#[test]
fn guarded_restore_is_explicit_strict_and_part_of_the_exact_contract_identity() {
    let legacy = action(vec![]);
    let old_hash = legacy.contract_hash().unwrap();
    let value = serde_json::to_value(&legacy).unwrap();
    assert!(value["contract"]["ownership"]
        .get("restorePolicy")
        .is_none());
    assert!(!legacy.contract.supports_guarded_setter());
    let mut guarded = value.clone();
    guarded["contract"]["ownership"]["restorePolicy"] = json!("if-unchanged");
    let port: PortDescriptor = serde_json::from_value(guarded.clone()).unwrap();
    port.validate().unwrap();
    assert!(port.contract.supports_guarded_setter());
    assert_ne!(port.contract_hash().unwrap(), old_hash);
    assert_eq!(serde_json::to_value(&port).unwrap(), guarded);
    for bad in [Value::Null, json!("always"), json!(true), json!(1)] {
        let mut invalid = guarded.clone();
        invalid["contract"]["ownership"]["restorePolicy"] = bad;
        assert!(serde_json::from_value::<PortDescriptor>(invalid).is_err());
    }
    for (field, bad) in [("readable", json!(false)), ("semantics", json!("one-shot"))] {
        let mut restricted = guarded.clone();
        restricted["contract"][field] = bad;
        let port: PortDescriptor = serde_json::from_value(restricted).unwrap();
        assert!(!port.contract.supports_guarded_setter());
    }
    for (scope, max_lease_ms) in [("device", 5000), ("endpoint", 0), ("endpoint", 60001)] {
        assert!(!OwnershipCapability {
            scope: scope.into(),
            max_lease_ms,
            restore_policy: Some(RestorePolicy::IfUnchanged)
        }
        .supports_guarded_restore());
    }
    let state = PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 10,
        fallback: None,
    };
    assert!(!state.supports_guarded_setter());
}
