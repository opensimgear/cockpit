use super::*;

#[test]
fn newly_seen_vehicle_notifies_once_and_identical_fresh_capture_is_quiet() {
    let directory = crate::test_directory();
    let mut tracker = Tracker::new(directory.path().join("vehicles.json"));
    let changes = Arc::new(crate::game_read_models::signals::Signal::default());
    tracker.watch_changes(changes.clone());
    let game = json!({"id":"assetto-corsa","integrationCompatible":true,"integrationKey":"assetto-corsa","provider":crate::game_discovery::integration_provider("assetto-corsa")});
    tracker.observe(&game, &json!({}), None, "setup", false, 1);
    tracker.entries.get_mut("assetto-corsa").unwrap().reader = Some(Reader::Rally {
        source: Box::new(game_telemetry::FixtureSource::new()),
        decoder: game_telemetry::Session::new(1500).unwrap(),
    });
    let before = changes.wake.observed();
    for index in 0..2 {
        let candidates = tracker.poll_candidates();
        let mut poll = tracker.begin_polls(candidates).pop().unwrap().read();
        assert_eq!(poll.capture_changed, index == 0);
        let (_, capture) = poll.result.take().unwrap().unwrap();
        poll.result = Some(Ok((Some("car".into()), capture)));
        let previous = changes.wake.observed();
        tracker.apply_poll(poll);
        if index == 0 { assert_ne!(changes.wake.observed(), before); }
        else { assert_eq!(changes.wake.observed(), previous); }
    }
    assert_eq!(tracker.seen["assetto-corsa"].len(), 1);
}

#[test]
fn disappearance_notifies_once_even_if_telemetry_expired_before_the_poll_pass() {
    let directory = crate::test_directory();
    let mut tracker = Tracker::new(directory.path().join("vehicles.json"));
    let changes = Arc::new(crate::game_read_models::signals::Signal::default());
    tracker.watch_changes(changes.clone());
    tracker.set_test_vehicle("assetto-corsa", Some("car"));
    let before = changes.wake.observed();
    let entry = tracker.entries.get_mut("assetto-corsa").unwrap();
    entry.injected = false;
    entry.observed -= Duration::from_secs(3);
    assert!(tracker.current("assetto-corsa").is_none());
    let candidates = tracker.poll_candidates();
    tracker.begin_polls(candidates);
    assert_ne!(changes.wake.observed(), before);
    let cleared = changes.wake.observed();
    let candidates = tracker.poll_candidates();
    tracker.begin_polls(candidates);
    assert_eq!(changes.wake.observed(), cleared);
}

#[test]
fn deadline_renewal_only_reschedules_and_elapsed_cached_view_withdraws_without_polling() {
    let directory = crate::test_directory();
    let mut tracker = Tracker::new(directory.path().join("vehicles.json"));
    let changes = Arc::new(crate::game_read_models::signals::Signal::default());
    tracker.watch_changes(changes.clone());
    let mut game = json!({"id":"assetto-corsa","integrationCompatible":true,"integrationKey":"assetto-corsa","provider":crate::game_discovery::integration_provider("assetto-corsa")});
    tracker.observe(&game, &json!({}), None, "setup", false, 1);
    tracker.entries.get_mut("assetto-corsa").unwrap().reader = Some(Reader::Rally {
        source: Box::new(game_telemetry::FixtureSource::new()),
        decoder: game_telemetry::Session::new(1500).unwrap(),
    });
    let candidates = tracker.poll_candidates();
    let mut poll = tracker.begin_polls(candidates).pop().unwrap().read();
    let (_, capture) = poll.result.take().unwrap().unwrap();
    poll.result = Some(Ok((Some("car".into()), capture)));
    tracker.apply_poll(poll);
    let initial = tracker.view_deadline().unwrap();
    tracker.decorate_summary(&mut game);
    assert_eq!(game["currentVehicle"]["id"], "car");
    let revision = changes.revision();
    let wake = changes.wake.observed();
    // Renew both conservative source fences through the producer publication
    // hook. Identity and metadata are unchanged, so no model revision is dirty.
    let entry = tracker.entries.get_mut("assetto-corsa").unwrap();
    let renewed = Instant::now();
    entry.observed = renewed;
    Arc::make_mut(entry.telemetry.as_mut().unwrap()).at = renewed;
    tracker.publish_visible("assetto-corsa");
    assert!(tracker.view_deadline().unwrap() > initial);
    assert_ne!(changes.wake.observed(), wake);
    assert_eq!(changes.revision(), revision);
    let entry = tracker.entries.get_mut("assetto-corsa").unwrap();
    Arc::make_mut(entry.telemetry.as_mut().unwrap()).at = Instant::now() - Duration::from_secs(3);
    assert_eq!(tracker.view_deadline(), None);
    tracker.decorate_summary(&mut game);
    assert!(game["currentVehicle"].is_null());
    assert_eq!(game["vehicleStatus"]["status"], "waiting");
    // Expired views return no timer, while injected fixtures are timer-free.
    assert_eq!(tracker.view_deadline(), None);
    tracker.set_test_vehicle("assetto-corsa", Some("car"));
    assert_eq!(tracker.view_deadline(), None);
}
