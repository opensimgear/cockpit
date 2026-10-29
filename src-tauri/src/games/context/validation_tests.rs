use super::*;

#[test]
fn process_evidence_is_timestamped_before_native_work() {
    let scratch = Scratch::new();
    let (runtime, platform, _) = runtime(&scratch);
    let prepared = runtime.prepare_profile_game_context("x-plane-12").unwrap().unwrap();
    let (entered, observed) = std::sync::mpsc::channel();
    *platform.observation_hook.lock().unwrap() = Some((std::thread::current().id(), Box::new(move || {
        entered.send(Instant::now()).unwrap();
    })));
    let captured = runtime.profile_validation_task(prepared.proof, None)().unwrap();
    assert!(captured <= observed.recv().unwrap());
    runtime.shutdown();
}

#[test]
fn observation_rechecks_setup_preparation_owner_and_installation_before_publishing() {
    for change in ["setup", "preparation", "owner", "executable", "processExecutables", "installPath", "selectedInstallationId"] {
        let scratch = Scratch::new();
        let (runtime, platform, _) = runtime(&scratch);
        let prepared = runtime.prepare_profile_game_context("x-plane-12").unwrap().unwrap();
        runtime.set_profile_context_owner(Some(owner(Some("x-plane-12")))).unwrap();
        let token = runtime.admit_prepared_game_context(&prepared.proof).unwrap();
        let inner = runtime.inner.clone();
        *platform.observation_hook.lock().unwrap() = Some((std::thread::current().id(), Box::new(move || {
            match change {
                "setup" => {
                    let revision = inner.setup.provider_setup_revision("x-plane-12");
                    let mut connection = crate::xplane::defaults(12);
                    connection["freshnessMs"] = json!(2000);
                    let game = inner.settings.lock().unwrap().catalogue.iter()
                        .find(|game| game["id"] == "x-plane-12").unwrap().clone();
                    inner.setup.request(&json!({"command":"game_integration_settings_save","gameId":"x-plane-12","expectedSetupRevision":revision,"connection":connection}), &game, inner.platform.clone()).unwrap();
                }
                "preparation" => inner.contexts.lock().unwrap().invalidate("x-plane-12", "revoked during observation".into()),
                "owner" => inner.contexts.lock().unwrap().set_owner(None).unwrap(),
                field => {
                    let replacement = if field == "processExecutables" {
                        json!(["/replacement/X-Plane.exe"])
                    } else { json!("/replacement/X-Plane.exe") };
                    inner.settings.lock().unwrap().catalogue.iter_mut()
                        .find(|game| game["id"] == "x-plane-12").unwrap()[field] = replacement;
                }
            }
        })));
        let error = runtime.profile_validation_task(prepared.proof, Some(token))().unwrap_err();
        assert!(!error.is_empty(), "{change} changed during observation");
        runtime.shutdown();
    }
}

#[test]
fn library_presentation_updates_do_not_revoke_process_evidence() {
    let scratch = Scratch::new();
    let (runtime, platform, _) = runtime(&scratch);
    let prepared = runtime.prepare_profile_game_context("x-plane-12").unwrap().unwrap();
    runtime.set_profile_context_owner(Some(owner(Some("x-plane-12")))).unwrap();
    let token = runtime.admit_prepared_game_context(&prepared.proof).unwrap();
    let inner = runtime.inner.clone();
    *platform.observation_hook.lock().unwrap() = Some((std::thread::current().id(), Box::new(move || {
        let mut settings = inner.settings.lock().unwrap();
        let game = settings.catalogue.iter_mut().find(|game| game["id"] == "x-plane-12").unwrap();
        game["name"] = json!("A new library name");
        game["background"] = json!({"url":"https://example.test/artwork.png"});
        game["defaultBackground"] = json!({"url":"https://example.test/default.png"});
        game["installations"] = json!([{"id":"unselected-installation","name":"New metadata"}]);
    })));
    runtime.profile_validation_task(prepared.proof, Some(token))().unwrap();
    runtime.shutdown();
}
