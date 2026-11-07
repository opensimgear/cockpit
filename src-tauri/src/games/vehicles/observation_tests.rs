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
fn quiet_reader_admission_wakes_idle_maintenance_and_withdrawal_parks_it() {
    let (_directory, mut tracker) = tracker();
    let wake = Arc::new(osg_streams::NativeWake::default());
    tracker.watch_captures(wake.clone());
    let baseline = wake.observed();
    assert!(tracker.maintenance_delay(Instant::now()).is_none());
    let opening = read(work(&mut tracker, 1));
    assert!(tracker.maintenance_delay(Instant::now()).is_none(), "Opening runs on the context owner");
    assert!(tracker.poll_candidates().is_empty(), "Another reader's fallback must not qualify this opening");
    tracker.apply_observation(opening);
    assert_ne!(wake.observed(), baseline, "A quiet provider must wake admission without a packet");
    assert_eq!(tracker.maintenance_delay(Instant::now()), Some(Duration::ZERO));

    let candidates = tracker.poll_candidates();
    let poll = tracker.begin_polls(candidates).pop().unwrap();
    assert!(tracker.entries["x-plane-12"].reader.is_none());
    assert!(tracker.maintenance_delay(Instant::now() + Duration::from_secs(1)).is_some(),
        "Temporarily detached I/O still owns maintenance");
    tracker.begin_observation(&game(), &json!({}), None, "setup", false, 2);
    let withdrawn = wake.observed();
    tracker.apply_poll(poll.read());
    assert!(tracker.maintenance_delay(Instant::now()).is_none());
    assert_eq!(wake.observed(), withdrawn, "Late I/O cannot readmit a withdrawn reader");

    let retry = read(work(&mut tracker, 3));
    tracker.apply_observation(retry);
    assert_ne!(wake.observed(), withdrawn);
    assert!(tracker.maintenance_delay(Instant::now()).is_some());
}

#[test]
fn failed_reader_open_does_not_schedule_vehicle_maintenance() {
    let (_directory, mut tracker) = tracker();
    let failed = work(&mut tracker, 1).read_with(|_, _, _| Err("Fixture unavailable".into()));
    tracker.apply_observation(failed);
    assert!(tracker.maintenance_delay(Instant::now()).is_none());
    assert!(tracker.entries["x-plane-12"].error.is_some());
    assert!(tracker.poll_candidates().is_empty(), "Open retries are scheduled by the context owner");
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
