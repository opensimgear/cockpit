use super::*;
use crate::{
    compiler_models::{CompileSnapshot, PlanReadiness},
    ResourceKind,
};
fn port(id: &str, schema: ScalarSchema, fallback: PortValue) -> Port {
    Port {
        id: id.into(),
        name: id.into(),
        direction: Direction::Output,
        contract: PortContract::State {
            schema,
            fallback: Some(fallback),
            freshness_ms: 1000,
        },
    }
}
fn document() -> crate::ProfileDocument {
    let mut document = crate::default_document();
    document.resources.push(Resource {
        id: "debug".into(),
        name: "Debug controller".into(),
        kind: ResourceKind::DebugController,
        required: false,
        ports: vec![
            port(
                "axis-1",
                ScalarSchema::Integer {
                    min: -32768,
                    max: 32767,
                    unit: None,
                },
                PortValue::Number(0.0),
            ),
            port(
                "axis-16",
                ScalarSchema::Integer {
                    min: 0,
                    max: 32767,
                    unit: None,
                },
                PortValue::Number(0.0),
            ),
            port(
                "button-128",
                ScalarSchema::Boolean {},
                PortValue::Boolean(false),
            ),
            port(
                "hat-4",
                ScalarSchema::Integer {
                    min: -1,
                    max: 7,
                    unit: None,
                },
                PortValue::Number(-1.0),
            ),
        ],
    });
    document
}
#[test]
fn portable_debug_controls_validate_roundtrip_and_compile_without_inventory() {
    let mut document = document();
    let mut empty = document.clone();
    empty.resources[0].ports.clear();
    assert!(crate::validate_document(&empty).is_err());
    let encoded = document.export().unwrap();
    assert_eq!(crate::ProfileDocument::parse(&encoded).unwrap(), document);
    let original = document.resources[0].ports[0].clone();
    for id in ["axis-0", "axis-01", "axis-17", "button-129", "hat-5", "raw"] {
        let mut invalid = document.clone();
        invalid.resources[0].ports[0].id = id.into();
        assert!(crate::validate_document(&invalid).is_err(), "{id}");
    }
    for replacement in [
        port(
            "axis-1",
            ScalarSchema::Integer {
                min: -100,
                max: 100,
                unit: None,
            },
            PortValue::Number(0.0),
        ),
        port(
            "axis-1",
            ScalarSchema::Integer {
                min: -32768,
                max: 32767,
                unit: Some("ratio".into()),
            },
            PortValue::Number(0.0),
        ),
        port(
            "axis-1",
            ScalarSchema::Number {
                min: -32768.0,
                max: 32767.0,
                unit: None,
            },
            PortValue::Number(0.0),
        ),
        port(
            "button-1",
            ScalarSchema::Boolean {},
            PortValue::Boolean(true),
        ),
        port(
            "hat-1",
            ScalarSchema::Integer {
                min: -1,
                max: 7,
                unit: None,
            },
            PortValue::Number(0.0),
        ),
    ] {
        let mut invalid = document.clone();
        invalid.resources[0].ports[0] = replacement;
        assert!(crate::validate_document(&invalid).is_err());
    }
    let mut invalid = document.clone();
    invalid.resources[0].ports[0].direction = Direction::Input;
    assert!(crate::validate_document(&invalid).is_err());
    invalid = document.clone();
    invalid.resources[0].ports[0].contract = PortContract::Event {
        schema: ScalarSchema::Boolean {},
    };
    assert!(crate::validate_document(&invalid).is_err());
    invalid = document.clone();
    invalid.visibility.push(crate::VisibilityIntent {
        resource_id: "debug".into(),
        mode: crate::VisibilityMode::HiddenWhileActive,
    });
    assert!(crate::validate_document(&invalid).is_err());
    let assignment = crate::store::LocalAssignment {
        resource_id: "debug".into(),
        source_id: "fake".into(),
        ports: vec![],
    };
    assert!(crate::store::validate_assignments(&document, &[assignment])
        .unwrap_err()
        .contains("do not use"));
    let mut input = original.clone();
    input.id = "in".into();
    input.direction = Direction::Input;
    let mut output = original;
    output.id = "out".into();
    document.processing_nodes.push(crate::ProcessingNode {
        id: "identity".into(),
        name: "Identity".into(),
        operation: crate::Operation::Identity {},
        inputs: vec![input],
        outputs: vec![output],
        draft: None,
    });
    document.bindings.push(crate::Binding {
        id: "route".into(),
        from: crate::Endpoint {
            node_id: "debug".into(),
            port_id: "axis-1".into(),
        },
        to: crate::Endpoint {
            node_id: "identity".into(),
            port_id: "in".into(),
        },
    });
    let snapshot = CompileSnapshot {
        inventory_session: "test".into(),
        inventory_revision: "1".into(),
        assignments: vec![],
        resources: vec![],
        game_context: None,
        backend: Default::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    };
    let report = crate::compiler::compile(&document, &snapshot);
    assert_eq!(
        report.readiness,
        PlanReadiness::Ready,
        "{:?}",
        report.diagnostics
    );
    let plan = report.plan.unwrap();
    assert_eq!(plan.sources.len(), 1);
    let source = &plan.sources[0];
    assert_eq!(source.readiness, PlanReadiness::Ready);
    assert!(
        source.route.is_none()
            && source.token.is_none()
            && source.preparation.is_none()
            && source.inventory_resource_id.is_none()
    );
}
