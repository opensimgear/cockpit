// Included with the native UDP fixture; no native process or command authority.
fn wake_test_games(folder: &Path) -> GamesRuntime {
    GamesRuntime::test_runtime(
        Arc::new(|_| json!({"ok":true})),
        crate::games::Paths {
            installations: folder.join("installations.json"),
            provider_setup: folder.join("setup.json"),
            recovery: folder.join("recovery.json"),
        },
        Arc::new(ProcessFixture { process: Mutex::new(None) }),
    )
}

#[test]
fn assetto_service_needs_live_watch_and_validation_and_reverts_on_close() {
    let (mut source, _, _folder) = baseline_reader();
    let wake = Arc::new(osg_streams::NativeWake::default());
    source.set_wake(Some(wake.clone()));
    source.reader = Some(Reader::Acr {
        source: Some(Box::new(crate::game_telemetry::FixtureSource::new_for_game(crate::game_telemetry::Game::Corsa))),
        decoder: crate::game_telemetry::Session::new_for_game(100, crate::game_telemetry::Game::Corsa).unwrap(),
        outputs: vec![],
    });
    source.acquired = true;
    assert!(!source.watched_game_source());
    let (watch, close) = crate::acr_windows::Watch::fixture(wake.clone());
    source.install_assetto_watch_fixture(watch);
    assert!(!source.watched_game_source(), "A producer watch cannot substitute for native proof");
    source.validation.start(Some(Arc::downgrade(&wake)), || Ok(Instant::now()));
    assert!(source.watched_game_source());
    assert!(source.assetto_capture_deadline().is_none(), "A waiting decoder has no capture to expire");
    close();
    assert!(!source.watched_game_source(), "A retired watch restores bounded service");
}

pub(crate) fn watched_assetto_scheduling_fixture(
    plan: &ExecutionPlan,
    wake: Arc<osg_streams::NativeWake>,
) -> (GameSources, PreparedGame, impl FnOnce()) {
    let game = crate::game_telemetry::Game::Corsa;
    let prepared = prepared(game.id(), json!({"source":"shared-memory","freshnessMs":100}),
        crate::game_telemetry::catalogue_for_game(game.id()));
    let mut source = authored_sources(plan, Some(prepared.clone())).unwrap();
    source.reader = Some(Reader::Acr {
        source: Some(Box::new(crate::game_telemetry::FixtureSource::new_for_game(game))),
        decoder: crate::game_telemetry::Session::new_for_game(100, game).unwrap(),
        outputs: vec![],
    });
    source.acquired = true;
    source.set_wake(Some(wake.clone()));
    let (watch, close) = crate::acr_windows::Watch::fixture(wake.clone());
    source.install_assetto_watch_fixture(watch);
    source.validation.start(Some(Arc::downgrade(&wake)), || Ok(Instant::now()));
    (source, prepared, close)
}

pub(crate) fn watched_xplane_scheduling_fixture(
    plan: &ExecutionPlan,
    games: &GamesRuntime,
    wake: Arc<osg_streams::NativeWake>,
) -> (GameSources, PreparedGame, impl Fn(u16)) {
    let prepared = prepared("x-plane-12", crate::xplane::defaults(12), crate::xplane::catalogue());
    let mut source = authored_sources(plan, Some(prepared.clone())).unwrap();
    let (reader, publish) = crate::xplane::Source::inspector_fixture();
    source.reader = Some(Reader::Xplane {
        connection: prepared.connection.clone(),
        outputs: vec![],
        canonical_outputs: vec![],
        source: Some(reader),
    });
    source.acquired = true;
    source.set_wake(Some(wake.clone()));
    source.refresh_source_wakes(games).unwrap();
    assert!(!source.watched_game_source(), "The watch alone cannot replace proof");
    source.validation.start(Some(Arc::downgrade(&wake)), || Ok(Instant::now()));
    (source, prepared, publish)
}

pub(crate) fn watched_dcs_scheduling_fixture(
    games: &GamesRuntime,
    wake: Arc<osg_streams::NativeWake>,
) -> (GameSources, PreparedGame, tempfile::TempDir) {
    let fixture = Dcs::new();
    let mut source = fixture.sources();
    let Some(Reader::Dcs { network, transport, decoder, .. }) = &mut source.reader else { panic!() };
    network.listen_port = 0;
    *transport = Some(crate::dcs_bios::decoded::Source::open(network, decoder.decode_spec().unwrap()).unwrap());
    source.set_wake(Some(wake.clone()));
    source.refresh_source_wakes(games).unwrap();
    source.validation.start(Some(Arc::downgrade(&wake)), || Ok(Instant::now()));
    assert!(!source.watched_game_source(), "DCS also requires a catalogue proof worker");
    source.catalogue_validation.start(Some(Arc::downgrade(&wake)), || Ok(Instant::now()));
    (source, fixture.game, fixture.folder)
}

#[test]
fn dcs_scheduling_requires_ownship_watch_when_selected_and_no_bios_retry() {
    let folder = crate::test_directory();
    let games = wake_test_games(folder.path());
    let (mut source, _, _catalogue) =
        watched_dcs_scheduling_fixture(&games, Arc::new(osg_streams::NativeWake::default()));
    assert!(source.watched_game_source());
    let Some(Reader::Dcs { physics_outputs, .. }) = &mut source.reader else { panic!() };
    physics_outputs.push(json!({"id":"fixture-ownship"}));
    assert!(!source.watched_game_source(), "Ownship must wake independently of BIOS");
    let Some(Reader::Dcs { physics_outputs, .. }) = &mut source.reader else { panic!() };
    physics_outputs.clear();
    source.bios_retry_at = Some(Instant::now());
    assert!(!source.watched_game_source(), "Pending BIOS retry retains fallback");
}

#[test]
fn passive_physics_api_cannot_acquire_an_unobserved_process_or_manufacture_a_wake() {
    let folder = crate::test_directory();
    let games = wake_test_games(folder.path());
    let wake = Arc::new(osg_streams::NativeWake::default());
    for epoch in [1, 2] {
        let process = crate::game_discovery::ProcessIdentity {
            pid: 42, path: "DCS.exe".into(), epoch,
        };
        assert!(games.dcs_physics_snapshot(&process).is_none());
        assert!(games.dcs_physics_watch(&process, wake.clone()).unwrap().is_none());
    }
    assert_eq!(wake.observed(), 0, "A passive lookup cannot create a producer or grant a baseline");
}

#[test]
fn native_bios_commits_wake_a_parked_profile_reader_without_service_polling() {
    let (mut source, address, folder) = baseline_reader();
    source.authored_fixture = false;
    let games = wake_test_games(folder.path());
    let wake = Arc::new(osg_streams::NativeWake::default());
    source.set_wake(Some(wake.clone()));
    source.refresh_source_wakes(&games).unwrap();
    let (ready, started) = std::sync::mpsc::channel();
    let worker_wake = wake.clone();
    let worker = std::thread::spawn(move || {
        let observed = worker_wake.observed();
        let clock = source.clock;
        let initial = source.poll(&games, || clock.now_us());
        assert!(!initial.fresh);
        ready.send(observed).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let observed = worker_wake.observed();
            let clock = source.clock;
            let poll = source.poll(&games, || clock.now_us());
            if poll.fresh {
                assert!(poll.frames.iter().any(|frame| match frame {
                    Admission::Native(frame) => frame.changes.iter()
                        .any(|change| matches!(change.sample, SignalSample::Known(_))),
                    _ => false,
                }));
                source.stop();
                return;
            }
            assert!(Instant::now() < deadline, "Producer commits did not wake routing");
            // The full timeout is a test failure guard, not an adapter service tick.
            worker_wake.wait(observed, deadline.saturating_duration_since(Instant::now()));
        }
    });
    wake.bind_thread(worker.thread().clone()).unwrap();
    let observed = started.recv_timeout(Duration::from_secs(1)).unwrap();
    let producer = UdpSocket::bind("127.0.0.1:0").unwrap();
    producer.send_to(&bytes(1, true), address).unwrap();
    producer.send_to(&bytes(2, true), address).unwrap();
    worker.join().unwrap();
    assert!(wake.observed() > observed);
    // Notification leases retain only journal state, never the native socket.
    UdpSocket::bind(address).unwrap();
}

#[test]
fn unavailable_notification_capacity_does_not_withdraw_valid_native_samples() {
    let (mut source, address, folder) = baseline_reader();
    source.authored_fixture = false;
    let games = wake_test_games(folder.path());
    let wake = Arc::new(osg_streams::NativeWake::default());
    let Some(Reader::Dcs { transport: Some(transport), .. }) = source.reader.as_ref() else { panic!() };
    // The real data reader occupies one of the 64 combined slots.
    let watches = (0..63).map(|_| transport.watch(wake.clone()).unwrap()).collect::<Vec<_>>();
    source.set_wake(Some(wake));
    assert!(source.refresh_source_wakes(&games).is_err());
    let producer = UdpSocket::bind("127.0.0.1:0").unwrap();
    for counter in 1..=2 {
        producer.send_to(&bytes(counter, true), address).unwrap();
        let now = source.clock.now_us();
        receive_counter(&mut source, u16::from(counter), now);
    }
    let clock = source.clock;
    let poll = source.poll(&games, || clock.now_us());
    assert!(poll.fresh);
    assert!(!poll.context_lost);
    assert!(poll.diagnostics.iter().any(|message| message.contains("notifications are unavailable")));
    drop(watches);
    source.refresh_source_wakes(&games).unwrap();
    source.stop();
}
