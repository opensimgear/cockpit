use super::*;
use std::sync::Mutex;

#[test]
fn captures_commits_selection_and_reset_but_not_failed_writes() {
    let mut service = Service::new(None);
    let observed = Arc::new(Mutex::new(Vec::new()));
    let output = observed.clone();
    service.observe_read_model(Arc::new(move |snapshot| output.lock().unwrap().push(snapshot)));
    let created = service.request(&json!({"command":"profile_create","name":"Second client"})).unwrap();
    let id = created["document"]["id"].as_str().unwrap();
    service.request(&json!({"command":"profile_select","id":id,"selectionSource":"manual"})).unwrap();
    let count = observed.lock().unwrap().len();
    assert!(service.request(&json!({"command":"profile_rename","id":id,"expectedRevision":0,"name":"Bad"})).is_err());
    service.request(&json!({"command":"profile_status"})).unwrap();
    service.request(&json!({"command":"profile_list"})).unwrap();
    assert_eq!(observed.lock().unwrap().len(), count);
    service.request(&json!({"command":"profile_rename","id":id,"expectedRevision":1,"name":"Committed"})).unwrap();
    assert_eq!(observed.lock().unwrap().last().unwrap().versions[id], "2");
    service.request(&json!({"command":"profile_reset","confirm":true})).unwrap();
    let events = observed.lock().unwrap();
    assert_eq!(events.last().unwrap().generation, 2);
    assert_eq!(events.last().unwrap().versions[DEFAULT_PROFILE_ID], "1");
    assert_eq!(events.last().unwrap().profiles.len(), 1);
}

#[test]
fn local_assignment_commit_advances_version_and_failed_storage_is_silent() {
    let mut service = Service::new(None);
    let output = Arc::new(Mutex::new(Vec::new()));
    let received = output.clone();
    service.observe_read_model(Arc::new(move |snapshot| received.lock().unwrap().push(snapshot)));
    service.request(&json!({"command":"profile_assignments_save","id":DEFAULT_PROFILE_ID,"expectedRevision":1,"assignments":[]})).unwrap();
    assert_eq!(output.lock().unwrap().last().unwrap().versions[DEFAULT_PROFILE_ID], "2");
    // A storage load error has a coherent initial diagnostic projection, and no write event.
    let root = crate::test_directory();
    let file = root.path().join("file");
    std::fs::write(&file, b"not a directory").unwrap();
    let mut broken = Service::new(Some(file));
    let initial = broken.read_model_snapshot();
    assert!(initial.read_only);
    assert!(broken.request(&json!({"command":"profile_create","name":"Fails"})).is_err());
    assert_eq!(initial, broken.read_model_snapshot());
}

#[test]
fn local_virtual_slot_commit_changes_version_without_portable_revision_or_hash() {
    let mut service = Service::new(None);
    let mut document = crate::ProfileDocument::parse(include_bytes!("../../../../../tests/fixtures/profiles/mixed.json")).unwrap();
    document.id = crate::new_document("Local slots").unwrap().id;
    service.request(&json!({"command":"profile_editor_save","document":document,"expectedRevision":null,"assignments":[]})).unwrap();
    let before = service.read_model_snapshot();
    service.request(&json!({"command":"profile_virtual_slot_set","id":document.id,"expectedRevision":1,
        "nodeId":document.virtual_outputs[0].id,"slot":3})).unwrap();
    let after = service.read_model_snapshot();
    assert_eq!(before.profiles, after.profiles);
    assert_eq!(before.versions[&document.id], "1");
    assert_eq!(after.versions[&document.id], "2");
}
