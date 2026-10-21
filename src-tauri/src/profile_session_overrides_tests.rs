use super::*;
use osg_profiles::compiler_models::*;
#[test]
fn runtime_enable_or_disable_never_uses_persistent_device_lifecycle() {
    let snapshot = CompileSnapshot {
        inventory_session: uuid::Uuid::new_v4().to_string(),
        inventory_revision: "1".into(),
        assignments: vec![],
        resources: vec![],
        game_context: None,
        backend: Default::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    };
    let mut plan = osg_profiles::compiler::compile(&osg_profiles::default_document(), &snapshot)
        .plan
        .unwrap();
    assert!(Staged::prepare(&plan, &snapshot)
        .unwrap()
        .targets
        .is_empty());
    for enabled in [true, false] {
        plan.runtime_overrides = vec![osg_profiles::RuntimeOverride {
            resource_id: "physical".into(),
            enabled,
        }];
        let error = Staged::prepare(&plan, &snapshot).err().unwrap();
        assert!(error.contains("Bind an exact native CAN controller"));
    }
}

#[test]
fn absent_publication_targets_are_skipped_but_online_unsupported_targets_still_block() {
    use osg_profiles::{resources::*, store::LocalAssignment};
    let mut snapshot = CompileSnapshot { inventory_session: uuid::Uuid::new_v4().to_string(), inventory_revision: "1".into(),
        assignments: vec![LocalAssignment { resource_id: "physical".into(), source_id: "retained".into(), ports: vec![] }],
        resources: vec![], game_context: None, backend: Default::default(), inventory_diagnostics: vec![], read_only: false };
    let mut empty = snapshot.clone(); empty.assignments.clear();
    let mut plan = osg_profiles::compiler::compile(&osg_profiles::default_document(), &empty).plan.unwrap();
    plan.runtime_overrides.push(osg_profiles::RuntimeOverride { resource_id: "physical".into(), enabled: true });
    assert!(Staged::prepare(&plan, &snapshot).unwrap().targets.is_empty());
    snapshot.resources.push(CapturedResource { summary: ResourceSummary { id: "retained".into(), name: "Offline CAN panel".into(), resource_type: ResourceType::OsgModule,
        availability: Availability::Offline, generation: "1".into(), capability_revision: format!("sha256:{}", "0".repeat(64)), port_count: 0, cached_port_count: 0,
        connections: vec![], attributes: Default::default(), diagnostics: vec![] }, ports: vec![], tokens: vec![], provider_setup_revision: None, game_id: None });
    assert!(Staged::prepare(&plan, &snapshot).unwrap().targets.is_empty());
    snapshot.resources[0].summary.availability = Availability::Online;
    assert!(Staged::prepare(&plan, &snapshot).err().unwrap().contains("no advertised"));
}
