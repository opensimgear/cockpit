use super::*;
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
    assert_eq!(
        fs::read_dir(&scratch.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsString::from("cockpit.sqlite3")]
    );
}
#[test]
fn removed_user_folder_can_be_replaced_without_destroying_saved_configuration() {
    let scratch = Scratch::new();
    let missing = scratch.0.join("removed");
    let path = scratch.0.join("setup.json");
    let value = json!({"schemaVersion":1,"games":{"dcs":{"userFolder":missing,"connection":dcs_connection(&missing.join("Scripts/DCS-BIOS/doc/json"))}}});
    fixture_write_settings(&path, &value);
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
    for (id, filename) in [
        ("assetto-corsa", "acs.exe"),
        ("assetto-corsa-competizione", "AC2-Win64-Shipping.exe"),
        ("assetto-corsa-evo", "AssettoCorsaEVO.exe"),
    ] {
        let executable = scratch.0.join(filename);
        fs::write(
            &executable,
            b"Unlaunched authored executable identity fixture",
        )
        .unwrap();
        let game = json!({"id":id,"provider":id,"installed":true,"executable":executable,"installPath":scratch.0});
        runtime
            .request(
                &json!({"command":"game_integration_setup","connection":{"freshnessMs":1800}}),
                &game,
                Arc::new(FakePlatform(false)),
            )
            .unwrap();
        wait_setup(&runtime);
        let saved = read_settings(&path).unwrap();
        let record = &saved["games"][id];
        assert_eq!(record["connection"]["freshnessMs"], 1800);
        assert!(record["configuredFingerprint"]["fingerprint"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert_eq!(record["verifiedFingerprint"], Value::Null);
        assert!(!record.to_string().contains("processId"));
        assert_eq!(runtime.inner.inspect(&game, None)["verified"], false);
        assert_eq!(
            fs::read(&executable).unwrap(),
            b"Unlaunched authored executable identity fixture"
        );
        let identity = record["configuredFingerprint"].clone();
        runtime
            .inner
            .accept_assetto_proof(id, &json!({"nativeVerified":true,"fingerprint":identity}))
            .unwrap();
        assert_eq!(runtime.inner.inspect(&game, None)["verified"], true);
        fs::write(&executable, b"Updated authored build").unwrap();
        assert_eq!(
            runtime.inner.inspect(&game, None)["state"],
            "needs-attention"
        );
    }
    assert_eq!(
        read_settings(&path).unwrap()["games"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
}
