use super::*;
use crate::{
    compiler::{compile, compile_for_preparation},
    compiler_models::*,
    store::LocalAssignment,
    *,
};
fn digest(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}
fn descriptor(id: &str, direction: Direction) -> PortDescriptor {
    PortDescriptor {
        id: id.into(),
        name: id.into(),
        direction,
        contract: PortContract::State {
            schema: crate::resources_ports::ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: (direction == Direction::Output).then_some(PortValue::Boolean(false)),
        },
    }
}
fn batch(revision: u64, generation: &str, desired: bool) -> ProviderBatch {
    ProviderBatch {
        scope: "companions".into(),
        session: "provider-session".into(),
        revision,
        complete: true,
        observations: vec![Observation {
            key: "panel-one".into(),
            name: "Companion".into(),
            resource_type: ResourceType::Companion,
            evidence: vec![IdentityEvidence {
                kind: EvidenceKind::CompanionInstance,
                value: "panel-one".into(),
            }],
            attributes: BTreeMap::from([(
                "publicationHash".into(),
                digest(if desired { 'b' } else { 'a' }),
            )]),
            diagnostics: vec![],
            route: Some(Route {
                id: "companion:panel-one".into(),
                transport: Transport::Network,
                provider_generation: generation.into(),
                attributes: BTreeMap::new(),
                baseline_ready: true,
                writable: true,
                priority: 1,
            }),
            ports: Some(
                PortUpdate::complete(if desired {
                    vec![
                        descriptor("pressed", Direction::Output),
                        descriptor("indicator", Direction::Input),
                    ]
                } else {
                    vec![descriptor("old-control", Direction::Output)]
                })
                .unwrap(),
            ),
        }],
    }
}
fn fixture() -> (
    Registry,
    ProfileDocument,
    CompileSnapshot,
    Vec<PortPreparation>,
) {
    let mut registry = Registry::new();
    registry.reconcile(batch(1, "old-session", false)).unwrap();
    let id = registry.list(0, 1).unwrap().resources[0].id.clone();
    let details = registry.inspect(&id, 0, 128).unwrap();
    let descriptors = vec![
        descriptor("pressed", Direction::Output),
        descriptor("indicator", Direction::Input),
    ];
    let mut document = new_document("Prospective publication").unwrap();
    document.resources.push(Resource {
        id: "companion".into(),
        name: "Companion".into(),
        kind: ResourceKind::Companion,
        required: true,
        ports: descriptors
            .iter()
            .map(|p| crate::Port {
                id: p.id.clone(),
                name: p.name.clone(),
                direction: p.direction,
                contract: p.contract.clone(),
            })
            .collect(),
    });
    document.companion_assignments.push(CompanionAssignment {
        resource_id: "companion".into(),
        publication_hash: digest('b'),
    });
    document.bindings.push(Binding {
        id: "indicator-edge".into(),
        from: Endpoint {
            node_id: "companion".into(),
            port_id: "pressed".into(),
        },
        to: Endpoint {
            node_id: "companion".into(),
            port_id: "indicator".into(),
        },
    });
    let pins = descriptors
        .iter()
        .map(|p| {
            registry
                .prepare_companion_port(
                    &id,
                    "panel-one",
                    &digest('b'),
                    &digest('c'),
                    p.clone(),
                    if p.direction == Direction::Output {
                        Access::Read
                    } else {
                        Access::Write
                    },
                )
                .unwrap()
        })
        .collect();
    let snapshot = CompileSnapshot {
        inventory_session: details.session,
        inventory_revision: details.revision,
        assignments: vec![LocalAssignment {
            resource_id: "companion".into(),
            source_id: id,
            ports: descriptors
                .iter()
                .map(|p| PortExpectation {
                    profile_port_id: p.id.clone(),
                    observed_port_id: p.id.clone(),
                    expected_contract_hash: p.contract_hash().unwrap(),
                })
                .collect(),
        }],
        resources: vec![CapturedResource {
            summary: details.resource,
            ports: details.ports,
            tokens: vec![],
            provider_setup_revision: None,
            game_id: None,
        }],
        game_context: None,
        backend: BackendCapabilities::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    };
    (registry, document, snapshot, pins)
}
#[test]
fn prospective_publication_compiles_only_for_acquisition_without_mutating_current_inventory() {
    let (mut registry, document, snapshot, pins) = fixture();
    let before = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(
        compile(&document, &snapshot).readiness,
        PlanReadiness::Blocked
    );
    let prepared = compile_for_preparation(&document, &snapshot, &pins);
    assert_eq!(
        prepared.readiness,
        PlanReadiness::Ready,
        "{:?}",
        prepared.diagnostics
    );
    let plan = prepared.plan.as_ref().unwrap();
    assert!(plan.sources.iter().all(|p| p.token.is_none()));
    assert!(plan.sinks.iter().all(|p| p.token.is_none()));
    assert!(pins[0].requires_baseline());
    assert!(pins[1].requires_provider_acquisition());
    assert_eq!(serde_json::to_value(&snapshot).unwrap(), before);
    assert_eq!(
        registry
            .inspect(&snapshot.assignments[0].source_id, 0, 128)
            .unwrap()
            .resource
            .attributes["publicationHash"],
        digest('a')
    );
    for pin in &pins {
        registry.validate_preparation(pin).unwrap();
        assert!(registry
            .token(
                &pin.resolved().resource_id,
                &pin.resolved().port.id,
                pin.access()
            )
            .is_err());
    }
    assert!(!serde_json::to_string(&prepared)
        .unwrap()
        .contains("companion_publication"));
    registry
        .reconcile(batch(2, "incoming-session", true))
        .unwrap();
    for pin in &pins {
        assert!(registry.validate_preparation(pin).is_err());
    }
    let details = registry
        .inspect(&snapshot.assignments[0].source_id, 0, 128)
        .unwrap();
    let mut delivered = snapshot.clone();
    delivered.resources[0].summary = details.resource;
    delivered.resources[0].ports = details.ports;
    delivered.resources[0].tokens = pins
        .iter()
        .map(|p| {
            registry
                .token(&p.resolved().resource_id, &p.resolved().port.id, p.access())
                .unwrap()
        })
        .collect();
    let actual = compile(&document, &delivered);
    assert_eq!(actual.readiness, PlanReadiness::Ready);
    assert_eq!(prepared.program_hash, actual.program_hash);
}
#[test]
fn prospective_proof_is_exact_and_never_substitutes_for_document_assignment_or_current_identity() {
    let (mut registry, document, snapshot, pins) = fixture();
    let id = &snapshot.assignments[0].source_id;
    let mut wrong = document.clone();
    wrong.companion_assignments.clear();
    assert_eq!(
        compile_for_preparation(&wrong, &snapshot, &pins).readiness,
        PlanReadiness::Blocked
    );
    wrong.companion_assignments = document.companion_assignments.clone();
    wrong.companion_assignments[0].publication_hash = digest('d');
    assert_eq!(
        compile_for_preparation(&wrong, &snapshot, &pins).readiness,
        PlanReadiness::Blocked
    );
    let mut changed = snapshot.clone();
    changed.resources[0].summary.generation = "999".into();
    assert_eq!(
        compile_for_preparation(&document, &changed, &pins).readiness,
        PlanReadiness::Blocked
    );
    changed = snapshot.clone();
    changed.resources[0].summary.capability_revision = digest('e');
    assert_eq!(
        compile_for_preparation(&document, &changed, &pins).readiness,
        PlanReadiness::Blocked
    );
    changed = snapshot.clone();
    changed.inventory_session = "other".into();
    assert_eq!(
        compile_for_preparation(&document, &changed, &pins).readiness,
        PlanReadiness::Blocked
    );
    assert_eq!(
        compile_for_preparation(
            &document,
            &snapshot,
            &[pins[0].clone(), pins[0].clone(), pins[1].clone()]
        )
        .readiness,
        PlanReadiness::Blocked
    );
    assert!(registry
        .prepare_companion_port(
            id,
            "wrong-instance",
            &digest('b'),
            &digest('c'),
            descriptor("pressed", Direction::Output),
            Access::Read
        )
        .is_err());
    for (publication, capability, access) in [
        ("bad".into(), digest('c'), Access::Read),
        (digest('b'), "bad".into(), Access::Read),
        (digest('b'), digest('c'), Access::Invoke),
        (digest('b'), digest('c'), Access::Write),
    ] {
        assert!(registry
            .prepare_companion_port(
                id,
                "panel-one",
                &publication,
                &capability,
                descriptor("pressed", Direction::Output),
                access
            )
            .is_err());
    }
    let other = registry
        .prepare_companion_port(
            id,
            "panel-one",
            &digest('b'),
            &digest('f'),
            descriptor("pressed", Direction::Output),
            Access::Read,
        )
        .unwrap();
    assert_ne!(pins[0].fingerprint().unwrap(), other.fingerprint().unwrap());
    let mut replaced = batch(2, "old-session", false);
    replaced.session = "new-provider".into();
    registry.reconcile(replaced).unwrap();
    assert!(registry.validate_preparation(&pins[0]).is_err());
    registry
        .reconcile(ProviderBatch {
            scope: "companions".into(),
            session: "new-provider".into(),
            revision: 3,
            complete: true,
            observations: vec![],
        })
        .unwrap();
    assert!(registry.validate_preparation(&pins[0]).is_err());
}
