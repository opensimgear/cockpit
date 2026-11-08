use super::*;

#[test]
fn catalogue_metadata_changes_notify_but_repeated_inventory_and_reader_errors_do_not() {
    let directory = crate::test_directory();
    let mut tracker = Tracker::new(directory.path().join("vehicles.json"));
    let changes = Arc::new(crate::game_read_models::signals::Signal::default());
    tracker.watch_changes(changes.clone());
    let game = json!({"id":"dcs","integrationCompatible":true,"integrationKey":"dcs","provider":"dcs-bios"});
    let source = CatalogueSource::Dcs(directory.path().to_path_buf());
    let baseline = Ok(vec![json!({"id":"aircraft","name":"original"})]);
    tracker.publish_catalogue(source.clone(), baseline.clone());
    let mut first = tracker.begin_observation(&game, &json!({}), None, "setup", false, 1).unwrap();
    first.reader = Some(Err("Source closed".into()));
    tracker.apply_observation(first);
    let original = changes.wake.observed();
    let mut repeated = tracker.begin_observation(&game, &json!({}), None, "setup", false, 2).unwrap();
    tracker.publish_catalogue(source.clone(), baseline);
    repeated.reader = Some(Err("Source closed".into()));
    tracker.apply_observation(repeated);
    assert_eq!(changes.wake.observed(), original);
    tracker.publish_catalogue(source, Ok(vec![json!({"id":"aircraft","name":"renamed"})]));
    assert_ne!(changes.wake.observed(), original);
}
