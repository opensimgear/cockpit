use super::*;
use osg_profiles::{compiler_models::*, resources_ports::*, *};

fn endpoint(node: &str) -> Endpoint {
    Endpoint {
        node_id: node.into(),
        port_id: "value".into(),
    }
}
fn contract() -> PortContract {
    PortContract::State {
        schema: ScalarSchema::Boolean {},
        freshness_ms: 1000,
        fallback: None,
    }
}
fn resource(id: &str, required: bool) -> Resource {
    Resource {
        id: id.into(),
        name: id.into(),
        kind: ResourceKind::Companion,
        required,
        ports: vec![],
    }
}
fn fixture() -> (ProfileDocument, ExecutionPlan) {
    let mut document = new_document("Health policy").unwrap();
    document.resources = vec![resource("optional", false), resource("required", true)];
    let plan = ExecutionPlan {
        requirements: ExecutionRequirements::default(),
        program: osg_mapping::program::Program {
            sources: vec![],
            nodes: vec![],
            sinks: vec![],
            controllers: vec![],
        },
        topological_order: vec![],
        trace: vec![],
        sources: vec![],
        sinks: vec![],
        branches: vec![],
        controllers: vec![],
        visibility: vec![],
        runtime_overrides: vec![],
        companion_assignments: vec![],
        budgets: PlanBudgets::default(),
    };
    (document, plan)
}
fn source(id: &str, readiness: PlanReadiness) -> SourcePlan {
    SourcePlan {
        endpoint: endpoint(id),
        inventory_resource_id: None,
        observed_port_id: None,
        contract: contract(),
        token: None,
        preparation: None,
        route: None,
        readiness,
    }
}
fn sink(id: &str, readiness: PlanReadiness) -> SinkPlan {
    SinkPlan {
        endpoint: endpoint(id),
        adapter: SinkAdapter::CompanionFeedback,
        inventory_resource_id: None,
        observed_port_id: None,
        contract: contract(),
        token: None,
        preparation: None,
        route: None,
        readiness,
    }
}
fn branch(sink: &str, sources: &[&str], required: bool, readiness: PlanReadiness) -> BranchPlan {
    BranchPlan {
        sink: endpoint(sink),
        source_ports: sources.iter().map(|id| endpoint(id)).collect(),
        processing_nodes: vec![],
        edge_ids: vec![],
        required,
        readiness,
    }
}

#[test]
fn optional_absent_input_does_not_degrade_its_required_downstream_branch() {
    let (document, mut plan) = fixture();
    plan.sources
        .push(source("optional", PlanReadiness::Unavailable));
    plan.sinks.push(sink("required", PlanReadiness::Ready));
    plan.branches.push(branch(
        "required",
        &["optional"],
        true,
        PlanReadiness::Unavailable,
    ));
    plan.visibility.push(VisibilitySummary {
        resource_id: "optional".into(),
        inventory_resource_id: None,
        mode: VisibilityMode::Visible,
        readiness: PlanReadiness::Unavailable,
    });
    let absent = BTreeMap::from([("optional".into(), "native-optional".into())]);
    assert!(admission_degradation(&plan, &document, &absent).is_empty());
    assert_eq!(
        plan.branches[0].readiness,
        PlanReadiness::Unavailable,
        "The inspector must retain the unavailable branch."
    );
}

#[test]
fn optional_absent_feedback_target_does_not_degrade_ready_required_inputs() {
    let (document, mut plan) = fixture();
    plan.sources.push(source("required", PlanReadiness::Ready));
    plan.sinks
        .push(sink("optional", PlanReadiness::Unavailable));
    plan.branches.push(branch(
        "optional",
        &["required"],
        true,
        PlanReadiness::Unavailable,
    ));
    assert!(admission_degradation(&plan, &document, &BTreeMap::new()).is_empty());
}

#[test]
fn required_absence_is_retained_even_when_optional_absence_also_affects_branch() {
    let (document, mut plan) = fixture();
    plan.sources = vec![
        source("optional", PlanReadiness::Unavailable),
        source("required", PlanReadiness::Unavailable),
    ];
    plan.sinks
        .push(sink("optional", PlanReadiness::Unavailable));
    plan.branches.push(branch(
        "optional",
        &["required", "optional"],
        true,
        PlanReadiness::Unavailable,
    ));
    plan.visibility.push(VisibilitySummary {
        resource_id: "required".into(),
        inventory_resource_id: None,
        mode: VisibilityMode::Visible,
        readiness: PlanReadiness::Unavailable,
    });
    let messages = admission_degradation(
        &plan,
        &document,
        &BTreeMap::from([("required".into(), "native-required".into())]),
    );
    assert_eq!(messages.len(), 4);
    assert!(messages
        .iter()
        .any(|message| message.starts_with("required:value is offline")));
    assert!(messages
        .iter()
        .any(|message| message.starts_with("optional:value is unavailable")));
}

#[test]
fn unexplained_unavailable_and_blocked_branches_remain_unhealthy() {
    let (document, mut plan) = fixture();
    plan.branches = vec![
        branch("unknown", &[], false, PlanReadiness::Unavailable),
        branch("optional", &[], false, PlanReadiness::Blocked),
    ];
    assert_eq!(
        admission_degradation(&plan, &document, &BTreeMap::new()).len(),
        2
    );
    plan.sources
        .push(source("unknown", PlanReadiness::Unavailable));
    assert!(admission_degradation(&plan, &document, &BTreeMap::new())
        .iter()
        .any(|message| message.starts_with("unknown:value is offline")));
}

#[test]
fn unavailable_samples_remain_unhealthy_between_packets_until_known_again() {
    use osg_mapping::{engine::{typed::{Change, Frame, Phase}, Counter, Reason}, program::SignalSample};
    let (document, mut plan) = fixture();
    plan.program.sources.push(osg_mapping::program::SourcePort {
        endpoint: osg_mapping::program::Endpoint { node_id: "required".into(), port_id: "value".into() }, contract: contract(),
    });
    let mut phase = Phase::new(plan.program.clone(), Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
    let frame = |sequence, reason, sample| Frame {
        source: "required".into(), generation: Counter(1), sequence: Counter(sequence),
        captured_us: Counter(sequence * 100), reason,
        changes: vec![Change { port_id: "value".into(), sample }],
    };
    phase.accept_batch([frame(1, Reason::Baseline, SignalSample::scalar(PortValue::Boolean(false)))]).unwrap();
    assert!(input_degradation(&phase, Some(&plan), &document).is_empty());
    phase.accept_batch([frame(2, Reason::Change, SignalSample::Unknown)]).unwrap();
    let unavailable = input_degradation(&phase, Some(&plan), &document);
    assert_eq!(unavailable, vec!["required has unavailable input samples; dependent work was revoked."]);
    for _ in 0..5 {
        assert!(phase.accept_batch([]).unwrap().is_empty(), "No new packet was received");
        assert_eq!(input_degradation(&phase, Some(&plan), &document), unavailable,
            "An idle heartbeat must not fabricate a recovery");
    }
    phase.accept_batch([frame(3, Reason::Change, SignalSample::Unknown)]).unwrap();
    assert_eq!(input_degradation(&phase, Some(&plan), &document), unavailable);
    phase.accept_batch([frame(4, Reason::Change, SignalSample::scalar(PortValue::Boolean(true)))]).unwrap();
    assert!(input_degradation(&phase, Some(&plan), &document).is_empty());
    phase.advance(Counter(2_000_000)).unwrap();
    assert_eq!(input_degradation(&phase, Some(&plan), &document), unavailable,
        "Expired samples remain visible even without an incoming frame");
}

#[test]
fn absent_optional_sources_and_quiet_events_do_not_add_health_warnings() {
    use osg_mapping::{engine::{typed::Phase, Counter}, program::SourcePort};
    let (document, mut plan) = fixture();
    plan.sources.push(source("optional", PlanReadiness::Unavailable));
    plan.program.sources = vec![
        SourcePort { endpoint: osg_mapping::program::Endpoint { node_id: "optional".into(), port_id: "value".into() }, contract: contract() },
        SourcePort { endpoint: osg_mapping::program::Endpoint { node_id: "required".into(), port_id: "value".into() }, contract: PortContract::Event { schema: ScalarSchema::Boolean {} } },
    ];
    let phase = Phase::new(plan.program.clone(), Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
    assert!(input_degradation(&phase, Some(&plan), &document).is_empty());
    plan.sources[0].readiness = PlanReadiness::Ready;
    assert_eq!(input_degradation(&phase, Some(&plan), &document),
        vec!["optional has unavailable input samples; dependent work was revoked."],
        "A present optional source must still report actual unavailable data");
}
