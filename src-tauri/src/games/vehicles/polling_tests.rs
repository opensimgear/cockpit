use super::*;

fn fixture() -> (tempfile::TempDir, Tracker, Value) {
    let directory = crate::test_directory();
    let mut tracker = Tracker::new(directory.path().join("vehicles.json"));
    let game = json!({"id":"x-plane-12","integrationCompatible":true,"integrationKey":"x-plane-12",
        "provider":"x-plane","installed":true,"executable":"X-Plane.exe"});
    tracker.observe(
        &game,
        &crate::xplane::defaults(12),
        Some(ProcessIdentity {
            pid: 42,
            path: "X-Plane.exe".into(),
            epoch: 10,
        }),
        "setup",
        true,
        1,
    );
    tracker.entries.get_mut("x-plane-12").unwrap().reader = Some(Reader::Rally {
        source: Box::new(game_telemetry::FixtureSource::new()),
        decoder: game_telemetry::Session::new(1500).unwrap(),
    });
    (directory, tracker, game)
}
fn take_poll(tracker: &mut Tracker) -> VehiclePoll {
    let candidates = tracker.poll_candidates();
    tracker.begin_polls(candidates).pop().unwrap()
}
#[test]
fn late_poll_cannot_restore_replaced_or_withdrawn_process_reader() {
    for changed in ["epoch", "process", "setup", "installation", "expiry"] {
        let (_directory, mut tracker, game) = fixture();
        let mut poll = take_poll(&mut tracker).read();
        if changed == "epoch" {
            tracker.begin_observation(&game, &crate::xplane::defaults(12), None, "setup", true, 2);
        } else if changed == "expiry" {
            poll.at -= Duration::from_secs(3);
        } else {
            let mut current_game = game;
            if changed == "installation" {
                current_game["executable"] = json!("other.exe");
            }
            poll = poll.verify(
                &[current_game],
                |_| if changed == "setup" { "new" } else { "setup" }.into(),
                |_| changed != "process",
            );
        }
        tracker.apply_poll(poll);
        assert!(tracker.entries["x-plane-12"].reader.is_none(), "{changed}");
        assert_eq!(tracker.current("x-plane-12"), None);
    }
}
#[test]
fn detached_poll_keeps_conservative_capture_time_and_handles_failure() {
    let (_directory, mut tracker, game) = fixture();
    let poll = take_poll(&mut tracker);
    let captured = poll.at;
    let poll = poll.read().verify(&[game], |_| "setup".into(), |_| true);
    tracker.apply_poll(poll);
    assert_eq!(
        tracker.entries["x-plane-12"].telemetry.as_ref().unwrap().at,
        captured
    );
    let mut failed = take_poll(&mut tracker);
    failed.result = Some(Err("fixture reader failed".into()));
    tracker.apply_poll(failed);
    assert_eq!(
        tracker.entries["x-plane-12"].error.as_deref(),
        Some("fixture reader failed")
    );
    assert!(tracker.entries["x-plane-12"].reader.is_none());
}
#[test]
fn detached_verification_and_reads_do_not_retain_the_published_state_mutex() {
    use std::sync::{mpsc, Arc, Mutex};
    let (_directory, tracker, game) = fixture();
    let tracker = Arc::new(Mutex::new(tracker));
    let candidate = tracker.lock().unwrap().poll_candidates().pop().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let peer = tracker.clone();
    let worker = std::thread::spawn(move || {
        let qualified = candidate.verify(
            &[game],
            |_| "setup".into(),
            |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                true
            },
        );
        let polls = peer.lock().unwrap().begin_polls(vec![qualified]);
        for poll in polls {
            let poll = poll.read();
            peer.lock().unwrap().apply_poll(poll);
        }
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(tracker.try_lock().unwrap().current("x-plane-12"), None);
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    assert!(tracker.lock().unwrap().entries["x-plane-12"]
        .reader
        .is_some());
}

#[test]
fn detached_reader_preserves_qualified_telemetry_without_refreshing_capture() {
    let (_directory, mut tracker, game) = fixture();
    let poll = take_poll(&mut tracker).read();
    tracker.apply_poll(poll);
    let captured = tracker.telemetry(&game).unwrap().at;
    let poll = take_poll(&mut tracker);
    assert_eq!(tracker.telemetry(&game).unwrap().at, captured);
    tracker.entries.get_mut("x-plane-12").unwrap().observed -= Duration::from_secs(3);
    assert!(tracker.telemetry(&game).is_none());
    tracker.apply_poll(poll.read());
    assert!(tracker.telemetry(&game).is_none());
}

#[test]
fn detached_dcs_physics_keeps_its_original_age_and_expires_without_new_data() {
    let (_directory, mut tracker, _) = fixture();
    let process = tracker.entries["x-plane-12"].process.clone().unwrap();
    let entry = tracker.entries.get_mut("x-plane-12").unwrap();
    entry.integration = "dcs".into();
    entry.reader = None;
    entry.polling = true;
    let mut snapshot = crate::dcs_physics::Session::new().snapshot(0);
    snapshot.age_ms = Some(100);
    snapshot.status = "live".into();
    snapshot.fresh = true;
    snapshot
        .values
        .insert("flight.ownship.inAir".into(), json!(true));
    entry.detached_physics = Some((snapshot, Instant::now()));
    let snapshot = tracker.dcs_physics(&process).unwrap();
    assert!(snapshot.fresh);
    assert!(snapshot.age_ms.unwrap() >= 100);
    tracker
        .entries
        .get_mut("x-plane-12")
        .unwrap()
        .detached_physics
        .as_mut()
        .unwrap()
        .1 -= Duration::from_millis(crate::dcs_physics::FRESHNESS_MS);
    let snapshot = tracker.dcs_physics(&process).unwrap();
    assert!(!snapshot.fresh);
    assert_eq!(snapshot.status, "stale");
    assert!(snapshot.values.is_empty());
    assert!(tracker
        .dcs_physics(&ProcessIdentity {
            epoch: 11,
            ..process
        })
        .is_none());
}

#[test]
fn process_renewal_cannot_keep_an_old_aircraft_capture_current() {
    for detached in [false, true] {
        let (_directory, mut tracker, _) = fixture();
        let poll = take_poll(&mut tracker).read();
        tracker.apply_poll(poll);
        let entry = tracker.entries.get_mut("x-plane-12").unwrap();
        entry.current = Some(json!({"id":"old-aircraft","name":"old-aircraft"}));
        assert!(tracker.current("x-plane-12").is_some());
        let pending = detached.then(|| take_poll(&mut tracker));
        let entry = tracker.entries.get_mut("x-plane-12").unwrap();
        entry.telemetry.as_mut().unwrap().at -= Duration::from_secs(3);
        entry.observed = Instant::now();
        assert!(tracker.current("x-plane-12").is_none());
        drop(pending);
    }
}

#[test]
fn rejected_and_replaced_readers_are_destroyed_after_the_state_guard_is_released() {
    use std::sync::{mpsc, Arc, Mutex};
    struct SlowDrop {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl game_telemetry::Source for SlowDrop {
        fn poll(&mut self) -> Result<Option<game_telemetry::RawSample>, String> {
            Ok(None)
        }
        fn close(&mut self) {}
    }
    impl Drop for SlowDrop {
        fn drop(&mut self) {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
        }
    }
    for path in ["signature", "qualification", "rejected-poll"] {
        let (_directory, mut tracker, game) = fixture();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        tracker.entries.get_mut("x-plane-12").unwrap().reader = Some(Reader::Rally {
            source: Box::new(SlowDrop {
                entered: entered_tx,
                release: release_rx,
            }),
            decoder: game_telemetry::Session::new(1500).unwrap(),
        });
        let tracker = Arc::new(Mutex::new(tracker));
        let peer = tracker.clone();
        let worker = std::thread::spawn(move || {
            let retired = {
                let mut state = peer.lock().unwrap();
                match path {
                    "signature" => {
                        state.begin_observation(&game, &json!({}), None, "setup", true, 2);
                    }
                    "qualification" => {
                        let candidates = state
                            .poll_candidates()
                            .into_iter()
                            .map(|candidate| candidate.verify(&[], |_| "setup".into(), |_| false))
                            .collect();
                        state.begin_polls(candidates);
                    }
                    _ => {
                        let poll = take_poll(&mut state);
                        state.begin_observation(&game, &json!({}), None, "setup", true, 2);
                        state.apply_poll(poll);
                    }
                }
                state.take_retired()
            };
            drop(retired);
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let unlocked = tracker.try_lock().is_ok();
        release_tx.send(()).unwrap();
        worker.join().unwrap();
        assert!(
            unlocked,
            "{path}: provider destruction retained the vehicle-state lock"
        );
    }
}
