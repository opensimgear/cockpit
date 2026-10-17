use super::*;
use osg_companion_service::{Hub, Publication, PublicationUpload};
use osg_mapping_host::companions::CompanionProvider;
use sha2::{Digest, Sha256};
use std::{sync::mpsc, time::Duration};

fn publish(hub: &Hub, title: &str) -> Publication {
    let mut upload: PublicationUpload = serde_json::from_str(include_str!("../../crates/companion-service/tests/fixtures/c1-flight-publication.json")).unwrap();
    upload.canonical_document = upload.canonical_document.replace("Flight instruments", title);
    upload.publication_hash = format!("sha256:{:x}", Sha256::digest(upload.canonical_document.as_bytes()));
    upload.capability_fingerprint = None;
    let publication = Publication::validate(upload.clone()).unwrap();
    let mut request = serde_json::to_value(upload).unwrap();
    request["command"] = json!("companion_publish");
    hub.command(&request).unwrap();
    publication
}
fn saved_panel(service: &mut osg_profiles::service::Service, publication: &str) {
    let mut document = osg_profiles::default_document();
    document.resources.push(osg_profiles::Resource { id: "panel".into(), name: "Panel".into(), kind: osg_profiles::ResourceKind::Companion, required: false, ports: vec![] });
    document.companion_assignments.push(osg_profiles::CompanionAssignment { resource_id: "panel".into(), publication_hash: publication.into() });
    service.request(&json!({"command":"profile_save","document":document,"expectedRevision":1})).unwrap();
}

#[test]
fn native_prune_preserves_saved_unassigned_publication_despite_empty_browser_pins() {
    let provider = Arc::new(CompanionProvider::new(Arc::new(|| 1)));
    let hub = Hub::new("saved-reference-prune".into(), provider);
    let saved = publish(&hub, "Saved profile panel");
    let unused = publish(&hub, "Unused panel revision");
    let mut service = osg_profiles::service::Service::new(None);
    saved_panel(&mut service, &saved.publication_hash);
    let profiles = Mutex::new(service);
    let before = profiles.lock().unwrap().request(&json!({"command":"profile_status"})).unwrap();
    let request = json!({"command":"companion_prune","companion":{"hashes":[],"panelIds":[saved.ui_id]}});
    let result = with_saved_references(&profiles, &request, |protected| {
        let mut command = protected["companion"].clone();
        command["command"] = json!("companion_prune");
        hub.command(&command)
    }).unwrap();
    assert_eq!(result["removed"], 1);
    let retained = hub.publication_summaries();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0]["publicationHash"], saved.publication_hash);
    assert_ne!(retained[0]["publicationHash"], unused.publication_hash);
    assert_eq!(profiles.lock().unwrap().request(&json!({"command":"profile_status"})).unwrap(), before);
    assert!(request["companion"]["hashes"].as_array().unwrap().is_empty());
}

#[test]
fn prune_and_saved_profile_mutation_use_one_reference_capture() {
    let profiles = Arc::new(Mutex::new(osg_profiles::service::Service::new(None)));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let pending = profiles.clone();
    let prune = std::thread::spawn(move || with_saved_references(&pending, &json!({"command":"companion_prune","companion":{"hashes":[],"panelIds":[]}}), |_| {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        Ok(json!({"removed":0}))
    }));
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (saved_tx, saved_rx) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let result = profiles.lock().unwrap().request(&json!({"command":"profile_create","name":"Concurrent profile"}));
        saved_tx.send(result).unwrap();
    });
    assert!(saved_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).unwrap();
    assert!(prune.join().unwrap().is_ok());
    assert!(saved_rx.recv_timeout(Duration::from_secs(3)).unwrap().is_ok());
    writer.join().unwrap();
}

#[test]
fn invalid_browser_pins_or_unreadable_saved_references_never_reach_prune() {
    let healthy = Mutex::new(osg_profiles::service::Service::new(None));
    for hashes in [Value::Null, json!(["invalid"]), json!([1]), json!(vec![format!("sha256:{}", "a".repeat(64));101])] {
        assert!(with_saved_references(&healthy, &json!({"companion":{"hashes":hashes}}), |_| panic!("invalid retention reached native pruning")).is_err());
    }
    let directory = tempfile::tempdir().unwrap();
    let original = b"{unreadable saved profiles";
    std::fs::write(directory.path().join("library.json"), original).unwrap();
    let corrupt = Mutex::new(osg_profiles::service::Service::new(Some(directory.path().to_owned())));
    assert!(with_saved_references(&corrupt, &json!({"companion":{"hashes":[]}}), |_| panic!("unknown saved references reached native pruning")).unwrap_err().contains("preserve companion publications"));
    assert_eq!(std::fs::read(directory.path().join("library.json")).unwrap(), original);
}

#[test]
fn too_many_genuine_saved_references_preserve_publications_instead_of_truncating() {
    let mut service = osg_profiles::service::Service::new(None);
    for batch in 0..2 {
        let created = service.request(&json!({"command":"profile_create","name":format!("Saved panels {batch}")})).unwrap();
        let mut document: osg_profiles::ProfileDocument = serde_json::from_value(created["document"].clone()).unwrap();
        for index in 0..64 {
            let id = format!("panel-{index}");
            document.resources.push(osg_profiles::Resource { id: id.clone(), name: id.clone(), kind: osg_profiles::ResourceKind::Companion, required: false, ports: vec![] });
            document.companion_assignments.push(osg_profiles::CompanionAssignment { resource_id: id, publication_hash: format!("sha256:{:064x}",batch*64+index) });
        }
        service.request(&json!({"command":"profile_save","document":document,"expectedRevision":1})).unwrap();
    }
    assert_eq!(service.companion_references().unwrap()["hashes"].as_array().unwrap().len(), 128);
    assert!(with_saved_references(&Mutex::new(service), &json!({"companion":{"hashes":[]}}), |_| panic!("oversized saved reference set reached bounded prune")).unwrap_err().contains("Preserve them"));
}
