use super::*;
#[test]
fn remove_native_api_configuration_resets_readiness_without_changing_game_files() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    let xplane = json!({"id":"x-plane-12","integrationCompatible":true,"integrationKey":"x-plane-12","provider":"x-plane","installed":true,"executable":"/fixture/X-Plane.exe"});
    runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &xplane,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    let acr = acr_game(&scratch);
    let executable = Path::new(acr["executable"].as_str().unwrap());
    let bytes = fs::read(executable).unwrap();
    acr_record(&runtime, &acr, true);
    let mut catalogue = vec![xplane.clone(), acr.clone()];
    runtime.refresh(&catalogue);
    runtime.decorate(&mut catalogue);
    for game in &catalogue {
        assert_eq!(game["integrationSetup"]["installed"], true);
        assert_eq!(game["integrationSetup"]["removable"], true);
    }
    let (entered, _observed) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    release.send(()).unwrap();
    *runtime.inner.inspection_pause.lock().unwrap() = Some((entered, blocked));
    runtime.set_setup_watch(acr["id"].as_str().unwrap(), true);
    assert!(runtime.refresh_watched_setup(acr["id"].as_str().unwrap(), &acr));
    assert!(runtime.inner.inspection_pause.lock().unwrap().is_none());

    for game in [&xplane, &acr] {
        runtime
            .request(
                &json!({"command":"game_integration_remove"}),
                game,
                Arc::new(FakePlatform(false)),
            )
            .unwrap();
    }
    runtime.decorate(&mut catalogue);
    for game in &catalogue {
        assert_eq!(game["integrationSetup"]["state"], "setup-required");
        assert_eq!(game["integrationSetup"]["installed"], false);
        assert_eq!(game["integrationSetup"]["removable"], false);
    }
    assert!(read_settings(&path).unwrap()["games"]
        .as_object()
        .unwrap()
        .is_empty());
    assert_eq!(fs::read(executable).unwrap(), bytes);
}

#[test]
fn remove_dcs_blocks_running_game_then_restores_owned_loader_and_preserves_external_module() {
    let scratch = Scratch::new();
    let target = scratch.0.join("DCS");
    fs::create_dir(&target).unwrap();
    crate::dcs_setup::fixture_module(&target);
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    runtime
        .request(
            &json!({"command":"game_integration_setup","userFolder":target}),
            &game(),
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    wait_setup(&runtime);
    let mut catalogue = vec![game()];
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["installed"], true);
    assert_eq!(catalogue[0]["integrationSetup"]["removable"], true);
    let (entered, _observed) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    release.send(()).unwrap();
    *runtime.inner.inspection_pause.lock().unwrap() = Some((entered, blocked));
    runtime.set_setup_watch("dcs", true);
    assert!(runtime.refresh_watched_setup("dcs", &catalogue[0]));
    assert!(runtime.inner.inspection_pause.lock().unwrap().is_none());

    let remove = json!({"command":"game_integration_remove","userFolder":target});
    assert!(runtime
        .request(&remove, &game(), Arc::new(FakePlatform(true)))
        .unwrap_err()
        .contains("Close DCS"));
    assert!(target.join("Scripts/Export.lua").exists());
    runtime
        .request(&remove, &game(), Arc::new(FakePlatform(false)))
        .unwrap();
    wait_setup(&runtime);
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["installed"], true);
    assert_eq!(catalogue[0]["integrationSetup"]["removable"], false);
    assert!(catalogue[0]["integrationSetup"]["removalMessage"]
        .as_str()
        .unwrap()
        .contains("outside Cockpit"));
    assert!(target.join("Scripts/DCS-BIOS/BIOS.lua").exists());
    assert!(!target.join("Scripts/Export.lua").exists());
    assert!(read_settings(&path).unwrap()["games"]["dcs"].is_null());
}
#[test]
fn xplane_setup_is_repeatable_preserves_overrides_and_separates_live_status() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let setup = GameIntegrationSetup::new(path.clone(), true);
    let game = json!({"id":"x-plane-11","integrationCompatible":true,"integrationKey":"x-plane-11","provider":"x-plane","installed":true,"executable":"/fixture/X-Plane.exe"});
    assert_eq!(setup.inner.inspect(&game, None)["state"], "setup-required");
    let request = json!({"command":"game_integration_setup","connection":{"port":49005,"updateHz":5,"freshnessMs":2000}});
    setup
        .request(&request, &game, Arc::new(FakePlatform(false)))
        .unwrap();
    let before = read_settings(&path).unwrap();
    assert_eq!(before["games"]["x-plane-11"]["connection"]["port"], 49005);
    setup
        .request(
            &json!({"command":"game_integration_setup"}),
            &game,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    assert_eq!(read_settings(&path).unwrap(), before);
    let mut catalogue = vec![game.clone()];
    setup.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "ready");
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "waiting");
    assert_eq!(catalogue[0]["integrationSetup"]["verified"], false);
    setup.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "waiting");
    let mut changed = game.clone();
    changed["executable"] = json!("/another/X-Plane.exe");
    assert_eq!(
        setup.inner.inspect(&changed, None)["state"],
        "setup-required"
    );
    for connection in [
        json!({"host":"192.168.0.5"}),
        json!({"version":12}),
        json!({"port":0}),
    ] {
        assert!(setup
            .request(
                &json!({"command":"game_integration_setup","connection":connection}),
                &game,
                Arc::new(FakePlatform(false))
            )
            .is_err());
    }
    assert_eq!(read_settings(&path).unwrap(), before);
}
#[test]
fn setup_is_separate_from_game_availability_and_live_telemetry() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), false);
    let mut catalogue = vec![game()];
    decorate(&runtime, &mut catalogue);
    assert_eq!(
        catalogue[0]["integrationSetup"]["state"],
        "unsupported-platform"
    );
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "waiting");
    decorate(&runtime, &mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "waiting");
    assert!(runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &game(),
            Arc::new(FakePlatform(false))
        )
        .unwrap_err()
        .contains("Windows"));
    assert!(!scratch.0.join("setup.json").exists());
}
#[test]
fn invalid_saved_settings_remain_visible_after_recheck_and_are_not_overwritten() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    fs::write(&path, b"broken").unwrap();
    crate::persistence::test_support::record(&path, "games", "setup", b"broken");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    runtime.refresh(&[game()]);
    let mut catalogue = vec![game()];
    decorate(&runtime, &mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "needs-attention");
    assert!(runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &game(),
            Arc::new(FakePlatform(false))
        )
        .is_err());
    assert_eq!(fs::read(path).unwrap(), b"broken");
}
#[test]
fn checks_and_cancel_have_no_provisioning_side_effects() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let missing = json!({"id":"dcs","integrationCompatible":true,"integrationKey":"dcs","provider":"dcs-bios","installed":false});
    runtime
        .request(
            &json!({"command":"game_integration_check"}),
            &missing,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    let mut catalogue = vec![missing.clone()];
    decorate(&runtime, &mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "not-installed");
    assert!(runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &missing,
            Arc::new(FakePlatform(false))
        )
        .is_err());
    assert!(runtime
        .request(
            &json!({"command":"game_integration_cancel"}),
            &missing,
            Arc::new(FakePlatform(false)),
        )
        .is_err());
    assert_eq!(
        fs::read_dir(&scratch.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsString::from("cockpit.sqlite3")]
    );
}
#[test]
fn running_native_dcs_blocks_installation_before_files_change() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    assert!(runtime
        .request(
            &json!({"command":"game_integration_setup","userFolder":scratch.0}),
            &game(),
            Arc::new(FakePlatform(true))
        )
        .unwrap_err()
        .contains("Close DCS"));
    assert_eq!(
        fs::read_dir(&scratch.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsString::from("cockpit.sqlite3")]
    );
}
#[test]
fn local_settings_validation_rejects_invalid_endpoints_and_runtime_pid() {
    let scratch = Scratch::new();
    let connection = dcs_connection(&scratch.0);
    validate_connection("dcs", &connection).unwrap();
    for (key, value) in [
        ("commandHost", json!("bad host")),
        ("commandPort", json!(0)),
        ("listenPort", json!(65536)),
        ("freshnessMs", json!(99)),
        ("multicastGroup", json!("127.0.0.1")),
        ("cataloguePath", json!("relative")),
        ("processId", json!(1)),
    ] {
        let mut invalid = connection.clone();
        invalid[key] = value;
        assert!(validate_connection("dcs", &invalid).is_err());
    }
    validate_connection(
        "assetto-corsa-rally",
        &json!({"source":"shared-memory","freshnessMs":1500}),
    )
    .unwrap();
    assert!(validate_connection(
        "assetto-corsa-rally",
        &json!({"source":"shared-memory","freshnessMs":1500,"processId":42})
    )
    .is_err());
    assert!(validate_connection(
        "assetto-corsa-rally",
        &json!({"source":"synthetic","freshnessMs":1500})
    )
    .is_err());
}
#[test]
fn persisted_records_are_bounded_and_revalidated_on_load() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let value = json!({"schemaVersion":1,"games":{"dcs":{"userFolder":scratch.0,"connection":dcs_connection(&scratch.0)}}});
    fixture_write_settings(&path, &value);
    assert_eq!(read_settings(&path).unwrap(), value);
    fs::write(&path, b"retired source").unwrap();
    assert_eq!(read_settings(&path).unwrap(), value);
    fixture_write_settings(
        &path,
        &json!({"schemaVersion":1,"games":{},"oversized":"x".repeat(40000)}),
    );
    assert!(read_settings(&path).is_err());
    fixture_write_settings(&path, &json!({"schemaVersion":2,"games":{}}));
    assert!(read_settings(&path).is_err());
    fixture_write_settings(&path, &json!({"schemaVersion":1,"games":{"unknown":{}}}));
    assert!(read_settings(&path).is_err());
}

#[test]
fn failed_dcs_recovery_updates_the_passive_setup_view_before_replying() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let game = game();
    runtime.refresh(std::slice::from_ref(&game));
    let mut catalogue = [game.clone()];
    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["recoveryNeeded"], false);

    let journal = runtime.inner.work.join("dcs-setup-journal.json");
    let mut evidence = osg_persistence::Evidence::open(&journal, "recovery-dcs", 16 * 1024 * 1024).unwrap();
    evidence.write(b"invalid DCS evidence").unwrap();
    assert!(runtime.request(
        &json!({"command":"game_integration_recover"}), &game, Arc::new(FakePlatform(false)),
    ).is_err());

    runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["recoveryNeeded"], true);
}
