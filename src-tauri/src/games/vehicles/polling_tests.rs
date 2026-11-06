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
fn xplane_capture_wakes_the_inspector_without_renewing_process_evidence() {
    let (_directory, mut tracker, game) = fixture();
    let (source, publish) = crate::xplane::Source::inspector_fixture();
    let entry = tracker.entries.get_mut("x-plane-12").unwrap();
    entry.reader = Some(Reader::Xplane(source));
    let observed = entry.observed;
    let mut other = game.clone();
    other["id"] = json!("x-plane-11");
    other["integrationKey"] = json!("x-plane-11");
    let process = tracker.entries["x-plane-12"].process.clone();
    tracker.observe(&other, &crate::xplane::defaults(11), process, "setup", true, 1);
    let (quiet, _publish_quiet) = crate::xplane::Source::inspector_fixture();
    tracker.entries.get_mut("x-plane-11").unwrap().reader = Some(Reader::Xplane(quiet));
    let wake = Arc::new(osg_streams::NativeWake::default());
    tracker.watch_captures(wake.clone());
    assert_eq!(tracker.capture_candidates().len(), 2);
    let baseline = wake.observed();
    publish(42);
    assert_ne!(wake.observed(), baseline);
    let mut candidates = tracker.capture_candidates();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, "x-plane-12");
    let checked = candidates.pop().unwrap()
        .verify(std::slice::from_ref(&game), |_| "setup".into(), |_| true).without_renewal();
    let poll = tracker.begin_polls(vec![checked]).pop().unwrap();
    tracker.apply_poll(poll.read());
    let entry = &tracker.entries["x-plane-12"];
    assert_eq!(entry.observed, observed);
    assert_eq!(entry.telemetry.as_ref().unwrap().packet, json!(42));
    assert!(tracker.entries["x-plane-11"].telemetry.is_none());
    let games = [game, other];
    let maintenance = tracker.poll_candidates().into_iter()
        .map(|candidate| candidate.verify(&games, |_| "setup".into(), |_| true)).collect();
    assert!(tracker.begin_polls(maintenance).is_empty(), "Quiet X-Plane readers need no maintenance projection");
    assert!(tracker.entries["x-plane-12"].observed >= observed);
    let receipt = Arc::new(crate::observations::TelemetryLease::default());
    tracker.inspect("x-plane-11", "quiet-inspector", vec!["engine_1_rpm".into()], Some(Arc::downgrade(&receipt))).unwrap();
    let _ = tracker.capture_candidates();
    let before_close = wake.observed();
    drop(receipt);
    assert_ne!(wake.observed(), before_close);
    assert!(tracker.capture_deadline().is_some_and(|deadline| deadline <= Instant::now()));
    let release = tracker.capture_candidates().into_iter()
        .map(|candidate| candidate.verify(&games, |_| "setup".into(), |_| true).without_renewal()).collect();
    let mut release = tracker.begin_polls(release);
    assert_eq!(release.len(), 1, "Closing a quiet inspection must still update native demand");
    let poll = release.pop().unwrap();
    assert_eq!(poll.id, "x-plane-11");
    assert!(poll.inspection.is_empty());
    tracker.apply_poll(poll.read());
    let exited = tracker.poll_candidates().into_iter()
        .map(|candidate| candidate.verify(&games, |_| "setup".into(), |_| false)).collect();
    assert!(tracker.begin_polls(exited).is_empty());
    assert!(tracker.entries["x-plane-12"].telemetry.is_none());
}

#[test]
fn capture_drain_does_not_extend_the_process_observation() {
    let (_directory, mut tracker, game) = fixture();
    assert!(tracker.capture_candidates().is_empty());
    let entry = tracker.entries.get_mut("x-plane-12").unwrap();
    entry.observed = Instant::now() - Duration::from_secs(1);
    let observed = entry.observed;
    let checked = tracker.poll_candidates().pop().unwrap()
        .verify(&[game], |_| "setup".into(), |_| true).without_renewal();
    let poll = tracker.begin_polls(vec![checked]).pop().unwrap();
    tracker.apply_poll(poll.read());
    assert_eq!(tracker.entries["x-plane-12"].observed, observed);
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
fn detached_dcs_physics_observes_new_reports_and_deadline_without_bios_read_completion() {
    use osg_streams::Delivery;
    use std::net::{Ipv4Addr,UdpSocket};
    let (_directory, mut tracker, _) = fixture();
    let process = tracker.entries["x-plane-12"].process.clone().unwrap();
    let source=crate::dcs_physics::Source::bind_loopback_ephemeral().unwrap();
    let mut events=source.subscribe().unwrap();
    events.recv().unwrap();
    let sender=UdpSocket::bind((Ipv4Addr::LOCALHOST,0)).unwrap();
    let entry = tracker.entries.get_mut("x-plane-12").unwrap();
    entry.integration = "dcs".into(); entry.reader = None; entry.polling = true;
    entry.detached_physics=Some(source.view());
    let packet=|sequence,in_air|json!({"schemaVersion":1,"source":"osg-dcs-ownship",
        "session":"detached-integration","sequence":sequence,"modelTime":sequence as f64*0.05,
        "planeId":5,"aircraft":"FA-18C_hornet","allowed":true,"paused":false,"activePause":false,
        "stopped":false,"values":{"flight.ownship.inAir":in_air}});
    let read_until=|events:&mut crate::dcs_physics::Reader, fresh:bool, sequence:u32| {
        let deadline=Instant::now()+Duration::from_secs(2);
        loop {
            let delivery=events.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap().expect("Ownship event deadline");
            let snapshot=match delivery {Delivery::Snapshot(frame)=>frame.value,Delivery::Event(frame)=>frame.value,Delivery::Loss(_)=>continue};
            if snapshot.fresh==fresh && snapshot.sequence==Some(sequence) {return snapshot;}
        }
    };
    sender.send_to(packet(1,false).to_string().as_bytes(),source.local_addr()).unwrap();
    sender.send_to(packet(2,false).to_string().as_bytes(),source.local_addr()).unwrap();
    read_until(&mut events,true,2);
    assert_eq!(tracker.dcs_physics(&process).unwrap().values["flight.ownship.inAir"],false);
    sender.send_to(packet(3,true).to_string().as_bytes(),source.local_addr()).unwrap();
    let current=read_until(&mut events,true,3);
    assert_eq!(tracker.dcs_physics(&process).unwrap().values,current.values);
    assert!(tracker.dcs_physics(&ProcessIdentity{epoch:11,..process.clone()}).is_none());
    let expired=read_until(&mut events,false,3);
    assert_eq!(expired.status,"stale");
    let retained=tracker.dcs_physics(&process).unwrap();
    assert!(!retained.fresh && retained.values.is_empty());
    assert!(tracker.entries["x-plane-12"].polling,"No BIOS completion was required for either update.");
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
        Arc::make_mut(entry.telemetry.as_mut().unwrap()).at -= Duration::from_secs(3);
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
