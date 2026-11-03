use super::*;

#[test]
fn catalogue_metadata_changes_notify_but_repeated_inventory_and_reader_errors_do_not() {
    let directory = crate::test_directory();
    let mut tracker = Tracker::new(directory.path().join("vehicles.json"));
    let changes = Arc::new(crate::game_read_models::signals::Signal::default());
    tracker.watch_changes(changes.clone());
    let game = json!({"id":"dcs","integrationCompatible":true,"integrationKey":"dcs","provider":"dcs-bios"});
    let mut first = tracker.begin_observation(&game, &json!({}), None, "setup", false, 1).unwrap();
    first.catalogue = Some(vec![json!({"id":"aircraft","name":"original"})]);
    first.reader = Some(Err("Source closed".into()));
    tracker.apply_observation(first);
    let original = changes.wake.observed();
    let mut repeated = tracker.begin_observation(&game, &json!({}), None, "setup", false, 2).unwrap();
    repeated.catalogue = Some(vec![json!({"id":"aircraft","name":"original"})]);
    repeated.reader = Some(Err("Source closed".into()));
    tracker.apply_observation(repeated);
    assert_eq!(changes.wake.observed(), original);
    let mut renamed = tracker.begin_observation(&game, &json!({}), None, "setup", false, 3).unwrap();
    renamed.catalogue = Some(vec![json!({"id":"aircraft","name":"renamed"})]);
    tracker.apply_observation(renamed);
    assert_ne!(changes.wake.observed(), original);
}
