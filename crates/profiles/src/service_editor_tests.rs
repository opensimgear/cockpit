use super::*;
use crate::resources_ports::ScalarSchema;
use crate::{Direction, Port, PortContract, Resource, ResourceKind};
fn draft() -> ProfileDocument {
    let mut document = new_document("Flight").unwrap();
    document.resources.push(Resource {
        id: "panel".into(),
        name: "Panel".into(),
        kind: ResourceKind::Device,
        required: true,
        ports: vec![Port {
            id: "0".into(),
            name: "Pressed".into(),
            direction: Direction::Output,
            contract: PortContract::Event {
                schema: ScalarSchema::Boolean {},
            },
        }],
    });
    document
}
fn assignments(document: &ProfileDocument) -> Value {
    json!([{"resourceId":"panel","sourceId":"native:opaque","ports":[{"profilePortId":"0","observedPortId":"button:0","expectedContractHash":document.resources[0].ports[0].descriptor().contract_hash().unwrap()}]}])
}
fn save(
    service: &mut Service,
    document: &ProfileDocument,
    expected: Value,
    pins: Value,
) -> Result<Value, String> {
    service.request(&json!({"command":"profile_editor_save","document":document,"expectedRevision":expected,"assignments":pins}))
}
include!("../../../tests/fixtures/profiles/editor-boundary.rs");

#[test]
fn duplicate_can_require_the_observed_source_revision_without_creating_on_conflict() {
    let mut service = Service::new(None);
    let created = service.request(&json!({"command":"profile_create","name":"Source"})).unwrap();
    let document = created["document"].clone();
    service.request(&json!({"command":"profile_rename","id":document["id"],"name":"Changed","expectedRevision":1})).unwrap();
    let stale = service.request(&json!({"command":"profile_duplicate","id":document["id"],"name":"Stale copy","expectedRevision":1})).unwrap_err();
    assert!(stale.contains("revision conflict"));
    assert_eq!(service.saved_documents().len(),2);
    let copied = service.request(&json!({"command":"profile_duplicate","id":document["id"],"name":"Fresh copy","expectedRevision":2})).unwrap();
    assert_eq!(copied["document"]["revision"],1);
    assert_eq!(copied["document"]["name"],"Fresh copy");
    assert_eq!(service.saved_documents().len(),3);
}

#[test]
fn draft_save_is_an_atomic_revision_checked_editor_transaction() {
    let temp = crate::test_directory();
    let root = temp.path().join("drafts");
    let mut service = Service::new(Some(root.clone()));
    let document = draft();
    let pins = assignments(&document);
    let selected = service.status()["session"].clone();
    let saved = service.request(&json!({"command":"profile_draft_save","document":document,"expectedRevision":null,"assignments":pins})).unwrap();
    assert_eq!(saved["document"],json!(document));
    assert_eq!(saved["assignments"],pins);
    assert_eq!(saved["session"],selected);
    let mut changed = document.clone(); changed.name = "Edited draft".into();
    let saved = service.request(&json!({"command":"profile_draft_save","document":changed,"expectedRevision":1,"assignments":pins})).unwrap();
    assert_eq!(saved["document"]["revision"],2);
    assert!(service.request(&json!({"command":"profile_draft_save","document":document,"expectedRevision":1,"assignments":[]})).unwrap_err().contains("revision conflict"));
    let restored = Service::new(Some(root));
    let (restored, restored_pins, _) = restored.compile_source(&document.id).unwrap();
    assert_eq!(restored.name,"Edited draft");
    assert_eq!(restored.revision,2);
    assert_eq!(json!(restored_pins),pins);
}

#[test]
fn new_and_existing_editor_saves_commit_graph_and_all_pins_in_one_revision() {
    let temp = crate::test_directory();
    let root = temp.path().join("profiles-v2");
    let mut service = Service::new(Some(root.clone()));
    let mut document = draft();
    document.eligibility.game_ids = vec!["dcs".into()];
    let pins = assignments(&document);
    let session = service.status()["session"].clone();
    let created = save(&mut service, &document, Value::Null, pins.clone()).unwrap();
    assert_eq!(created["document"], json!(document));
    assert_eq!(created["assignments"], pins);
    let summary = created["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|summary| summary["id"] == document.id)
        .unwrap();
    assert_eq!(summary["semanticHash"], document.semantic_hash().unwrap());
    assert_eq!(summary["revision"], 1);
    assert_eq!(summary["name"], document.name);
    assert_eq!(summary["isDefault"], false);
    assert_eq!(summary["gameIds"], json!(["dcs"]));
    assert_eq!(created["session"], session);
    document.name = "Edited graph".into();
    document.editor.viewport.zoom = 2.0;
    document.resources[0].ports[0].id = "gain:0".into();
    let mut pins = assignments(&document);
    pins[0]["sourceId"] = json!("native:replacement");
    pins[0]["ports"][0]["profilePortId"] = json!("gain:0");
    let saved = save(&mut service, &document, json!(1), pins.clone()).unwrap();
    assert_eq!(saved["document"]["revision"], 2);
    assert_eq!(saved["assignments"], pins);
    let committed: ProfileDocument = serde_json::from_value(saved["document"].clone()).unwrap();
    let summary = saved["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|summary| summary["id"] == document.id)
        .unwrap();
    assert_eq!(summary["semanticHash"], committed.semantic_hash().unwrap());
    assert_eq!(summary["revision"], 2);
    assert_eq!(summary["name"], document.name);
    assert_eq!(saved["session"], session);
    assert_eq!(summary["gameIds"], json!(["dcs"]));
    let mut restarted = Service::new(Some(root));
    let loaded = restarted
        .request(&json!({"command":"profile_assignments_get","id":document.id}))
        .unwrap();
    assert_eq!(loaded["document"], saved["document"]);
    assert_eq!(loaded["assignments"], pins);
    assert!(!restarted
        .request(&json!({"command":"profile_export","id":document.id}))
        .unwrap()
        .to_string()
        .contains("native:replacement"));
    for command in ["profile_duplicate", "profile_import"] {
        let request = if command == "profile_duplicate" {
            json!({"command":command,"id":document.id})
        } else {
            json!({"command":command,"document":saved["document"]})
        };
        let copied = restarted.request(&request).unwrap();
        assert!(restarted
            .compile_source(copied["document"]["id"].as_str().unwrap())
            .unwrap()
            .1
            .is_empty());
    }
}
#[test]
fn null_is_explicit_fresh_identity_creation_and_stale_saves_change_nothing() {
    let mut service = Service::new(None);
    let mut document = draft();
    let pins = assignments(&document);
    for expected in [json!(1), json!(0), json!(true), json!("1")] {
        assert!(save(&mut service, &document, expected, pins.clone()).is_err());
    }
    assert!(service
        .request(&json!({"command":"profile_editor_save","document":document,"assignments":pins}))
        .is_err());
    document.revision = 2;
    assert!(save(&mut service, &document, Value::Null, pins.clone()).is_err());
    document.revision = 1;
    save(&mut service, &document, Value::Null, pins.clone()).unwrap();
    let before = service.library.encode().unwrap();
    assert!(save(&mut service, &document, Value::Null, pins.clone())
        .unwrap_err()
        .contains("already exists"));
    let first = save(&mut service, &document, json!(1), pins.clone()).unwrap();
    let after = service.library.encode().unwrap();
    assert_ne!(before, after);
    document.name = "Stale graph".into();
    assert!(save(&mut service, &document, json!(1), json!([]))
        .unwrap_err()
        .contains("conflict"));
    assert_eq!(service.library.encode().unwrap(), after);
    document.revision = 1;
    assert!(save(&mut service, &document, json!(2), pins)
        .unwrap_err()
        .contains("document revision"));
    assert_eq!(service.library.encode().unwrap(), after);
    assert_eq!(first["document"]["revision"], 2);
    let mut invalid = draft();
    invalid.id = invalid.id.to_uppercase();
    assert!(save(&mut service, &invalid, Value::Null, json!([])).is_err());
}
#[test]
fn graph_and_assignment_validation_are_one_atomic_boundary() {
    let temp = crate::test_directory();
    let mut service = Service::new(Some(temp.path().to_owned()));
    let document = draft();
    let pins = assignments(&document);
    save(&mut service, &document, Value::Null, pins.clone()).unwrap();
    let before = std::fs::read(temp.path().join("library.json")).unwrap();
    for bad in [
        json!(null),
        json!([{"resourceId":"panel","sourceId":"native:opaque","ports":[]}]),
        json!([pins[0], pins[0]]),
        json!([{"resourceId":"missing","sourceId":"native:opaque","ports":[]}]),
        json!([{"resourceId":"panel","sourceId":"native:opaque","ports":[{"profilePortId":"0","observedPortId":"button:0","expectedContractHash":"stale"}]}]),
        json!([{"resourceId":"panel","sourceId":"native:opaque","ports":[pins[0]["ports"][0],pins[0]["ports"][0]]}]),
        json!([{"resourceId":"panel","sourceId":"native:opaque","ports":[{"profilePortId":"unknown","observedPortId":"button:0","expectedContractHash":pins[0]["ports"][0]["expectedContractHash"]}]}]),
    ] {
        let mut edit = document.clone();
        edit.name = "Never commit half".into();
        assert!(save(&mut service, &edit, json!(1), bad).is_err());
        assert_eq!(
            std::fs::read(temp.path().join("library.json")).unwrap(),
            before
        );
        assert_eq!(service.library.encode().unwrap(), before);
    }
    let mut bad_graph = document.clone();
    bad_graph.resources[0].ports[0].id = "bad\n".into();
    assert!(save(&mut service, &bad_graph, json!(1), pins.clone()).is_err());
    assert_eq!(service.library.encode().unwrap(), before);
    assert!(service.request(&json!({"command":"profile_editor_save","document":document,"expectedRevision":1,"assignments":pins,"activate":true})).is_err());
    // Unassigned resources remain authorable, including freshly seeded zero-port logical nodes.
    let mut seeded = draft();
    seeded.resources[0].ports.clear();
    let seeded_pins = json!([{"resourceId":"panel","sourceId":"native:offline","ports":[]}]);
    assert!(save(&mut service, &seeded, Value::Null, seeded_pins).is_ok());
    let unassigned = draft();
    assert!(save(&mut service, &unassigned, Value::Null, json!([])).is_ok());
}
#[test]
fn disk_failure_and_read_only_library_never_publish_half_an_editor_save() {
    let temp = crate::test_directory();
    let mut service = Service::new(Some(temp.path().to_owned()));
    let mut document = draft();
    let pins = assignments(&document);
    save(&mut service, &document, Value::Null, pins.clone()).unwrap();
    let before = std::fs::read(temp.path().join("library.json")).unwrap();
    let status = service.status();
    service.store = Some(Store::new(PathBuf::from("../blocked-editor-store")));
    document.name = "Never published".into();
    assert!(save(&mut service, &document, json!(1), json!([])).is_err());
    assert_eq!(service.library.encode().unwrap(), before);
    assert_eq!(service.status(), status);
    assert_eq!(
        std::fs::read(temp.path().join("library.json")).unwrap(),
        before
    );
    let fresh = draft();
    assert!(save(&mut service, &fresh, Value::Null, assignments(&fresh)).is_err());
    assert!(service.compile_source(&fresh.id).is_err());
    std::fs::write(temp.path().join("library.json"), b"{corrupt").unwrap();
    let mut blocked = Service::new(Some(temp.path().to_owned()));
    assert!(save(&mut blocked, &fresh, Value::Null, json!([]))
        .unwrap_err()
        .contains("read-only"));
    assert_eq!(
        std::fs::read(temp.path().join("library.json")).unwrap(),
        b"{corrupt"
    );
}
#[test]
fn default_label_is_protected_but_its_graph_and_ordinary_custom_labels_remain_editable() {
    let mut service = Service::new(None);
    let mut default = crate::default_document();
    default.name = "Renamed builtin".into();
    assert!(service.request(&json!({"command":"profile_rename","id":DEFAULT_PROFILE_ID,"name":default.name,"expectedRevision":1})).is_err());
    assert!(service
        .request(&json!({"command":"profile_save","document":default,"expectedRevision":1}))
        .is_err());
    assert!(save(&mut service, &default, json!(1), json!([])).is_err());
    default.name = "Default".into();
    default.resources = draft().resources;
    assert!(save(&mut service, &default, json!(1), assignments(&default)).is_ok());
    let mut ordinary = draft();
    ordinary.name = "Default".into();
    assert!(save(&mut service, &ordinary, Value::Null, json!([])).is_ok());
    let mut library = Library::default();
    library.profiles[0].name = "Different".into();
    assert!(library.validate().unwrap_err().contains("protected name"));
}
#[test]
fn creation_capacity_revision_ceiling_and_combined_request_budget_stay_bounded() {
    let mut service = Service::new(None);
    while service.library.profiles.len() < MAX_PROFILES {
        service
            .library
            .profiles
            .push(new_document("Retained").unwrap());
    }
    let document = draft();
    assert!(save(&mut service, &document, Value::Null, json!([]))
        .unwrap_err()
        .contains("limit"));
    let mut service = Service::new(None);
    let mut document = draft();
    document.revision = MAX_REVISION;
    service.library.profiles.push(document.clone());
    assert!(
        save(&mut service, &document, json!(MAX_REVISION), json!([]))
            .unwrap_err()
            .contains("limit")
    );
    assert_eq!(service.library.profiles[1].revision, MAX_REVISION);
    assert!(service
        .request(&json!({"command":"profile_editor_save","document":"x".repeat(2*1024*1024)}))
        .unwrap_err()
        .contains("size limit"));
}

#[test]
fn legal_combined_graph_and_assignment_payload_keeps_document_limit_and_full_pins() {
    let request = editor_boundary_request();
    assert!(request.to_string().len() > crate::MAX_PROFILE_BYTES + 65536);
    assert!(request["document"].to_string().len() < crate::MAX_PROFILE_BYTES);
    let mut service = Service::new(None);
    let result = service.request(&request).unwrap();
    assert_eq!(
        result["assignments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["ports"].as_array().unwrap().len())
            .sum::<usize>(),
        1024
    );
    assert!(
        result["document"] == request["document"],
        "The complete portable graph must roundtrip."
    );
}
