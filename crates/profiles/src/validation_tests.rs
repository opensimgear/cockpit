use super::*;
use crate::{default_document, new_document, DEFAULT_PROFILE_ID, MAX_REVISION};
use serde_json::{json, Value};

fn fixture(name: &str) -> ProfileDocument {
    let bytes = match name {
        "default" => include_bytes!("../../../tests/fixtures/profiles/default.json").as_slice(),
        "blank" => include_bytes!("../../../tests/fixtures/profiles/blank.json").as_slice(),
        _ => include_bytes!("../../../tests/fixtures/profiles/mixed.json").as_slice(),
    };
    ProfileDocument::parse(bytes).unwrap()
}
fn document_value() -> Value {
    serde_json::to_value(fixture("mixed")).unwrap()
}
fn rejected(value: Value) {
    assert!(ProfileDocument::parse(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn fixtures_roundtrip_and_default_is_stable_without_activation() {
    for name in ["default", "blank", "mixed"] {
        let document = fixture(name);
        assert_eq!(
            ProfileDocument::parse(&document.export().unwrap()).unwrap(),
            document
        );
        assert_eq!(
            document.semantic_hash().unwrap(),
            semantic_hash(&document).unwrap()
        );
    }
    assert_eq!(fixture("default"), default_document());
    assert_eq!(default_document().id, DEFAULT_PROFILE_ID);
    assert!(default_document().resources.is_empty());
    let first = new_document("  New profile  ").unwrap();
    let second = new_document("New profile").unwrap();
    assert_eq!(first.name, "New profile");
    assert_ne!(first.id, second.id);
    assert_eq!(
        first.semantic_hash().unwrap(),
        second.semantic_hash().unwrap()
    );
    assert!(new_document("").is_err());
    assert!(new_document(&"é".repeat(81)).is_err());
    assert!(new_document(&"é".repeat(80)).is_ok());
    assert!(new_document("Bad\nname").is_err());
}

#[test]
fn version_revision_uuid_and_missing_fields_are_strict() {
    let mut value = document_value();
    value["schemaVersion"] = json!(1);
    rejected(value);
    let mut value = document_value();
    value["revision"] = json!(0);
    rejected(value);
    let mut value = document_value();
    value["revision"] = json!(MAX_REVISION + 1);
    rejected(value);
    let mut value = document_value();
    value["id"] = json!("not-a-uuid");
    rejected(value);
    let mut value = document_value();
    value["id"] = json!("22222222222242228222222222222222");
    rejected(value);
    for pointer in ["/resources/0/ports/0", "/virtualOutputs/0/ports/0"] {
        let mut value = document_value();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("contract");
        rejected(value);
    }
    let mut value = document_value();
    value.as_object_mut().unwrap().remove("editor");
    rejected(value);
    let mut value = document_value();
    value["overrides"][0]
        .as_object_mut()
        .unwrap()
        .remove("enabled");
    rejected(value);
}

#[test]
fn machine_local_and_unknown_fields_are_rejected_at_every_boundary() {
    for (pointer, field, content) in [
        ("", "selectedProfileId", json!(DEFAULT_PROFILE_ID)),
        ("", "assignments", json!({"wheel":"serial-number"})),
        ("/resources/0", "serialNumber", json!("LOCAL-123")),
        ("/resources/0/ports/0", "rawCalibration", json!({"min":0})),
        ("/virtualOutputs/0", "vjoySlot", json!(1)),
        ("/eligibility", "pid", json!(123)),
        (
            "/companionAssignments/0",
            "companionId",
            json!("paired-installation"),
        ),
        ("/overrides/0", "password", json!("secret")),
        ("/processingNodes/0/operation", "backend", json!("driver")),
        (
            "/editor/viewport",
            "connection",
            json!({"host":"127.0.0.1"}),
        ),
    ] {
        let mut value = document_value();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(field.into(), content);
        rejected(value);
    }
    let mut value = document_value();
    value["processingNodes"][0]["operation"]["kind"] = json!("unknown-implicit-code");
    rejected(value);
}

#[test]
fn byte_and_collection_limits_apply_to_parse_and_direct_validation() {
    let mut oversized = vec![b' '; MAX_PROFILE_BYTES + 1];
    oversized.extend_from_slice(include_bytes!(
        "../../../tests/fixtures/profiles/blank.json"
    ));
    assert!(ProfileDocument::parse(&oversized)
        .unwrap_err()
        .contains("512 KiB"));
    let mut document = fixture("mixed");
    document.resources[0].name = "x".repeat(MAX_PROFILE_BYTES);
    assert!(validate_document(&document)
        .unwrap_err()
        .contains("512 KiB"));
    assert!(document.export().is_err());
    let mut document = fixture("blank");
    document.resources = (0..=MAX_RESOURCES)
        .map(|i| Resource {
            id: format!("device-{i}"),
            name: "Device".into(),
            kind: ResourceKind::Device,
            required: false,
            ports: vec![],
        })
        .collect();
    assert!(validate_document(&document).is_err());
    let mut document = fixture("mixed");
    document.resources[0].ports = (0..=MAX_PORTS_PER_NODE)
        .map(|i| {
            let mut p = document.resources[0].ports[0].clone();
            p.id = format!("axis-{i}");
            p
        })
        .collect();
    assert!(validate_document(&document).is_err());
}

#[test]
fn logical_nodes_ports_and_references_must_be_unique_and_resolvable() {
    let mut value = document_value();
    value["resources"][1]["id"] = json!("third-party-a");
    rejected(value);
    let mut value = document_value();
    value["resources"][0]["ports"][1]["id"] = json!("axis");
    rejected(value);
    let mut value = document_value();
    value["processingNodes"][0]["id"] = json!("third-party-a");
    rejected(value);
    let mut value = document_value();
    value["bindings"][0]["from"]["nodeId"] = json!("missing");
    rejected(value);
    let mut value = document_value();
    value["bindings"][0]["from"]["portId"] = json!("missing");
    rejected(value);
    let mut value = document_value();
    value["bindings"][0]["id"] = value["bindings"][1]["id"].clone();
    rejected(value);
    let mut value = document_value();
    value["editor"]["positions"][0]["nodeId"] = json!("missing");
    rejected(value);
    let mut value = document_value();
    value["resources"][0]["id"] = json!("C:/machine/path");
    rejected(value);
}

#[test]
fn directional_bindings_do_not_hide_converters_or_multiple_writers() {
    let mut value = document_value();
    value["bindings"][0]["from"] = json!({"nodeId":"combined-controller","portId":"steering"});
    rejected(value);
    let mut value = document_value();
    value["bindings"][0]["to"] = json!({"nodeId":"third-party-b","portId":"axis"});
    rejected(value);
    let mut value = document_value();
    value["virtualOutputs"][0]["ports"][0]["contract"]["schema"]["unit"] = json!("degrees");
    rejected(value);
    let mut value = document_value();
    value["virtualOutputs"][0]["ports"][0]["contract"]["schema"]["max"] = json!(500);
    rejected(value);
    let mut value = document_value();
    value["bindings"][0]["from"] = json!({"nodeId":"third-party-a","portId":"pressed"});
    rejected(value);
    let mut document = fixture("mixed");
    let mut extra = document.bindings[0].clone();
    extra.id = "second-writer".into();
    extra.from.node_id = "third-party-b".into();
    document.bindings.push(extra);
    assert!(validate_document(&document)
        .unwrap_err()
        .contains("multiple writers"));
}

#[test]
fn port_contracts_validate_units_ranges_enums_freshness_and_fallback() {
    for (pointer, content) in [
        ("/resources/0/ports/0/contract/freshnessMs", json!(0)),
        ("/resources/0/ports/0/contract/freshnessMs", Value::Null),
        ("/resources/0/ports/0/contract/fallback", json!(40000)),
        ("/resources/0/ports/0/contract/schema/min", json!(40000)),
        ("/resources/0/ports/0/contract/schema/unit", json!("")),
        (
            "/resources/0/ports/1/contract/schema/type",
            json!("unknown"),
        ),
    ] {
        let mut value = document_value();
        *value.pointer_mut(pointer).unwrap() = content;
        rejected(value);
    }
    let mut p = fixture("mixed").resources[0].ports[0].clone();
    p.contract = PortContract::State {
        schema: crate::resources_ports::ScalarSchema::Enum {
            values: vec!["up".into(), "down".into()],
        },
        freshness_ms: 1000,
        fallback: Some(PortValue::Text("up".into())),
    };
    assert!(port(&p).is_ok());
    if let PortContract::State {
        schema: crate::resources_ports::ScalarSchema::Enum { values },
        ..
    } = &mut p.contract
    {
        values.push("up".into());
    }
    assert!(port(&p).is_err());
    p.contract = PortContract::State {
        schema: crate::resources_ports::ScalarSchema::Integer {
            min: 0,
            max: 1,
            unit: None,
        },
        freshness_ms: 1000,
        fallback: Some(PortValue::Number(0.5)),
    };
    assert!(port(&p).is_err());
    p.contract = PortContract::State {
        schema: crate::resources_ports::ScalarSchema::Number {
            min: f64::NAN,
            max: 1.0,
            unit: None,
        },
        freshness_ms: 1000,
        fallback: None,
    };
    assert!(port(&p).is_err());
    // Null fallback is deliberate: unknown/stale never become a fabricated zero.
    p.contract = PortContract::State {
        schema: crate::resources_ports::ScalarSchema::Boolean {},
        freshness_ms: 1000,
        fallback: None,
    };
    assert!(port(&p).is_ok());
}

#[test]
fn visible_processors_require_explicit_valid_type_signatures() {
    let mut value = document_value();
    value["processingNodes"][0]["operation"]["kind"] = json!("identity");
    rejected(value);
    let mut value = document_value();
    value["processingNodes"][1]["operation"]["to"] = json!("seconds");
    rejected(value);
    let mut value = document_value();
    value["processingNodes"][2]["outputs"][0]["direction"] = json!("input");
    rejected(value);
    let mut value = document_value();
    value["processingNodes"][0]["inputs"] = json!([]);
    rejected(value);
    let mut document = fixture("mixed");
    document.processing_nodes[1].operation = Operation::Scale {
        factor: f64::INFINITY,
        offset: 0.0,
    };
    assert!(validate_document(&document).is_err());
    let mut document = fixture("mixed");
    document.processing_nodes[2].operation = Operation::Not {};
    assert!(validate_document(&document).is_err());
}

fn identity_node(name: &str, prototype: &Port) -> ProcessingNode {
    let mut input = prototype.clone();
    input.id = "input".into();
    input.direction = Direction::Input;
    let mut output = prototype.clone();
    output.id = "output".into();
    output.direction = Direction::Output;
    ProcessingNode {
        id: name.into(),
        name: name.into(),
        operation: Operation::Identity {},
        inputs: vec![input],
        outputs: vec![output],
    }
}
fn binding(name: &str, from: (&str, &str), to: (&str, &str)) -> Binding {
    Binding {
        id: name.into(),
        from: Endpoint {
            node_id: from.0.into(),
            port_id: from.1.into(),
        },
        to: Endpoint {
            node_id: to.0.into(),
            port_id: to.1.into(),
        },
    }
}
#[test]
fn processing_cycles_are_rejected_but_duplex_resources_are_not_implicit_feedback() {
    let mut document = fixture("mixed");
    document
        .processing_nodes
        .push(identity_node("loop-a", &document.resources[0].ports[0]));
    document
        .processing_nodes
        .push(identity_node("loop-b", &document.resources[0].ports[0]));
    document.bindings.push(binding(
        "loop-ab",
        ("loop-a", "output"),
        ("loop-b", "input"),
    ));
    document.bindings.push(binding(
        "loop-ba",
        ("loop-b", "output"),
        ("loop-a", "input"),
    ));
    assert!(validate_document(&document).unwrap_err().contains("cycle"));
    let mut document = fixture("mixed");
    let mut consumer = document.resources[0].ports[0].clone();
    consumer.id = "feedback".into();
    consumer.direction = Direction::Input;
    document.resources[0].ports.push(consumer);
    document.processing_nodes.push(identity_node(
        "feedback-copy",
        &document.resources[0].ports[0],
    ));
    document.bindings.push(binding(
        "feedback-input",
        ("third-party-a", "axis"),
        ("feedback-copy", "input"),
    ));
    document.bindings.push(binding(
        "feedback-output",
        ("feedback-copy", "output"),
        ("third-party-a", "feedback"),
    ));
    assert!(validate_document(&document).is_ok());
}

#[test]
fn intent_and_assignment_targets_stay_logical_and_typed() {
    for (pointer, content) in [
        ("/visibility/0/resourceId", json!("companion-panel")),
        ("/visibility/0/mode", json!("hidhide-driver")),
        ("/overrides/0/resourceId", json!("dcs")),
        ("/overrides/0/enabled", json!("true")),
        ("/companionAssignments/0/resourceId", json!("third-party-a")),
        (
            "/companionAssignments/0/publicationHash",
            json!("local/path/file.json"),
        ),
    ] {
        let mut value = document_value();
        *value.pointer_mut(pointer).unwrap() = content;
        rejected(value);
    }
    let mut value = document_value();
    value["overrides"][0] = json!({"resourceId":"can-a","enabled":null});
    rejected(value);
    let mut value = document_value();
    value["visibility"][1] = value["visibility"][0].clone();
    rejected(value);
}

#[test]
fn semantic_hash_ignores_cosmetics_identity_and_unordered_declarations() {
    let original = fixture("mixed");
    let expected = semantic_hash(&original).unwrap();
    let mut changed = original.clone();
    changed.id = new_document("Other").unwrap().id;
    changed.name = "Renamed".into();
    changed.revision += 1;
    changed.resources.reverse();
    changed.resources[0].name = "Different label".into();
    changed.resources[0].ports.reverse();
    changed.bindings.reverse();
    changed.processing_nodes.reverse();
    changed.virtual_outputs.reverse();
    changed.visibility.reverse();
    changed.editor.viewport.zoom = 2.0;
    changed.editor.positions.clear();
    assert_eq!(semantic_hash(&changed).unwrap(), expected);
    changed.overrides[0].enabled = false;
    assert_ne!(semantic_hash(&changed).unwrap(), expected);
}

#[test]
fn invalid_editor_state_and_safe_revision_boundary_are_checked() {
    let mut value = document_value();
    value["editor"]["viewport"]["zoom"] = json!(0);
    rejected(value);
    let mut value = document_value();
    value["editor"]["positions"][0]["x"] = json!(1000001);
    rejected(value);
    let mut document = fixture("mixed");
    document.editor.viewport.x = f64::INFINITY;
    assert!(validate_document(&document).is_err());
    document = fixture("mixed");
    document.revision = MAX_REVISION;
    assert!(validate_document(&document).is_ok());
}

#[test]
fn semantic_identity_preserves_action_arguments_controller_order_and_script_parameters() {
    let original = fixture("mixed");
    let expected = original.semantic_hash().unwrap();
    let mut reordered = original.clone();
    reordered.virtual_outputs[0].layout.axes.reverse();
    assert_ne!(reordered.semantic_hash().unwrap(), expected);
    let mut changed = original.clone();
    changed.processing_nodes[3].operation = osg_mapping::program::Operation::Script {
        source: "output = parameters.name".into(),
        parameters: std::collections::BTreeMap::from([(
            "name".into(),
            PortValue::Text("first".into()),
        )]),
    };
    let first = changed.semantic_hash().unwrap();
    if let osg_mapping::program::Operation::Script { parameters, .. } =
        &mut changed.processing_nodes[3].operation
    {
        parameters.insert("name".into(), PortValue::Text("second".into()));
    }
    assert_ne!(changed.semantic_hash().unwrap(), first);
    let mut graph = fixture("blank");
    let action = |id: &str| crate::resources_ports::ActionParameter {
        id: id.into(),
        name: id.into(),
        schema: crate::resources_ports::ScalarSchema::Boolean {},
    };
    graph.processing_nodes.push(ProcessingNode {
        id: "pack".into(),
        name: "Ordered arguments".into(),
        operation: Operation::SetOnChange {
            arguments: vec!["left".into(), "right".into()],
        },
        inputs: ["left", "right"]
            .into_iter()
            .map(|id| Port {
                id: id.into(),
                name: id.into(),
                direction: Direction::Input,
                contract: PortContract::State {
                    schema: crate::resources_ports::ScalarSchema::Boolean {},
                    freshness_ms: 1000,
                    fallback: Some(PortValue::Boolean(false)),
                },
            })
            .collect(),
        outputs: vec![Port {
            id: "request".into(),
            name: "Request".into(),
            direction: Direction::Output,
            contract: PortContract::Invocation {
                parameters: vec![action("left"), action("right")],
                semantics: crate::resources_ports::ActionSemantics::Setter,
            },
        }],
    });
    let original = graph.semantic_hash().unwrap();
    graph.processing_nodes[0].inputs.reverse();
    assert_eq!(graph.semantic_hash().unwrap(), original);
    if let Operation::SetOnChange { arguments } = &mut graph.processing_nodes[0].operation {
        arguments.reverse();
    }
    if let PortContract::Invocation { parameters, .. } =
        &mut graph.processing_nodes[0].outputs[0].contract
    {
        parameters.reverse();
    }
    assert_ne!(graph.semantic_hash().unwrap(), original);
}

#[test]
fn explicit_virtual_layout_and_invocation_boundaries_reject_hidden_processing() {
    let mut graph = fixture("mixed");
    graph.virtual_outputs[0].layout.axes[0].port_id = "missing".into();
    assert!(validate_document(&graph).is_err());
    graph = fixture("mixed");
    graph.virtual_outputs[0].layout.axes[0].fallback = 4;
    assert!(validate_document(&graph).is_err());
    graph = fixture("mixed");
    if let PortContract::State { freshness_ms, .. } =
        &mut graph.virtual_outputs[0].ports[0].contract
    {
        *freshness_ms = 2000;
    }
    assert!(validate_document(&graph).is_ok());
    let mut value = document_value();
    value["overrides"][0]["invertPorts"] = json!(["axis"]);
    rejected(value);
    let mut value = document_value();
    value["overrides"][0]["debounceMs"] = json!(10);
    rejected(value);
    let mut value = document_value();
    value["resources"][0]["ports"][1]["contract"] =
        json!({"delivery":"invocation","parameters":[],"semantics":"one-shot"});
    rejected(value);
    let mut value = document_value();
    value["processingNodes"][3]["outputs"][0]["contract"]["semantics"] = json!("one-shot");
    rejected(value);
}

#[test]
fn native_argument_port_names_roundtrip_without_relaxing_node_identities() {
    use crate::resources::PortExpectation;
    use crate::resources_ports::{ActionParameter, ActionSemantics, ScalarSchema};
    use crate::store::{Library, LocalAssignment};
    let mut graph = crate::default_document();
    let state = PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 1000,
        fallback: Some(PortValue::Boolean(false)),
    };
    let parameter = |id: &str| ActionParameter {
        id: id.into(),
        name: "Argument".into(),
        schema: ScalarSchema::Boolean {},
    };
    graph.resources.push(Resource {
        id: "device".into(),
        name: "Device".into(),
        kind: ResourceKind::Device,
        required: true,
        ports: ["0", "gain:0"]
            .into_iter()
            .map(|id| Port {
                id: id.into(),
                name: "Argument source".into(),
                direction: Direction::Output,
                contract: state.clone(),
            })
            .collect(),
    });
    graph.processing_nodes.push(ProcessingNode {
        id: "pack".into(),
        name: "Explicit argument tuple".into(),
        operation: Operation::SetOnChange {
            arguments: vec!["0".into(), "gain:0".into()],
        },
        inputs: ["0", "gain:0"]
            .into_iter()
            .map(|id| Port {
                id: id.into(),
                name: "Argument input".into(),
                direction: Direction::Input,
                contract: state.clone(),
            })
            .collect(),
        outputs: vec![Port {
            id: "request".into(),
            name: "Request".into(),
            direction: Direction::Output,
            contract: PortContract::Invocation {
                parameters: vec![parameter("0"), parameter("gain:0")],
                semantics: ActionSemantics::Setter,
            },
        }],
    });
    graph.resources.push(Resource {
        id: "module".into(),
        name: "Module".into(),
        kind: ResourceKind::Device,
        required: false,
        ports: vec![Port {
            id: "action:0".into(),
            name: "Action".into(),
            direction: Direction::Input,
            contract: PortContract::Action {
                parameters: vec![parameter("0"), parameter("gain:0")],
                semantics: ActionSemantics::Setter,
                endpoint: "module:0".into(),
                readable: true,
                display: None,
                ownership: None,
            },
        }],
    });
    graph.bindings = vec![
        binding("arg-zero", ("device", "0"), ("pack", "0")),
        binding("arg-gain", ("device", "gain:0"), ("pack", "gain:0")),
        binding("invoke", ("pack", "request"), ("module", "action:0")),
    ];
    validate_document(&graph).unwrap();
    assert_eq!(
        ProfileDocument::parse(&graph.export().unwrap()).unwrap(),
        graph
    );
    let mut library = Library::default();
    library.profiles[0] = graph.clone();
    library.local_assignments.insert(
        graph.id.clone(),
        graph
            .resources
            .iter()
            .map(|r| LocalAssignment {
                resource_id: r.id.clone(),
                source_id: format!("native:{}", r.id),
                ports: r
                    .ports
                    .iter()
                    .map(|p| PortExpectation {
                        profile_port_id: p.id.clone(),
                        observed_port_id: format!("observed:{}", p.id),
                        expected_contract_hash: p.descriptor().contract_hash().unwrap(),
                    })
                    .collect(),
            })
            .collect(),
    );
    let restored: Library = serde_json::from_slice(&library.encode().unwrap()).unwrap();
    restored.validate().unwrap();
    for id in [
        "0".to_owned(),
        "gain:0".into(),
        "native argument".into(),
        "é".repeat(40),
    ] {
        assert!(port_identifier(&id));
    }
    for id in [
        "".to_owned(),
        "bad\nport".into(),
        "bad\u{85}port".into(),
        "é".repeat(41),
    ] {
        assert!(!port_identifier(&id));
        let mut bad = graph.clone();
        bad.resources[0].ports[0].id = id.clone();
        assert!(validate_document(&bad).is_err());
        let mut bad = restored.clone();
        bad.local_assignments.get_mut(&graph.id).unwrap()[0].ports[0].profile_port_id = id;
        assert!(bad.validate().is_err());
    }
    graph.resources[0].id = "gain:0".into();
    assert!(validate_document(&graph).is_err());
}
