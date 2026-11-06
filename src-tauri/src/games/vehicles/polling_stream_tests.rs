use super::*;

#[test]
fn watched_assetto_reads_only_on_capture_and_expires_without_reading() {
    use std::{collections::VecDeque, sync::atomic::{AtomicBool, AtomicUsize, Ordering}};

    struct Input(VecDeque<game_telemetry::RawSample>, Arc<AtomicUsize>);
    impl game_telemetry::Source for Input {
        fn poll(&mut self) -> Result<Option<game_telemetry::RawSample>, String> {
            self.1.fetch_add(1, Ordering::Relaxed);
            Ok(self.0.pop_front())
        }
        fn close(&mut self) {}
    }

    let directory = crate::test_directory();
    let mut tracker = Tracker::new(directory.path().join("vehicles.json"));
    let game = json!({"id":"assetto-corsa-rally","integrationCompatible":true,"integrationKey":"assetto-corsa-rally",
        "provider":"assetto-corsa-rally","installed":true,"executable":"acr.exe"});
    let id = game["id"].as_str().unwrap();
    let process = ProcessIdentity { pid: 42, epoch: 10, path: "acr.exe".into() };
    tracker.begin_observation(&game, &json!({}), Some(process), "setup", true, 1).unwrap();

    let mut owner = game_telemetry::CaptureOwner::new();
    owner.begin_generation().unwrap();
    let at = Instant::now() - Duration::from_secs(1);
    let samples = [10u32, 11].map(|packet| {
        let mut physics = include_bytes!("../../../../tests/fixtures/acr/live.physics.bin").to_vec();
        physics[..4].copy_from_slice(&packet.to_le_bytes());
        let physics_at = owner.capture(at).unwrap();
        let graphics_at = owner.capture(at).unwrap();
        let statics_at = owner.capture(at).unwrap();
        game_telemetry::RawSample {
            source: game_telemetry::SourceIdentity { id: "fixture".into(), epoch: 1 },
            physics,
            graphics: include_bytes!("../../../../tests/fixtures/acr/live.graphics.bin").to_vec(),
            statics: include_bytes!("../../../../tests/fixtures/acr/live.static.bin").to_vec(),
            capture: Some(game_telemetry::CaptureSet::new(&owner, physics_at, graphics_at, statics_at).unwrap()),
        }
    });
    let reads = Arc::new(AtomicUsize::new(0));
    let process_current = Arc::new(AtomicBool::new(true));
    let wake = Arc::new(osg_streams::NativeWake::default());
    let entry = tracker.entries.get_mut(id).unwrap();
    entry.capture_watch = Some(CaptureBinding {
        _watch: sources::CaptureWatch::Fixture(process_current.clone()), wake: wake.clone(), drained: 0,
    });
    entry.reader = Some(Reader::Rally {
        source: Box::new(Input(samples.into(), reads.clone())),
        decoder: game_telemetry::Session::new(1500).unwrap(),
    });
    let check = |candidate: Qualification| candidate.verify(std::slice::from_ref(&game), |_| "setup".into(), |_| {
        panic!("The capture owner already monitors this process")
    });
    for _ in 0..2 {
        wake.notify();
        let candidates = tracker.capture_candidates().into_iter().map(|candidate| check(candidate).without_renewal()).collect();
        let poll = tracker.begin_polls(candidates).pop().unwrap();
        tracker.apply_poll(poll.read());
    }
    assert_eq!(reads.load(Ordering::Relaxed), 2);
    assert_eq!(tracker.entries[id].telemetry.as_ref().unwrap().status, "live");
    let deadline = at + Duration::from_millis(1500) + Duration::from_nanos(1);
    assert_eq!(tracker.capture_deadline(), Some(deadline));
    tracker.entries.get_mut(id).unwrap().observed -= Duration::from_secs(3);
    assert!(tracker.telemetry(&game).is_some());
    let now = Instant::now();
    assert!(tracker.maintenance_delay(now).is_some_and(|delay| delay > Duration::ZERO && delay <= deadline - now));
    let candidates = tracker.poll_candidates().into_iter().map(check).collect();
    assert!(tracker.begin_polls(candidates).is_empty());
    std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
    let candidates = tracker.capture_candidates().into_iter().map(|candidate| check(candidate).without_renewal()).collect();
    let poll = tracker.begin_polls(candidates).pop().unwrap();
    tracker.apply_poll(poll.read());
    assert_eq!(reads.load(Ordering::Relaxed), 2);
    assert_eq!(tracker.entries[id].telemetry.as_ref().unwrap().status, "stale");
    assert!(tracker.capture_deadline().is_none());
    assert!(tracker.capture_candidates().is_empty());
    assert!(tracker.maintenance_delay(Instant::now()).is_none());

    wake.notify();
    let candidates = tracker.capture_candidates().into_iter().map(check).collect();
    let poll = tracker.begin_polls(candidates).pop().unwrap();
    let poll = poll.read();
    process_current.store(false, Ordering::Release);
    assert!(tracker.telemetry(&game).is_none(), "Process loss hides cached telemetry during a detached read");
    tracker.apply_poll(poll);
    assert!(tracker.entries[id].reader.is_none());
}

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
