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
    assert!(runtime.refresh_watched_setup("dcs", &game));
    let admission = runtime.lock_admission();
    let before = signal.context_update_wake.observed();
    assert!(!runtime.refresh_watched_setup("dcs", &game));
    assert!(!runtime.refresh_watched_setup("dcs", &game));
    assert_eq!(signal.context_update_wake.observed(), before);
    drop(admission);
    assert_eq!(signal.context_update_wake.observed(), before + 1);

    assert!(runtime.refresh_watched_setup("dcs", &game));
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
    assert!(!runtime.refresh_background(std::slice::from_ref(&game)));
    assert_eq!(signal.context_update_wake.observed(), before);
    drop(admission);
    assert_eq!(signal.context_update_wake.observed(), before + 1);
    assert!(runtime.refresh_background(&[game]));
}

#[test]
fn failed_watch_withdraws_only_the_exact_setup_proof_until_recovered() {
    let root = crate::test_directory();
    let runtime = GameIntegrationSetup::new(root.path().join("setup.json"), false);
    let signal = Arc::new(crate::game_read_models::signals::Signal::default());
    runtime.watch_changes(signal.clone());
    let game = json!({"id":"dcs","provider":"dcs-bios","integrationCompatible":true,
        "installed":true,"executable":"first.exe"});
    let mut replacement = game.clone();
    replacement["executable"] = json!("replacement.exe");
    let record = runtime.inner.provider_snapshot("dcs").0;
    let ready = json!({"state":"ready","verified":true,"connection":{"port":7778}});
    runtime.inner.publish_setup(&game, &record, ready.clone());
    runtime.inner.publish_setup(&replacement, &record, ready);
    runtime.set_setup_watch_unavailable("dcs", &game, true);
    let wake = signal.context_update_wake.observed();
    runtime.set_setup_watch_unavailable("dcs", &game, true);
    assert_eq!(signal.context_update_wake.observed(), wake);
    assert_eq!(runtime.inner.cached_source_setup(&game, "dcs")["verified"], false);
    assert_eq!(runtime.inner.cached_source_setup(&replacement, "dcs")["state"], "ready");
    for view in [runtime.inner.cached_setup_summary(&game, "dcs"), runtime.inner.cached_setup(&game, "dcs")] {
        assert_eq!(view["state"], "needs-attention");
        assert_eq!(view["connection"]["port"], 7778);
    }
    runtime.set_setup_watch_unavailable("dcs", &game, false);
    assert_eq!(runtime.inner.cached_source_setup(&game, "dcs")["verified"], true);
}
