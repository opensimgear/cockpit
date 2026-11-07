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
fn fully_watched_external_setup_pass_skips_a_held_admission_gate() {
    let root = crate::test_directory();
    let runtime = Arc::new(GameIntegrationSetup::new(root.path().join("setup.json"), false));
    runtime.set_setup_watch("dcs", true);
    runtime.set_dcs_discovery_watch(true);
    let admission = runtime.lock_admission();
    let (done, received) = std::sync::mpsc::channel();
    let worker = {
        let runtime = runtime.clone();
        thread::spawn(move || {
            let game = json!({"id":"dcs","provider":"dcs-bios","integrationCompatible":true});
            runtime.refresh_external_setup(&[game]);
            done.send(()).unwrap();
        })
    };
    let completed = received.recv_timeout(std::time::Duration::from_secs(2));
    drop(admission);
    worker.join().unwrap();
    assert!(completed.is_ok(), "A covered scope must not enter background admission");
}
