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
    let captured = runtime.profile_validation_task(prepared.proof, None, None)().unwrap().unwrap();
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
    let mut validate = runtime.profile_validation_task(prepared.proof.clone(), Some(token.clone()), None);
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

#[cfg(windows)]
#[test]
fn exact_exit_watch_reuses_liveness_only_with_a_current_unique_inventory_row() {
    let scratch = Scratch::new();
    let (runtime, platform, _) = runtime(&scratch);
    runtime.stop_test_context_observer();
    let inner = &runtime.inner;
    let game = runtime.registered_profile_game("x-plane-12").unwrap();
    let process = ExitWatch::current_process_fixture();
    let watch = ExitWatch::open(&process).unwrap();
    let evidence = watch.evidence();
    *platform.process.lock().unwrap() = Some(process.clone());
    let continuity = Arc::new(crate::game_activation::ProcessContinuity::default());
    assert!(continuity.begin());
    inner.changes.set_context_process_scope(inner.changes.context_wake.observed(), Some(continuity.clone()));
    let epoch = inner.current_process_epoch().unwrap();
    let publish = |row| {
        let mut cache = process_cache::Cache::default();
        cache.read_using(&[&game], Some(epoch), |_| vec![row]);
        *inner.context_process_inventory.lock().unwrap() = cache.snapshot();
    };
    publish(Ok(Some(process.clone())));

    let before = platform.reads.load(Ordering::SeqCst);
    assert!(inner.observe_profile_process(&game, &process, Some(&evidence)).unwrap());
    assert_eq!(platform.reads.load(Ordering::SeqCst), before);

    let other = ProcessIdentity { epoch: process.epoch ^ 1, ..process.clone() };
    publish(Ok(Some(other.clone())));
    assert!(!inner.observe_profile_process(&game, &other, Some(&evidence)).unwrap());
    assert_eq!(platform.reads.load(Ordering::SeqCst), before + 1);

    publish(Err(ProcessLookupError::Ambiguous("Competing processes".into())));
    assert!(matches!(inner.observe_profile_process(&game, &process, Some(&evidence)),
        Err(ProcessLookupError::Ambiguous(_))));

    publish(Ok(Some(process.clone())));
    drop(watch);
    assert!(inner.observe_profile_process(&game, &process, Some(&evidence)).unwrap());
    assert_eq!(platform.reads.load(Ordering::SeqCst), before + 2);

    let replacement = ExitWatch::open(&process).unwrap();
    continuity.retire();
    assert!(inner.observe_profile_process(&game, &process, Some(&replacement.evidence())).unwrap());
    assert_eq!(platform.reads.load(Ordering::SeqCst), before + 4);
    runtime.shutdown();
}

#[cfg(windows)]
#[test]
fn shared_profile_process_does_not_rescan_or_extend_fallback_evidence() {
    use crate::game_discovery::{BackgroundProcesses, ACTIVE_PROCESS_PROOF_AGE, PROCESS_DISCOVERY_PERIOD};
    use crate::games::activation_runtime::BackgroundScan;
    let scratch = Scratch::new();
    let (runtime, platform, _) = runtime(&scratch);
    runtime.stop_test_context_observer();
    let inner = &runtime.inner;
    let game = runtime.registered_profile_game("x-plane-12").unwrap();
    let process = ExitWatch::current_process_fixture();
    let watch = ExitWatch::open(&process).unwrap();
    let evidence = watch.evidence();
    *platform.process.lock().unwrap() = Some(process.clone());
    let publish = |started, event_owned, row| {
        let epoch = (inner.changes.context_wake.observed(), inner.changes.context_process_wake.observed());
        *inner.background_processes.lock().unwrap() = Some(BackgroundScan::new(
            epoch, started, event_owned, BackgroundProcesses::fixture(&game, row),
        ));
    };
    let observe = || inner.observe_shared_profile_process(&game, &process, Some(&evidence));
    let before = platform.reads.load(Ordering::SeqCst);
    let started = Instant::now();
    publish(started, false, Ok(Some(process.clone())));
    assert_eq!(observe(), Ok(true));
    assert_eq!(inner.profile_background_process(&game).unwrap().1, started);
    assert_eq!(observe(), Ok(true));
    assert_eq!(inner.profile_background_process(&game).unwrap().1, started);
    publish(Instant::now() - (PROCESS_DISCOVERY_PERIOD * 2 - ACTIVE_PROCESS_PROOF_AGE / 2),
        false, Ok(Some(process.clone())));
    assert!(matches!(observe(), Err(ProcessLookupError::Unavailable(_))));

    publish(Instant::now(), false, Ok(None));
    assert_eq!(observe(), Ok(false));
    publish(Instant::now(), false, Ok(Some(ProcessIdentity { epoch: process.epoch ^ 1, ..process.clone() })));
    assert_eq!(observe(), Ok(false));
    publish(Instant::now(), false, Err(ProcessLookupError::Ambiguous("competing games".into())));
    assert!(matches!(observe(), Err(ProcessLookupError::Ambiguous(_))));
    publish(Instant::now(), false, Err(ProcessLookupError::Unavailable("scan failed".into())));
    assert!(matches!(observe(), Err(ProcessLookupError::Unavailable(_))));
    publish(Instant::now(), true, Ok(Some(process.clone())));
    assert!(matches!(observe(), Err(ProcessLookupError::Unavailable(_))));
    publish(Instant::now(), false, Ok(Some(process.clone())));
    inner.changes.context_process_wake.notify();
    assert!(matches!(observe(), Err(ProcessLookupError::Unavailable(_))));
    assert_eq!(platform.reads.load(Ordering::SeqCst), before);
    assert_eq!(inner.exact_process_current(&game, &process), Ok(true));
    assert!(platform.reads.load(Ordering::SeqCst) > before);
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
        let error = runtime.profile_validation_task(prepared.proof, Some(token), None)().unwrap_err();
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
    assert!(runtime.profile_validation_task(prepared.proof, Some(token), None)().unwrap().is_some());
    runtime.shutdown();
}

#[test]
fn unavailable_lookup_withholds_proof_and_commands_then_recovers_the_same_context() {
    let scratch = Scratch::new();
    let (runtime, platform, _) = runtime(&scratch);
    let prepared = runtime.prepare_profile_game_context("x-plane-12").unwrap().unwrap();
    runtime.set_profile_context_owner(Some(owner(Some("x-plane-12")))).unwrap();
    let token = runtime.admit_prepared_game_context(&prepared.proof).unwrap();
    let mut validate = runtime.profile_validation_task(prepared.proof.clone(), Some(token.clone()), None);
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

    runtime.inner.refresh_game_contexts_with_inventory(SetupRefresh::Cached, |games| {
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

#[test]
fn inventory_superseded_during_capture_cannot_reach_context_or_profile_readers() {
    for change in ["process", "library", "continuity"] {
        let scratch = Scratch::new();
        let (runtime, _, _) = runtime(&scratch);
        runtime.stop_test_context_observer();
        let inner = &runtime.inner;
        let game = runtime.registered_profile_game("x-plane-12").unwrap();
        let library = inner.changes.context_wake.observed();
        let continuity = Arc::new(crate::game_activation::ProcessContinuity::default());
        assert!(continuity.begin());
        inner.changes.set_context_process_scope(library, Some(continuity.clone()));
        let epoch = inner.current_process_epoch().unwrap();
        let mut cache = process_cache::Cache::default();
        let rows = inner.read_context_inventory(&mut cache, epoch, &[&game], |_| {
            match change {
                "process" => inner.changes.context_process_wake.notify(),
                "library" => inner.changes.context_wake.notify(),
                "continuity" => continuity.retire(),
                _ => unreachable!(),
            }
            vec![Ok(None)]
        });
        assert!(matches!(&rows[..], [Err(crate::game_discovery::ProcessLookupError::Unavailable(error))]
            if error == "Game process observation was superseded."));
        assert!(cache.snapshot().is_none());
        assert!(inner.context_process_inventory.lock().unwrap().is_none());
        runtime.shutdown();
    }
}
