use super::*;
use osg_profiles::{
    compiler_models::{CompileSnapshot, PlanReadiness},
    Direction, Resource,
};

fn owner() -> RunOwner {
    RunOwner {
        session: "11111111-1111-4111-8111-111111111111".into(),
        generation: 1,
        profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(),
        profile_revision: 1,
        game_id: None,
    }
}
fn fixture() -> (ProfileDocument, ExecutionPlan) {
    let mut document = osg_profiles::default_document();
    let port = |id: &str, schema, fallback| Port {
        id: id.into(),
        name: id.into(),
        direction: Direction::Output,
        contract: PortContract::State {
            schema,
            fallback: Some(fallback),
            freshness_ms: 1000,
        },
    };
    document.resources.push(Resource {
        id: "debug".into(),
        name: "Debug".into(),
        kind: ResourceKind::DebugController,
        required: false,
        ports: vec![
            port(
                "axis-1",
                osg_mapping::signal::ScalarSchema::Integer {
                    min: -32768,
                    max: 32767,
                    unit: None,
                },
                PortValue::Number(0.0),
            ),
            port(
                "axis-2",
                osg_mapping::signal::ScalarSchema::Integer {
                    min: 0,
                    max: 32767,
                    unit: None,
                },
                PortValue::Number(0.0),
            ),
            port(
                "button-1",
                osg_mapping::signal::ScalarSchema::Boolean {},
                PortValue::Boolean(false),
            ),
            port(
                "hat-1",
                osg_mapping::signal::ScalarSchema::Integer {
                    min: -1,
                    max: 7,
                    unit: None,
                },
                PortValue::Number(-1.0),
            ),
        ],
    });
    let snapshot = CompileSnapshot {
        inventory_session: "debug-test".into(),
        inventory_revision: "1".into(),
        assignments: vec![],
        resources: vec![],
        game_context: None,
        backend: Default::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    };
    let report = osg_profiles::compiler::compile(&document, &snapshot);
    assert_eq!(
        report.readiness,
        PlanReadiness::Ready,
        "{:?}",
        report.diagnostics
    );
    let mut plan = report.plan.unwrap();
    for port in &document.resources[0].ports[..3] {
        plan.sources
            .push(osg_profiles::compiler_models::SourcePlan {
                endpoint: osg_profiles::Endpoint {
                    node_id: "debug".into(),
                    port_id: port.id.clone(),
                },
                inventory_resource_id: None,
                observed_port_id: None,
                contract: port.contract.clone(),
                token: None,
                preparation: None,
                route: None,
                readiness: PlanReadiness::Ready,
            });
    }
    (document, plan)
}
fn input(sequence: u64, changes: Value) -> Request {
    Request::parse(&json!({"command":"profile_debug_controller_input","owner":owner(),"nodeId":"debug","streamId":"22222222-2222-4222-8222-222222222222","sequence":sequence.to_string(),"changes":changes})).unwrap()
}
#[test]
fn native_debug_baseline_atomic_changes_replay_and_unused_values() {
    let (document, plan) = fixture();
    let (mut state, baselines) = DebugInputs::capture(&document, &plan, 1, 100).unwrap();
    assert_eq!(baselines.len(), 1);
    assert_eq!(baselines[0].changes.len(), 3);
    assert_eq!(baselines[0].reason, Reason::Baseline);
    let update = input(
        1,
        json!([{"portId":"axis-1","value":-12000},{"portId":"axis-2","value":22000},{"portId":"button-1","value":true},{"portId":"hat-1","value":3}]),
    );
    assert_eq!(state.request(&update, 110).unwrap()["replayed"], false);
    assert_eq!(state.pending.len(), 1);
    assert_eq!(state.pending[0].changes.len(), 3);
    assert_eq!(
        state.sources["debug"].values["hat-1"],
        PortValue::Number(3.0)
    );
    assert_eq!(state.request(&update, 120).unwrap()["replayed"], true);
    assert_eq!(state.pending.len(), 1);
    assert!(state
        .request(&input(1, json!([{"portId":"axis-1","value":1}])), 120)
        .is_err());
    for changes in [
        json!([{"portId":"axis-1","value":5},{"portId":"axis-2","value":-1}]),
        json!([{"portId":"axis-1","value":32768}]),
        json!([{"portId":"axis-1","value":1.5}]),
        json!([{"portId":"button-1","value":1}]),
        json!([{"portId":"hat-1","value":8}]),
        json!([{"portId":"missing","value":0}]),
        json!([{"portId":"axis-1","value":1},{"portId":"axis-1","value":2}]),
    ] {
        assert!(state.request(&input(2, changes), 130).is_err());
        assert_eq!(
            state.sources["debug"].values["axis-1"],
            PortValue::Number(-12000.0)
        );
    }
    let frames = state.poll();
    assert_eq!(frames[0].sequence, Counter(2));
    assert!(state.poll().is_empty());
    state.clear();
    assert!(state.request(&update, 140).is_err());
}
#[test]
fn native_debug_streams_and_ordered_edges_have_bounded_backpressure() {
    let (document, plan) = fixture();
    let (mut state, _) = DebugInputs::capture(&document, &plan, 1, 100).unwrap();
    for sequence in 1..=256 {
        state
            .request(
                &input(
                    sequence,
                    json!([{"portId":"button-1","value":sequence%2==1}]),
                ),
                100 + sequence,
            )
            .unwrap();
    }
    assert!(state
        .request(
            &input(257, json!([{"portId":"button-1","value":true}])),
            400
        )
        .is_err());
    let frames = state.poll();
    assert_eq!(frames.len(), 256);
    for (index, frame) in frames.iter().enumerate() {
        assert_eq!(frame.sequence.0, index as u64 + 2);
        assert_eq!(
            frame.changes[0].sample,
            SignalSample::scalar(PortValue::Boolean(index % 2 == 0))
        );
    }
    state
        .request(
            &input(257, json!([{"portId":"button-1","value":true}])),
            401,
        )
        .unwrap();
    assert!(state
        .request(&input(1, json!([{"portId":"button-1","value":true}])), 402)
        .is_err());
    for index in 1..16 {
        let mut request = input(1, json!([{"portId":"hat-1","value":0}]));
        request.stream_id = Some(format!("22222222-2222-4222-8222-{index:012}"));
        state.request(&request, 403).unwrap();
    }
    let mut request = input(1, json!([{"portId":"hat-1","value":0}]));
    request.stream_id = Some("33333333-3333-4333-8333-333333333333".into());
    assert!(state.request(&request, 404).is_err());
}
#[test]
fn native_debug_requests_require_exact_shapes_and_canonical_stream_sequences() {
    let good =
        json!({"command":"profile_debug_controller_status","owner":owner(),"nodeId":"debug"});
    assert!(Request::parse(&good).is_ok());
    for (field, value) in [
        ("extra", json!(true)),
        ("streamId", json!("bad")),
        ("sequence", json!("1")),
        ("nodeId", json!("9bad")),
        ("command", json!("other")),
    ] {
        let mut bad = good.clone();
        bad[field] = value;
        assert!(Request::parse(&bad).is_err(), "{bad}");
    }
    for sequence in [
        json!(0),
        json!("0"),
        json!("01"),
        json!("18446744073709551616"),
    ] {
        let mut request = json!({"command":"profile_debug_controller_input","owner":owner(),"nodeId":"debug","streamId":"22222222-2222-4222-8222-222222222222","sequence":sequence,"changes":[{"portId":"axis-1","value":0}]});
        assert!(Request::parse(&request).is_err());
        request["changes"] = json!([]);
        assert!(Request::parse(&request).is_err());
    }
}
