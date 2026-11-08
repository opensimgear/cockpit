use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn xplane(executable: &str) -> Value {
    json!({"id":"custom-title","integrationKey":"x-plane-12","provider":"x-plane",
        "integrationCompatible":true,"installed":true,"executable":executable,
        "installPath":"/fixture","selectedInstallationId":executable})
}

#[test]
fn cold_decoration_is_memory_only_and_does_not_enter_admission_or_inspection() {
    let scratch = Scratch::new();
    let runtime = Arc::new(GameIntegrationSetup::new(scratch.0.join("setup.json"), true));
    let admission = runtime.admission.write().unwrap();
    let fingerprints = runtime.inner.fingerprints.hold_for_test(crate::game_telemetry::Game::Rally);
    let states = runtime.inner.states.lock().unwrap();
    let reader = runtime.clone();
    let (completed, received) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut catalogue = vec![xplane("/absent/X-Plane.exe")];
        reader.decorate(&mut catalogue);
        let mut source = catalogue.clone();
        reader.decorate_source(&mut source);
        assert_eq!(source[0]["integrationSetup"]["state"], "checking");
        completed.send(catalogue).unwrap();
    });
    let result = received.recv_timeout(Duration::from_secs(1));
    drop(states);
    drop(fingerprints);
    drop(admission);
    worker.join().unwrap();
    let catalogue = result.expect("Decoration must not wait for source or admission locks");
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "checking");
    assert_eq!(catalogue[0]["integrationSetup"]["verified"], false);
    assert!(runtime.inner.states.lock().unwrap().is_empty());
    assert!(runtime.inner.catalogues.lock().unwrap().is_empty());
    assert!(!runtime.busy());
    assert!(!runtime.inner.work.exists());
}

#[test]
fn exact_installations_keep_separate_views_and_changed_scopes_never_reuse_ready() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let first = xplane("/fixture/one/X-Plane.exe");
    let second = xplane("/fixture/two/X-Plane.exe");
    runtime.inner.saved.lock().unwrap()["games"]["x-plane-12"] =
        json!({"executable":first["executable"],"connection":crate::xplane::defaults(12)});
    runtime.refresh(&[first.clone(), second.clone()]);
    let mut catalogue = vec![first.clone(), second];
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "ready");
    assert_eq!(catalogue[1]["integrationSetup"]["state"], "setup-required");
    let mut source = catalogue.clone();
    runtime.decorate_source(&mut source);
    assert_eq!(source[0]["integrationSetup"]["state"], "ready");
    assert_eq!(source[1]["integrationSetup"]["state"], "setup-required");
    let stable = catalogue.clone();
    runtime.decorate(&mut catalogue);
    assert_eq!(stable, catalogue);
    for (field, replacement) in [
        ("executable", json!("/changed/X-Plane.exe")),
        ("installPath", json!("/changed/root")),
        ("selectedInstallationId", json!("another-copy")),
        ("integrationCompatible", json!(false)),
        ("processExecutables", json!(["/another/process.exe"])),
    ] {
        let mut changed = first.clone();
        changed[field] = replacement;
        runtime.decorate(std::slice::from_mut(&mut changed));
        let expected = if field == "integrationCompatible" { "setup-required" } else { "checking" };
        assert_eq!(changed["integrationSetup"]["state"], expected, "{field}");
    }
    runtime.inner.saved.lock().unwrap()["games"]["x-plane-12"]["connection"]["port"] = json!(49001);
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "checking");
    assert_eq!(catalogue[0]["integrationSetup"]["connection"]["port"], 49001);
    runtime.decorate_source(&mut source);
    assert_eq!(source[0]["integrationSetup"]["state"], "checking");
}

#[test]
fn blocked_source_inspection_leaves_passive_views_and_native_admission_independent() {
    let scratch = Scratch::new();
    let runtime = Arc::new(GameIntegrationSetup::new(scratch.0.join("setup.json"), true));
    let game = acr_game(&scratch);
    acr_record(&runtime, &game, true);
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.inspection_pause.lock().unwrap() = Some((entered, blocked));
    let source = runtime.clone();
    let source_game = game.clone();
    let worker = thread::spawn(move || source.refresh(&[source_game]));
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    let reader = runtime.clone();
    let (completed, received) = mpsc::channel();
    let getter = thread::spawn(move || {
        let mut catalogue = vec![game.clone()];
        reader.decorate(&mut catalogue);
        let validation = reader.validate_native_build(&game, false);
        completed.send((catalogue, validation)).unwrap();
    });
    let result = received.recv_timeout(Duration::from_secs(1));
    release.send(()).unwrap();
    worker.join().unwrap();
    getter.join().unwrap();
    let (catalogue, validation) = result.expect("Passive inspection cannot gate native admission");
    validation.unwrap();
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "ready");
}

#[test]
fn worker_progress_and_terminal_errors_are_published_for_the_exact_scope() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let game = xplane("/fixture/one/X-Plane.exe");
    let bound = bound_game(&game).unwrap();
    runtime.inner.publish_start(&bound, json!({"state":"setting-up","message":"Preparing","verified":false}));
    runtime.inner.publish_progress(&bound, "Checking native telemetry", Some(42));
    let mut catalogue = vec![game.clone()];
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "setting-up");
    assert_eq!(catalogue[0]["integrationSetup"]["progress"], "Checking native telemetry");
    assert_eq!(catalogue[0]["integrationSetup"]["percent"], 42);
    let (record, mut status) = runtime.inner.inspect_scoped(&bound);
    status["state"] = json!("needs-attention");
    status["message"] = json!("Cancelled after the final publication boundary");
    runtime.inner.publish_setup(&bound, &record, status);
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "needs-attention");
    assert_eq!(catalogue[0]["integrationSetup"]["message"], "Cancelled after the final publication boundary");
}

#[test]
fn stale_inspection_cannot_publish_under_a_new_saved_record() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let game = xplane("/fixture/X-Plane.exe");
    let bound = bound_game(&game).unwrap();
    let (record, _) = runtime.inner.inspect_scoped(&bound);
    runtime.inner.saved.lock().unwrap()["games"]["x-plane-12"] =
        json!({"executable":game["executable"],"connection":crate::xplane::defaults(12)});
    runtime.inner.publish_setup(&bound, &record, json!({"state":"ready"}));
    let mut catalogue = vec![game];
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "checking");
    assert!(runtime.inner.states.lock().unwrap().is_empty());
}

#[test]
fn cached_catalogue_metadata_misses_never_load_files_or_mint_entries() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    runtime.inner.saved.lock().unwrap()["games"]["dcs"] =
        json!({"connection":dcs_connection(&scratch.0.join("absent-catalogue"))});
    for (id, vehicle) in [("dcs", None), ("dcs", Some("A-10C")), ("x-plane-12", None)] {
        assert!(runtime.cached_integration_catalogue_metadata_for_vehicle(id, vehicle).is_none());
    }
    assert!(runtime.inner.catalogues.lock().unwrap().is_empty());
    let full = runtime.resource_integration_catalogue("x-plane-12").unwrap();
    let metadata = runtime.cached_integration_catalogue_metadata_for_vehicle("x-plane-12", None).unwrap();
    assert_eq!(metadata["catalogueRevision"], full["catalogueRevision"]);
    runtime.inner.saved.lock().unwrap()["games"]["x-plane-12"] = json!({"executable":"changed"});
    assert!(runtime.cached_integration_catalogue_metadata_for_vehicle("x-plane-12", None).is_none());
}

#[test]
fn setup_views_cover_the_full_title_limit_and_large_channels_do_not_consume_ui_budget() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let mut catalogue = (0..519).map(|index| xplane(&format!("/fixture/{index}/X-Plane.exe"))).collect::<Vec<_>>();
    runtime.refresh(&catalogue);
    runtime.decorate(&mut catalogue);
    assert!(catalogue.iter().all(|game| game["integrationSetup"]["state"] == "setup-required"));
    let bound = bound_game(&catalogue[0]).unwrap();
    let record = runtime.inner.provider_snapshot("x-plane-12").0;
    runtime.inner.publish_setup(&bound, &record, json!({"state":"ready","channels":"x".repeat(300_000),"optionalMaxRpm":true}));
    runtime.decorate(&mut catalogue[..1]);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "ready");
    assert!(catalogue[0]["integrationSetup"].get("channels").is_none());
    assert!(catalogue[0]["integrationSetup"].get("optionalMaxRpm").is_none());
    runtime.inner.publish_setup(&bound, &record, json!({"state":"ready","message":"x".repeat(300_000)}));
    runtime.decorate(&mut catalogue[..1]);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "unavailable");
    assert!(catalogue[0]["integrationSetup"]["message"].as_str().unwrap().contains("display limit"));
    assert_eq!(runtime.inner.states.lock().unwrap()["x-plane-12"]["state"], "ready");
}

#[test]
fn display_budget_does_not_control_authoritative_source_readiness() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let game = xplane("/fixture/X-Plane.exe");
    runtime.inner.saved.lock().unwrap()["games"]["x-plane-12"] =
        json!({"executable":game["executable"],"connection":crate::xplane::defaults(12)});
    let bound = bound_game(&game).unwrap();
    let record = runtime.inner.provider_snapshot("x-plane-12").0;
    runtime.inner.publish_setup(&bound, &record,
        json!({"state":"ready","message":"x".repeat(300_000)}));
    let mut catalogue = vec![game];
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "unavailable");
    assert_eq!(runtime.inner.states.lock().unwrap()["x-plane-12"]["state"], "ready");
    runtime.decorate_source(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "ready");
    assert!(catalogue[0]["integrationSetup"]["providerSetupRevision"].is_string());
    let run = runtime.inner.publish_start(&bound, json!({"state":"setting-up"}));
    runtime.decorate_source(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "setting-up");
    runtime.inner.finish_job(&run, &json!({"state":"ready"}));
}

#[test]
fn cold_loaded_settings_errors_remain_visible_without_source_refresh() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    crate::persistence::test_support::record(&path, "games", "setup", b"broken");
    let runtime = GameIntegrationSetup::new(path, false);
    let _admission = runtime.admission.write().unwrap();
    let mut catalogue = vec![xplane("/absent/X-Plane.exe")];
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "needs-attention");
    assert_eq!(catalogue[0]["integrationSetup"]["settingsError"], true);
    assert_eq!(catalogue[0]["integrationSetup"]["recoveryNeeded"], false);
    assert!(catalogue[0]["integrationSetup"]["message"].is_string());
}

#[test]
fn cold_incompatible_installation_has_actionable_memory_only_setup_state() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let _admission = runtime.admission.write().unwrap();
    let mut game = xplane("/absent/X-Plane.exe");
    game["integrationCompatible"] = json!(false);
    game["installed"] = json!(false);
    runtime.decorate(std::slice::from_mut(&mut game));
    assert_eq!(game["integrationSetup"]["state"], "setup-required");
    assert_eq!(game["integrationSetup"]["verified"], false);
    assert!(game["integrationSetup"]["message"].as_str().unwrap().contains("compatible executable"));
    assert!(runtime.inner.states.lock().unwrap().is_empty());
}

#[test]
fn checking_another_dcs_folder_never_qualifies_the_broken_configured_folder() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let ready = scratch.0.join("ReadyDCS");
    fs::create_dir(&ready).unwrap();
    crate::dcs_setup::fixture_module(&ready);
    runtime.request(&json!({"command":"game_integration_setup","userFolder":ready}),
        &game(), Arc::new(FakePlatform(false))).unwrap();
    wait_setup(&runtime);
    let broken = scratch.0.join("BrokenDCS");
    fs::create_dir(&broken).unwrap();
    let connection = dcs_connection(&broken.join("Scripts/DCS-BIOS/doc/json"));
    {
        let mut saved = runtime.inner.saved.lock().unwrap();
        saved["games"]["dcs"]["userFolder"] = json!(broken);
        saved["games"]["dcs"]["connection"] = connection.clone();
    }
    runtime.request(&json!({"command":"game_integration_check","userFolder":ready}),
        &game(), Arc::new(FakePlatform(false))).unwrap();
    let mut catalogue = vec![game()];
    runtime.decorate(&mut catalogue);
    let status = &catalogue[0]["integrationSetup"];
    assert_ne!(status["state"], "ready");
    assert_eq!(status["userFolder"], json!(broken));
    assert_eq!(status["connection"], connection);
    assert_eq!(status["checkPreview"]["state"], "ready");
    assert_eq!(status["checkPreview"]["userFolder"], json!(ready));
    assert_eq!(status["checkPreview"]["connection"]["cataloguePath"],
        json!(fs::canonicalize(ready.join("Scripts/DCS-BIOS/doc/json")).unwrap()));
    runtime.refresh(&[game()]);
    runtime.decorate(&mut catalogue);
    assert_ne!(catalogue[0]["integrationSetup"]["state"], "ready");
    assert_eq!(catalogue[0]["integrationSetup"]["checkPreview"]["userFolder"], json!(ready));
    runtime.decorate_source(&mut catalogue);
    assert_ne!(catalogue[0]["integrationSetup"]["state"], "ready");
    assert!(catalogue[0]["integrationSetup"].get("checkPreview").is_none());
    assert_eq!(runtime.inner.saved.lock().unwrap()["games"]["dcs"]["userFolder"], json!(broken));
    runtime.request(&json!({"command":"game_integration_check","userFolder":broken}),
        &game(), Arc::new(FakePlatform(false))).unwrap();
    runtime.decorate(&mut catalogue);
    assert!(catalogue[0]["integrationSetup"].get("checkPreview").is_none());
}

#[test]
fn dcs_worker_retains_current_progress_after_saving_before_blocked_final_inspection() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let target = scratch.0.join("DCS");
    fs::create_dir(&target).unwrap();
    crate::dcs_setup::fixture_module(&target);
    let previous_revision = runtime.provider_setup_revision("dcs");
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.inspection_pause.lock().unwrap() = Some((entered, blocked));
    runtime.request(&json!({"command":"game_integration_setup","userFolder":target}),
        &game(), Arc::new(FakePlatform(false))).unwrap();
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    // The worker has committed the new record and is waiting in its final
    // source inspection. Capture observations before releasing that boundary.
    let was_busy = runtime.busy();
    let current_revision = runtime.provider_setup_revision("dcs");
    let configured = runtime.inner.saved.lock().unwrap()["games"]["dcs"]["connection"].is_object();
    let mut catalogue = vec![game()];
    runtime.decorate(&mut catalogue);
    let display = catalogue[0]["integrationSetup"].clone();
    runtime.decorate_source(&mut catalogue);
    let source = catalogue[0]["integrationSetup"].clone();
    release.send(()).unwrap();
    wait_setup(&runtime);
    assert!(was_busy);
    assert!(configured);
    assert_ne!(current_revision, previous_revision);
    assert_eq!(display["state"], "setting-up");
    assert_eq!(display["progress"], "Finalizing DCS integration setup");
    assert_eq!(display["providerSetupRevision"], current_revision);
    assert_eq!(source["state"], "setting-up");
    assert_eq!(source["verified"], false);
    assert_eq!(source["providerSetupRevision"], current_revision);
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "ready");
}
