use super::*;
use crate::game_discovery::ProcessIdentity;
#[test]
fn xplane_setup_is_repeatable_preserves_overrides_and_separates_live_status() {
    let scratch = Scratch::new();
    let path = scratch.0.join("setup.json");
    let setup = GameIntegrationSetup::new(path.clone(), true);
    let game = json!({"id":"x-plane-11","provider":"x-plane","installed":true,"executable":"/fixture/X-Plane.exe"});
    assert_eq!(setup.inner.inspect(&game, None)["state"], "setup-required");
    let request = json!({"command":"game_integration_setup","connection":{"port":49005,"updateHz":5,"freshnessMs":2000}});
    setup
        .request(
            &request,
            &game,
            Arc::new(FakePlatform(false)),
            &panel(false),
        )
        .unwrap();
    let before = read_settings(&path).unwrap();
    assert_eq!(before["games"]["x-plane-11"]["connection"]["port"], 49005);
    setup
        .request(
            &json!({"command":"game_integration_setup"}),
            &game,
            Arc::new(FakePlatform(false)),
            &panel(false),
        )
        .unwrap();
    assert_eq!(read_settings(&path).unwrap(), before);
    let mut catalogue = vec![game.clone()];
    setup.decorate(&mut catalogue, &json!({}), &json!({}));
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "ready");
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "waiting");
    assert_eq!(catalogue[0]["integrationSetup"]["verified"], false);
    setup.decorate(&mut catalogue,&json!({"running":true,"config":{"document":{"provider":"x-plane"},"connection":{"version":11}},"telemetry":{"fresh":true}}),&json!({}));
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "receiving");
    let mut changed = game.clone();
    changed["executable"] = json!("/another/X-Plane.exe");
    assert_eq!(
        setup.inner.inspect(&changed, None)["state"],
        "setup-required"
    );
    let profile = json!({"gameId":"x-plane-11","integration":{"connection":before["games"]["x-plane-11"]["connection"]}});
    assert!(setup.preflight(&profile, &game).is_ok());
    assert!(setup.preflight(&profile, &changed).is_err());
    for connection in [
        json!({"host":"192.168.0.5"}),
        json!({"version":12}),
        json!({"port":0}),
    ] {
        assert!(setup
            .request(
                &json!({"command":"game_integration_setup","connection":connection}),
                &game,
                Arc::new(FakePlatform(false)),
                &panel(false)
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
fn panel(running: bool) -> Panel {
    Arc::new(
        move |_| json!({"ok":true,"state":{"integration":{"running":running,"config":{"document":{"provider":"dcs-bios"}},"feedbackFresh":running},"dcsBios":{"running":false}}}),
    )
}
fn game() -> Value {
    json!({"id":"dcs","provider":"dcs-bios","installed":true})
}
#[test]
fn setup_is_separate_from_game_availability_and_live_telemetry() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), false);
    let mut catalogue = vec![game()];
    decorate(&runtime, &mut catalogue, &panel(false));
    assert_eq!(
        catalogue[0]["integrationSetup"]["state"],
        "unsupported-platform"
    );
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "waiting");
    decorate(&runtime, &mut catalogue, &panel(true));
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "receiving");
    assert!(runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &game(),
            Arc::new(FakePlatform(false)),
            &panel(false)
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
    decorate(&runtime, &mut catalogue, &panel(false));
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "needs-attention");
    assert!(runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &game(),
            Arc::new(FakePlatform(false)),
            &panel(false)
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
            &panel(false),
        )
        .unwrap();
    let mut catalogue = vec![missing.clone()];
    decorate(&runtime, &mut catalogue, &panel(false));
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "not-installed");
    assert!(runtime
        .request(
            &json!({"command":"game_integration_setup"}),
            &missing,
            Arc::new(FakePlatform(false)),
            &panel(false)
        )
        .is_err());
    runtime
        .request(
            &json!({"command":"game_integration_cancel"}),
            &missing,
            Arc::new(FakePlatform(false)),
            &panel(false),
        )
        .unwrap();
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 0);
}
#[test]
fn active_integration_or_running_dcs_blocks_installation_before_files_change() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let request = json!({"command":"game_integration_setup","userFolder":scratch.0});
    assert!(runtime
        .request(
            &request,
            &game(),
            Arc::new(FakePlatform(false)),
            &panel(true)
        )
        .unwrap_err()
        .contains("Stop"));
    assert!(runtime
        .request(
            &request,
            &game(),
            Arc::new(FakePlatform(true)),
            &panel(false)
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
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while runtime.busy() {
        assert!(std::time::Instant::now() < end, "Setup did not finish");
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
        .request(
            &request,
            &game(),
            Arc::new(FakePlatform(false)),
            &panel(false),
        )
        .unwrap();
    wait_setup(&runtime);
    let mut catalogue = vec![game()];
    decorate(&runtime, &mut catalogue, &panel(false));
    assert_eq!(
        catalogue[0]["integrationSetup"]["state"], "ready",
        "{}",
        catalogue[0]
    );
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "waiting");
    let before = fs::read(target.join("Scripts/Export.lua")).unwrap();
    runtime
        .request(
            &request,
            &game(),
            Arc::new(FakePlatform(false)),
            &panel(false),
        )
        .unwrap();
    wait_setup(&runtime);
    assert_eq!(fs::read(target.join("Scripts/Export.lua")).unwrap(), before);
    let saved = read_settings(&path).unwrap();
    assert_eq!(
        saved["games"]["dcs"]["connection"]["cataloguePath"],
        json!(target.join("Scripts/DCS-BIOS/doc/json"))
    );
    let profile =
        json!({"gameId":"dcs","integration":{"connection":saved["games"]["dcs"]["connection"]}});
    runtime.preflight(&profile, &game()).unwrap();
    runtime.shutdown();
    let restarted = GameIntegrationSetup::new(path, true);
    decorate(&restarted, &mut catalogue, &panel(false));
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "ready");
    fs::write(
        target.join("Scripts/Export.lua"),
        b"-- loader was removed externally\n",
    )
    .unwrap();
    assert!(restarted.preflight(&profile, &game()).is_err());
    restarted.refresh(&[game()]);
    decorate(&restarted, &mut catalogue, &panel(false));
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "setup-required");
}
#[test]
fn incompatible_udp_overrides_are_preserved_and_never_reported_ready() {
    let scratch = Scratch::new();
    let target = scratch.0.join("DCS");
    fs::create_dir(&target).unwrap();
    crate::dcs_setup::fixture_module(&target);
    let path = scratch.0.join("setup.json");
    let runtime = GameIntegrationSetup::new(path.clone(), true);
    runtime.request(&json!({"command":"game_integration_setup","userFolder":target,"connection":{"commandPort":9000,"freshnessMs":2400}}),&game(),Arc::new(FakePlatform(false)),&panel(false)).unwrap();
    wait_setup(&runtime);
    let saved = read_settings(&path).unwrap();
    assert_eq!(saved["games"]["dcs"]["connection"]["commandPort"], 9000);
    assert_eq!(saved["games"]["dcs"]["connection"]["freshnessMs"], 2400);
    let mut catalogue = vec![game()];
    decorate(&runtime, &mut catalogue, &panel(false));
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "needs-attention");
    assert!(runtime.preflight(&json!({"gameId":"dcs","integration":{"connection":saved["games"]["dcs"]["connection"]}}), &game()).is_err());
}
#[test]
fn invalid_target_and_unknown_command_do_not_write_scripts() {
    let scratch = Scratch::new();
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let platform = Arc::new(FakePlatform(false));
    let panel = panel(false);
    assert!(runtime
        .request(
            &json!({"command":"unknown"}),
            &game(),
            platform.clone(),
            &panel
        )
        .is_err());
    assert!(runtime
        .request(
            &json!({"command":"game_integration_setup","userFolder":"relative"}),
            &game(),
            platform.clone(),
            &panel
        )
        .is_err());
    runtime
        .request(
            &json!({"command":"game_integration_check","userFolder":"relative"}),
            &game(),
            platform,
            &panel,
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
    decorate(&runtime, &mut catalogue, &panel(false));
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "setup-required");
    let target = scratch.0.join("DCS");
    fs::create_dir(&target).unwrap();
    crate::dcs_setup::fixture_module(&target);
    runtime
        .request(
            &json!({"command":"game_integration_setup","userFolder":target}),
            &game(),
            Arc::new(FakePlatform(false)),
            &panel(false),
        )
        .unwrap();
    wait_setup(&runtime);
    assert_eq!(
        read_settings(&path).unwrap()["games"]["dcs"]["userFolder"],
        json!(target)
    );
}

fn decorate(runtime: &GameIntegrationSetup, catalogue: &mut [Value], panel: &Panel) {
    let response = panel(json!({"command":"integration_get_state"}));
    runtime.decorate(
        catalogue,
        &response["state"]["integration"],
        &response["state"]["dcsBios"],
    );
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
            &panel(false),
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
    decorate(&runtime, &mut catalogue, &panel(false));
    assert_eq!(catalogue[0]["integrationSetup"]["verified"], false);
}
#[test]
fn acr_exact_build_preflight_allows_first_launch_and_requires_setup_after_update() {
    let scratch = Scratch::new();
    let game = acr_game(&scratch);
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    let profile = json!({"gameId":"assetto-corsa-rally","integration":{"connection":{"source":"shared-memory","freshnessMs":1700}}});
    assert!(runtime.preflight(&profile, &game).is_err());
    acr_record(&runtime, &game, false);
    runtime.preflight(&profile, &game).unwrap();
    let mut catalogue = vec![game.clone()];
    decorate(&runtime, &mut catalogue, &panel(false));
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "needs-attention");
    fs::write(
        game["executable"].as_str().unwrap(),
        b"Updated authored build",
    )
    .unwrap();
    assert!(runtime.preflight(&profile, &game).is_err());
    runtime.refresh(&[game.clone()]);
    decorate(&runtime, &mut catalogue, &panel(false));
    assert!(catalogue[0]["integrationSetup"]["message"]
        .as_str()
        .unwrap()
        .contains("changed"));
    runtime.preflight(&json!({"gameId":"assetto-corsa-rally","integration":{"connection":{"source":"synthetic"}}}),&game).unwrap();
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
    decorate(&restarted, &mut catalogue, &panel(false));
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "needs-attention");
    assert_eq!(catalogue[0]["integrationSetup"]["verified"], false);
    assert_eq!(
        catalogue[0]["integrationSetup"]["message"],
        "Unsupported ACR layout"
    );
    assert!(restarted.preflight(&json!({"gameId":"assetto-corsa-rally","integration":{"connection":{"source":"shared-memory"}}}),&game).is_err());
}
#[test]
fn synthetic_data_cannot_verify_native_setup_and_shutdown_cannot_admit_another_probe() {
    let scratch = Scratch::new();
    let game = acr_game(&scratch);
    let runtime = GameIntegrationSetup::new(scratch.0.join("setup.json"), true);
    acr_record(&runtime, &game, false);
    let synthetic = json!({"running":true,"feedbackFresh":true,"config":{"document":{"provider":"assetto-corsa-rally"},"connection":{"source":"synthetic"}}});
    runtime.auto_check(&[game.clone()], Arc::new(FakePlatform(false)), &synthetic);
    assert!(!runtime.busy());
    let mut catalogue = vec![game.clone()];
    runtime.decorate(&mut catalogue, &synthetic, &Value::Null);
    assert_eq!(catalogue[0]["integrationSetup"]["live"], "waiting");
    assert_eq!(catalogue[0]["integrationSetup"]["verified"], false);
    runtime.shutdown();
    let mut native = synthetic;
    native["config"]["connection"]["source"] = json!("shared-memory");
    runtime.auto_check(&[game.clone()], Arc::new(FakePlatform(false)), &native);
    assert!(!runtime.busy());
    assert!(runtime
        .request(
            &json!({"command":"game_integration_check"}),
            &game,
            Arc::new(FakePlatform(false)),
            &panel(false)
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
            Arc::new(FakePlatform(false)),
            &panel(false)
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
            &panel(false),
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
    decorate(&runtime, &mut catalogue, &panel(false));
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "needs-attention");
    assert!(catalogue[0]["integrationSetup"]["message"]
        .as_str()
        .unwrap()
        .contains("Close DCS"));
    runtime.shutdown();
}
