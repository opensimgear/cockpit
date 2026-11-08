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
