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
    tracker.apply_observation(current.verify(&[game()], "setup", |p| p == &process()));
    assert!(
        tracker.entries["x-plane-12"].reader.is_some(),
        "Concurrent observation cannot open a second reader"
    );
    let current = read(work(&mut tracker, 3));
    tracker.apply_observation(current.verify(&[game()], "setup", |p| p == &process()));
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
            |_| changed != "process",
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
