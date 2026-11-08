use super::*;

#[test]
fn unchanged_publication_reuses_views_and_summary_omits_large_collections() {
    let root = crate::test_directory();
    let runtime = GameIntegrationSetup::new(root.path().join("setup.json"), false);
    let signal = Arc::new(crate::game_read_models::signals::Signal::default());
    runtime.watch_changes(signal.clone());
    let game = json!({"id":"dcs","provider":"dcs-bios","integrationCompatible":true});
    let record = runtime.inner.provider_snapshot("dcs").0;
    let status = json!({"state":"ready","verified":true,"candidates":["large"],"aircraftExports":["large"],"checkPreview":{"state":"ready"}});
    let context = signal.context_update_wake.observed();
    runtime.inner.publish_setup(&game, &record, status.clone());
    let published = signal.context_update_wake.observed();
    assert_ne!(published, context);
    let before = signal.wake.observed();
    let key = source_scope(&game, "dcs");
    let first = runtime.inner.views.lock().unwrap().entries[&key].1.clone();
    runtime.inner.publish_setup(&game, &record, status);
    assert_eq!(signal.wake.observed(), before);
    assert_eq!(signal.context_update_wake.observed(), published);
    assert!(Arc::ptr_eq(&first, &runtime.inner.views.lock().unwrap().entries[&key].1));
    let summary = runtime.inner.cached_setup_summary(&game, "dcs");
    assert_eq!(summary["verified"], true);
    assert!(summary.get("candidates").is_none());
    assert!(summary.get("aircraftExports").is_none());
    assert!(summary.get("checkPreview").is_none());
}

#[test]
fn old_finish_cannot_end_new_job_and_rejected_cancel_emits_no_cancelling() {
    let root = crate::test_directory();
    let runtime = GameIntegrationSetup::new(root.path().join("setup.json"), false);
    let signal = Arc::new(crate::game_read_models::signals::Signal::default());
    runtime.watch_changes(signal.clone());
    let game = json!({"id":"assetto-corsa"});
    let old = runtime.inner.publish_start(&game, json!({"state":"setting-up"}));
    let run = runtime.inner.publish_start(&game, json!({"state":"setting-up"}));
    runtime.inner.finish_job(&old, &json!({"state":"ready"}));
    assert_eq!(runtime.inner.job.lock().unwrap().as_ref().unwrap().1, run);
    let publication = Arc::new(car_publication::Publication::default());
    publication.begin_commit().unwrap();
    *runtime.inner.ac_cars_publication.lock().unwrap() = Some(publication);
    let before = signal.wake.observed();
    assert!(runtime.inner.cancel().is_err());
    assert_eq!(signal.wake.observed(), before);
    runtime.inner.finish_job(&run, &json!({"state":"ready"}));
    assert!(runtime.inner.job.lock().unwrap().is_none());
}

#[test]
fn releasing_setup_admission_wakes_one_deferred_file_inspection() {
    let root = crate::test_directory();
    let runtime = GameIntegrationSetup::new(root.path().join("setup.json"), false);
    let signal = Arc::new(crate::game_read_models::signals::Signal::default());
    runtime.watch_changes(signal.clone());
    let game = json!({"id":"dcs","provider":"dcs-bios","integrationCompatible":true});
    assert!(runtime.refresh_watched_setup("dcs", &game, || true));
    let admission = runtime.lock_admission();
    let before = signal.context_update_wake.observed();
    assert!(!runtime.refresh_watched_setup("dcs", &game, || true));
    assert!(!runtime.refresh_watched_setup("dcs", &game, || true));
    assert_eq!(signal.context_update_wake.observed(), before);
    drop(admission);
    assert_eq!(signal.context_update_wake.observed(), before + 1);

    assert!(runtime.refresh_watched_setup("dcs", &game, || true));
    let after_retry = signal.context_update_wake.observed();
    drop(runtime.lock_admission());
    assert_eq!(signal.context_update_wake.observed(), after_retry);
}

#[test]
fn library_baseline_defers_a_held_setup_gate_and_wakes_on_release() {
    let root = crate::test_directory();
    let runtime = GameIntegrationSetup::new(root.path().join("setup.json"), false);
    let signal = Arc::new(crate::game_read_models::signals::Signal::default());
    runtime.watch_changes(signal.clone());
    let game = json!({"id":"dcs","provider":"dcs-bios","integrationCompatible":true});
    let admission = runtime.lock_admission();
    let before = signal.context_update_wake.observed();
    assert!(!runtime.refresh_background_scope(std::slice::from_ref(&game), || true));
    assert_eq!(signal.context_update_wake.observed(), before);
    drop(admission);
    assert_eq!(signal.context_update_wake.observed(), before + 1);
    assert!(runtime.refresh_background_scope(&[game], || true));
}

#[test]
fn failed_watch_withdraws_only_the_exact_setup_proof_until_recovered() {
    let root = crate::test_directory();
    let runtime = GameIntegrationSetup::new(root.path().join("setup.json"), false);
    let signal = Arc::new(crate::game_read_models::signals::Signal::default());
    runtime.watch_changes(signal.clone());
    let game = json!({"id":"dcs","provider":"dcs-bios","integrationKey":"dcs","integrationCompatible":true,
        "installed":true,"executable":"first.exe"});
    let mut replacement = game.clone();
    replacement["executable"] = json!("replacement.exe");
    let record = runtime.inner.provider_snapshot("dcs").0;
    let ready = json!({"state":"ready","verified":true,"connection":{"port":7778}});
    runtime.inner.publish_setup(&game, &record, ready.clone());
    runtime.inner.publish_setup(&replacement, &record, ready);
    let revision = runtime.vehicle_setup_revision(&game);
    assert_eq!(revision, runtime.provider_setup_revision("dcs"));
    let before = signal.vehicle_scope_wake.observed();
    runtime.set_setup_watch_unavailable("dcs", &game, true);
    let scope_wake = signal.vehicle_scope_wake.observed();
    assert_eq!(scope_wake, before + 1);
    assert!(runtime.vehicle_setup_revision(&game).is_empty());
    assert_eq!(runtime.vehicle_setup_revision(&replacement), revision);
    let wake = signal.context_update_wake.observed();
    runtime.set_setup_watch_unavailable("dcs", &game, true);
    assert_eq!(signal.context_update_wake.observed(), wake);
    assert_eq!(signal.vehicle_scope_wake.observed(), scope_wake);
    assert_eq!(runtime.inner.cached_source_setup(&game, "dcs")["verified"], false);
    assert_eq!(runtime.inner.cached_source_setup(&replacement, "dcs")["state"], "ready");
    for view in [runtime.inner.cached_setup_summary(&game, "dcs"), runtime.inner.cached_setup(&game, "dcs")] {
        assert_eq!(view["state"], "needs-attention");
        assert_eq!(view["connection"]["port"], 7778);
    }
    runtime.set_setup_watch_unavailable("dcs", &game, false);
    assert_eq!(signal.vehicle_scope_wake.observed(), scope_wake + 1);
    assert_eq!(runtime.vehicle_setup_revision(&game), revision);
    assert_eq!(runtime.inner.cached_source_setup(&game, "dcs")["verified"], true);
}

#[test]
fn background_baselines_are_independent_and_a_retired_scope_cannot_publish() {
    let root = crate::test_directory();
    let runtime = GameIntegrationSetup::new(root.path().join("setup.json"), false);
    let game = |id: &str, provider: &str| json!({
        "id": id, "provider": provider, "integrationKey": id, "integrationCompatible": true,
    });
    let first = game("dcs", "dcs-bios");
    let sibling = game("x-plane-12", "x-plane");
    let current = AtomicBool::new(true);
    let (entered, entered_rx) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    *runtime.inner.inspection_pause.lock().unwrap() = Some((entered, release_rx));

    std::thread::scope(|threads| {
        let first_result = threads.spawn(|| runtime.refresh_background_scope(
            std::slice::from_ref(&first), || current.load(Ordering::Acquire)));
        let blocked = entered_rx.recv_timeout(std::time::Duration::from_secs(2)).is_ok();
        let sibling_published = runtime.refresh_background_scope(std::slice::from_ref(&sibling), || true);
        current.store(false, Ordering::Release);
        let _ = release.send(());
        assert!(blocked && sibling_published);
        assert!(!first_result.join().unwrap());
    });
    let views = runtime.inner.views.lock().unwrap();
    assert!(!views.source.contains_key(&source_scope(&first, "dcs")));
    assert!(views.source.contains_key(&source_scope(&sibling, "x-plane-12")));
}

#[test]
fn a_setup_job_withholds_only_its_provider_readiness() {
    let root = crate::test_directory();
    let runtime = GameIntegrationSetup::new(root.path().join("setup.json"), false);
    let game = |id: &str, provider: &str| json!({
        "id": id, "provider": provider, "integrationKey": id, "integrationCompatible": true,
    });
    let dcs = game("dcs", "dcs-bios");
    let mut alternate = dcs.clone();
    alternate["installPath"] = json!("alternate");
    let rally = game("assetto-corsa-rally", "assetto-corsa-rally");
    let ready = json!({"state":"ready","verified":true});
    for game in [&dcs, &alternate, &rally] {
        let record = runtime.inner.provider_snapshot(game["id"].as_str().unwrap()).0;
        runtime.inner.publish_setup(game, &record, ready.clone());
    }
    let rally_revision = runtime.vehicle_setup_revision(&rally);
    assert!(!rally_revision.is_empty());

    runtime.inner.busy.store(true, Ordering::Release);
    let run = runtime.inner.publish_start(&dcs, json!({"state":"setting-up"}));
    let mut catalogue = [dcs.clone(), alternate, rally.clone()];
    runtime.decorate_source(&mut catalogue);
    assert_eq!(catalogue[0]["integrationSetup"]["state"], "setting-up");
    assert_eq!(catalogue[1]["integrationSetup"]["state"], "setting-up");
    assert!(runtime.vehicle_setup_revision(&catalogue[1]).is_empty());
    assert_eq!(catalogue[2]["integrationSetup"]["state"], "ready");
    assert_eq!(runtime.vehicle_setup_revision(&rally), rally_revision);

    let record = runtime.inner.provider_snapshot("dcs").0;
    runtime.inner.publish_setup(&dcs, &record, ready.clone());
    runtime.inner.finish_job(&run, &ready);
    runtime.inner.busy.store(false, Ordering::Release);
    assert!(!runtime.vehicle_setup_revision(&dcs).is_empty());
}
