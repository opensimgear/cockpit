use super::*;
#[test]
fn acr_setup_preserves_freshness_and_never_modifies_game_files_or_persists_pid() {
    let scratch = Scratch::new();
    let game = acr_game(&scratch);
    let executable = Path::new(game["executable"].as_str().unwrap());
    let bytes = fs::read(executable).unwrap();
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    runtime
        .request(
            &json!({"command":"game_integration_setup","connection":{"freshnessMs":2200}}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    wait_setup(&runtime);
    let record = &read_settings(&path).unwrap()["games"]["assetto-corsa-rally"];
    assert_eq!(
        record["connection"],
        json!({"source":"shared-memory","freshnessMs":2200})
    );
    assert!(record.to_string().contains("sha256"));
    assert!(!record.to_string().contains("processId"));
    assert_eq!(fs::read(executable).unwrap(), bytes);
    assert!(!scratch.0.join("Scripts").exists());
    let mut catalogue = vec![game.clone()];
    decorate(&runtime, &mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["verified"], false);
}
#[test]
fn acr_setup_is_native_and_bound_to_the_exact_executable_fingerprint() {
    let scratch = Scratch::new();
    let game = acr_game(&scratch);
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    assert_eq!(
        runtime.inner.inspect(&game, None)["state"],
        "setup-required"
    );
    acr_record(&runtime, &game, false);
    assert_eq!(
        runtime.inner.inspect(&game, None)["state"],
        "needs-attention"
    );
    fs::write(
        game["executable"].as_str().unwrap(),
        b"Updated authored build",
    )
    .unwrap();
    runtime.refresh(&[game.clone()]);
    let mut catalogue = vec![game.clone()];
    runtime.decorate(&mut catalogue);
    assert!(catalogue[0]["integrationSetup"]["message"]
        .as_str()
        .unwrap()
        .contains("changed"));
}
#[test]
fn acr_layout_failures_invalidate_build_proof_and_diagnostics_survive_restart() {
    let scratch = Scratch::new();
    let game = acr_game(&scratch);
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    let identity = acr_record(&runtime, &game, true);
    runtime.inner.accept_acr_proof(&json!({"nativeVerified":false,"status":"paused","fingerprint":identity,"diagnostics":["Paused"]})).unwrap();
    assert_eq!(
        runtime.inner.saved.lock().unwrap()["games"]["assetto-corsa-rally"]["verifiedFingerprint"],
        identity
    );
    runtime.inner.accept_acr_proof(&json!({"nativeVerified":false,"status":"unsupported-layout","fingerprint":identity,"diagnostics":["Unsupported ACR layout"]})).unwrap();
    let restarted = GameIntegrationSetup::new(path, true);
    let mut catalogue = vec![game.clone()];
    decorate(&restarted, &mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "needs-attention");
    assert_eq!(catalogue[0]["integrationSetup"]["verified"], false);
    assert_eq!(
        catalogue[0]["integrationSetup"]["message"],
        "Unsupported ACR layout"
    );
}
#[test]
fn metadata_cannot_claim_live_delivery_and_shutdown_cannot_admit_another_probe() {
    let scratch = Scratch::new();
    let game = acr_game(&scratch);
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    acr_record(&runtime, &game, false);
    let mut catalogue = vec![game.clone()];
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "waiting");
    assert_eq!(catalogue[0]["integrationSetup"]["verified"], false);
    runtime.shutdown();
    assert!(runtime
        .request(
            &json!({"command":"game_integration_check"}),
            &game,
            Arc::new(FakePlatform(false))
        )
        .unwrap_err()
        .contains("closed"));
    assert!(!scratch.0.join("setup.json").exists());
}
#[test]
fn ac_setup_repair_replaces_catalogue_even_for_same_build_and_check_never_rescans() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let game = ac_catalogue_game(&scratch.0.join("AC"));
    let root = Path::new(game["installPath"].as_str().unwrap());
    ac_catalogue_car(root, "lotus_elise_sc", "Lotus Elise SC");
    ac_catalogue_car(root, "old_mod", "Old Mod");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    let setup = json!({"command":"game_integration_setup"});
    runtime
        .request(&setup, &game, Arc::new(FakePlatform(false)))
        .unwrap();
    wait_setup(&runtime);
    let initial =
        json!([{"id":"lotus_elise_sc","name":"Lotus Elise SC"},{"id":"old_mod","name":"Old Mod"}]);
    assert_eq!(ac_catalogue_choices(&runtime, &game), initial);
    assert_eq!(runtime.inner.inspect(&game, None)["verified"], false);
    let identity =
        read_settings(&path).unwrap()["games"]["assetto-corsa"]["configuredFingerprint"].clone();

    fs::remove_file(root.join("content/cars/old_mod/data.acd")).unwrap();
    ac_catalogue_car(root, "lotus_elise_sc", "Renamed Lotus");
    ac_catalogue_car(root, "new_mod", "New Mod");
    runtime.refresh(std::slice::from_ref(&game));
    let mut passive = vec![game.clone()];
    runtime.decorate(&mut passive);
    assert_eq!(ac_catalogue_choices(&runtime, &game), initial);
    runtime
        .request(
            &json!({"command":"game_integration_check"}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    wait_setup(&runtime);
    assert_eq!(
        ac_catalogue_choices(&runtime, &game),
        initial,
        "Check must not scan changed content"
    );

    runtime
        .request(&setup, &game, Arc::new(FakePlatform(false)))
        .unwrap();
    wait_setup(&runtime);
    let repaired =
        json!([{"id":"lotus_elise_sc","name":"Renamed Lotus"},{"id":"new_mod","name":"New Mod"}]);
    assert_eq!(ac_catalogue_choices(&runtime, &game), repaired);
    assert_eq!(
        read_settings(&path).unwrap()["games"]["assetto-corsa"]["configuredFingerprint"],
        identity,
        "Repair must refresh car content even when the executable identity did not change"
    );
    runtime.shutdown();

    let restored = GameIntegrationSetup::new(path, true);
    assert_eq!(
        ac_catalogue_choices(&restored, &game),
        repaired,
        "Friendly names must survive restart"
    );
    let other = ac_catalogue_game(&scratch.0.join("Other AC"));
    assert_eq!(
        ac_catalogue_choices(&restored, &other),
        json!([]),
        "A different installation cannot inherit the old installed roster"
    );
    restored.shutdown();
}

#[test]
fn ac_catalogue_publication_waits_for_setup_completion_and_cancelled_repair_preserves_it() {
    use std::sync::mpsc;
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let game = ac_catalogue_game(&scratch.0.join("AC"));
    let root = Path::new(game["installPath"].as_str().unwrap());
    ac_catalogue_car(root, "car", "Original Car");
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.native_probe_pause.lock().unwrap() = Some((entered, blocked));
    runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    observed
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(runtime.busy());
    assert_eq!(
        ac_catalogue_choices(&runtime, &game),
        json!([]),
        "An unfinished setup must not publish its scanned choices"
    );
    release.send(()).unwrap();
    wait_setup(&runtime);
    let initial = json!([{"id":"car","name":"Original Car"}]);
    assert_eq!(ac_catalogue_choices(&runtime, &game), initial);

    ac_catalogue_car(root, "new_car", "New Car");
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.native_probe_pause.lock().unwrap() = Some((entered, blocked));
    runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    observed
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert_eq!(ac_catalogue_choices(&runtime, &game), initial);
    runtime
        .request(
            &json!({"command":"game_integration_cancel"}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    release.send(()).unwrap();
    wait_setup(&runtime);
    assert_eq!(
        ac_catalogue_choices(&runtime, &game),
        initial,
        "Cancelled repair must retain the last completed catalogue"
    );
    runtime.shutdown();
}

#[test]
fn ac_cancel_at_final_publication_boundary_preserves_memory_and_disk_catalogue() {
    use std::sync::mpsc;
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let cache_path = path.with_file_name("assetto-corsa-cars-v1.json");
    let runtime = GameIntegrationSetup::new(path, true);
    let game = ac_catalogue_game(&scratch.0.join("AC"));
    let root = Path::new(game["installPath"].as_str().unwrap());
    ac_catalogue_car(root, "car", "Original Car");
    let setup = json!({"command":"game_integration_setup"});
    runtime
        .request(&setup, &game, Arc::new(FakePlatform(false)))
        .unwrap();
    wait_setup(&runtime);
    let initial = ac_catalogue_choices(&runtime, &game);
    let db = osg_persistence::Database::for_namespace(&cache_path).unwrap();
    let persisted = db.read("games", "ac-cars").unwrap().unwrap();

    ac_catalogue_car(root, "new_car", "New Car");
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.ac_cars_publication_pause.lock().unwrap() = Some((entered, blocked));
    runtime
        .request(&setup, &game, Arc::new(FakePlatform(false)))
        .unwrap();
    // The worker has finished scanning/probing and passed its early cancellation
    // check. Pause only the final publication, then acknowledge a real cancel.
    observed
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(runtime.busy());
    runtime
        .request(
            &json!({"command":"game_integration_cancel"}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    assert!(runtime.inner.cancel.load(Ordering::Acquire));
    release.send(()).unwrap();
    wait_setup(&runtime);

    assert_eq!(ac_catalogue_choices(&runtime, &game), initial);
    let retained = db.read("games", "ac-cars").unwrap().unwrap();
    assert_eq!(retained.payload, persisted.payload);
    assert_eq!(retained.revision, persisted.revision);
    assert!(
        runtime.inner.states.lock().unwrap()["assetto-corsa"]["message"]
            .as_str()
            .unwrap()
            .contains("cancelled")
    );
    runtime.shutdown();
}

#[test]
fn other_assetto_setup_providers_never_create_the_original_ac_car_catalogue() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    for (id, filename) in [
        ("assetto-corsa-rally", "acr.exe"),
        ("assetto-corsa-competizione", "AC2-Win64-Shipping.exe"),
        ("assetto-corsa-evo", "AssettoCorsaEVO.exe"),
    ] {
        let executable = scratch.0.join(filename);
        fs::write(&executable, b"Unlaunched other-Assetto setup fixture").unwrap();
        let game = json!({"id":id,"integrationCompatible":true,"integrationKey":id,"provider":id,"installed":true,"executable":executable});
        runtime
            .request(
                &json!({"command":"game_integration_setup"}),
                &game,
                Arc::new(FakePlatform(false)),
            )
            .unwrap();
        wait_setup(&runtime);
        assert!(!path.with_file_name("assetto-corsa-cars-v1.json").exists());
        let mut decorated = game.clone();
        runtime.decorate_installed_cars(&mut decorated);
        assert_eq!(decorated, game, "Car discovery must leave {id} unchanged");
    }
    runtime.shutdown();
}

#[test]
fn native_build_admission_rechecks_metadata_without_waiting_for_passive_refresh() {
    let scratch = Scratch::new();
    let game = acr_game(&scratch);
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    acr_record(&runtime, &game, true);
    runtime.validate_native_build(&game, false).unwrap();
    let executable = Path::new(game["executable"].as_str().unwrap());
    fs::write(executable, b"Changed game build").unwrap();
    assert!(runtime.validate_native_build(&game, false).is_err());
}

#[test]
fn passive_refresh_cannot_overwrite_a_newly_admitted_setup_worker() {
    use std::sync::mpsc;
    let scratch = Scratch::new();
    let game = ac_catalogue_game(&scratch.0.join("AC"));
    let runtime = Arc::new(GameIntegrationSetup::new(
        scratch.0.join("setup.json"),
        true,
    ));
    runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    wait_setup(&runtime);
    // Pause a real passive inspection after it has captured saved settings.
    let fingerprints = runtime.inner.fingerprints.lock().unwrap();
    let refreshed = runtime.clone();
    let refreshing_game = game.clone();
    let refresh = std::thread::spawn(move || refreshed.refresh(&[refreshing_game]));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while runtime.admission.try_lock().is_ok() {
        assert!(
            std::time::Instant::now() < deadline,
            "Passive refresh did not retain admission through inspection"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.native_probe_pause.lock().unwrap() = Some((entered, blocked));
    let platform = Arc::new(FakePlatform(false));
    let (submitted, received) = mpsc::channel();
    let setup = runtime.clone();
    let setup_game = game.clone();
    let request = std::thread::spawn(move || {
        submitted
            .send(setup.request(
                &json!({"command":"game_integration_setup"}),
                &setup_game,
                platform,
            ))
            .unwrap()
    });
    assert!(matches!(
        received.recv_timeout(std::time::Duration::from_millis(30)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(!runtime.busy());
    drop(fingerprints);
    refresh.join().unwrap();
    received
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap()
        .unwrap();
    request.join().unwrap();
    observed
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    assert!(runtime.busy());
    runtime.refresh(&[game.clone()]);
    let mut catalogue = vec![game];
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "setting-up");
    release.send(()).unwrap();
    wait_setup(&runtime);
    runtime.shutdown();
}
