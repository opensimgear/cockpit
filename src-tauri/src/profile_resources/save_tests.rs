use super::*;

#[test]
fn ui_mcp_and_saved_graph_writes_reject_broken_references_before_persistence() {
    let manager = GatewayManager::mcp_test_manager();
    let mut document = osg_profiles::new_document("Broken reference").unwrap();
    document.resources.push(osg_profiles::Resource {
        id: "missing".into(), name: "Deleted controller".into(),
        kind: osg_profiles::ResourceKind::Device, required: false, ports: vec![],
    });
    let pins = json!([{"resourceId":"missing","sourceId":"removed-native-resource","ports":[]}]);
    for command in ["profile_editor_save", "profile_draft_save"] {
        let reply = manager.request(json!({"command":command,"clientId":"resource-save-test","document":document,"assignments":pins,"expectedRevision":null}));
        assert_eq!(reply["ok"], false, "{reply}");
        assert!(reply["error"].as_str().unwrap().contains("Remove broken components"), "{reply}");
        assert_eq!(manager.profiles.lock().unwrap().saved_documents().len(), 1);
    }
    manager.profiles.lock().unwrap().request(&json!({"command":"profile_editor_save","document":document,"assignments":pins,"expectedRevision":null})).unwrap();
    let reply = manager.request(json!({"command":"profile_save","clientId":"resource-save-test","document":document,"expectedRevision":1}));
    assert_eq!(reply["ok"], false, "{reply}");
    assert!(reply["error"].as_str().unwrap().contains("Remove broken components"), "{reply}");
    assert_eq!(manager.profiles.lock().unwrap().saved_documents().iter().find(|saved|saved.id==document.id).unwrap().revision, 1);
    let assigned=manager.request(json!({"command":"profile_assignments_save","clientId":"resource-save-test","id":document.id,"assignments":pins,"expectedRevision":1}));
    assert_eq!(assigned["ok"],false,"{assigned}");
    assert!(assigned["error"].as_str().unwrap().contains("Remove broken components"),"{assigned}");
    document.resources.clear();
    let cleaned = manager.request(json!({"command":"profile_editor_save","clientId":"resource-save-test","document":document,"assignments":[],"expectedRevision":1}));
    assert_eq!(cleaned["ok"], true, "{cleaned}");
    manager.shutdown();
}
