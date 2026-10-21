use super::*;
use serde_json::json;

fn fixture() -> (ProfileDocument, CompileSnapshot) {
    let mut document = mixed();
    let event = document
        .resources
        .iter()
        .find(|r| r.id == "third-party-a")
        .unwrap()
        .ports
        .iter()
        .find(|p| p.id == "pressed")
        .unwrap()
        .clone();
    document.resources.push(Resource {
        id: "can-command".into(),
        name: "CAN command module".into(),
        kind: ResourceKind::Device,
        required: true,
        ports: vec![event],
    });
    document
        .bindings
        .iter_mut()
        .find(|b| b.id == "dcs-command-trigger")
        .unwrap()
        .from
        .node_id = "can-command".into();
    let captured = snapshot(&document);
    (document, captured)
}
fn module(snapshot: &mut CompileSnapshot) -> &mut CapturedResource {
    snapshot
        .resources
        .iter_mut()
        .find(|r| r.summary.id.ends_with("can-command"))
        .unwrap()
}
fn set_status(snapshot: &mut CompileSnapshot, status: &str) {
    let r = module(snapshot);
    let route = &r.summary.connections[0].route;
    r.summary.attributes.insert(
        hid_policy::attribute(&route.id),
        hid_policy::evidence(route, status),
    );
}

#[test]
fn an_absent_command_input_acquires_no_hid_publication_lease_until_it_returns() {
    let (document, mut snapshot) = fixture();
    set_status(&mut snapshot, "published");
    assert_eq!(compile(&document, &snapshot).readiness, PlanReadiness::Blocked);
    let resource = module(&mut snapshot);
    resource.summary.availability = Availability::Offline;
    resource.summary.connections.clear(); resource.tokens.clear();
    for port in &mut resource.ports { port.current = false; port.route_ids.clear(); }
    let report = compile(&document, &snapshot);
    assert_eq!(report.readiness, PlanReadiness::Ready, "{:?}", report.diagnostics);
    assert!(!has(&report, "dcsHidPublicationRequired"));
    assert!(valid(report).branches.iter().any(|branch| branch.sink.node_id == "dcs" && branch.readiness == PlanReadiness::Unavailable));
}
fn capable(snapshot: &mut CompileSnapshot) {
    let r = module(snapshot);
    let id = "4f534744123456789012345678901234";
    let gateway = "stm32:0123456789abcdef01234567";
    let route = format!("can:{gateway}:{id}");
    r.summary.connections[0].route.id = route.clone();
    for p in &mut r.ports {
        p.route_ids = vec![route.clone()];
    }
    for t in &mut r.tokens {
        t.route_id = route.clone();
    }
    r.summary.attributes.insert("moduleId".into(), id.into());
    r.summary
        .attributes
        .insert("gatewayId".into(), gateway.into());
    r.summary.attributes.insert(
        "controllerPublicationOverride".into(),
        json!({"schema":1,"revision":7,"module":{"id":id}}).to_string(),
    );
}

#[test]
fn direct_dcs_requires_every_native_publisher_disabled_and_unknown_is_blocked() {
    let (document, mut snapshot) = fixture();
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Ready
    );
    for status in ["published", "unknown", "lease-disabled"] {
        set_status(&mut snapshot, status);
        let report = compile(&document, &snapshot);
        assert_eq!(
            report.readiness,
            PlanReadiness::Blocked,
            "{status}: {:?}",
            report.diagnostics
        );
        assert!(has(&report, "dcsHidPublicationRequired"));
    }
    set_status(&mut snapshot, "disabled");
    let r = module(&mut snapshot);
    let mut extra = r.summary.connections[0].clone();
    extra.route.id = "usb:other".into();
    extra.route.transport = Transport::Usb;
    extra.scope = "module-usb".into();
    r.summary.connections.push(extra.clone());
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Blocked
    );
    let r = module(&mut snapshot);
    r.summary.attributes.insert(
        hid_policy::attribute(&extra.route.id),
        hid_policy::evidence(&extra.route, "disabled"),
    );
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Ready
    );
    module(&mut snapshot).summary.connections[1].route.transport = Transport::Hid;
    module(&mut snapshot).summary.connections[1].scope = "host-controllers".into();
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Blocked
    );
}

#[test]
fn publication_metadata_key_is_bounded_but_proof_keeps_exact_route_and_generation() {
    let (document, mut snapshot) = fixture();
    let key = hid_policy::attribute(&"usb:".repeat(120));
    assert_eq!(key.len(), 71);
    let r = module(&mut snapshot);
    let route = &r.summary.connections[0].route;
    let mut proof: serde_json::Value =
        serde_json::from_str(&hid_policy::evidence(route, "disabled")).unwrap();
    proof["routeId"] = json!("different-route");
    r.summary
        .attributes
        .insert(hid_policy::attribute(&route.id), proof.to_string());
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Blocked
    );
    set_status(&mut snapshot, "disabled");
    let r = module(&mut snapshot);
    let route = &r.summary.connections[0].route;
    let mut proof: serde_json::Value =
        serde_json::from_str(&hid_policy::evidence(route, "disabled")).unwrap();
    proof["providerGeneration"] = json!("changed");
    r.summary
        .attributes
        .insert(hid_policy::attribute(&route.id), proof.to_string());
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Blocked
    );
}

#[test]
fn explicit_disable_is_prospective_only_with_native_capability_and_delivery_needs_actual_disable() {
    let (mut document, mut snapshot) = fixture();
    document.overrides.push(crate::RuntimeOverride {
        resource_id: "can-command".into(),
        enabled: false,
    });
    capable(&mut snapshot);
    set_status(&mut snapshot, "disabled");
    let base = compile(&document, &snapshot).plan.unwrap();
    set_status(&mut snapshot, "published");
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Blocked
    );
    let mut sinks = base.sinks.clone();
    let mut diagnostics = vec![];
    hid_policy::apply(
        &document,
        &snapshot,
        &base.sources,
        &mut sinks,
        true,
        &mut diagnostics,
    );
    assert!(!diagnostics
        .iter()
        .any(|d| d.code == "dcsHidPublicationRequired"));
    assert!(sinks
        .iter()
        .filter(|s| s.adapter == SinkAdapter::SimulatorCommand)
        .all(|s| s.readiness == PlanReadiness::Ready));
    module(&mut snapshot)
        .summary
        .attributes
        .remove("controllerPublicationOverride");
    hid_policy::apply(
        &document,
        &snapshot,
        &base.sources,
        &mut sinks,
        true,
        &mut diagnostics,
    );
    assert!(diagnostics
        .iter()
        .any(|d| d.code == "dcsHidPublicationRequired"));
    capable(&mut snapshot);
    set_status(&mut snapshot, "lease-disabled");
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Ready
    );
    document
        .overrides
        .iter_mut()
        .find(|intent| intent.resource_id == "can-command")
        .unwrap()
        .enabled = true;
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Blocked
    );
}

#[test]
fn no_game_wrong_game_and_read_only_telemetry_do_not_require_publication_override() {
    let (mut document, mut snapshot) = fixture();
    set_status(&mut snapshot, "published");
    snapshot.game_context = None;
    let report = compile(&document, &snapshot);
    assert!(!has(&report, "dcsHidPublicationRequired"));
    assert!(report.plan.unwrap().sinks.iter().any(
        |s| s.adapter == SinkAdapter::SimulatorCommand && s.readiness == PlanReadiness::Dormant
    ));
    let (_, mut snapshot) = fixture();
    set_status(&mut snapshot, "published");
    snapshot.game_context.as_mut().unwrap().game_id = "x-plane".into();
    assert!(!has(
        &compile(&document, &snapshot),
        "dcsHidPublicationRequired"
    ));
    let (_, mut snapshot) = fixture();
    set_status(&mut snapshot, "published");
    document.bindings.retain(|b| b.id != "dcs-command-trigger");
    assert!(!has(
        &compile(&document, &snapshot),
        "dcsHidPublicationRequired"
    ));
}
