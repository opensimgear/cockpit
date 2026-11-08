use super::*;
use crate::game_telemetry::{CaptureOwner, CaptureSet, RawSample, Source, SourceIdentity};
use std::sync::mpsc;

struct FixtureSource(Box<dyn FnMut() -> Option<RawSample> + Send>);

impl Source for FixtureSource {
    fn poll(&mut self) -> Result<Option<RawSample>, String> {
        Ok(self.0())
    }

    fn close(&mut self) {}
}

fn capture(owner: &mut CaptureOwner, packet: u32) -> RawSample {
    let now = Instant::now();
    let physics = owner.capture(now).unwrap();
    let graphics = owner.capture(now).unwrap();
    let statics = owner.capture(now).unwrap();
    let mut physics_page = include_bytes!("../../../../tests/fixtures/acr/live.physics.bin").to_vec();
    physics_page[..4].copy_from_slice(&packet.to_le_bytes());
    RawSample {
        source: SourceIdentity { id: "fixture".into(), epoch: 1 },
        physics: physics_page,
        graphics: include_bytes!("../../../../tests/fixtures/acr/live.graphics.bin").to_vec(),
        statics: include_bytes!("../../../../tests/fixtures/acr/live.static.bin").to_vec(),
        capture: Some(CaptureSet::new(owner, physics, graphics, statics).unwrap()),
    }
}

#[test]
fn blocked_integration_cannot_delay_another_integration_capture() {
    let scratch = Scratch::new();
    let platform = Arc::new(FakePlatform { context_supported: false, ..FakePlatform::new() });
    let runtime = fixture_runtime(
        Arc::new(|_| panic!("Passive telemetry cannot submit output")),
        Paths::in_directory(&scratch.path),
        platform,
    );
    let game = |id: &str| json!({
        "id": id, "integrationKey": id, "integrationCompatible": true,
        "provider": crate::game_discovery::integration_provider(id),
        "installed": true, "executable": format!("{id}.exe"),
    });
    let blocked_game = game("assetto-corsa");
    let healthy_game = game("assetto-corsa-rally");
    runtime.seed_test_catalogue(vec![blocked_game.clone(), healthy_game.clone()]);

    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let mut blocked = Some(move || {
        entered.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(10));
        let mut owner = CaptureOwner::new();
        owner.begin_generation().unwrap();
        capture(&mut owner, 10)
    });
    let blocked_source = FixtureSource(Box::new(move || blocked.take().map(|read| read())));
    let mut owner = CaptureOwner::new();
    owner.begin_generation().unwrap();
    let mut packets = [10, 11].into_iter();
    let healthy_source = FixtureSource(Box::new(move || packets.next().map(|id| capture(&mut owner, id))));
    let inner = &runtime.inner;
    let blocked_revision = inner.setup.vehicle_setup_revision(&blocked_game);
    let healthy_revision = inner.setup.vehicle_setup_revision(&healthy_game);
    let (blocked_wake, healthy_wake) = inner.update_vehicles(|vehicles| {
        vehicles.watch_capture_lanes(inner.changes.vehicle_capture_wakes.clone());
        (
            vehicles.seed_capture_reader(&blocked_game, &blocked_revision, Box::new(blocked_source)),
            vehicles.seed_capture_reader(&healthy_game, &healthy_revision, Box::new(healthy_source)),
        )
    });
    let published = &inner.changes.wake;
    published.bind_thread(thread::current()).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let workers = capture_workers::start(inner.clone(), stop.clone());

    blocked_wake.notify();
    let entered = entered_rx.recv_timeout(Duration::from_secs(2)).is_ok();
    let wait_for_capture = |status| {
        healthy_wake.notify();
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            let observed = published.observed();
            if inner.update_vehicles(|vehicles| vehicles.telemetry(&healthy_game))
                .is_some_and(|capture| capture.status == status)
            {
                break true;
            }
            let remaining = until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break false;
            }
            published.wait(observed, remaining);
        }
    };
    let healthy_live = entered && wait_for_capture("waiting") && wait_for_capture("live");
    runtime.seed_test_catalogue(vec![healthy_game]);
    let _ = release.send(());
    stop.store(true, Ordering::Release);
    inner.changes.vehicle_capture_wakes.notify_all();
    for worker in workers {
        worker.join().unwrap();
    }

    assert!(entered, "The blocked fixture must enter its native read");
    assert!(healthy_live, "The healthy adapter must publish while the other read is still blocked");
    assert!(inner.update_vehicles(|vehicles| vehicles.telemetry(&blocked_game)).is_none(),
        "A read that finished after removal must not publish");
    runtime.shutdown();
}

#[test]
fn shared_inventory_loss_and_blocked_assetto_preserve_independent_dcs_and_xplane_captures() {
    use crate::game_discovery::ProcessLookupError;
    use std::net::UdpSocket;
    use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

    let scratch = Scratch::new();
    let platform = Arc::new(FakePlatform { context_supported: false, ..FakePlatform::new() });
    let runtime = fixture_runtime(
        Arc::new(|_| panic!("Passive telemetry cannot submit output")),
        Paths::in_directory(&scratch.path),
        platform,
    );
    runtime.stop_test_context_observer();
    let [assetto, dcs, xplane] = ["assetto-corsa-rally", "dcs", "x-plane-12"].map(|id| json!({
        "id": id, "integrationKey": id, "integrationCompatible": true,
        "provider": crate::game_discovery::integration_provider(id),
        "installed": true, "executable": format!("{id}.exe"),
    }));
    runtime.seed_test_catalogue(vec![assetto.clone(), dcs.clone(), xplane.clone()]);
    let inner = &runtime.inner;
    let [assetto_setup, dcs_setup, xplane_setup] = [&assetto, &dcs, &xplane]
        .map(|game| inner.setup.vehicle_setup_revision(game));

    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let (returned, returned_rx) = mpsc::channel();
    let mut read = Some(move || {
        entered.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(10));
        let mut owner = CaptureOwner::new();
        owner.begin_generation().unwrap();
        let sample = capture(&mut owner, 10);
        let _ = returned.send(());
        sample
    });
    let source = FixtureSource(Box::new(move || read.take().map(|read| read())));
    let (assetto_wake, dcs_address, close_dcs_watch, send_xplane) = inner.update_vehicles(|vehicles| {
        vehicles.watch_capture_lanes(inner.changes.vehicle_capture_wakes.clone());
        let assetto_wake = vehicles.seed_capture_reader(&assetto, &assetto_setup, Box::new(source));
        let (address, close_watch) = vehicles.seed_dcs_capture_reader(&dcs, &dcs_setup);
        let send_xplane = vehicles.seed_xplane_capture_reader(&xplane, &xplane_setup);
        (assetto_wake, address, close_watch, send_xplane)
    });
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    let send_dcs = |counter: u8| {
        let mut frame = vec![0x55; 4];
        let mut aircraft = [0; 24];
        aircraft[..5].copy_from_slice(b"A-10C");
        frame.extend_from_slice(&[0, 0, 24, 0]);
        frame.extend_from_slice(&aircraft);
        frame.extend_from_slice(&[254, 255, 2, 0, counter, 0]);
        sender.send_to(&frame, dcs_address).unwrap();
    };

    let changed = &inner.changes.wake;
    changed.bind_thread(thread::current()).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let workers = capture_workers::start(inner.clone(), stop.clone());
    let current = |game: &Value| inner.update_vehicles(|vehicles| vehicles.telemetry(game));
    let wait_for_live_packet = |game: &Value, packet: u64| {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let observed = changed.observed();
            if let Some(capture) = current(game).filter(|capture| capture.status == "live" && capture.packet == packet) {
                break capture;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "Missing live packet {packet} for {}", game["id"]);
            changed.wait(observed, remaining);
        }
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        assetto_wake.notify();
        entered_rx.recv_timeout(Duration::from_secs(2)).expect("Assetto read did not block");
        send_dcs(1);
        send_dcs(2);
        send_xplane(1., 110.);
        send_xplane(2., 115.);
        let initial_dcs = wait_for_live_packet(&dcs, 2);
        let initial_xplane = wait_for_live_packet(&xplane, 1);
        assert_eq!(initial_dcs.context["aircraft"], "A-10C");
        assert_eq!(initial_xplane.context["aircraft"], "Aircraft/Test.acf");

        // A failed shared process inventory cannot renew or withdraw the
        // already-admitted readers, including the detached Assetto reader.
        inner.update_vehicles(|vehicles| {
            for (game, setup) in [(&assetto, &assetto_setup), (&dcs, &dcs_setup), (&xplane, &xplane_setup)] {
                assert!(vehicles.begin_inventory_observation(
                    game, &json!({}), Err(ProcessLookupError::Unavailable("Fixture inventory contention".into())),
                    setup, true, 2,
                ).is_none());
            }
        });
        assert!(Arc::ptr_eq(&current(&dcs).unwrap(), &initial_dcs));
        assert!(Arc::ptr_eq(&current(&xplane).unwrap(), &initial_xplane));

        send_xplane(3., 120.);
        let next_xplane = wait_for_live_packet(&xplane, 2);
        assert_eq!(next_xplane.context, initial_xplane.context);
        assert!(Arc::ptr_eq(&current(&dcs).unwrap(), &initial_dcs), "Quiet DCS must retain its original capture");
        send_dcs(3);
        let next_dcs = wait_for_live_packet(&dcs, 3);
        assert_eq!(next_dcs.context["epoch"], initial_dcs.context["epoch"]);
        assert!(Arc::ptr_eq(&current(&xplane).unwrap(), &next_xplane), "Quiet X-Plane must retain its original capture");
        assert!(matches!(returned_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }));

    runtime.seed_test_catalogue(vec![dcs.clone(), xplane.clone()]);
    let _ = release.send(());
    let returned = returned_rx.recv_timeout(Duration::from_secs(2)).is_ok();
    stop.store(true, Ordering::Release);
    inner.changes.vehicle_capture_wakes.notify_all();
    for worker in workers {
        worker.join().unwrap();
    }
    let retired = current(&assetto).is_none();
    close_dcs_watch();
    runtime.shutdown();
    if let Err(error) = result { resume_unwind(error); }
    assert!(returned, "The blocked read must return after removal");
    assert!(retired, "A capture returned after removal must not publish");
}
