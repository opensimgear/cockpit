use super::*;
use osg_companion_service::{SourceChange, SourceDefinition, SourceSink, SourceToken};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

fn old_upload() -> PublicationUpload {
    serde_json::from_str(include_str!(
        "../../crates/companion-service/tests/fixtures/c1-flight-publication.json"
    ))
    .unwrap()
}
fn compiled_upload() -> PublicationUpload {
    serde_json::from_str(include_str!(
        "../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json"
    ))
    .unwrap()
}
fn publish(hub: &Hub, upload: &PublicationUpload) {
    if serde_json::from_str::<Value>(&upload.canonical_document).unwrap()["schemaVersion"] != 4 {
        hub.retain_legacy_test_fixture(upload.clone()).unwrap();
        return;
    }
    let mut command = serde_json::to_value(upload).unwrap();
    command["command"] = json!("companion_publish");
    hub.command(&command).unwrap();
}
fn paired(hub: &Hub) -> String {
    let offer = hub
        .command(&json!({"command":"companion_pair_offer","kind":"browser"}))
        .unwrap();
    let request = hub.handle_wire(
        json!({"type":"pair","secret":offer["secret"],"companionId":"browser","name":"Browser"}),
        "pair",
        0,
    );
    assert_eq!(request["ok"], true, "{request}");
    hub.command(&json!({"command":"companion_pair_approve","requestId":request["requestId"],"companionId":"browser"})).unwrap();
    let reply = hub.handle_wire(
        json!({"type":"pair-status","requestId":request["requestId"],"secret":offer["secret"]}),
        "pair",
        0,
    );
    reply["token"].as_str().unwrap().into()
}
#[derive(Default)]
struct Sink {
    losses: AtomicUsize,
}
impl SourceSink for Sink {
    fn register(&self, _: SourceDefinition) -> Result<(), String> {
        Ok(())
    }
    fn baseline(&self, _: &SourceToken, _: u64, _: &[SourceChange]) -> Result<(), String> {
        Ok(())
    }
    fn changes(&self, _: &SourceToken, _: u64, _: &[SourceChange]) -> Result<(), String> {
        Ok(())
    }
    fn loss(&self, _: &SourceToken) -> Result<(), String> {
        self.losses.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
fn protected_files(directory: &Path) -> BTreeMap<&'static str, Vec<u8>> {
    fs::create_dir_all(directory).unwrap();
    let files = BTreeMap::from([
        ("companion-host-state.dpapi", b"sealed credentials".to_vec()),
        ("companion-listener.json", b"saved listener".to_vec()),
        ("companion-tls-state.dpapi", b"saved certificates".to_vec()),
        ("unrelated.json", b"keep settings".to_vec()),
    ]);
    for (name, bytes) in &files {
        fs::write(directory.join(name), bytes).unwrap();
    }
    files
}
fn assert_files(directory: &Path, files: &BTreeMap<&str, Vec<u8>>) {
    for (name, bytes) in files {
        assert_eq!(&fs::read(directory.join(name)).unwrap(), bytes, "{name}");
    }
}

#[test]
fn widget_cutover_discards_old_visuals_neutralizes_assignments_and_retains_compiled_panels_and_pairing(
) {
    let root = crate::test_directory();
    let directory = root.path().join("companions");
    let files = protected_files(&directory);
    let old = old_upload();
    let compiled = compiled_upload();
    write(&directory, "companion-panels.json", &json!([old, compiled])).unwrap();
    write(&directory,"companion-panel-heads.json",&json!({"flight-panel":old.publication_hash,"toggle-native-fixture":compiled.publication_hash})).unwrap();
    let retired_archive = format!("companion-panels.rejected-{}.json", uuid::Uuid::new_v4());
    let compiled_archive = format!("companion-panels.rejected-{}.json", uuid::Uuid::new_v4());
    write(&directory, &retired_archive, &json!([old])).unwrap();
    write(&directory, &compiled_archive, &json!([compiled])).unwrap();
    let sink = Arc::new(Sink::default());
    let hub = Hub::new("host".into(), sink.clone());
    publish(&hub, &old);
    publish(&hub, &compiled);
    let token = paired(&hub);
    for (instance, hash) in [
        ("old", &old.publication_hash),
        ("new", &compiled.publication_hash),
    ] {
        hub.command(&json!({"command":"companion_assign","companionId":"browser","instanceId":instance,"publicationHash":hash})).unwrap();
    }
    assert_eq!(
        hub.handle_wire(json!({"type":"auth","token":token}), "browser", 0)["ok"],
        true
    );
    let open = hub.handle_wire(json!({"type":"open","instanceId":"old"}), "browser", 0);
    assert_eq!(open["ok"], true, "{open}");
    let p = Publication::validate_legacy_test_fixture(old.clone()).unwrap();
    let baseline = json!({"version":1,"type":"baseline","clientId":open["clientId"],"sessionId":open["sessionId"],"generation":open["generation"],"publicationHash":open["publicationHash"],"sequence":"0","values":p.controls.iter().map(|c|(c.id.clone(),c.fallback())).collect::<BTreeMap<_,_>>()});
    assert_eq!(hub.handle_wire(baseline, "browser", 1)["ok"], true);
    let warning = panels(&hub, &directory).unwrap().unwrap();
    assert!(warning.contains("unassigned 1"));
    assert!(!directory.join(retired_archive).exists());
    assert!(directory.join(compiled_archive).is_file());
    assert_eq!(sink.losses.load(Ordering::SeqCst), 1);
    let status = hub.command(&json!({"command":"companion_status"})).unwrap();
    assert_eq!(status["instances"].as_array().unwrap().len(), 1);
    assert_eq!(status["instances"][0]["instanceId"], "new");
    assert_eq!(status["clients"][0]["companionId"], "browser");
    let saved: Vec<PublicationUpload> =
        serde_json::from_slice(&fs::read(directory.join("companion-panels.json")).unwrap())
            .unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].publication_hash, compiled.publication_hash);
    let heads: Value =
        serde_json::from_slice(&fs::read(directory.join("companion-panel-heads.json")).unwrap())
            .unwrap();
    assert_eq!(
        heads,
        json!({"toggle-native-fixture":compiled.publication_hash})
    );
    assert!(panels(&hub, &directory).unwrap().is_none());
    assert_files(&directory, &files);
}

fn seed_profiles(root: &Path) -> osg_profiles::ProfileDocument {
    let document = osg_profiles::ProfileDocument::parse(include_bytes!(
        "../../tests/fixtures/profiles/complete-rig.json"
    ))
    .unwrap();
    let mut library = osg_profiles::store::Library::default();
    library.profiles.push(document.clone());
    library.local_assignments.insert(
        document.id.clone(),
        document
            .resources
            .iter()
            .filter(|r| r.kind == ResourceKind::Companion || r.id == "third-party-a")
            .map(|resource| osg_profiles::store::LocalAssignment {
                resource_id: resource.id.clone(),
                source_id: format!("observed:{}", resource.id),
                ports: resource
                    .ports
                    .iter()
                    .map(|port| osg_profiles::resources::PortExpectation {
                        profile_port_id: port.id.clone(),
                        observed_port_id: port.id.clone(),
                        expected_contract_hash: format!("sha256:{}", "a".repeat(64)),
                    })
                    .collect(),
            })
            .collect(),
    );
    osg_profiles::store::Store::new(root.join("profiles-v2"))
        .save(&library)
        .unwrap();
    document
}
fn seed_inventory(root: &Path) {
    use osg_profiles::{
        resources::{SavedCatalogue, SavedRegistry, SavedResource, SavedSighting},
        resources_ports::{catalogue_hash, PortContract, PortDescriptor, ScalarSchema},
    };
    let port = PortDescriptor {
        id: "old-button".into(),
        name: "Old button".into(),
        direction: osg_profiles::Direction::Output,
        contract: PortContract::State {
            schema: ScalarSchema::Boolean {},
            freshness_ms: 1000,
            fallback: Some(osg_profiles::PortValue::Boolean(false)),
        },
    };
    let hash = catalogue_hash(&[port.clone()]).unwrap();
    let sighting = SavedSighting {
        scope: "companions".into(),
        key: "browser".into(),
        evidence: vec![],
        attributes: BTreeMap::from([
            ("publicationHash".into(), old_upload().publication_hash),
            (
                "capabilityFingerprint".into(),
                format!("sha256:{}", "b".repeat(64)),
            ),
        ]),
        diagnostics: vec![],
        catalogue: Some(SavedCatalogue {
            revision: hash.clone(),
            total: 1,
            complete: true,
            ports: vec![port],
            hash,
        }),
    };
    let saved = SavedRegistry {
        schema_version: 1,
        resources: vec![
            SavedResource {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Companion".into(),
                resource_type: ResourceType::Companion,
                sightings: vec![sighting.clone()],
            },
            SavedResource {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Unrelated controller".into(),
                resource_type: ResourceType::HostController,
                sightings: vec![SavedSighting {
                    scope: "controllers".into(),
                    key: "physical".into(),
                    ..sighting
                }],
            },
        ],
        redirects: BTreeMap::new(),
    };
    osg_profiles::resources_store::Store::new(root.join("profile-resources-v1"))
        .save(&saved)
        .unwrap();
}
#[test]
fn widget_cutover_removes_only_obsolete_companion_profile_mappings_and_inventory_ports() {
    let root = crate::test_directory();
    let directory = root.path().join("companions");
    let files = protected_files(&directory);
    let before = seed_profiles(root.path());
    seed_inventory(root.path());
    let inventory_store =
        osg_profiles::resources_store::Store::new(root.path().join("profile-resources-v1"));
    let inventory_before = inventory_store.load().unwrap().unwrap();
    let warning = profiles(&directory).unwrap().unwrap();
    assert!(warning.contains("old companion resources"));
    let library = osg_profiles::store::Store::new(root.path().join("profiles-v2"))
        .load()
        .unwrap()
        .unwrap();
    let after = library.profiles.iter().find(|p| p.id == before.id).unwrap();
    assert_eq!(after.revision, before.revision + 1);
    assert!(after
        .resources
        .iter()
        .all(|r| r.kind != ResourceKind::Companion));
    assert!(after.companion_assignments.is_empty());
    assert_eq!(
        after.resources,
        before
            .resources
            .iter()
            .filter(|r| r.kind != ResourceKind::Companion)
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(after.processing_nodes, before.processing_nodes);
    assert_eq!(after.virtual_outputs, before.virtual_outputs);
    assert_eq!(after.eligibility, before.eligibility);
    assert_eq!(after.desktop_outputs, before.desktop_outputs);
    assert!(library.local_assignments[&before.id]
        .iter()
        .all(|a| a.resource_id != "companion-panel"));
    let inventory = inventory_store.load().unwrap().unwrap();
    assert!(inventory.resources[0].sightings[0].catalogue.is_none());
    assert!(!inventory.resources[0].sightings[0]
        .attributes
        .contains_key("publicationHash"));
    assert_eq!(
        serde_json::to_value(&inventory.resources[1]).unwrap(),
        serde_json::to_value(&inventory_before.resources[1]).unwrap()
    );
    let bytes = fs::read(root.path().join("profiles-v2/library.json")).unwrap();
    assert!(profiles(&directory).unwrap().is_none());
    assert_eq!(
        fs::read(root.path().join("profiles-v2/library.json")).unwrap(),
        bytes
    );
    assert_files(&directory, &files);
}
#[test]
fn widget_cutover_records_completion_last_and_retries_partial_work_idempotently() {
    let root = crate::test_directory();
    let directory = root.path().join("companions");
    protected_files(&directory);
    let before = seed_profiles(root.path());
    assert!(profiles_with_finish(
        &directory,
        |_| Err("simulated marker commit failure".into())
    )
    .is_err());
    assert!(!directory.join(PROFILE_MARKER).exists());
    let first = fs::read(root.path().join("profiles-v2/library.json")).unwrap();
    profiles(&directory).unwrap();
    assert!(directory.join(PROFILE_MARKER).is_file());
    assert_eq!(
        fs::read(root.path().join("profiles-v2/library.json")).unwrap(),
        first
    );
    let library = osg_profiles::store::Store::new(root.path().join("profiles-v2"))
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(
        library
            .profiles
            .iter()
            .find(|p| p.id == before.id)
            .unwrap()
            .revision,
        before.revision + 1
    );
    fs::write(
        directory.join("companion-panels.json"),
        b"invalid old cache",
    )
    .unwrap();
    write(
        &directory,
        "companion-panel-heads.json",
        &json!({"flight-panel":old_upload().publication_hash}),
    )
    .unwrap();
    let hub = Hub::new(
        "host".into(),
        Arc::new(osg_companion_service::NullSourceSink),
    );
    assert!(
        panels_with_finish(&hub, &directory, |_| Err("simulated marker failure".into())).is_err()
    );
    assert!(!directory.join(PANEL_MARKER).exists());
    assert_eq!(
        fs::read(directory.join("companion-panels.json")).unwrap(),
        b"[]"
    );
    panels(&hub, &directory).unwrap();
    assert!(directory.join(PANEL_MARKER).is_file());
}
#[test]
fn widget_cutover_preserves_corrupt_profile_storage_and_blocks_instead_of_claiming_completion() {
    let root = crate::test_directory();
    let directory = root.path().join("companions");
    protected_files(&directory);
    fs::create_dir(root.path().join("profiles-v2")).unwrap();
    let path = root.path().join("profiles-v2/library.json");
    fs::write(&path, b"invalid unrelated profile data").unwrap();
    assert!(profiles(&directory).is_err());
    assert_eq!(fs::read(path).unwrap(), b"invalid unrelated profile data");
    assert!(!directory.join(PROFILE_MARKER).exists());
    assert!(profiles(Path::new("relative")).is_err());
    fs::create_dir(directory.join(PROFILE_MARKER)).unwrap();
    assert!(profiles(&directory).is_err());
}
