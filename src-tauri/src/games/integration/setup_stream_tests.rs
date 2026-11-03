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
    runtime.inner.publish_setup(&game, &record, status.clone());
    let before = signal.wake.observed();
    let key = source_scope(&game, "dcs");
    let first = runtime.inner.views.lock().unwrap().entries[&key].1.clone();
    runtime.inner.publish_setup(&game, &record, status);
    assert_eq!(signal.wake.observed(), before);
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
