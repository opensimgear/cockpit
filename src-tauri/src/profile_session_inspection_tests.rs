use super::*;
use osg_mapping::{
    engine::{
        typed::{Change, Frame},
        Counter, Reason,
    },
    program::Operation,
};
use osg_profiles::{
    compiler::compile,
    compiler_models::{BackendCapabilities, CompileSnapshot},
    Binding, PortValue, ProcessingNode, Resource, ResourceKind,
};
use serde_json::{json, Value};
fn endpoint(node: &str, port: &str) -> Endpoint {
    Endpoint {
        node_id: node.into(),
        port_id: port.into(),
    }
}
fn port(id: &str, direction: Direction) -> Port {
    Port {
        id: id.into(),
        name: id.into(),
        direction,
        contract: PortContract::State {
            schema: osg_profiles::resources_ports::ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: None,
        },
    }
}
fn fixture() -> (ProfileDocument, Phase, OwnedRun) {
    let mut document = osg_profiles::default_document();
    document.resources.push(Resource {
        id: "panel".into(),
        name: "Panel".into(),
        kind: ResourceKind::Companion,
        required: false,
        ports: vec![
            port("pressed", Direction::Output),
            port("indicator", Direction::Input),
            port("unused", Direction::Output),
        ],
    });
    document.processing_nodes.push(ProcessingNode { draft: None,
        id: "invert".into(),
        name: "Invert".into(),
        operation: Operation::Not {},
        inputs: vec![port("input", Direction::Input)],
        outputs: vec![port("output", Direction::Output)],
    });
    document.bindings = vec![
        Binding {
            id: "to-invert".into(),
            from: endpoint("panel", "pressed"),
            to: endpoint("invert", "input"),
        },
        Binding {
            id: "to-indicator".into(),
            from: endpoint("invert", "output"),
            to: endpoint("panel", "indicator"),
        },
    ];
    let snapshot = CompileSnapshot {
        inventory_session: "fixture".into(),
        inventory_revision: "1".into(),
        assignments: vec![],
        resources: vec![],
        game_context: None,
        backend: BackendCapabilities::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    };
    let report = compile(&document, &snapshot);
    let session = uuid::Uuid::new_v4().to_string();
    let phase = Phase::new(report.plan.unwrap().program, Counter(1), session.clone()).unwrap();
    let run = OwnedRun {
        owner: RunOwner {
            session,
            generation: 1,
            profile_id: document.id.clone(),
            profile_revision: document.revision,
            game_id: None,
        },
        snapshot: osg_profiles::session::AppliedSnapshot::capture(&document)
            .unwrap()
            .summary()
            .clone(),
        program_hash: report.program_hash.unwrap(),
        capture_fingerprint: report.capture_fingerprint,
        receipts: vec![],
        attempted: vec![],
    };
    (document, phase, run)
}
fn request(run: &OwnedRun, fields: Value) -> Request {
    let mut value = json!({"command":"profile_runtime_inspect","owner":run.owner});
    value
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    Request::parse(&value).unwrap()
}
#[test]
fn cached_graph_projection_preserves_real_timestamps_and_never_advances_or_drains() {
    let (document, mut phase, run) = fixture();
    let request = request(&run, json!({}));
    let initial = capture(&document, &phase, &run, &request).unwrap();
    assert_eq!(initial.total, 5);
    assert!(initial
        .ports
        .iter()
        .all(|p| matches!(p.sample, SignalSample::Unknown) && p.observed_us.is_none()));
    phase
        .accept(Frame {
            source: "panel".into(),
            generation: Counter(1),
            sequence: Counter(1),
            captured_us: Counter(12_000),
            reason: Reason::Baseline,
            changes: vec![Change {
                port_id: "pressed".into(),
                sample: SignalSample::scalar(PortValue::Boolean(true)),
            }],
        })
        .unwrap();
    let clock = phase.at_us();
    let mut last = None;
    for _ in 0..10 {
        last = Some(capture(&document, &phase, &run, &request).unwrap());
        assert_eq!(phase.at_us(), clock);
    }
    let page = last.unwrap();
    assert_eq!(page.captured_us, "12000");
    for (node, port, value) in [
        ("panel", "pressed", true),
        ("invert", "input", true),
        ("invert", "output", false),
        ("panel", "indicator", false),
    ] {
        let actual = page
            .ports
            .iter()
            .find(|p| p.endpoint == endpoint(node, port))
            .unwrap();
        assert_eq!(
            actual.sample,
            SignalSample::scalar(PortValue::Boolean(value))
        );
        assert_eq!(actual.observed_us.as_deref(), Some("12000"));
    }
    assert!(
        !phase.drain().is_empty(),
        "Inspection must preserve pending graph effects."
    );
    phase.advance(Counter(1_012_000)).unwrap();
    let stale = capture(&document, &phase, &run, &request).unwrap();
    assert!(matches!(
        stale
            .ports
            .iter()
            .find(|p| p.endpoint == endpoint("panel", "pressed"))
            .unwrap()
            .sample,
        SignalSample::Stale
    ));
}
#[test]
fn applied_pages_are_exact_sorted_bounded_and_reject_changed_owners_documents_and_nodes() {
    let (document, mut phase, run) = fixture();
    let filtered = capture(
        &document,
        &phase,
        &run,
        &request(&run, json!({"nodeId":"panel","offset":1,"limit":1})),
    )
    .unwrap();
    assert_eq!(filtered.total, 3);
    assert_eq!(filtered.ports[0].endpoint, endpoint("panel", "pressed"));
    assert!(capture(
        &document,
        &phase,
        &run,
        &request(&run, json!({"nodeId":"missing"}))
    )
    .is_err());
    assert!(capture(&document, &phase, &run, &request(&run, json!({"offset":6}))).is_err());
    assert!(
        capture(&document, &phase, &run, &request(&run, json!({"offset":5})))
            .unwrap()
            .ports
            .is_empty()
    );
    let mut changed = document.clone();
    changed.name = "Cosmetic edits".into();
    assert!(capture(&changed, &phase, &run, &request(&run, json!({}))).is_ok());
    changed.revision += 1;
    assert!(capture(&changed, &phase, &run, &request(&run, json!({}))).is_err());
    let mut wrong = request(&run, json!({}));
    wrong.owner.generation += 1;
    assert!(capture(&document, &phase, &run, &wrong).is_err());
    phase.stop();
    assert!(capture(&document, &phase, &run, &request(&run, json!({}))).is_err());
}
#[test]
fn request_schema_rejects_foreign_fields_invalid_owners_and_unbounded_pages() {
    let (_, _, run) = fixture();
    let base = json!({"command":"profile_runtime_inspect","owner":run.owner});
    for patch in [
        json!({"limit":0}),
        json!({"limit":65}),
        json!({"offset":1025}),
        json!({"offset":-1}),
        json!({"nodeId":"bad node"}),
        json!({"nodeId":""}),
        json!({"samples":[]}),
        json!({"command":"profile_apply"}),
    ] {
        let mut value = base.clone();
        value
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(Request::parse(&value).is_err(), "{value}");
    }
    for (key, value) in [
        ("session", json!("forged")),
        ("generation", json!(0)),
        ("profileRevision", json!(0)),
        ("gameId", json!("wrong-game")),
        ("profileId", json!("other")),
        ("generation", json!(9_007_199_254_740_992u64)),
    ] {
        let mut request = base.clone();
        request["owner"][key] = value;
        assert!(Request::parse(&request).is_err());
    }
}
