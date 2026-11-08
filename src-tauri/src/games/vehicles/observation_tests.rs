use super::*;

fn game() -> Value {
    json!({"id":"x-plane-12","integrationCompatible":true,"integrationKey":"x-plane-12",
        "provider":"x-plane","installed":true,"executable":"X-Plane.exe"})
}
fn process() -> ProcessIdentity {
    ProcessIdentity {
        pid: 42,
        path: "X-Plane.exe".into(),
        epoch: 10,
    }
}
fn tracker() -> (tempfile::TempDir, Tracker) {
    let directory = crate::test_directory();
    let tracker = Tracker::new(directory.path().join("vehicles.json"));
    (directory, tracker)
}
fn work(tracker: &mut Tracker, epoch: u64) -> VehicleObservation {
    tracker
        .begin_observation(
            &game(),
            &crate::xplane::defaults(12),
            Some(process()),
            "setup",
            true,
            epoch,
        )
        .unwrap()
}
fn read(work: VehicleObservation) -> VehicleObservation {
    work.read_with(|_, _, _| {
        Ok(Reader::Rally {
            source: Box::new(game_telemetry::FixtureSource::new()),
            decoder: game_telemetry::Session::new(1500).unwrap(),
        })
    })
}

#[test]
fn inventory_errors_preserve_only_the_existing_configuration_and_lease() {
    use crate::game_discovery::ProcessLookupError;
    let (_directory, mut tracker) = tracker();
    let game = game();
    let id = "x-plane-12";
    let connection = crate::xplane::defaults(12);
    let initial = read(work(&mut tracker, 1));
    tracker.apply_observation(initial);
    let generation = tracker.entries[id].generation;
    let observed = tracker.entries[id].observed;

    assert!(tracker.begin_inventory_observation(&game, &connection, Err(ProcessLookupError::Unavailable("inventory busy".into())),
        "setup", true, 2).is_none());
    assert_eq!(tracker.entries[id].generation, generation);
    assert_eq!(tracker.entries[id].observed, observed);
    assert!(tracker.entries[id].reader.is_some());

    let current = work(&mut tracker, 3).verify(std::slice::from_ref(&game), "setup", |_| Err("access denied".into()));
    tracker.apply_observation(current);
    assert_eq!(tracker.entries[id].generation, generation);
    assert!(tracker.entries[id].reader.is_some());

    tracker.begin_inventory_observation(&game, &connection, Err(ProcessLookupError::Unavailable("inventory busy".into())), "replacement", true, 4);
    assert_ne!(tracker.entries[id].generation, generation);
    assert!(tracker.entries[id].reader.is_none());
    assert!(tracker.entries[id].process.is_none());
}

#[test]
fn unavailable_inventory_fences_older_process_results_without_renewing_the_reader() {
    use crate::game_discovery::ProcessLookupError;
    let (_directory, mut tracker) = tracker();
    let game = game();
    let id = game["id"].as_str().unwrap();
    let connection = crate::xplane::defaults(12);
    let initial = read(work(&mut tracker, 1));
    tracker.apply_observation(initial);
    let generation = tracker.entries[id].generation;
    let observed = tracker.entries[id].observed;
    let replacement = ProcessIdentity { pid: 43, ..process() };

    tracker.begin_inventory_observation(&game, &connection,
        Err(ProcessLookupError::Unavailable("inventory busy".into())), "setup", true, 3);
    tracker.begin_inventory_observation(&game, &connection, Ok(Some(process())), "setup", true, 2);
    assert_eq!(tracker.entries[id].observed, observed, "An older result cannot renew the process proof");
    let late = tracker.begin_inventory_observation(&game, &connection,
        Ok(Some(replacement.clone())), "setup", true, 2);
    assert_eq!(tracker.entries[id].generation, generation, "An older result cannot replace the reader");
    assert!(late.is_none());
    assert_eq!(tracker.entries[id].observed, observed);
    assert_eq!(tracker.entries[id].process, Some(process()));
    assert!(tracker.entries[id].reader.is_some());

    let replacement_work = tracker.begin_inventory_observation(&game, &connection,
        Ok(Some(replacement.clone())), "setup", true, 4).unwrap();
    tracker.apply_observation(read(replacement_work));
    assert_ne!(tracker.entries[id].generation, generation);
    assert_eq!(tracker.entries[id].process, Some(replacement));
    assert!(tracker.entries[id].reader.is_some());
}

#[test]
fn ambiguous_inventory_retires_the_previously_current_reader() {
    use crate::game_discovery::ProcessLookupError;
    let (_directory, mut tracker) = tracker();
    let initial = read(work(&mut tracker, 1));
    tracker.apply_observation(initial);
    let id = "x-plane-12";
    let generation = tracker.entries[id].generation;
    tracker.begin_inventory_observation(&game(), &crate::xplane::defaults(12),
        Err(ProcessLookupError::Ambiguous("competing processes".into())), "setup", true, 2);
    assert_ne!(tracker.entries[id].generation, generation);
    assert!(tracker.entries[id].reader.is_none());
    assert!(tracker.entries[id].process.is_none());
}

#[test]
fn failed_verification_cannot_publish_a_new_reader() {
    let (_directory, mut tracker) = tracker();
    let opened = read(work(&mut tracker, 1)).verify(&[game()], "setup", |_| Err("access denied".into()));
    tracker.apply_observation(opened);
    let entry = &tracker.entries["x-plane-12"];
    assert!(entry.reader.is_none());
    assert!(entry.opening.is_none());
    assert!(entry.retry > Instant::now());
}
#[test]
fn quiet_reader_admission_wakes_capture_without_a_timer() {
    let (_directory, mut tracker) = tracker();
    let wake = Arc::new(osg_streams::NativeWake::default());
    tracker.watch_captures(wake.clone());
    let baseline = wake.observed();
    let (source, _stop_monitor) = crate::xplane::Source::monitored_inspector_fixture();
    let opening = work(&mut tracker, 1).read_with(|_, _, _| Ok(Reader::Xplane(source)));
    assert!(tracker.capture_deadline().is_none());
    assert!(tracker.scoped_capture_candidates(true).is_empty());
    tracker.apply_observation(opening);
    assert_ne!(wake.observed(), baseline, "A quiet provider must wake admission without a packet");
    assert_eq!(tracker.entries["x-plane-12"].retained_process_current(), Some(true));
    assert!(tracker.capture_deadline().is_none());

    let candidates = tracker.capture_candidates();
    let poll = tracker.begin_polls(candidates).pop().unwrap();
    assert!(tracker.entries["x-plane-12"].reader.is_none());
    assert!(tracker.capture_deadline().is_none());
    tracker.begin_observation(&game(), &json!({}), None, "setup", false, 2);
    tracker.apply_poll(poll.read());
    assert!(tracker.entries["x-plane-12"].reader.is_none());
    assert!(tracker.telemetry(&game()).is_none());
    assert!(tracker.capture_deadline().is_none());
    assert!(tracker.scoped_capture_candidates(true).is_empty());
}

#[test]
fn capture_admission_requires_notifications_and_exact_process_evidence() {
    let (source, _publish) = crate::xplane::Source::inspector_fixture();
    let fixtures = [
        (Reader::Rally {
            source: Box::new(game_telemetry::FixtureSource::new()),
            decoder: game_telemetry::Session::new(1500).unwrap(),
        }, "native capture notification"),
        (Reader::Xplane(source), "exact game process"),
    ];
    for (reader, diagnostic) in fixtures {
        let (_directory, mut tracker) = tracker();
        tracker.watch_captures(Arc::new(osg_streams::NativeWake::default()));
        let opening = work(&mut tracker, 1).read_with(|_, _, _| Ok(reader));
        tracker.apply_observation(opening);
        let entry = &tracker.entries["x-plane-12"];
        assert!(entry.error.as_deref().unwrap().contains(diagnostic));
        assert!(entry.reader.is_none() && entry.capture_watch.is_none() && entry.opening.is_none());
        assert_eq!(tracker.context_deadline(&["x-plane-12".into()], Instant::now()), Some(entry.retry));
        assert!(tracker.capture_deadline().is_none());
        assert!(tracker.scoped_capture_candidates(true).is_empty());
    }
}

#[test]
fn failed_reader_open_schedules_only_a_context_retry() {
    let (_directory, mut tracker) = tracker();
    let failed = work(&mut tracker, 1).read_with(|_, _, _| Err("Fixture unavailable".into()));
    tracker.apply_observation(failed);
    let entry = &tracker.entries["x-plane-12"];
    assert!(entry.error.is_some());
    assert_eq!(tracker.context_deadline(&["x-plane-12".into()], Instant::now()), Some(entry.retry));
    assert!(tracker.capture_deadline().is_none());
    assert!(tracker.scoped_capture_candidates(true).is_empty());
}
#[test]
fn detached_observation_publishes_only_matching_current_identity_and_epoch() {
    let (_directory, mut tracker) = tracker();
    let old = read(work(&mut tracker, 1));
    let current = work(&mut tracker, 2)
        .read_with(|_, _, _| panic!("Only one opener may run for the same identity"));
    tracker.apply_observation(old);
    assert!(
        tracker.entries["x-plane-12"].reader.is_some(),
        "A later matching observation must preserve the sole opener"
    );
    tracker.apply_observation(current.verify(&[game()], "setup", |p| Ok(p == &process())));
    assert!(
        tracker.entries["x-plane-12"].reader.is_some(),
        "Concurrent observation cannot open a second reader"
    );
    let current = read(work(&mut tracker, 3));
    tracker.apply_observation(current.verify(&[game()], "setup", |p| Ok(p == &process())));
    assert!(tracker.entries["x-plane-12"].reader.is_some());
    assert!(tracker
        .begin_observation(&game(), &json!({}), None, "setup", false, 1)
        .is_none());
    let stale = read(work(&mut tracker, 4));
    let mut replacement = process();
    replacement.epoch += 1;
    tracker.begin_observation(
        &game(),
        &crate::xplane::defaults(12),
        Some(replacement),
        "setup",
        true,
        5,
    );
    tracker.apply_observation(stale);
    assert!(tracker.entries["x-plane-12"].reader.is_none());
}
#[test]
fn failed_post_read_requalification_or_expiry_cannot_publish_a_reader() {
    for changed in ["setup", "installation", "process", "expiry"] {
        let (_directory, mut tracker) = tracker();
        let observed = read(work(&mut tracker, 1));
        let mut current_game = game();
        if changed == "installation" {
            current_game["executable"] = json!("other.exe");
        }
        let observed = observed.verify(
            &[current_game],
            if changed == "setup" { "new" } else { "setup" },
            |_| Ok(changed != "process"),
        );
        if changed == "expiry" {
            tracker.entries.get_mut("x-plane-12").unwrap().observed -= Duration::from_secs(3);
        }
        tracker.apply_observation(observed);
        assert!(tracker.entries["x-plane-12"].reader.is_none(), "{changed}");
        assert_eq!(tracker.current("x-plane-12"), None);
    }
}

#[test]
fn slow_process_work_keeps_admitted_reader_until_confirmed_process_loss() {
    use std::sync::atomic::AtomicBool;

    for detached in [false, true] {
        let (_directory, mut tracker) = tracker();
        let initial = read(work(&mut tracker, 1));
        tracker.apply_observation(initial);
        let id = "x-plane-12";
        let wake = Arc::new(osg_streams::NativeWake::default());
        let entry = tracker.entries.get_mut(id).unwrap();
        entry.capture_watch = Some(CaptureBinding {
            _watch: sources::CaptureWatch::Fixture(Arc::new(AtomicBool::new(true))),
            wake: wake.clone(),
            drained: 0,
        });
        entry.current = vehicle(id, "Aircraft/Test/Test.acf");
        let capture = Arc::new(Capture {
            status: "live".into(),
            source: "fixture".into(),
            context: json!({"aircraft":"Aircraft/Test/Test.acf"}),
            packet: json!(7),
            age_ms: Some(0),
            values: BTreeMap::new(),
            ages: BTreeMap::new(),
            stale_ids: Default::default(),
            message: None,
            at: Instant::now(),
        });
        entry.telemetry = Some(capture.clone());
        let current = tracker.current(id).unwrap();
        let generation = tracker.entries[id].generation;
        let poll = detached.then(|| {
            wake.notify();
            let candidates = tracker.capture_candidates();
            tracker.begin_polls(candidates).pop().unwrap()
        });
        let slow = work(&mut tracker, 2).verify(&[game()], "setup", |_| Ok(true));
        tracker.entries.get_mut(id).unwrap().observed -= Duration::from_secs(3);
        let observed = tracker.entries[id].observed;
        tracker.apply_observation(slow);

        let entry = &tracker.entries[id];
        assert_eq!(entry.generation, generation);
        assert_eq!(entry.observed, observed);
        assert!(entry.capture_watch.is_some());
        assert_eq!(entry.polling, detached);
        assert_eq!(entry.reader.is_none(), detached);
        assert_eq!(tracker.current(id), Some(current));
        assert!(Arc::ptr_eq(&capture, &tracker.telemetry(&game()).unwrap()));

        let lost = work(&mut tracker, 3).verify(&[game()], "setup", |_| Ok(false));
        tracker.apply_observation(lost);
        if let Some(poll) = poll {
            tracker.apply_poll(poll.read());
        }
        assert_ne!(tracker.entries[id].generation, generation);
        assert!(tracker.entries[id].reader.is_none());
        assert!(tracker.current(id).is_none());
        assert!(tracker.telemetry(&game()).is_none());
    }
}

#[test]
fn failed_reader_open_is_retained_as_retryable_diagnostic_without_current_vehicle() {
    let (_directory, mut tracker) = tracker();
    let game = json!({"id":"custom","integrationCompatible":true,"integrationKey":"unknown","provider":"unknown"});
    let mut observed = tracker
        .begin_observation(&game, &json!({}), Some(process()), "setup", false, 1)
        .unwrap();
    observed.process = Some(process());
    tracker.apply_observation(observed.read());
    assert!(tracker.entries["custom"].error.is_some());
    assert!(tracker.entries["custom"].reader.is_none());
    assert!(tracker
        .begin_observation(&json!({"id":"invalid/id"}), &json!({}), None, "", false, 1)
        .is_none());
}
