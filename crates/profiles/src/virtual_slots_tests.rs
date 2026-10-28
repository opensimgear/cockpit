use super::*;

fn fixture(service: &mut Service) -> ProfileDocument {
    let mut document = ProfileDocument::parse(include_bytes!(
        "../../../tests/fixtures/profiles/mixed.json"
    ))
    .unwrap();
    document.id = crate::new_document("Slots").unwrap().id;
    service.request(&json!({"command":"profile_editor_save","document":document,"expectedRevision":null,"assignments":[]})).unwrap();
    document
}
fn set(service: &mut Service, document: &ProfileDocument, slot: Value) -> Result<Value, String> {
    service.request(&json!({"command":"profile_virtual_slot_set","id":document.id,"expectedRevision":document.revision,"nodeId":document.virtual_outputs[0].id,"slot":slot}))
}

#[test]
fn virtual_slots_persist_locally_without_revision_or_export_leakage() {
    let temp = crate::test_directory();
    let root = temp.path().join("profiles");
    let mut service = Service::new(Some(root.clone()));
    let document = fixture(&mut service);
    assert_eq!(
        set(&mut service, &document, json!(16)).unwrap()["profileRevision"],
        document.revision
    );
    let export = service
        .request(&json!({"command":"profile_export","id":document.id}))
        .unwrap();
    assert_eq!(export["document"], json!(document));
    let mut service = Service::new(Some(root));
    assert_eq!(
        service.compile_virtual_source(&document.id).unwrap().3[&document.virtual_outputs[0].id],
        16
    );
    let duplicate = service.request(&json!({"command":"profile_duplicate","id":document.id,"expectedRevision":document.revision})).unwrap();
    assert!(service
        .compile_virtual_source(duplicate["document"]["id"].as_str().unwrap())
        .unwrap()
        .3
        .is_empty());
    let imported = service
        .request(&json!({"command":"profile_import","document":document}))
        .unwrap();
    assert!(service
        .compile_virtual_source(imported["document"]["id"].as_str().unwrap())
        .unwrap()
        .3
        .is_empty());
    set(&mut service, &document, Value::Null).unwrap();
    assert!(service
        .compile_virtual_source(&document.id)
        .unwrap()
        .3
        .is_empty());
    set(&mut service, &document, json!(2)).unwrap();
    // The mixed fixture may contain grouping references; use the normal editor save validation.
    let mut empty = crate::new_document("Slots").unwrap();
    empty.id = document.id.clone();
    service.request(&json!({"command":"profile_editor_save","document":empty,"expectedRevision":1,"assignments":[]})).unwrap();
    assert!(service
        .compile_virtual_source(&document.id)
        .unwrap()
        .3
        .is_empty());
    service
        .request(&json!({"command":"profile_reset","confirm":true}))
        .unwrap();
    assert!(service.library.local_virtual_slots.is_empty());
}

#[test]
fn virtual_slots_validate_slot_saved_node_revision_and_envelope() {
    let mut service = Service::new(None);
    let document = fixture(&mut service);
    for slot in [
        json!(0),
        json!(17),
        json!(-1),
        json!(1.5),
        json!("2"),
        json!(true),
    ] {
        assert!(set(&mut service, &document, slot).is_err());
    }
    let request = json!({"command":"profile_virtual_slot_set","id":document.id,"expectedRevision":document.revision,"nodeId":document.virtual_outputs[0].id,"slot":2});
    for (key, value) in [
        ("id", json!("missing")),
        ("nodeId", json!("missing")),
        ("expectedRevision", json!(2)),
        ("extra", json!(true)),
    ] {
        let mut invalid = request.clone();
        invalid[key] = value;
        assert!(service.request(&invalid).is_err());
    }
    let mut missing = request.clone();
    missing.as_object_mut().unwrap().remove("slot");
    assert!(service.request(&missing).is_err());
    let response = service
        .request(&json!({"command":"profile_virtual_slots_get","id":document.id}))
        .unwrap();
    assert_eq!(
        response["virtualOutputIds"][0],
        document.virtual_outputs[0].id
    );
    assert!(response["virtualSlots"][0]["slot"].is_null());
    assert!(service
        .request(&json!({"command":"profile_virtual_slots_get","id":"missing"}))
        .is_err());
    assert!(validate(&document, &BTreeMap::from([("missing".into(), 2)])).is_err());
    assert!(validate(
        &document,
        &BTreeMap::from([(document.virtual_outputs[0].id.clone(), 17)])
    )
    .is_err());
    service.read_only = true;
    assert!(set(&mut service, &document, json!(2)).is_err());
    assert!(
        service.virtual_slots_response(&document.id).unwrap()["readOnly"]
            .as_bool()
            .unwrap()
    );
    let old = json!({"profiles":[crate::default_document()],"localAssignments":{}});
    let library: crate::store::Library = serde_json::from_value(old).unwrap();
    assert!(library.local_virtual_slots.is_empty());
    let mut library = service.library.clone();
    library
        .local_virtual_slots
        .insert("missing".into(), BTreeMap::from([("missing".into(), 2)]));
    assert!(library.validate().is_err());
    library.prune_virtual_slots();
    assert!(library.local_virtual_slots.is_empty());
}
