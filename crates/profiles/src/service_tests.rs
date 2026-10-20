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
fn saved_companion_references_follow_all_documents_without_editor_or_runtime_effects() {
    let temp = crate::test_directory();
    let root = temp.path().join("profiles-v2");
    let mut service = Service::new(Some(root.clone()));
    let mut saved = vec![];
    for (name, letter) in [("First", 'b'), ("Second", 'a'), ("Third", 'b')] {
        let mut document = create(&mut service, name);
        document["resources"] = json!([{"id":"panel","name":"Panel","kind":"companion","required":false,"ports":[]}]);
        document["companionAssignments"] = json!([{"resourceId":"panel","publicationHash":format!("sha256:{}",letter.to_string().repeat(64))}]);
        saved.push(service.request(&json!({"command":"profile_save","document":document,"expectedRevision":1})).unwrap()["document"].clone());
    }
    let bytes = std::fs::read(root.join("library.json")).unwrap();
    let status = service.status();
    let references = service.request(&json!({"command":"profile_companion_references"})).unwrap()["publicationReferences"].clone();
    assert_eq!(references["schemaVersion"], 1);
    assert_eq!(references["hashes"], json!([format!("sha256:{}","a".repeat(64)),format!("sha256:{}","b".repeat(64))]));
    assert_eq!(references["libraryFingerprint"].as_str().unwrap().len(), 71);
    assert_eq!(std::fs::read(root.join("library.json")).unwrap(), bytes);
    assert_eq!(service.status(), status);
    service.request(&json!({"command":"profile_select","id":saved[0]["id"],"selectionSource":"manual"})).unwrap();
    assert_eq!(service.companion_references().unwrap(), references);
    assert!(service.status()["session"]["appliedSnapshot"].is_null());
    for document in [&saved[0], &saved[2]] {
        service.request(&json!({"command":"profile_delete","id":document["id"],"expectedRevision":2})).unwrap();
    }
    let remaining = service.companion_references().unwrap();
    assert_ne!(remaining["libraryFingerprint"], references["libraryFingerprint"]);
    assert_eq!(remaining["hashes"], json!([format!("sha256:{}","a".repeat(64))]));
    assert_eq!(Service::new(Some(root)).companion_references().unwrap(), remaining);
}

fn save_panel_profile(service: &mut Service, name: &str, panel_id: Option<&str>, publication: &str) -> String {
    let mut document = create(service, name);
    document["resources"] = json!([{"id":"screen","name":"Screen","kind":"companion","required":false,"ports":[]}]);
    document["companionAssignments"] = json!([{"resourceId":"screen","publicationHash":publication}]);
    if let Some(panel_id) = panel_id { document["companionAssignments"][0]["panelId"] = json!(panel_id); }
    service.request(&json!({"command":"profile_save","document":document,"expectedRevision":1})).unwrap()["document"]["id"].as_str().unwrap().into()
}

#[test]
fn saved_panel_refresh_updates_all_followers_and_legacy_hashes_atomically_once() {
    let temp = crate::test_directory(); let root = temp.path().join("profiles");
    let mut service = Service::new(Some(root.clone()));
    let old = format!("sha256:{}", "a".repeat(64)); let latest = format!("sha256:{}", "b".repeat(64));
    let stable = save_panel_profile(&mut service, "Stable", Some("panel-one"), &old);
    let legacy = save_panel_profile(&mut service, "Legacy", None, &old);
    let other = save_panel_profile(&mut service, "Explicit other identity", Some("panel-other"), &old);
    let current = save_panel_profile(&mut service, "Current hash", None, &latest);
    // Two companion blocks in one profile still advance its saved version once.
    let document = service.library.profiles.iter_mut().find(|document|document.id == stable).unwrap();
    let mut resource = document.resources[0].clone(); resource.id = "second-screen".into(); document.resources.push(resource);
    let mut assignment = document.companion_assignments[0].clone(); assignment.resource_id = "second-screen".into(); document.companion_assignments.push(assignment);
    let status = service.status();
    let changed = service.refresh_companion_panel("panel-one", &latest, &std::collections::BTreeSet::from([old.clone()])).unwrap();
    assert_eq!(changed, vec![stable.clone(), legacy.clone(), current.clone()]);
    for id in [&stable, &legacy, &current] {
        let document = service.document(id).unwrap(); assert_eq!(document.revision, 3);
        assert!(document.companion_assignments.iter().all(|assignment|assignment.panel_id.as_deref() == Some("panel-one") && assignment.publication_hash == latest));
    }
    assert_eq!(service.document(&other).unwrap().revision, 2);
    assert_eq!(service.document(&other).unwrap().companion_assignments[0].panel_id.as_deref(), Some("panel-other"));
    assert_eq!(service.status(), status); // No editor selection or runtime authority changes.
    let saved = std::fs::read(root.join("library.json")).unwrap();
    assert!(service.refresh_companion_panel("panel-one", &latest, &std::collections::BTreeSet::from([old])).unwrap().is_empty());
    assert_eq!(std::fs::read(root.join("library.json")).unwrap(), saved);
    let restarted = Service::new(Some(root));
    for id in [&stable, &legacy, &current, &other] { assert_eq!(restarted.document(id).unwrap(), service.document(id).unwrap()); }
    assert_eq!(service.companion_references().unwrap()["hashes"], json!([format!("sha256:{}", "a".repeat(64)), latest]));
}

#[test]
fn saved_panel_refresh_preserves_every_profile_on_storage_or_revision_failure() {
    let temp = crate::test_directory(); let root = temp.path().join("profiles");
    let mut service = Service::new(Some(root.clone()));
    let old = format!("sha256:{}", "a".repeat(64)); let latest = format!("sha256:{}", "b".repeat(64));
    let first = save_panel_profile(&mut service, "First", Some("panel-one"), &old);
    let second = save_panel_profile(&mut service, "Second", Some("panel-one"), &old);
    let aliases = std::collections::BTreeSet::from([old]);
    let original = serde_json::to_value(&service.library).unwrap();
    std::fs::remove_file(root.join("library.json")).unwrap(); std::fs::create_dir(root.join("library.json")).unwrap();
    assert!(service.refresh_companion_panel("panel-one", &latest, &aliases).is_err());
    assert_eq!(serde_json::to_value(&service.library).unwrap(), original);
    service.library.profiles.iter_mut().find(|document|document.id == second).unwrap().revision = MAX_REVISION;
    let original = serde_json::to_value(&service.library).unwrap();
    assert!(service.refresh_companion_panel("panel-one", &latest, &aliases).unwrap_err().contains("limit"));
    assert_eq!(serde_json::to_value(&service.library).unwrap(), original);
    assert_eq!(service.document(&first).unwrap().revision, 2);
    assert!(service.refresh_companion_panel("bad panel", &latest, &aliases).is_err());
    assert!(service.refresh_companion_panel("panel-one", "sha256:bad", &aliases).is_err());
    assert!(service.refresh_companion_panel("panel-one", &latest, &std::collections::BTreeSet::from(["bad".into()])).is_err());
    service.read_only = true;
    assert!(service.refresh_companion_panel("panel-one", &latest, &aliases).unwrap_err().contains("read-only"));
    assert!(service.request(&json!({"command":"profile_refresh_companion_panel","panelId":"panel-one","publicationHash":latest})).is_err());
}

#[test]
fn companion_reference_reads_refuse_corrupt_storage_and_browser_authority() {
    let temp = crate::test_directory();
    let root = temp.path().join("profiles-v2");
    std::fs::create_dir_all(&root).unwrap();
    let original = b"{corrupt saved references";
    std::fs::write(root.join("library.json"), original).unwrap();
    let mut service = Service::new(Some(root.clone()));
    assert!(service.request(&json!({"command":"profile_companion_references"})).unwrap_err().contains("preserve companion publications"));
    assert!(service.request(&json!({"command":"profile_companion_references","hashes":[]})).unwrap_err().contains("Unknown field"));
    assert_eq!(std::fs::read(root.join("library.json")).unwrap(), original);
    service.request(&json!({"command":"profile_reset","confirm":true})).unwrap();
    assert_eq!(service.companion_references().unwrap()["hashes"], json!([]));
}

#[test]
fn crud_restarts_with_independent_identity_and_revision_conflicts() {
    let temp = crate::test_directory();
    let root = temp.path().join("profiles-v2");
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
            ports: vec![],
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
    let selected = service
        .request(
            &json!({"command":"profile_select","id":document["id"],"selectionSource":"automatic"}),
        )
        .unwrap();
    assert_eq!(selected["session"]["state"], "stopped");
    assert_eq!(selected["session"]["selectedProfileId"], document["id"]);
    let deleted = service
        .request(&json!({"command":"profile_delete","id":document["id"],"expectedRevision":1}))
        .unwrap();
    assert_eq!(deleted["session"]["selectedProfileId"], DEFAULT_PROFILE_ID);
    assert_eq!(deleted["session"]["state"], "stopped");
    for paused in [true, false] {
        assert!(service.request(&json!({"command":"profile_pause","paused":paused})).is_err());
    }
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
    let root = temp.path().join("profiles-v2");
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

#[test]
fn exact_local_assignments_are_atomic_revisioned_and_never_portable() {
    use crate::resources::PortExpectation;
    let temp = crate::test_directory();
    let root = temp.path().join("profiles-v2");
    let mut service = Service::new(Some(root.clone()));
    let mut document = crate::default_document();
    document.resources.push(Resource {
        id: "panel".into(),
        name: "Panel".into(),
        kind: ResourceKind::Device,
        required: true,
        ports: vec![crate::Port {
            id: "pressed".into(),
            name: "Pressed".into(),
            direction: crate::Direction::Output,
            contract: crate::resources_ports::PortContract::Event {
                schema: crate::resources_ports::ScalarSchema::Boolean {},
            },
        }],
    });
    let saved = service
        .request(&json!({"command":"profile_save","document":document,"expectedRevision":1}))
        .unwrap();
    let original_hash = saved["document"].clone();
    let original_hash: ProfileDocument = serde_json::from_value(original_hash).unwrap();
    let expected_hash = original_hash.semantic_hash().unwrap();
    let assignments = vec![LocalAssignment {
        resource_id: "panel".into(),
        source_id: "native:opaque".into(),
        ports: vec![PortExpectation {
            profile_port_id: "pressed".into(),
            observed_port_id: "button:0".into(),
            expected_contract_hash: format!("sha256:{}", "a".repeat(64)),
        }],
    }];
    let result=service.request(&json!({"command":"profile_assignments_save","id":DEFAULT_PROFILE_ID,"expectedRevision":2,"assignments":assignments})).unwrap();
    assert_eq!(result["document"]["revision"], 3);
    let (mut captured, mut bindings, read_only) =
        service.compile_source(DEFAULT_PROFILE_ID).unwrap();
    assert!(!read_only);
    assert_eq!(captured.semantic_hash().unwrap(), expected_hash);
    assert!(!String::from_utf8(captured.export().unwrap())
        .unwrap()
        .contains("native:opaque"));
    captured.name = "Mutated clone".into();
    bindings.clear();
    assert_eq!(
        service.compile_source(DEFAULT_PROFILE_ID).unwrap().1.len(),
        1
    );
    let mut restarted = Service::new(Some(root.clone()));
    let read = restarted
        .request(&json!({"command":"profile_assignments_get","id":DEFAULT_PROFILE_ID}))
        .unwrap();
    assert_eq!(read["assignments"], json!(assignments));
    let before = std::fs::read(root.join("library.json")).unwrap();
    for replacement in [
        json!([{"resourceId":"panel","sourceId":"native:opaque","ports":[]}]),
        json!([{"resourceId":"panel","sourceId":"native:opaque","ports":[{"profilePortId":"missing","observedPortId":"button:0","expectedContractHash":format!("sha256:{}","a".repeat(64))}]}]),
        json!([{"resourceId":"panel","sourceId":"native:opaque","ports":[{"profilePortId":"pressed","observedPortId":"button:0","expectedContractHash":"stale"}]}]),
        json!([{"resourceId":"panel","sourceId":"native:opaque"}]),
        json!([{"resourceId":"panel","sourceId":"native:opaque","ports":[],"token":"invented"}]),
    ] {
        assert!(restarted.request(&json!({"command":"profile_assignments_save","id":DEFAULT_PROFILE_ID,"expectedRevision":3,"assignments":replacement})).is_err());
        assert_eq!(std::fs::read(root.join("library.json")).unwrap(), before);
        assert_eq!(
            restarted
                .compile_source(DEFAULT_PROFILE_ID)
                .unwrap()
                .0
                .revision,
            3
        );
    }
    assert!(restarted.request(&json!({"command":"profile_assignments_save","id":DEFAULT_PROFILE_ID,"expectedRevision":2,"assignments":assignments})).is_err());
    assert!(restarted
        .request(&json!({"command":"profile_assignments_get","id":DEFAULT_PROFILE_ID,"extra":true}))
        .is_err());
    assert!(restarted.compile_source("missing").is_err());
    // Graph changes invalidate incomplete local pins rather than making a draft impossible to save.
    let (mut edit, _, _) = restarted.compile_source(DEFAULT_PROFILE_ID).unwrap();
    edit.resources[0].ports.clear();
    restarted
        .request(&json!({"command":"profile_save","document":edit,"expectedRevision":3}))
        .unwrap();
    assert!(restarted
        .compile_source(DEFAULT_PROFILE_ID)
        .unwrap()
        .1
        .is_empty());
}

#[test]
fn schema_one_library_is_never_migrated_or_read_from_new_namespace() {
    let temp = crate::test_directory();
    let old = temp.path().join("profiles-v1");
    std::fs::create_dir(&old).unwrap();
    let bytes = br#"{"schemaVersion":1,"profiles":[],"localAssignments":{}}"#;
    std::fs::write(old.join("library.json"), bytes).unwrap();
    let fresh = Service::new(Some(temp.path().join("profiles-v2")));
    assert_eq!(
        fresh
            .compile_source(DEFAULT_PROFILE_ID)
            .unwrap()
            .0
            .schema_version,
        2
    );
    assert_eq!(std::fs::read(old.join("library.json")).unwrap(), bytes);
    let blocked = Service::new(Some(old.clone()));
    assert!(blocked.compile_source(DEFAULT_PROFILE_ID).unwrap().2);
    assert_eq!(std::fs::read(old.join("library.json")).unwrap(), bytes);
}
