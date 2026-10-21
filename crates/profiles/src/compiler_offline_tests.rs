use super::*;

fn absent(resource: &mut CapturedResource) {
    resource.summary.availability = Availability::Offline;
    resource.summary.connections.clear();
    resource.tokens.clear();
    for port in &mut resource.ports { port.current = false; port.route_ids.clear(); }
}

#[test]
fn required_offline_sources_and_sinks_preserve_the_program_and_independent_online_branches() {
    let mut document = mixed();
    document.resources.iter_mut().find(|resource| resource.id == "companion-panel").unwrap().required = true;
    let before_document = serde_json::to_value(&document).unwrap();
    let mut captured = snapshot(&document);
    let online = compile(&document, &captured);
    for resource in &mut captured.resources {
        if ["third-party-a", "can-a", "companion-panel"].iter().any(|id| resource.summary.id.ends_with(id)) { absent(resource); }
    }
    let before_capture = serde_json::to_value(&captured).unwrap();
    let report = compile(&document, &captured);
    assert_eq!(report.readiness, PlanReadiness::Ready, "{:?}", report.diagnostics);
    assert_eq!(report.program_hash, online.program_hash);
    assert_ne!(report.capture_fingerprint, online.capture_fingerprint);
    assert!(has(&report, "resourceUnavailable"));
    let plan = valid(report);
    for source in plan.sources.iter().filter(|source| ["third-party-a", "can-a", "companion-panel"].contains(&source.endpoint.node_id.as_str())) {
        assert_eq!(source.readiness, PlanReadiness::Unavailable);
        assert!(source.token.is_none() && source.preparation.is_none() && source.route.is_none());
    }
    for sink in plan.sinks.iter().filter(|sink| ["can-a", "companion-panel"].contains(&sink.endpoint.node_id.as_str())) {
        assert_eq!(sink.readiness, PlanReadiness::Unavailable);
        assert!(sink.token.is_none() && sink.preparation.is_none() && sink.route.is_none());
    }
    assert!(plan.branches.iter().any(|branch| branch.required && branch.readiness == PlanReadiness::Unavailable));
    assert!(plan.branches.iter().any(|branch| branch.readiness == PlanReadiness::Ready));
    let preparation = valid(compile_for_preparation(&document, &captured, &[]));
    assert!(preparation.sources.iter().filter(|source| source.endpoint.node_id == "third-party-a").all(|source| source.readiness == PlanReadiness::Unavailable && source.preparation.is_none()));
    assert_eq!(serde_json::to_value(PlanReadiness::Unavailable).unwrap(), "unavailable");
    assert_eq!(serde_json::to_value(&document).unwrap(), before_document);
    assert_eq!(serde_json::to_value(&captured).unwrap(), before_capture);
}

#[test]
fn assigned_absent_devices_and_companions_are_unavailable_without_fabricating_delivery_authority() {
    let document = mixed();
    let mut captured = snapshot(&document);
    let online = compile(&document, &captured);
    captured.resources.retain(|resource| !["third-party-a", "can-a", "companion-panel"].iter().any(|id| resource.summary.id.ends_with(id)));
    let report = compile(&document, &captured);
    assert_eq!(report.readiness, PlanReadiness::Ready, "{:?}", report.diagnostics);
    assert_eq!(report.program_hash, online.program_hash);
    assert!(has(&report, "resourceMissing"));
    let plan = valid(report);
    let missing = plan.sources.iter().find(|source| source.endpoint.node_id == "third-party-a").unwrap();
    assert_eq!(missing.readiness, PlanReadiness::Unavailable);
    assert!(missing.inventory_resource_id.is_some() && missing.observed_port_id.is_some());
    assert!(missing.token.is_none() && missing.preparation.is_none() && missing.route.is_none());
    assert!(plan.sinks.iter().any(|sink| sink.endpoint.node_id == "companion-panel" && sink.readiness == PlanReadiness::Unavailable));
}

#[test]
fn offline_presence_never_bypasses_assignment_kind_contract_or_backend_validation() {
    let document = button_graph();
    for variant in 0..8 {
        let mut captured = snapshot(&document);
        absent(&mut captured.resources[0]);
        let code = match variant {
            0 => { captured.assignments[0].ports.clear(); "portUnassigned" },
            1 => { captured.resources[0].summary.resource_type = ResourceType::Companion; "resourceKindChanged" },
            2 => { captured.assignments[0].ports[0].expected_contract_hash = "changed".into(); "portContractChanged" },
            3 => { captured.resources[0].ports[0].contract_hash = "forged".into(); "invalidCapture" },
            4 => { captured.assignments.remove(0); "resourceUnassigned" },
            5 => { captured.resources[0].summary.availability = Availability::Unavailable; "resourceUnavailable" },
            6 => { captured.backend.available = false; "virtualBackendNotReady" },
            _ => {
                if let PortContract::State { freshness_ms, .. } = &mut captured.resources[0].ports[0].descriptor.contract { *freshness_ms = 2000; }
                let changed_hash = captured.resources[0].ports[0].descriptor.contract_hash().unwrap();
                captured.resources[0].ports[0].contract_hash = changed_hash.clone();
                captured.assignments[0].ports[0].expected_contract_hash = changed_hash;
                "portContractChanged"
            },
        };
        let report = compile(&document, &captured);
        assert_eq!(report.readiness, if variant == 3 { PlanReadiness::Invalid } else { PlanReadiness::Blocked }, "{variant}: {:?}", report.diagnostics);
        assert!(has(&report, code), "{variant}: {:?}", report.diagnostics);
    }
}

#[test]
fn offline_ports_with_current_ambiguous_authority_remain_blocked() {
    let document = button_graph();
    let mut captured = snapshot(&document);
    let resource = &mut captured.resources[0];
    resource.summary.availability = Availability::Offline;
    let mut connection = resource.summary.connections[0].clone();
    connection.route.id = "equally-authoritative-route".into();
    let mut token = resource.tokens[0].clone(); token.route_id = connection.route.id.clone();
    resource.ports[0].route_ids.push(connection.route.id.clone());
    resource.summary.connections.push(connection); resource.tokens.push(token);
    let report = compile(&document, &captured);
    assert_eq!(report.readiness, PlanReadiness::Blocked);
    assert!(has(&report, "routeAmbiguous"));
    assert!(valid(report).sources[0].token.is_none());
}

#[test]
fn absent_physical_visibility_is_inactive_while_online_visibility_retains_its_backend_gate() {
    let mut document = button_graph();
    document.visibility.push(crate::VisibilityIntent { resource_id: "device-a".into(), mode: crate::VisibilityMode::HiddenWhileActive });
    for missing in [false, true] {
        let mut captured = snapshot(&document);
        captured.backend.visibility_available = false; captured.backend.visibility_qualified = false;
        assert_eq!(compile(&document, &captured).readiness, PlanReadiness::Blocked);
        if missing { captured.resources.remove(0); } else { absent(&mut captured.resources[0]); }
        let report = compile(&document, &captured);
        assert_eq!(report.readiness, PlanReadiness::Ready, "{:?}", report.diagnostics);
        assert!(has(&report, "visibilityResourceUnavailable"));
        let plan = valid(report);
        assert_eq!(plan.visibility[0].readiness, PlanReadiness::Unavailable);
        assert!(plan.visibility[0].inventory_resource_id.is_some());
    }
}
