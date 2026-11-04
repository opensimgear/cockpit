use super::*;
use osg_profiles::{Resource, ResourceKind, store::LocalAssignment};

#[test]
fn authoring_save_accepts_retained_offline_components_but_rejects_removed_or_changed_identity() {
    for kind in [ResourceKind::Device, ResourceKind::Companion] {
        let runtime = Runtime::new(None, Arc::new(|_| panic!("Save must not discover hardware")), false);
        let mut item = observed(1);
        let scope = if kind == ResourceKind::Companion { item.resource_type = ResourceType::Companion; "companions" } else { "host" };
        runtime.state.lock().unwrap().registry.reconcile(batch(scope, "session", 1, vec![item.clone()])).unwrap();
        let id = runtime.state.lock().unwrap().registry.list(0, 1).unwrap().resources[0].id.clone();
        let mut document = osg_profiles::new_document("Retained controller").unwrap();
        document.resources.push(Resource { id: "control".into(), name: "Flight controls".into(), kind, required: true, ports: vec![] });
        let pins = vec![LocalAssignment { resource_id: "control".into(), source_id: id.clone(), ports: vec![] }];
        let before = runtime.state.lock().unwrap().registry.saved_fingerprint().unwrap();
        runtime.validate_profile_save(&document, &pins).unwrap();
        runtime.state.lock().unwrap().read_only=true;
        assert!(runtime.validate_profile_save(&document,&pins).unwrap_err().contains("could not be verified"));
        runtime.validate_profile_save(&document,&[]).unwrap();
        runtime.state.lock().unwrap().read_only=false;
        assert_eq!(runtime.state.lock().unwrap().registry.saved_fingerprint().unwrap(), before);
        item.route = None; item.ports = None;
        runtime.state.lock().unwrap().registry.reconcile(batch(scope, "session", 2, vec![item])).unwrap();
        assert_eq!(runtime.state.lock().unwrap().registry.inspect(&id, 0, 1).unwrap().resource.availability, osg_profiles::resources::Availability::Offline);
        runtime.validate_profile_save(&document, &pins).unwrap();
        document.resources[0].kind = ResourceKind::Game;
        assert!(runtime.validate_profile_save(&document, &pins).unwrap_err().contains("Remove broken"));
        document.resources[0].kind = kind;
        runtime.state.lock().unwrap().registry = Registry::new();
        assert!(runtime.validate_profile_save(&document, &pins).unwrap_err().contains("Flight controls"));
        runtime.validate_profile_save(&document, &[]).unwrap();
        document.resources.clear(); runtime.validate_profile_save(&document, &pins).unwrap();
        document.resources.push(Resource { id: "control".into(), name: "Debug".into(), kind: ResourceKind::DebugController, required: true, ports: vec![] });
        runtime.validate_profile_save(&document, &pins).unwrap();
        runtime.shutdown();
    }
}
