#[test]
fn late_old_stage_failure_preserves_the_admitted_replacement_process_and_owner() {
    let scratch = Scratch::new();
    let (runtime, platform, _) = runtime(&scratch);
    let old = runtime
        .prepare_profile_game_context("x-plane-12")
        .unwrap()
        .unwrap();
    runtime
        .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
        .unwrap();
    platform.process.lock().unwrap().as_mut().unwrap().epoch += 1;
    let current = runtime
        .prepare_profile_game_context("x-plane-12")
        .unwrap()
        .unwrap();
    let mut next = owner(Some("x-plane-12"));
    next.run_generation += 1;
    runtime
        .set_profile_context_owner(Some(next.clone()))
        .unwrap();
    let token = runtime.admit_prepared_game_context(&current.proof).unwrap();
    assert_eq!(token.owner, next);
    assert!(runtime.validate_game_preparation(&old.proof).is_err());
    runtime
        .validate_admitted_game_context(&token, &current.proof)
        .unwrap();
    assert_eq!(runtime.profile_game_context("x-plane-12").unwrap(), token);
    *platform.process.lock().unwrap() = None;
    assert!(runtime.validate_game_preparation(&current.proof).is_err());
    assert!(runtime
        .inner
        .contexts
        .lock()
        .unwrap()
        .token("x-plane-12")
        .is_err());
    runtime.shutdown();
}
#[test]
fn process_replacement_and_exit_withdraw_context_without_retired_provider_calls() {
    let scratch = Scratch::new();
    let (runtime, platform, calls) = runtime(&scratch);
    runtime
        .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
        .unwrap();
    let token = runtime.profile_game_context("x-plane-12").unwrap();
    platform.process.lock().unwrap().as_mut().unwrap().epoch += 1;
    assert!(runtime.validate_profile_game_context(&token).is_err());
    let replacement = runtime.profile_game_context("x-plane-12").unwrap();
    assert_ne!(replacement.session, token.session);
    *platform.process.lock().unwrap() = None;
    assert!(runtime.validate_profile_game_context(&replacement).is_err());
    assert!(calls.lock().unwrap().is_empty());
    runtime.shutdown();
}
#[test]
fn settings_save_synchronously_revokes_context_without_background_or_legacy_state() {
    let scratch = Scratch::new();
    let (runtime, _, calls) = runtime(&scratch);
    runtime
        .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
        .unwrap();
    let token = runtime.profile_game_context("x-plane-12").unwrap();
    let mut connection = crate::xplane::defaults(12);
    connection["freshnessMs"] = json!(2000);
    runtime.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-12","expectedSetupRevision":token.provider_setup_revision,"connection":connection})).unwrap();
    assert!(runtime
        .inner
        .contexts
        .lock()
        .unwrap()
        .validate(&token)
        .is_err());
    assert_ne!(
        runtime
            .profile_game_context("x-plane-12")
            .unwrap()
            .provider_setup_revision,
        token.provider_setup_revision
    );
    assert!(calls.lock().unwrap().is_empty());
    runtime.shutdown();
}
#[test]
fn inspection_failure_and_pause_revoke_authority_without_legacy_output_stop() {
    let scratch = Scratch::new();
    let (runtime, platform, calls) = runtime(&scratch);
    runtime
        .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
        .unwrap();
    let token = runtime.profile_game_context("x-plane-12").unwrap();
    platform.failed.store(true, Ordering::Release);
    assert!(runtime.validate_profile_game_context(&token).is_err());
    platform.failed.store(false, Ordering::Release);
    let fresh = runtime.profile_game_context("x-plane-12").unwrap();
    runtime.set_profile_context_owner(None).unwrap();
    assert!(runtime.validate_profile_game_context(&fresh).is_err());
    assert!(calls.lock().unwrap().is_empty());
    runtime.shutdown();
}
struct DcsPlatform;
impl Platform for DcsPlatform {
    fn integration_setup_supported(&self) -> bool {
        true
    }
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        vec![
            json!({"id":"dcs","name":"DCS World","provider":"dcs-bios","integrationCompatible":true,"integrationKey":"dcs","installed":true,"executable":"/fixture/DCS.exe","launchSupported":true,"processMonitoringSupported":true}),
        ]
    }
    fn validate(&self, id: &str, path: &str) -> Result<(), String> {
        if id == "dcs" && path == "/fixture/DCS.exe" {
            Ok(())
        } else {
            Err("Wrong DCS executable".into())
        }
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        Err("Unexpected launch".into())
    }
    fn find(&self, _: &Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(Some(ProcessIdentity {
            pid: 42,
            path: "/fixture/DCS.exe".into(),
            epoch: 10,
        }))
    }
    fn alive(&self, identity: &ProcessIdentity) -> Result<bool, String> {
        Ok(self.find(&json!({}))?.as_ref() == Some(identity))
    }
}
#[test]
fn loaded_dcs_aircraft_selects_module_without_saved_selection_and_editor_reads_do_not_revoke_it() {
    let scratch = Scratch::new();
    let target = scratch.path.join("DCS");
    std::fs::create_dir(&target).unwrap();
    crate::dcs_setup::fixture_module(&target);
    std::fs::write(
        target.join("Scripts/Export.lua"),
        b"dofile(lfs.writedir() .. [[Scripts\\DCS-BIOS\\BIOS.lua]])\n",
    )
    .unwrap();
    crate::dcs_setup::fixture_exports(&target);
    let catalogue_path = target.join("Scripts/DCS-BIOS/doc/json");
    std::fs::copy(
        catalogue_path.join("A-10C.json"),
        catalogue_path.join("F-16C_50.json"),
    )
    .unwrap();
    let aliases_path = catalogue_path.join("AircraftAliases.json");
    let mut aliases =
        crate::dcs_bios::read_json(&aliases_path, crate::dcs_bios::MAX_DOCUMENT).unwrap();
    aliases["F-16C_50"] = json!(["F-16C_50"]);
    aliases["ViperAlias"] = json!(["F-16C_50"]);
    std::fs::write(&aliases_path, serde_json::to_vec(&aliases).unwrap()).unwrap();
    let paths = Paths::in_directory(&scratch.path);
    let settings_path = paths.provider_setup.clone();
    let saved = json!({"schemaVersion":1,"games":{"dcs":{"userFolder":target,"connection":crate::game_integration_setup::dcs_connection(&catalogue_path),"aircraftModule":"A-10C"}}});
    std::fs::write(&settings_path, serde_json::to_vec(&saved).unwrap()).unwrap();
    crate::persistence::test_support::record(
        &settings_path,
        "games",
        "setup",
        &serde_json::to_vec(&saved).unwrap(),
    );
    let before = std::fs::read(&settings_path).unwrap();
    let runtime = fixture_runtime(
        Arc::new(|request| {
            panic!("DCS catalogue/vehicle introspection must not submit output: {request}")
        }),
        paths,
        Arc::new(DcsPlatform),
    );
    runtime.inner.closed.store(true, Ordering::Release);
    for observer in [&runtime.observer, &runtime.activation_observer] {
        if let Some(observer) = observer.lock().unwrap().take() {
            observer.join().unwrap();
        }
    }
    runtime.inner.closed.store(false, Ordering::Release);
    runtime.set_test_vehicle("dcs", Some("A-10C"));
    let first = runtime
        .prepare_profile_game_context("dcs")
        .unwrap()
        .unwrap();
    assert_eq!(first.aircraft_module.as_deref(), Some("A-10C"));
    runtime
        .set_profile_context_owner(Some(owner(Some("dcs"))))
        .unwrap();
    let token = runtime.admit_prepared_game_context(&first.proof).unwrap();
    let browsed = runtime.resource_dcs_vehicle_catalogue("F-16C_50").unwrap();
    assert_eq!(browsed["aircraftModule"], "F-16C_50");
    runtime
        .validate_admitted_game_context(&token, &first.proof)
        .unwrap();
    assert_eq!(std::fs::read(&settings_path).unwrap(), before);
    runtime.set_test_vehicle("dcs", Some("ViperAlias"));
    assert!(runtime
        .validate_admitted_game_context(&token, &first.proof)
        .is_err());
    let replacement = runtime
        .prepare_profile_game_context("dcs")
        .unwrap()
        .unwrap();
    assert_eq!(replacement.aircraft_module.as_deref(), Some("F-16C_50"));
    assert_eq!(replacement.proof.vehicle_id.as_deref(), Some("ViperAlias"));
    runtime
        .admit_prepared_game_context(&replacement.proof)
        .unwrap();
    runtime.set_test_vehicle("dcs", Some("F-16C_50"));
    assert!(
        runtime
            .validate_game_preparation(&replacement.proof)
            .is_err(),
        "An alias transition must fence the previous flight even when its module is unchanged"
    );
    let current = runtime
        .prepare_profile_game_context("dcs")
        .unwrap()
        .unwrap();
    assert_eq!(current.aircraft_module, replacement.aircraft_module);
    assert_ne!(current.proof.session, replacement.proof.session);
    let display = runtime.resource_catalogue();
    assert_eq!(
        display[0]["integrationSetup"]["activeAircraftModule"],
        "F-16C_50"
    );
    runtime.set_test_vehicle("dcs", None);
    assert!(runtime.validate_game_preparation(&current.proof).is_err());
    let base = runtime
        .prepare_profile_game_context("dcs")
        .unwrap()
        .unwrap();
    assert_eq!(base.aircraft_module.as_deref(), Some("CommonData"));
    assert_eq!(std::fs::read(&settings_path).unwrap(), before);
    runtime.shutdown();
}

fn fixture_runtime(panel: CleanupIo, paths: Paths, platform: Arc<dyn Platform>) -> GamesRuntime {
    let catalogue = platform.catalogue(&BTreeMap::new());
    let runtime = GamesRuntime::test_runtime(panel, paths, platform);
    runtime.inner.settings.lock().unwrap().catalogue = catalogue;
    runtime
}
