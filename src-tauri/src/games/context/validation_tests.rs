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
    let captured = runtime.profile_validation_task(prepared.proof, None)().unwrap().unwrap();
    assert!(captured <= observed.recv().unwrap());
    runtime.shutdown();
}

#[test]
fn active_telemetry_reuses_only_live_inventory_and_commands_still_check_exactly() {
    let scratch = Scratch::new();
    let (runtime, platform, _) = runtime(&scratch);
    let prepared = runtime.prepare_profile_game_context("x-plane-12").unwrap().unwrap();
    runtime.set_profile_context_owner(Some(owner(Some("x-plane-12")))).unwrap();
    let token = runtime.admit_prepared_game_context(&prepared.proof).unwrap();
    let inner = &runtime.inner;
    let game = runtime.registered_profile_game("x-plane-12").unwrap();
    let continuity = Arc::new(crate::game_activation::ProcessContinuity::default());
    assert!(continuity.begin());
    inner.changes.set_context_process_scope(inner.changes.context_wake.observed(), Some(continuity.clone()));
    let epoch = inner.current_process_epoch().unwrap();
    let mut cache = process_cache::Cache::default();
    assert_eq!(cache.read(platform.as_ref(), &[&game], Some(epoch))[0].as_ref().unwrap().as_ref(), Some(&prepared.proof.process));
    *inner.context_process_inventory.lock().unwrap() = cache.snapshot();

    let (strict, called) = std::sync::mpsc::channel();
    *platform.observation_hook.lock().unwrap() = Some((std::thread::current().id(), Box::new(move || strict.send(()).unwrap())));
    let mut validate = runtime.profile_validation_task(prepared.proof.clone(), Some(token.clone()));
    validate().unwrap();
    assert!(called.try_recv().is_err(), "Active telemetry should use the exact watched inventory row");
    runtime.validate_admitted_game_context(&token, &prepared.proof).unwrap();
    called.try_recv().expect("Command authorization must independently check discovery");

    let (fallback, called) = std::sync::mpsc::channel();
    *platform.observation_hook.lock().unwrap() = Some((std::thread::current().id(), Box::new(move || fallback.send(()).unwrap())));
    continuity.retire();
    validate().unwrap();
    called.try_recv().expect("A retired watch cannot authorize cached lookup");
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
    assert!(runtime.profile_validation_task(prepared.proof, Some(token))().unwrap().is_some());
    runtime.shutdown();
}

#[test]
fn unavailable_lookup_withholds_proof_and_commands_then_recovers_the_same_context() {
    let scratch = Scratch::new();
    let (runtime, platform, _) = runtime(&scratch);
    let prepared = runtime.prepare_profile_game_context("x-plane-12").unwrap().unwrap();
    runtime.set_profile_context_owner(Some(owner(Some("x-plane-12")))).unwrap();
    let token = runtime.admit_prepared_game_context(&prepared.proof).unwrap();
    let mut validate = runtime.profile_validation_task(prepared.proof.clone(), Some(token.clone()));
    assert!(validate().unwrap().is_some());

    platform.failed.store(true, Ordering::Release);
    assert!(matches!(runtime.validate_admitted_game_context(&token, &prepared.proof),
        Err(ValidationFailure::ProcessUnavailable(_))));
    runtime.inner.refresh_game_contexts(false);
    assert!(validate().unwrap().is_none());
    {
        let contexts = runtime.inner.contexts.lock().unwrap();
        assert_eq!(contexts.token("x-plane-12").unwrap(), token);
        assert_eq!(contexts.view("x-plane-12")["processVerified"], false);
    }

    platform.failed.store(false, Ordering::Release);
    runtime.inner.refresh_game_contexts(false);
    assert!(validate().unwrap().is_some());
    assert_eq!(runtime.profile_game_context("x-plane-12").unwrap(), token);

    runtime.inner.refresh_game_contexts_with_inventory(false, |games| {
        games.iter().map(|_| Err(ProcessLookupError::Ambiguous("Competing processes".into()))).collect()
    });
    assert!(validate().is_err());
    assert!(runtime.inner.contexts.lock().unwrap().token("x-plane-12").is_err());
    runtime.shutdown();
}

#[test]
fn unavailable_command_lookup_does_not_mask_owner_retirement_during_native_io() {
    let scratch = Scratch::new();
    let (runtime, platform, _) = runtime(&scratch);
    let prepared = runtime.prepare_profile_game_context("x-plane-12").unwrap().unwrap();
    runtime.set_profile_context_owner(Some(owner(Some("x-plane-12")))).unwrap();
    let token = runtime.admit_prepared_game_context(&prepared.proof).unwrap();
    platform.failed.store(true, Ordering::Release);
    let inner = runtime.inner.clone();
    *platform.observation_hook.lock().unwrap() = Some((std::thread::current().id(), Box::new(move || {
        inner.contexts.lock().unwrap().set_owner(None).unwrap();
    })));
    assert!(matches!(runtime.validate_admitted_game_context(&token, &prepared.proof),
        Err(ValidationFailure::Invalid(_))));
    runtime.shutdown();
}
