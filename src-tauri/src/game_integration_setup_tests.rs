use super::*;
use crate::game_discovery::ProcessIdentity;

#[test]
fn remove_native_api_configuration_resets_readiness_without_changing_game_files() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    let xplane = json!({"id":"x-plane-12","provider":"x-plane","installed":true,"executable":"/fixture/X-Plane.exe"});
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
    runtime.decorate(&mut catalogue);
    for game in &catalogue {
        assert_eq!(game["integrationSetup"]["installed"], true);
        assert_eq!(game["integrationSetup"]["removable"], true);
    }
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
    let game = json!({"id":"x-plane-11","provider":"x-plane","installed":true,"executable":"/fixture/X-Plane.exe"});
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
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("osg-setup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct FakePlatform(bool);
impl Platform for FakePlatform {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![]
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        Err("Unexpected launch".into())
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(self.0.then(|| ProcessIdentity {
            pid: 42,
            path: "DCS.exe".into(),
            epoch: 1,
        }))
    }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
        Ok(self.0)
    }
}
fn game() -> Value {
    json!({"id":"dcs","provider":"dcs-bios","installed":true})
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
    let missing = json!({"id":"dcs","provider":"dcs-bios","installed":false});
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
    runtime
        .request(
            &json!({"command":"game_integration_cancel"}),
            &missing,
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 0);
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
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 0);
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
    write_settings(&path, &value).unwrap();
    assert_eq!(read_settings(&path).unwrap(), value);
    fs::write(&path, vec![0; 40000]).unwrap();
    assert!(read_settings(&path).is_err());
    fs::write(&path, b"{\"schemaVersion\":2,\"games\":{}}").unwrap();
    assert!(read_settings(&path).is_err());
    fs::write(&path, b"{\"schemaVersion\":1,\"games\":{\"unknown\":{}}}").unwrap();
    assert!(read_settings(&path).is_err());
}
fn wait_setup(runtime: &GameIntegrationSetup) {
    // These fixtures verify completion, not setup latency. Instrumented suites
    // run filesystem sync and recovery work alongside all native tests.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while runtime.busy() {
        if std::time::Instant::now() >= end {
            let states = runtime.inner.states.try_lock().map(|states| states.clone());
            let worker_finished = runtime
                .worker
                .try_lock()
                .map(|worker| worker.as_ref().map(JoinHandle::is_finished));
            assert!(
                !runtime.busy(),
                "Setup did not finish within 30 seconds; pending={}, worker_finished={worker_finished:?}, states={states:?}",
                runtime.pending()
            );
            break;
        }
        thread::sleep(std::time::Duration::from_millis(5));
    }
}
#[test]
fn existing_compatible_install_is_configured_without_network_or_profile_writes() {
    let scratch = Scratch::new();
    let target = scratch.0.join("DCS");
    fs::create_dir(&target).unwrap();
    crate::dcs_setup::fixture_module(&target);
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    let request = json!({"command":"game_integration_setup","userFolder":target});
    runtime
        .request(&request, &game(), Arc::new(FakePlatform(false)))
        .unwrap();
    wait_setup(&runtime);
    let mut catalogue = vec![game()];
    decorate(&runtime, &mut catalogue);
    assert_eq!(
        catalogue[0]["integrationSetup"]["state"], "ready",
        "{}",
        catalogue[0]
    );
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "waiting");
    let before = fs::read(target.join("Scripts/Export.lua")).unwrap();
    runtime
        .request(&request, &game(), Arc::new(FakePlatform(false)))
        .unwrap();
    wait_setup(&runtime);
    assert_eq!(fs::read(target.join("Scripts/Export.lua")).unwrap(), before);
    let saved = read_settings(&path).unwrap();
    assert_eq!(
        saved["games"]["dcs"]["connection"]["cataloguePath"],
        json!(target.join("Scripts/DCS-BIOS/doc/json"))
    );
    assert_eq!(runtime.inner.inspect(&game(), None)["state"], "ready");
    runtime.shutdown();
    let restarted = GameIntegrationSetup::new(path, true);
    decorate(&restarted, &mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "ready");
    fs::write(
        target.join("Scripts/Export.lua"),
        b"-- loader was removed externally\n",
    )
    .unwrap();
    assert_ne!(restarted.inner.inspect(&game(), None)["state"], "ready");
    restarted.refresh(&[game()]);
    decorate(&restarted, &mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "setup-required");
}

#[test]
fn installer_preserves_game_owned_aircraft_and_never_imports_retired_connection_settings() {
    let scratch = Scratch::new();
    let target = scratch.0.join("DCS");
    fs::create_dir(&target).unwrap();
    crate::dcs_setup::fixture_module(&target);
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    let mut connection = dcs_connection(&target.join("Scripts/DCS-BIOS/doc/json"));
    connection["freshnessMs"] = json!(2200);
    runtime.save_provider_settings(&json!({"command":"game_integration_settings_save","expectedSetupRevision":runtime.provider_setup_revision("dcs"),"userFolder":target,"connection":connection,"aircraftModule":"A-10C"}),&game()).unwrap();
    runtime
        .request(
            &json!({"command":"game_integration_setup","userFolder":target}),
            &game(),
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    wait_setup(&runtime);
    let saved = read_settings(&path).unwrap();
    assert_eq!(saved["games"]["dcs"]["aircraftModule"], "A-10C");
    assert_eq!(saved["games"]["dcs"]["connection"]["commandPort"], 7778);
    assert_eq!(saved["games"]["dcs"]["connection"]["freshnessMs"], 2200);
}
#[test]
fn incompatible_udp_overrides_are_preserved_and_never_reported_ready() {
    let scratch = Scratch::new();
    let target = scratch.0.join("DCS");
    fs::create_dir(&target).unwrap();
    crate::dcs_setup::fixture_module(&target);
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    runtime.request(&json!({"command":"game_integration_setup","userFolder":target,"connection":{"commandPort":9000,"freshnessMs":2400}}),&game(),Arc::new(FakePlatform(false))).unwrap();
    wait_setup(&runtime);
    let saved = read_settings(&path).unwrap();
    assert_eq!(saved["games"]["dcs"]["connection"]["commandPort"], 9000);
    assert_eq!(saved["games"]["dcs"]["connection"]["freshnessMs"], 2400);
    let mut catalogue = vec![game()];
    decorate(&runtime, &mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "needs-attention");
    assert_eq!(
        runtime.inner.inspect(&game(), None)["state"],
        "needs-attention"
    );
}
#[test]
fn invalid_target_and_unknown_command_do_not_write_scripts() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let platform = Arc::new(FakePlatform(false));
    assert!(runtime
        .request(&json!({"command":"unknown"}), &game(), platform.clone())
        .is_err());
    assert!(runtime
        .request(
            &json!({"command":"game_integration_setup","userFolder":"relative"}),
            &game(),
            platform.clone()
        )
        .is_err());
    runtime
        .request(
            &json!({"command":"game_integration_check","userFolder":"relative"}),
            &game(),
            platform,
        )
        .unwrap();
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 0);
}
#[test]
fn removed_user_folder_can_be_replaced_without_destroying_saved_configuration() {
    let scratch = Scratch::new();
    let missing = scratch.0.join("removed");
    let path = scratch.0.join("setup.json");
    let value = json!({"schemaVersion":1,"games":{"dcs":{"userFolder":missing,"connection":dcs_connection(&missing.join("Scripts/DCS-BIOS/doc/json"))}}});
    write_settings(&path, &value).unwrap();
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    let mut catalogue = vec![game()];
    decorate(&runtime, &mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "setup-required");
    let target = scratch.0.join("DCS");
    fs::create_dir(&target).unwrap();
    crate::dcs_setup::fixture_module(&target);
    runtime
        .request(
            &json!({"command":"game_integration_setup","userFolder":target}),
            &game(),
            Arc::new(FakePlatform(false)),
        )
        .unwrap();
    wait_setup(&runtime);
    assert_eq!(
        read_settings(&path).unwrap()["games"]["dcs"]["userFolder"],
        json!(target)
    );
}

fn decorate(runtime: &GameIntegrationSetup, catalogue: &mut [Value]) {
    runtime.decorate(catalogue);
}
fn acr_game(scratch: &Scratch) -> Value {
    let executable = scratch.0.join("acr.exe");
    fs::write(
        &executable,
        b"Authored executable identity fixture, never launched",
    )
    .unwrap();
    json!({"id":"assetto-corsa-rally","provider":"assetto-corsa-rally","installed":true,"executable":executable})
}
fn acr_record(runtime: &GameIntegrationSetup, game: &Value, qualified: bool) -> Value {
    let identity =
        crate::acr_setup::fingerprint(Path::new(game["executable"].as_str().unwrap())).unwrap();
    let record = json!({"connection":{"source":"shared-memory","freshnessMs":1700},"configuredFingerprint":identity,"verifiedFingerprint":if qualified {identity.clone()} else {Value::Null}});
    runtime.inner.saved.lock().unwrap()["games"]["assetto-corsa-rally"] = record;
    runtime.refresh(&[game.clone()]);
    identity
}
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
fn dcs_setup_rejects_game_installation_descendants_before_download_or_file_changes() {
    let scratch = Scratch::new();
    let installation = scratch.0.join("DCSWorld");
    let target = installation.join("Scripts");
    fs::create_dir_all(&target).unwrap();
    let mut game = game();
    game["installPath"] = json!(installation);
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    assert!(runtime
        .request(
            &json!({"command":"game_integration_setup","userFolder":target}),
            &game,
            Arc::new(FakePlatform(false))
        )
        .unwrap_err()
        .contains("outside the game installation"));
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    assert!(!runtime.busy());
    let sibling = scratch.0.join("DCSWorldUserData");
    fs::create_dir(&sibling).unwrap();
    reject_game_install_folder(&sibling, &game).unwrap();
}

struct DcsStartsDuringSetup {
    queries: std::sync::atomic::AtomicUsize,
}
impl Platform for DcsStartsDuringSetup {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![]
    }
    fn validate(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        panic!("Setup must not launch DCS")
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        let query = self.queries.fetch_add(1, Ordering::AcqRel);
        Ok((query > 0).then(|| ProcessIdentity {
            pid: 42,
            path: "DCS.exe".into(),
            epoch: 1,
        }))
    }
    fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
        Ok(true)
    }
}
#[test]
fn dcs_started_after_async_admission_blocks_worker_before_game_files_change() {
    let scratch = Scratch::new();
    let target = scratch.0.join("DCS");
    fs::create_dir(&target).unwrap();
    crate::dcs_setup::fixture_module(&target);
    let export = target.join("Scripts/Export.lua");
    let original = b"-- Keep the existing third-party export\n";
    fs::write(&export, original).unwrap();
    let runtime_file = target.join("Scripts/DCS-BIOS/BIOS.lua");
    let original_runtime = fs::read(&runtime_file).unwrap();
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    let platform = Arc::new(DcsStartsDuringSetup {
        queries: std::sync::atomic::AtomicUsize::new(0),
    });
    runtime
        .request(
            &json!({"command":"game_integration_setup","userFolder":target}),
            &game(),
            platform.clone(),
        )
        .unwrap();
    wait_setup(&runtime);
    assert_eq!(platform.queries.load(Ordering::Acquire), 2);
    assert_eq!(fs::read(export).unwrap(), original);
    assert_eq!(fs::read(runtime_file).unwrap(), original_runtime);
    assert!(
        !path.exists(),
        "Failed setup must not persist prepared settings"
    );
    assert!(
        !runtime.pending(),
        "No owned changes should remain after restoration"
    );
    let mut catalogue = vec![game()];
    decorate(&runtime, &mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "needs-attention");
    assert!(catalogue[0]["integrationSetup"]["message"]
        .as_str()
        .unwrap()
        .contains("Close DCS"));
    runtime.shutdown();
}

#[test]
fn assetto_setup_preserves_separate_build_evidence_and_requires_native_verification() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    fs::create_dir_all(scratch.0.join("content/cars")).unwrap();
    for (id, filename) in [("assetto-corsa", "acs.exe"), ("assetto-corsa-competizione", "AC2-Win64-Shipping.exe"), ("assetto-corsa-evo", "AssettoCorsaEVO.exe")] {
        let executable = scratch.0.join(filename);
        fs::write(&executable, b"Unlaunched authored executable identity fixture").unwrap();
        let game = json!({"id":id,"provider":id,"installed":true,"executable":executable,"installPath":scratch.0});
        runtime.request(&json!({"command":"game_integration_setup","connection":{"freshnessMs":1800}}), &game, Arc::new(FakePlatform(false))).unwrap();
        wait_setup(&runtime);
        let saved = read_settings(&path).unwrap();
        let record = &saved["games"][id];
        assert_eq!(record["connection"]["freshnessMs"], 1800);
        assert!(record["configuredFingerprint"]["fingerprint"].as_str().unwrap().starts_with("sha256:"));
        assert_eq!(record["verifiedFingerprint"], Value::Null);
        assert!(!record.to_string().contains("processId"));
        assert_eq!(runtime.inner.inspect(&game, None)["verified"], false);
        assert_eq!(fs::read(&executable).unwrap(), b"Unlaunched authored executable identity fixture");
        let identity = record["configuredFingerprint"].clone();
        runtime.inner.accept_assetto_proof(id, &json!({"nativeVerified":true,"fingerprint":identity})).unwrap();
        assert_eq!(runtime.inner.inspect(&game, None)["verified"], true);
        fs::write(&executable, b"Updated authored build").unwrap();
        assert_eq!(runtime.inner.inspect(&game, None)["state"], "needs-attention");
    }
    assert_eq!(read_settings(&path).unwrap()["games"].as_object().unwrap().len(), 3);
}

fn ac_catalogue_game(root: &Path) -> Value {
    fs::create_dir_all(root.join("content/cars")).unwrap();
    let executable = root.join("acs.exe");
    fs::write(&executable, b"Unlaunched AC car catalogue setup fixture").unwrap();
    json!({"id":"assetto-corsa","provider":"assetto-corsa","installed":true,
        "installPath":root,"executable":executable,"currentVehicle":null,
        "vehicleSupport":{"vehicles":[]}})
}

fn ac_catalogue_car(root: &Path, id: &str, name: &str) {
    let car = root.join("content/cars").join(id);
    fs::create_dir_all(car.join("ui")).unwrap();
    fs::create_dir_all(car.join("skins/default")).unwrap();
    fs::write(car.join("ui/ui_car.json"), json!({"name":name}).to_string()).unwrap();
    fs::write(car.join("data.acd"), b"Authored data archive presence fixture").unwrap();
    fs::write(car.join("skins/default/livery.dds"), b"Authored skin presence fixture").unwrap();
}

fn ac_catalogue_choices(runtime: &GameIntegrationSetup, game: &Value) -> Value {
    let mut decorated = game.clone();
    runtime.decorate_installed_cars(&mut decorated);
    assert_eq!(decorated["currentVehicle"], Value::Null, "Installed choices cannot establish a live car");
    decorated["vehicleSupport"]["vehicles"].clone()
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
    runtime.request(&setup, &game, Arc::new(FakePlatform(false))).unwrap();
    wait_setup(&runtime);
    let initial = json!([{"id":"lotus_elise_sc","name":"Lotus Elise SC"},{"id":"old_mod","name":"Old Mod"}]);
    assert_eq!(ac_catalogue_choices(&runtime, &game), initial);
    assert_eq!(runtime.inner.inspect(&game, None)["verified"], false);
    let identity = read_settings(&path).unwrap()["games"]["assetto-corsa"]["configuredFingerprint"].clone();

    fs::remove_file(root.join("content/cars/old_mod/data.acd")).unwrap();
    ac_catalogue_car(root, "lotus_elise_sc", "Renamed Lotus");
    ac_catalogue_car(root, "new_mod", "New Mod");
    runtime.refresh(std::slice::from_ref(&game));
    let mut passive = vec![game.clone()];
    runtime.decorate(&mut passive);
    assert_eq!(ac_catalogue_choices(&runtime, &game), initial);
    runtime.request(&json!({"command":"game_integration_check"}), &game, Arc::new(FakePlatform(false))).unwrap();
    wait_setup(&runtime);
    assert_eq!(ac_catalogue_choices(&runtime, &game), initial, "Check must not scan changed content");

    runtime.request(&setup, &game, Arc::new(FakePlatform(false))).unwrap();
    wait_setup(&runtime);
    let repaired = json!([{"id":"lotus_elise_sc","name":"Renamed Lotus"},{"id":"new_mod","name":"New Mod"}]);
    assert_eq!(ac_catalogue_choices(&runtime, &game), repaired);
    assert_eq!(read_settings(&path).unwrap()["games"]["assetto-corsa"]["configuredFingerprint"], identity,
        "Repair must refresh car content even when the executable identity did not change");
    runtime.shutdown();

    let restored = GameIntegrationSetup::new(path, true);
    assert_eq!(ac_catalogue_choices(&restored, &game), repaired, "Friendly names must survive restart");
    let other = ac_catalogue_game(&scratch.0.join("Other AC"));
    assert_eq!(ac_catalogue_choices(&restored, &other), json!([]), "A different installation cannot inherit the old installed roster");
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
    runtime.request(&json!({"command":"game_integration_setup"}), &game,
        Arc::new(FakePlatform(false))).unwrap();
    observed.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert!(runtime.busy());
    assert_eq!(ac_catalogue_choices(&runtime, &game), json!([]), "An unfinished setup must not publish its scanned choices");
    release.send(()).unwrap();
    wait_setup(&runtime);
    let initial = json!([{"id":"car","name":"Original Car"}]);
    assert_eq!(ac_catalogue_choices(&runtime, &game), initial);

    ac_catalogue_car(root, "new_car", "New Car");
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.native_probe_pause.lock().unwrap() = Some((entered, blocked));
    runtime.request(&json!({"command":"game_integration_setup"}), &game,
        Arc::new(FakePlatform(false))).unwrap();
    observed.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(ac_catalogue_choices(&runtime, &game), initial);
    runtime.request(&json!({"command":"game_integration_cancel"}), &game, Arc::new(FakePlatform(false))).unwrap();
    release.send(()).unwrap();
    wait_setup(&runtime);
    assert_eq!(ac_catalogue_choices(&runtime, &game), initial, "Cancelled repair must retain the last completed catalogue");
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
    runtime.request(&setup, &game, Arc::new(FakePlatform(false))).unwrap();
    wait_setup(&runtime);
    let initial = ac_catalogue_choices(&runtime, &game);
    let persisted = fs::read(&cache_path).unwrap();

    ac_catalogue_car(root, "new_car", "New Car");
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *runtime.inner.ac_cars_publication_pause.lock().unwrap() = Some((entered, blocked));
    runtime.request(&setup, &game, Arc::new(FakePlatform(false))).unwrap();
    // The worker has finished scanning/probing and passed its early cancellation
    // check. Pause only the final publication, then acknowledge a real cancel.
    observed.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert!(runtime.busy());
    runtime.request(&json!({"command":"game_integration_cancel"}), &game,
        Arc::new(FakePlatform(false))).unwrap();
    assert!(runtime.inner.cancel.load(Ordering::Acquire));
    release.send(()).unwrap();
    wait_setup(&runtime);

    assert_eq!(ac_catalogue_choices(&runtime, &game), initial);
    assert_eq!(fs::read(&cache_path).unwrap(), persisted);
    assert!(runtime.inner.states.lock().unwrap()["assetto-corsa"]["message"]
        .as_str().unwrap().contains("cancelled"));
    runtime.shutdown();
}

#[test]
fn other_assetto_setup_providers_never_create_the_original_ac_car_catalogue() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    for (id, filename) in [("assetto-corsa-rally","acr.exe"),("assetto-corsa-competizione","AC2-Win64-Shipping.exe"),("assetto-corsa-evo","AssettoCorsaEVO.exe")] {
        let executable = scratch.0.join(filename);
        fs::write(&executable, b"Unlaunched other-Assetto setup fixture").unwrap();
        let game = json!({"id":id,"provider":id,"installed":true,"executable":executable});
        runtime.request(&json!({"command":"game_integration_setup"}), &game, Arc::new(FakePlatform(false))).unwrap();
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
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"),true);
    acr_record(&runtime,&game,true);
    runtime.validate_native_build(&game,false).unwrap();
    let executable = Path::new(game["executable"].as_str().unwrap());
    fs::write(executable,b"Changed game build").unwrap();
    assert!(runtime.validate_native_build(&game,false).is_err());
}

#[test]
fn passive_refresh_cannot_overwrite_a_newly_admitted_setup_worker() {
    use std::sync::mpsc;
    let scratch = Scratch::new();
    let game = acr_game(&scratch);
    let runtime = Arc::new(GameIntegrationSetup::new(scratch.0.join("setup.json"),true));
    acr_record(&runtime,&game,false);
    // Pause a real passive inspection after it has captured saved settings.
    let fingerprints = runtime.inner.fingerprints.lock().unwrap();
    let refreshed = runtime.clone(); let refreshing_game = game.clone();
    let refresh = std::thread::spawn(move||refreshed.refresh(&[refreshing_game]));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while runtime.admission.try_lock().is_ok() {
        assert!(std::time::Instant::now() < deadline,"Passive refresh did not retain admission through inspection");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let (entered, observed) = mpsc::channel(); let (release, blocked) = mpsc::channel();
    *runtime.inner.native_probe_pause.lock().unwrap() = Some((entered, blocked));
    let platform = Arc::new(FakePlatform(false));
    let (submitted, received) = mpsc::channel();
    let setup = runtime.clone(); let setup_game = game.clone();
    let request = std::thread::spawn(move||submitted.send(setup.request(&json!({"command":"game_integration_setup"}),&setup_game,platform)).unwrap());
    assert!(matches!(received.recv_timeout(std::time::Duration::from_millis(30)),Err(mpsc::RecvTimeoutError::Timeout)));
    assert!(!runtime.busy());
    drop(fingerprints);
    refresh.join().unwrap();
    received.recv_timeout(std::time::Duration::from_secs(2)).unwrap().unwrap();
    request.join().unwrap();
    observed.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    assert!(runtime.busy());
    runtime.refresh(&[game.clone()]);
    let mut catalogue = vec![game]; runtime.decorate(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"],"setting-up");
    release.send(()).unwrap();
    wait_setup(&runtime);
    runtime.shutdown();
}
