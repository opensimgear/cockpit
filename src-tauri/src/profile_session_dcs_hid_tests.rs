use super::*;
use osg_profiles::{
    compiler_models::*, resources::*, resources_ports::*, store::LocalAssignment, Endpoint,
};
use std::collections::BTreeMap;

const GATEWAY: &str = "stm32:0123456789abcdef01234567";
const MODULE: &str = "4f534744123456789012345678901234";
fn fixture() -> (ExecutionPlan, CompileSnapshot, Vec<(String, Value)>) {
    let mut snapshot = CompileSnapshot {
        inventory_session: "fixture".into(),
        inventory_revision: "1".into(),
        assignments: vec![],
        resources: vec![],
        game_context: None,
        backend: BackendCapabilities::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    };
    let mut plan = osg_profiles::compiler::compile(&osg_profiles::default_document(), &snapshot)
        .plan
        .unwrap();
    let endpoint = |node: &str| Endpoint {
        node_id: node.into(),
        port_id: "button".into(),
    };
    let contract = PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 250,
        fallback: None,
    };
    let route = Route {
        id: format!("can:{GATEWAY}:{MODULE}"),
        transport: Transport::Can,
        provider_generation: "[1,2,3,4,5]".into(),
        attributes: BTreeMap::from([
            ("moduleId".into(), MODULE.into()),
            ("gatewayId".into(), GATEWAY.into()),
        ]),
        baseline_ready: true,
        writable: true,
        priority: 50,
    };
    let summary = ResourceSummary {
        id: "inventory-module".into(),
        name: "Module".into(),
        resource_type: ResourceType::OsgModule,
        availability: Availability::Online,
        generation: "1".into(),
        capability_revision: "1".into(),
        port_count: 1,
        cached_port_count: 1,
        connections: vec![Connection {
            scope: format!("gateway:{GATEWAY}"),
            key: MODULE.into(),
            route: route.clone(),
        }],
        attributes: BTreeMap::from([("moduleId".into(), MODULE.into())]),
        diagnostics: vec![],
    };
    let mut game = summary.clone();
    game.id = "inventory-game".into();
    game.resource_type = ResourceType::GameIntegration;
    game.connections.clear();
    snapshot.resources = vec![
        CapturedResource {
            summary,
            ports: vec![],
            tokens: vec![],
            provider_setup_revision: None,
            game_id: None,
        },
        CapturedResource {
            summary: game,
            ports: vec![],
            tokens: vec![],
            provider_setup_revision: None,
            game_id: Some("dcs".into()),
        },
    ];
    snapshot.assignments = vec![
        LocalAssignment {
            resource_id: "panel".into(),
            source_id: "inventory-module".into(),
            ports: vec![],
        },
        LocalAssignment {
            resource_id: "game".into(),
            source_id: "inventory-game".into(),
            ports: vec![],
        },
    ];
    plan.sources.push(SourcePlan {
        endpoint: endpoint("panel"),
        inventory_resource_id: Some("inventory-module".into()),
        observed_port_id: Some("button:0".into()),
        contract: contract.clone(),
        token: None,
        preparation: None,
        route: Some(route),
        readiness: PlanReadiness::Ready,
    });
    plan.sinks.push(SinkPlan {
        endpoint: endpoint("game"),
        adapter: SinkAdapter::SimulatorCommand,
        inventory_resource_id: Some("inventory-game".into()),
        observed_port_id: Some("command".into()),
        contract,
        token: None,
        preparation: None,
        route: None,
        readiness: PlanReadiness::Ready,
    });
    plan.branches.push(BranchPlan {
        sink: endpoint("game"),
        source_ports: vec![endpoint("panel")],
        processing_nodes: vec![],
        edge_ids: vec!["command".into()],
        required: true,
        readiness: PlanReadiness::Ready,
    });
    let state = json!({"gatewaySerial":"0123456789abcdef01234567","connected":true,"canProtocol":3,"epoch":1,"layoutPending":false,
        "nativeSnapshotAtMs":hid_publication::now_ms(),"publication":{"phase":"idle","outcome":"no_change"},
        "modules":[{"id":MODULE,"online":true,"directUsb":false,"published":false,"boot":2,"lease":3,"configurationGeneration":4}],
        "moduleEvents":{MODULE:{"stream":5}}});
    (plan, snapshot, vec![(GATEWAY.into(), state)])
}

#[test]
fn publication_drift_blocks_commands_even_when_selected_input_generation_is_unchanged() {
    let (plan, snapshot, mut gateways) = fixture();
    let original = plan.sources[0].route.clone();
    assert!(
        validate_fixture(&plan, &snapshot, &gateways, &[], |_, _| panic!(
            "No lease needed for stable configured disable"
        ))
        .is_ok()
    );
    gateways[0].1["modules"][0]["published"] = json!(true);
    assert!(validate_fixture(&plan, &snapshot, &gateways, &[], |_, _| Ok(())).is_err());
    assert_eq!(plan.sources[0].route, original);
    gateways[0].1["modules"][0]["published"] = json!(false);
    gateways[0].1["modules"][0]["directUsb"] = json!(true);
    assert!(validate_fixture(&plan, &snapshot, &gateways, &[], |_, _| Ok(())).is_err());
}

#[test]
fn foreign_disabled_lease_new_publisher_and_route_changes_are_rejected() {
    let (plan, snapshot, mut gateways) = fixture();
    gateways[0].1["publication"] =
        json!({"phase":"owned","outcome":"ok","expectedReconnect":false});
    let mut proofs = 0;
    assert!(validate_fixture(&plan, &snapshot, &gateways, &[], |_, _| {
        proofs += 1;
        Err("Foreign owner".into())
    })
    .is_err());
    assert_eq!(proofs, 1);
    assert!(
        validate_fixture(&plan, &snapshot, &gateways, &[], |gateway, module| {
            assert_eq!((gateway, module), (GATEWAY, MODULE));
            Ok(())
        })
        .is_ok()
    );
    let mut changed = gateways.clone();
    changed[0].1["modules"][0]["lease"] = json!(9);
    assert!(validate_fixture(&plan, &snapshot, &changed, &[], |_, _| Ok(())).is_err());
    let mut other = gateways[0].1.clone();
    other["gatewaySerial"] = json!("1123456789abcdef01234567");
    gateways.push(("stm32:1123456789abcdef01234567".into(), other));
    assert!(validate_fixture(&plan, &snapshot, &gateways, &[], |_, _| Ok(())).is_err());
}

#[test]
fn dormant_and_other_game_or_companion_branches_do_not_require_hid_changes() {
    let (mut plan, mut snapshot, _) = fixture();
    plan.branches[0].readiness = PlanReadiness::Dormant;
    plan.sinks[0].readiness = PlanReadiness::Dormant;
    assert!(validate_fixture(&plan, &snapshot, &[], &[], |_, _| panic!("Dormant")).is_ok());
    plan.branches[0].readiness = PlanReadiness::Ready;
    plan.sinks[0].readiness = PlanReadiness::Ready;
    snapshot.resources[1].game_id = Some("x-plane".into());
    assert!(validate_fixture(&plan, &snapshot, &[], &[], |_, _| panic!("Other game")).is_ok());
    snapshot.resources[1].game_id = Some("dcs".into());
    snapshot.resources[0].summary.resource_type = ResourceType::Companion;
    assert!(validate_fixture(&plan, &snapshot, &[], &[], |_, _| panic!("Companion")).is_ok());
}

fn validate_fixture(
    plan: &ExecutionPlan,
    snapshot: &CompileSnapshot,
    gateways: &[(String, Value)],
    usb: &[crate::module_usb_fleet::SlotSnapshot],
    owned: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    validate_snapshots(
        &Guard::capture(plan, snapshot),
        plan,
        snapshot,
        gateways,
        usb,
        owned,
    )
}
#[test]
fn immutable_obligation_cannot_disappear_with_current_inventory_or_game_metadata() {
    let (plan, mut snapshot, gateways) = fixture();
    let guard = Guard::capture(&plan, &snapshot);
    snapshot.resources[1].game_id = None;
    snapshot.resources[0].summary.resource_type = ResourceType::Companion;
    assert!(validate_snapshots(&guard, &plan, &snapshot, &gateways, &[], |_, _| Ok(())).is_err());
    snapshot.resources.remove(0);
    assert!(validate_snapshots(&guard, &plan, &snapshot, &gateways, &[], |_, _| Ok(())).is_err());
}

#[test]
fn publication_withdrawal_is_per_sink_permanent_and_diagnostic_survives_blocked_branch() {
    let (mut plan, snapshot, gateways) = fixture();
    let mut other = plan.sinks[0].clone();
    other.endpoint.port_id = "companion-command".into();
    plan.sinks.push(other.clone());
    let mut unrelated = plan.branches[0].clone();
    unrelated.sink = other.endpoint.clone();
    unrelated.source_ports.clear();
    plan.branches.push(unrelated);
    let mut guard = Guard::capture(&plan, &snapshot);
    assert!(guard.needs_observation(&plan));
    let mut current = Ok((snapshot, gateways, vec![]));
    assert!(guard.observe(&plan, &current, |_, _| Ok(())).is_empty());
    current.as_mut().unwrap().1[0].1["modules"][0]["published"] = json!(true);
    assert_eq!(guard.observe(&plan, &current, |_, _| Ok(())).len(), 1);
    let endpoint = osg_mapping::program::Endpoint {
        node_id: "game".into(),
        port_id: "button".into(),
    };
    assert!(guard.blocked(&endpoint));
    assert!(!guard.needs_observation(&plan));
    assert!(!guard.blocked(&osg_mapping::program::Endpoint {
        node_id: "game".into(),
        port_id: "companion-command".into()
    }));
    current.as_mut().unwrap().1[0].1["modules"][0]["published"] = json!(false);
    assert!(guard.observe(&plan, &current, |_, _| Ok(())).is_empty());
    assert!(guard.blocked(&endpoint));
    plan.branches[0].readiness = PlanReadiness::Blocked;
    plan.sinks[0].readiness = PlanReadiness::Blocked;
    assert!(guard.observe(&plan, &current, |_, _| Ok(())).is_empty());
    assert_eq!(
        guard.revoked.len(),
        1,
        "Blocked delivery cannot erase the permanent diagnostic"
    );
    assert!(!guard.needs_observation(&plan));
    assert_eq!(guard.diagnostics().len(), 1);
    let (plan, snapshot, _) = fixture();
    let mut guard = Guard::capture(&plan, &snapshot);
    assert_eq!(
        guard
            .observe(&plan, &Err("Inventory unavailable".into()), |_, _| Ok(()))
            .len(),
        1
    );
}
#[test]
fn current_capture_cannot_adopt_a_changed_input_lease_or_usb_publisher() {
    let (plan, mut snapshot, mut gateways) = fixture();
    let guard = Guard::capture(&plan, &snapshot);
    snapshot.resources[0].summary.connections[0]
        .route
        .provider_generation = "[1,2,9,4,5]".into();
    gateways[0].1["modules"][0]["lease"] = json!(9);
    assert!(validate_snapshots(&guard, &plan, &snapshot, &gateways, &[], |_, _| Ok(())).is_err());
    let (mut plan, mut snapshot, _) = fixture();
    let mut slot = crate::profile_resource_sources::hid_publication::tests::usb();
    slot.snapshot.identity.as_mut().unwrap().module_id = MODULE.into();
    slot.snapshot.route.as_mut().unwrap().module_id = MODULE.into();
    let route = &mut snapshot.resources[0].summary.connections[0].route;
    route.id = format!("usb:{}", slot.key);
    route.transport = Transport::Usb;
    route.provider_generation = "7:8:9".into();
    snapshot.resources[0].summary.connections[0].scope = "module-usb".into();
    plan.sources[0].route = Some(snapshot.resources[0].summary.connections[0].route.clone());
    let guard = Guard::capture(&plan, &snapshot);
    assert!(validate_snapshots(
        &guard,
        &plan,
        &snapshot,
        &[],
        &[slot.clone()],
        |_, _| Ok(())
    )
    .is_ok());
    slot.snapshot.config.as_mut().unwrap()["layout"]["reportBytes"] = json!(1);
    assert!(validate_snapshots(&guard, &plan, &snapshot, &[], &[slot], |_, _| Ok(())).is_err());
}

#[test]
fn thirty_two_revoked_command_branches_keep_bounded_diagnostics_across_repeated_dispatch() {
    let (mut plan, snapshot, gateways) = fixture();
    let sink = plan.sinks[0].clone();
    let branch = plan.branches[0].clone();
    plan.sinks.clear();
    plan.branches.clear();
    for index in 0..32 {
        let mut sink = sink.clone();
        sink.endpoint.port_id = format!("command-{index}");
        let mut branch = branch.clone();
        branch.sink = sink.endpoint.clone();
        branch.edge_ids = vec![format!("edge-{index}")];
        plan.sinks.push(sink);
        plan.branches.push(branch);
    }
    assert!(plan.branches.len() < 512);
    let mut guard = Guard::capture(&plan, &snapshot);
    let lost = guard.observe(
        &plan,
        &Err("Native publication observation lost".into()),
        |_, _| Ok(()),
    );
    assert_eq!(lost.len(), 32);
    assert_eq!(guard.revoked.len(), 32);
    assert_eq!(
        guard.diagnostics(),
        vec!["Native publication observation lost"]
    );
    let mut dispatch = std::collections::BTreeSet::new();
    for _ in 0..32 {
        append_diagnostics(&mut dispatch, guard.diagnostics());
    }
    let health = unique_diagnostics(
        guard
            .diagnostics()
            .into_iter()
            .chain(dispatch)
            .chain(vec!["Ordinary source is available".into()]),
    );
    assert_eq!(health.len(), 2);
    assert!(
        health.len() <= 1024,
        "The existing native status bound remains valid without truncation"
    );
    assert!(guard
        .observe(&plan, &Ok((snapshot, gateways, vec![])), |_, _| Ok(()))
        .is_empty());
    assert!(!guard.needs_observation(&plan));
    for endpoint in lost {
        assert!(guard.blocked(&osg_mapping::program::Endpoint {
            node_id: endpoint.node_id,
            port_id: endpoint.port_id
        }));
    }
    assert_eq!(
        guard.diagnostics(),
        vec!["Native publication observation lost"]
    );
}
