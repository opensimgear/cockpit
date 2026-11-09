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
    assert!(tracker.capture_deadline().is_some_and(|at| at > now));
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
fn configuration_wakes_recheck_quiet_and_detached_captures_without_reading() {
    use std::sync::atomic::AtomicBool;

    for (changed, detached) in [("setup", false), ("installation", false), ("setup", true), ("installation", true)] {
        let directory = crate::test_directory();
        let mut tracker = Tracker::new(directory.path().join("vehicles.json"));
        let changes = Arc::new(crate::game_read_models::signals::Signal::default());
        tracker.watch_changes(changes.clone());
        let game = json!({"id":"assetto-corsa","integrationCompatible":true,"integrationKey":"assetto-corsa",
            "provider":"assetto-corsa","installed":true,"executable":"acs.exe"});
        let id = "assetto-corsa";
        let process = ProcessIdentity { pid: 42, epoch: 10, path: "acs.exe".into() };
        tracker.begin_observation(&game, &json!({}), Some(process), "setup", true, 1).unwrap();
        let wake = Arc::new(osg_streams::NativeWake::default());
        let entry = tracker.entries.get_mut(id).unwrap();
        entry.capture_watch = Some(CaptureBinding {
            _watch: sources::CaptureWatch::Fixture(Arc::new(AtomicBool::new(true))),
            wake: wake.clone(),
            drained: 0,
        });
        entry.reader = Some(Reader::Rally {
            source: Box::new(game_telemetry::FixtureSource::new()),
            decoder: game_telemetry::Session::new(1500).unwrap(),
        });
        let check = |candidate: Qualification, game: &Value, revision: &str| {
            candidate.verify(std::slice::from_ref(game), |_| revision.into(), |_| {
                panic!("The capture owner already monitors this process")
            }).without_renewal()
        };
        wake.notify();
        let baseline = tracker.scoped_capture_candidates(false).into_iter()
            .map(|candidate| check(candidate, &game, "setup")).collect();
        let mut poll = tracker.begin_polls(baseline).pop().unwrap().read();
        let (_, capture) = poll.result.take().unwrap().unwrap();
        poll.result = Some(Ok((Some("fixture-car".into()), capture)));
        tracker.apply_poll(poll);
        let original = tracker.telemetry(&game).unwrap();
        let generation = tracker.entries[id].generation;
        assert_eq!(tracker.current(id).unwrap()["id"], "fixture-car");
        assert!(tracker.scoped_capture_candidates(false).is_empty());

        let unchanged = tracker.scoped_capture_candidates(true).into_iter()
            .map(|candidate| check(candidate, &game, "setup")).collect::<Vec<_>>();
        assert_eq!(unchanged.len(), 1, "A configuration wake checks a quiet reader");
        assert!(tracker.begin_polls(unchanged).is_empty(), "A matching configuration requires no capture read");
        assert!(Arc::ptr_eq(&tracker.telemetry(&game).unwrap(), &original));

        let late = detached.then(|| {
            wake.notify();
            let candidates = tracker.scoped_capture_candidates(false).into_iter()
                .map(|candidate| check(candidate, &game, "setup")).collect();
            tracker.begin_polls(candidates).pop().unwrap().read()
        });
        let mut current_game = game.clone();
        if changed == "installation" {
            current_game["executable"] = json!("other.exe");
        }
        let revision = if changed == "setup" { "replacement" } else { "setup" };
        let context_epoch = changes.context_update_wake.observed();
        let invalid = tracker.scoped_capture_candidates(true).into_iter()
            .map(|candidate| check(candidate, &current_game, revision)).collect::<Vec<_>>();
        assert_eq!(invalid.len(), 1);
        assert!(tracker.begin_polls(invalid).is_empty(), "Invalid scopes retire without capture reads");
        if let Some(poll) = late { tracker.apply_poll(poll); }
        let entry = &tracker.entries[id];
        assert_ne!(entry.generation, generation);
        assert!(!entry.polling && entry.reader.is_none() && entry.capture_watch.is_none());
        assert!(tracker.current(id).is_none() && tracker.telemetry(&game).is_none());
        assert_ne!(changes.context_update_wake.observed(), context_epoch);
        assert!(tracker.capture_deadline().is_none());
        assert!(tracker.scoped_capture_candidates(true).is_empty());
    }
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
    let mut original: Option<Arc<Capture>> = None;
    for index in 0..2 {
        let candidates = tracker.poll_candidates();
        let mut poll = tracker.begin_polls(candidates).pop().unwrap().read();
        assert_eq!(poll.capture_changed, index == 0);
        let (_, mut capture) = poll.result.take().unwrap().unwrap();
        if let Some(first) = &original {
            // A later host projection of the same packet is not new evidence.
            capture.at = first.at + Duration::from_millis(1);
        }
        poll.result = Some(Ok((Some("car".into()), capture)));
        let previous = changes.wake.observed();
        tracker.apply_poll(poll);
        let retained = tracker.entries["assetto-corsa"].telemetry.as_ref().unwrap();
        if let Some(first) = &original {
            assert!(Arc::ptr_eq(retained, first));
            assert_eq!(retained.at, first.at);
            assert_eq!(changes.wake.observed(), previous);
        } else {
            original = Some(Arc::clone(retained));
            assert_ne!(changes.wake.observed(), before);
        }
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

impl Tracker {
    pub(crate) fn seed_capture_reader(
        &mut self,
        game: &Value,
        setup_revision: &str,
        source: Box<dyn game_telemetry::Source>,
    ) -> Arc<osg_streams::NativeWake> {
        self.seed_test_reader(game, setup_revision, Reader::Rally {
            source, decoder: game_telemetry::Session::new(1500).unwrap(),
        })
    }

    pub(crate) fn seed_dcs_capture_reader(
        &mut self,
        game: &Value,
        setup_revision: &str,
    ) -> (std::net::SocketAddr, impl FnOnce()) {
        let catalogue_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios");
        let module = "A-10C";
        let connection = json!({"cataloguePath":catalogue_path,"commandHost":"127.0.0.1","commandPort":7778,
            "listenAddress":"127.0.0.1","listenPort":5010,"multicastGroup":"","multicastInterface":"0.0.0.0",
            "freshnessMs":1500});
        let mut network = dcs_bios::Network::parse(&connection).unwrap();
        network.listen_port = 0;
        let (decoder, guard, catalogue) = dcs_profile::passive_telemetry_sources(&catalogue_path, module, 1500).unwrap();
        let source = dcs_bios::decoded::Source::open(&network, decoder.decode_spec().unwrap()).unwrap();
        let address = source.local_addr().unwrap();
        let watch = self.seed_watched_test_reader(game, setup_revision, Reader::Dcs {
            source,
            process_exit: None,
            decoder,
            guard: dcs_profile::passive_watch::PassiveCatalogueGuard::without_watch(guard),
            connection,
            module: module.into(),
            resolved_aircraft: None,
            bios_channels: bios_channel_ids(&catalogue),
            physics: None,
            physics_error: None,
            physics_retry: Instant::now() + Duration::from_secs(60),
            capture_dirty: false,
        });
        (address, move || drop(watch))
    }

    pub(crate) fn seed_xplane_capture_reader(
        &mut self,
        game: &Value,
        setup_revision: &str,
    ) -> impl Fn(f32, f32) {
        let (source, send) = crate::xplane::Source::vehicle_capture_fixture();
        let watch = self.seed_watched_test_reader(game, setup_revision, Reader::Xplane(source));
        move |clock, speed| {
            let _keep_watch = &watch;
            send(clock, speed);
        }
    }

    fn seed_watched_test_reader(&mut self, game: &Value, setup_revision: &str, reader: Reader) -> CaptureWatch {
        let wake = self.seed_test_reader(game, setup_revision, reader);
        let id = game["id"].as_str().unwrap();
        // Keep the real journal watch alongside the fixture process proof.
        self.entries[id].reader.as_ref().unwrap().capture_watch(wake).unwrap().unwrap()
    }

    fn seed_test_reader(
        &mut self,
        game: &Value,
        setup_revision: &str,
        reader: Reader,
    ) -> Arc<osg_streams::NativeWake> {
        let id = game["id"].as_str().unwrap();
        let process = ProcessIdentity {
            pid: 42, epoch: 10, path: game["executable"].as_str().unwrap().into(),
        };
        self.begin_observation(game, &json!({}), Some(process), setup_revision, true, 1).unwrap();
        let worker = self.capture_wakes.as_ref().unwrap().get(id).unwrap();
        let wake = Arc::new(osg_streams::NativeWake::child(worker.clone()));
        let entry = self.entries.get_mut(id).unwrap();
        entry.opening = None;
        entry.capture_watch = Some(CaptureBinding {
            _watch: sources::CaptureWatch::Fixture(Arc::new(std::sync::atomic::AtomicBool::new(true))),
            wake: wake.clone(),
            drained: 0,
        });
        entry.reader = Some(reader);
        wake
    }
}
