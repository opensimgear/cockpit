use super::*;
use crate::store::LocalAssignment;
use crate::{Resource, ResourceKind};

fn create(service: &mut Service, name: &str) -> Value {
    service
        .request(&json!({"command":"profile_create","name":name}))
        .unwrap()["document"]
        .clone()
}

fn resource() -> Resource {
    Resource {
        id: "stick".into(),
        name: "Stick".into(),
        kind: ResourceKind::Device,
        required: true,
        ports: vec![],
    }
}

#[test]
fn crud_restarts_with_independent_identity_and_revision_conflicts() {
    let temp = crate::test_directory();
    let root = temp.path().join("profiles-v1");
    let mut service = Service::new(Some(root.clone()));
    let document = create(&mut service, "  Flight  ");
    assert_eq!(document["name"], "Flight");
    assert_ne!(document["id"], DEFAULT_PROFILE_ID);
    let renamed = service.request(&json!({"command":"profile_rename","id":document["id"],"name":"DCS","expectedRevision":1})).unwrap();
    assert_eq!(renamed["document"]["revision"], 2);
    assert!(service.request(&json!({"command":"profile_rename","id":document["id"],"name":"Stale","expectedRevision":1})).unwrap_err().contains("conflict"));
    assert!(service
        .request(&json!({"command":"profile_delete","id":DEFAULT_PROFILE_ID,"expectedRevision":1}))
        .unwrap_err()
        .contains("Default"));
    let mut restarted = Service::new(Some(root));
    let loaded = restarted
        .request(&json!({"command":"profile_get","id":document["id"]}))
        .unwrap();
    assert_eq!(loaded["document"]["name"], "DCS");
    assert_eq!(loaded["document"]["revision"], 2);
    assert_eq!(
        restarted
            .request(&json!({"command":"profile_status"}))
            .unwrap()["session"]["selectedProfileId"],
        DEFAULT_PROFILE_ID
    );
    assert!(restarted
        .request(&json!({"command":"profile_delete","id":document["id"],"expectedRevision":1}))
        .is_err());
    restarted
        .request(&json!({"command":"profile_delete","id":document["id"],"expectedRevision":2}))
        .unwrap();
    assert!(restarted
        .request(&json!({"command":"profile_get","id":document["id"]}))
        .is_err());
}

#[test]
fn duplicate_import_are_portable_deep_copies_and_never_copy_assignments() {
    let mut service = Service::new(None);
    let mut document: ProfileDocument =
        serde_json::from_value(create(&mut service, "Flight")).unwrap();
    document.resources.push(resource());
    let saved = service
        .request(&json!({"command":"profile_save","document":document,"expectedRevision":1}))
        .unwrap()["document"]
        .clone();
    let id = saved["id"].as_str().unwrap().to_owned();
    service.library.local_assignments.insert(
        id.clone(),
        vec![LocalAssignment {
            resource_id: "stick".into(),
            source_id: "host-controller:local-device".into(),
        }],
    );
    let duplicate = service
        .request(&json!({"command":"profile_duplicate","id":id}))
        .unwrap()["document"]
        .clone();
    let imported = service
        .request(&json!({"command":"profile_import","document":saved,"name":"Imported"}))
        .unwrap()["document"]
        .clone();
    for copy in [&duplicate, &imported] {
        assert_ne!(copy["id"], id);
        assert_eq!(copy["revision"], 1);
        assert_eq!(copy["resources"][0]["id"], "stick");
        assert!(!service
            .library
            .local_assignments
            .contains_key(copy["id"].as_str().unwrap()));
    }
    let mut edited = duplicate.clone();
    edited["resources"][0]["name"] = json!("Changed copy");
    service
        .request(&json!({"command":"profile_save","document":edited,"expectedRevision":1}))
        .unwrap();
    let original = service
        .request(&json!({"command":"profile_export","id":id}))
        .unwrap();
    assert_eq!(original["document"]["resources"][0]["name"], "Stick");
    assert!(original["document"].get("localAssignments").is_none());
    let mut edited_original = original["document"].clone();
    edited_original["resources"] = json!([]);
    service
        .request(&json!({"command":"profile_save","document":edited_original,"expectedRevision":2}))
        .unwrap();
    assert!(service.library.local_assignments[&id].is_empty());
}

#[test]
fn corrupted_library_requires_explicit_reset_and_never_overwrites_evidence() {
    let temp = crate::test_directory();
    let path = temp.path().join("library.json");
    std::fs::write(&path, "{corrupt").unwrap();
    let mut service = Service::new(Some(temp.path().to_owned()));
    let status = service
        .request(&json!({"command":"profile_status"}))
        .unwrap();
    assert_eq!(status["readOnly"], true);
    assert!(!status["diagnostics"].as_array().unwrap().is_empty());
    assert!(service
        .request(&json!({"command":"profile_create","name":"Oops"}))
        .unwrap_err()
        .contains("read-only"));
    assert!(service
        .request(&json!({"command":"profile_reset","confirm":false}))
        .is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{corrupt");
    let reset = service
        .request(&json!({"command":"profile_reset","confirm":true}))
        .unwrap();
    assert_eq!(reset["readOnly"], false);
    assert!(reset["diagnostics"].as_array().unwrap().is_empty());
    create(&mut service, "Recovered");
    assert_eq!(
        Service::new(Some(temp.path().to_owned()))
            .library
            .profiles
            .len(),
        2
    );
}

#[test]
fn editor_navigation_and_crud_never_select_or_apply() {
    let mut service = Service::new(None);
    let document = create(&mut service, "Flight");
    service
        .request(&json!({"command":"profile_get","id":document["id"]}))
        .unwrap();
    let status = service
        .request(&json!({"command":"profile_status"}))
        .unwrap();
    assert_eq!(status["session"]["selectedProfileId"], DEFAULT_PROFILE_ID);
    assert_eq!(status["session"]["state"], "stopped");
    assert!(status["session"]["appliedSnapshot"].is_null());
    assert!(service
        .request(&json!({"command":"profile_apply"}))
        .unwrap_err()
        .contains("P4"));
    let ignored = service
        .request(
            &json!({"command":"profile_select","id":document["id"],"selectionSource":"automatic"}),
        )
        .unwrap();
    assert_eq!(ignored["session"]["selectedProfileId"], DEFAULT_PROFILE_ID);
    service
        .request(&json!({"command":"profile_selection_mode","mode":"automatic"}))
        .unwrap();
    service
        .request(&json!({"command":"profile_pause","paused":true}))
        .unwrap();
    let selected = service
        .request(
            &json!({"command":"profile_select","id":document["id"],"selectionSource":"automatic"}),
        )
        .unwrap();
    assert_eq!(selected["session"]["state"], "paused");
    assert_eq!(selected["session"]["selectedProfileId"], document["id"]);
    let deleted = service
        .request(&json!({"command":"profile_delete","id":document["id"],"expectedRevision":1}))
        .unwrap();
    assert_eq!(deleted["session"]["selectedProfileId"], DEFAULT_PROFILE_ID);
    assert_eq!(deleted["session"]["state"], "paused");
    let resumed = service
        .request(&json!({"command":"profile_pause","paused":false}))
        .unwrap();
    assert_eq!(resumed["session"]["state"], "stopped");
}

#[test]
fn invalid_requests_cannot_change_documents() {
    let mut service = Service::new(None);
    let document = create(&mut service, "Flight");
    let mut invalid = document.clone();
    invalid["revision"] = json!(2);
    let cases = [
        json!(null),
        json!({"command":"profile_unknown"}),
        json!({"command":"profile_list","extra":true}),
        json!({"command":"profile_create","name":42}),
        json!({"command":"profile_rename","id":document["id"],"name":" ","expectedRevision":1}),
        json!({"command":"profile_save","document":invalid,"expectedRevision":1}),
        json!({"command":"profile_save","document":{},"expectedRevision":1}),
        json!({"command":"profile_rename","id":document["id"],"name":"Oops","expectedRevision":0}),
        json!({"command":"profile_select","id":document["id"],"selectionSource":"other"}),
        json!({"command":"profile_pause","paused":"yes"}),
        json!({"command":"profile_selection_mode","mode":"random"}),
        json!({"command":"profile_import","document":document,"name":null}),
        json!({"command":"profile_get","id":"../outside"}),
    ];
    for request in cases {
        assert!(service.request(&request).is_err(), "{request}");
    }
    assert_eq!(
        service
            .request(&json!({"command":"profile_get","id":document["id"]}))
            .unwrap()["document"],
        document
    );
    assert!(service.request(&json!({"command":"profile_create","name":"x".repeat(crate::MAX_PROFILE_BYTES + 64*1024)})).is_err());
}

#[test]
fn bounded_library_rejects_thirty_third_profile_and_max_names_duplicate() {
    let mut service = Service::new(None);
    let max_name = "é".repeat(80);
    let document = create(&mut service, &max_name);
    let copied = service
        .request(&json!({"command":"profile_duplicate","id":document["id"]}))
        .unwrap();
    assert_eq!(
        copied["document"]["name"].as_str().unwrap().chars().count(),
        80
    );
    for index in service.library.profiles.len()..MAX_PROFILES {
        create(&mut service, &format!("Profile {index}"));
    }
    assert!(service
        .request(&json!({"command":"profile_create","name":"Overflow"}))
        .is_err());
    assert!(service
        .request(&json!({"command":"profile_duplicate","id":DEFAULT_PROFILE_ID}))
        .is_err());
    assert!(service
        .request(&json!({"command":"profile_import","document":document}))
        .is_err());
    let list = service.request(&json!({"command":"profile_list"})).unwrap();
    assert_eq!(list["profiles"].as_array().unwrap().len(), MAX_PROFILES);
    assert!(list["profiles"][0].get("resources").is_none());
}

#[test]
fn failed_persistence_keeps_memory_revision_and_document_unchanged() {
    let temp = crate::test_directory();
    let root = temp.path().join("profiles-v1");
    let mut service = Service::new(Some(root.clone()));
    let document = create(&mut service, "Flight");
    std::fs::remove_file(root.join("library.json")).unwrap();
    std::fs::create_dir(root.join("library.json")).unwrap();
    assert!(service.request(&json!({"command":"profile_rename","id":document["id"],"name":"Unsaved","expectedRevision":1})).is_err());
    assert_eq!(
        service
            .request(&json!({"command":"profile_get","id":document["id"]}))
            .unwrap()["document"],
        document
    );
}
