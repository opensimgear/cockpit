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
