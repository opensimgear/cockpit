use super::*;
fn compiled_fixture() -> PublicationUpload {
    serde_json::from_str(include_str!(
        "../tests/fixtures/compiled-toggle-publication.json"
    ))
    .unwrap()
}
fn capabilities() -> Value {
    json!({"version":1,"id":"browser-raster-v1","rasterFormats":["rgb565le-a8"],"maxWidth":2048,"maxHeight":2048,"maxFrames":4096,"maxAssetBytes":2097152,"maxTotalBytes":8388608,"maxResidentBytes":67108864,"motion":true,"audio":true,"audioFormats":["wav-pcm16"]})
}

#[test]
fn compiled_public_ingress_rejects_legacy_even_when_the_fixture_feature_is_enabled() {
    let upload: PublicationUpload =
        serde_json::from_str(include_str!("../tests/fixtures/c1-flight-publication.json")).unwrap();
    assert!(Publication::validate(upload.clone())
        .unwrap_err()
        .contains("Only compiled raster"));
    let hub = Hub::new("host".into(), Arc::new(NullSourceSink));
    let mut command = serde_json::to_value(&upload).unwrap();
    command["command"] = json!("companion_publish");
    assert!(hub
        .command(&command)
        .unwrap_err()
        .contains("Only compiled raster"));
    assert!(hub.publication_summaries().is_empty());
    // The separately named test harness still exercises the full source-neutral session machinery.
    hub.retain_legacy_test_fixture(upload).unwrap();
    assert_eq!(hub.publication_summaries().len(), 1);
    let compiled = compiled_fixture();
    let mut command = serde_json::to_value(compiled).unwrap();
    command["command"] = json!("companion_publish");
    assert!(hub.command(&command).is_ok());
}

#[test]
fn compiled_open_negotiates_real_capabilities_and_cache_without_arming_or_bypassing_assignment() {
    let sink = Arc::new(Sink::default());
    let hub = Hub::new("host".into(), sink.clone());
    let token = paired(&hub, "browser");
    let publication = publish(&hub, compiled_fixture());
    assign(
        &hub,
        "browser",
        "toggle-panel",
        &publication.publication_hash,
    );
    assert_eq!(
        wire(&hub, "browser", json!({"type":"auth","token":token}))["ok"],
        true
    );
    let open =
        json!({"type":"open","instanceId":"toggle-panel","widgetCapabilities":capabilities()});
    let mut missing = open.clone();
    missing
        .as_object_mut()
        .unwrap()
        .remove("widgetCapabilities");
    let rejected = wire(&hub, "browser", missing);
    assert_eq!(rejected["ok"], false);
    assert!(rejected["error"].as_str().unwrap().contains("capability"));
    let mut small = open.clone();
    small["widgetCapabilities"]["maxResidentBytes"] = json!(768);
    assert_eq!(wire(&hub, "browser", small)["ok"], false);
    let mut corrupt = open.clone();
    corrupt["haveAssets"] = json!(["bad"]);
    assert_eq!(wire(&hub, "browser", corrupt)["ok"], false);
    let mut images = open.clone();
    images["externalImages"] = json!(true);
    assert_eq!(wire(&hub, "browser", images)["ok"], false);
    let mut other = open.clone();
    other["instanceId"] = json!("not-assigned");
    other["haveAssets"] = json!(["0".repeat(64)]);
    assert_eq!(wire(&hub, "browser", other)["ok"], false);
    assert!(sink.events.lock().unwrap().is_empty());
    let status = operator(&hub, json!({"command":"companion_status"}));
    assert_eq!(status["instances"][0]["connected"], false);
    let document: Value = serde_json::from_str(&publication.canonical_document).unwrap();
    let mut cached = open.clone();
    cached["haveAssets"] = json!([document["ui"]["assets"][0]["sha256"]]);
    let opened = wire(&hub, "browser", cached);
    assert_eq!(opened["ok"], true, "{opened}");
    assert_eq!(opened["publication"]["assets"], json!([]));
    assert_eq!(
        opened["publication"]["canonicalDocument"],
        publication.canonical_document
    );
    assert!(sink.events.lock().unwrap().is_empty());
    let baseline = baseline(&hub, "browser", &opened, &publication);
    assert_eq!(baseline["ok"], true, "{baseline}");
    assert_eq!(
        wire(&hub, "browser", input(&opened, &publication, "1"))["ok"],
        true
    );
    hub.disconnect_connection("browser", hub.now_ms());
    assert_eq!(
        wire(&hub, "fresh", json!({"type":"auth","token":token}))["ok"],
        true
    );
    let fresh = wire(&hub, "fresh", open);
    assert_eq!(fresh["ok"], true, "{fresh}");
    assert_eq!(
        fresh["publication"]["assets"].as_array().unwrap().len(),
        publication.assets.len()
    );
}

#[test]
fn compiled_assignment_and_profile_preparation_reject_esp32_without_replacing_existing_authority() {
    let hub = Hub::new("host".into(), Arc::new(NullSourceSink));
    let token = paired_kind(&hub, "screen", "esp32");
    let old = publish(&hub, fixture());
    assign(&hub, "screen", "panel", &old.publication_hash);
    let publication = publish(&hub, compiled_fixture());
    let rejected=hub.command(&json!({"command":"companion_assign","companionId":"screen","instanceId":"panel","publicationHash":publication.publication_hash})).unwrap_err();
    assert!(rejected.contains("browser companions"));
    assert!(hub
        .prepare_profile_assignment("panel", &publication.publication_hash)
        .unwrap_err()
        .contains("browser companions"));
    assert_eq!(
        operator(&hub, json!({"command":"companion_status"}))["instances"][0]["publicationHash"],
        old.publication_hash
    );
    assert_eq!(
        wire(&hub, "screen", json!({"type":"auth","token":token}))["ok"],
        true
    );
    let open = wire(
        &hub,
        "screen",
        json!({"type":"open","instanceId":"panel","publicationHash":publication.publication_hash,"widgetCapabilities":capabilities()}),
    );
    assert_eq!(open["ok"], false);
    assert!(open["error"]
        .as_str()
        .unwrap()
        .contains("revision mismatch"));
    assert_eq!(
        operator(&hub, json!({"command":"companion_status"}))["instances"][0]["connected"],
        false
    );
}

#[test]
fn validated_publication_target_fact_is_private_and_survives_clone() {
    let compiled = Publication::validate(compiled_fixture()).unwrap();
    let legacy = Publication::validate_legacy_test_fixture(fixture()).unwrap();
    assert!(compiled.requires_browser_companion());
    assert!(compiled.clone().requires_browser_companion());
    assert!(!legacy.requires_browser_companion());
    assert!(!legacy.clone().requires_browser_companion());
    for publication in [&compiled, &legacy] {
        let encoded = serde_json::to_value(publication).unwrap();
        assert!(encoded.get("browserOnly").is_none(), "The derived fact must never become an upload/storage field");
    }
}

#[test]
fn reloaded_compiled_assignment_keeps_browser_admission_and_usb_rejection() {
    let directory = tempfile::tempdir().unwrap();
    let open = || Hub::with_storage(Arc::new(crate::persistence::test_support::store(
        directory.path().join("companion-host-state.dpapi"))), Arc::new(NullSourceSink)).unwrap();
    let hub = open();
    paired_kind(&hub, "browser", "desktop");
    paired_kind(&hub, "usb-screen", "esp32");
    let publication = publish(&hub, compiled_fixture());
    assign(&hub, "browser", "browser-panel", &publication.publication_hash);
    drop(hub);
    // Loading archives revalidates immutable uploads and reconstructs the
    // derived format fact; it cannot default to accepting USB raster panels.
    let restored = open();
    assert!(restored.assigned_publication("browser-panel").unwrap().requires_browser_companion());
    let proof = restored.prepare_profile_assignment("browser-panel", &publication.publication_hash).unwrap();
    let receipt = restored.capture_profile_assignment(&proof, &uuid::Uuid::new_v4().to_string(), None).unwrap();
    restored.acquire_profile_assignment(&receipt).unwrap();
    for _ in 0..128 { restored.validate_profile_assignment(&receipt).unwrap(); }
    assert!(restored.command(&json!({"command":"companion_assign","companionId":"usb-screen",
        "instanceId":"usb-panel","publicationHash":publication.publication_hash})).unwrap_err().contains("browser companions"));
    assert!(restored.prepare_profile_assignment(&profile::profile_instance_id("usb-screen"),
        &publication.publication_hash).unwrap_err().contains("browser companions"));
    restored.validate_profile_assignment_owner(&receipt).unwrap();
}
