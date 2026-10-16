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
    value["schemaVersion"] = json!(2);
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
            .remove("unit");
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
    value["virtualOutputs"][0]["ports"][0]["unit"] = json!("degrees");
    rejected(value);
    let mut value = document_value();
    value["virtualOutputs"][0]["ports"][0]["range"]["max"] = json!(0.5);
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
        ("/resources/0/ports/0/freshnessMs", json!(0)),
        ("/resources/0/ports/0/freshnessMs", Value::Null),
        ("/resources/0/ports/0/fallback", Value::Null),
        ("/resources/0/ports/0/fallback", json!(2)),
        ("/resources/0/ports/0/range/min", json!(2)),
        ("/resources/0/ports/0/enumValues", json!(["bad"])),
        ("/resources/0/ports/1/fallback", json!(false)),
        ("/resources/0/ports/1/unit", json!("seconds")),
    ] {
        let mut value = document_value();
        *value.pointer_mut(pointer).unwrap() = content;
        rejected(value);
    }
    let mut document = fixture("mixed");
    let mut p = document.resources[0].ports[0].clone();
    p.value_type = ValueType::Enum;
    p.unit = None;
    p.range = None;
    p.enum_values = vec!["up".into(), "down".into()];
    p.fallback = Some(PortValue::Text("up".into()));
    assert!(port(&p).is_ok());
    p.enum_values.push("up".into());
    assert!(port(&p).is_err());
    p.enum_values.pop();
    p.fallback = Some(PortValue::Text("other".into()));
    assert!(port(&p).is_err());
    p.value_type = ValueType::Integer;
    p.enum_values.clear();
    p.range = Some(NumericRange { min: 0.0, max: 1.0 });
    p.fallback = Some(PortValue::Number(0.5));
    assert!(port(&p).is_err());
    document.resources[0].ports[0].range.as_mut().unwrap().min = f64::NAN;
    assert!(validate_document(&document).is_err());
}

#[test]
fn visible_processors_require_explicit_valid_type_signatures() {
    let mut value = document_value();
    value["processingNodes"][0]["operation"]["kind"] = json!("identity");
    rejected(value);
    let mut value = document_value();
    value["processingNodes"][1]["operation"]["factor"] = json!(1000);
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
        ("/overrides/0/debounceMs", json!(60001)),
        ("/overrides/0/invertPorts", json!(["missing"])),
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
    value["overrides"][0] =
        json!({"resourceId":"can-a","enabled":null,"debounceMs":null,"invertPorts":[]});
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
    changed.overrides[0].debounce_ms = Some(11);
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
