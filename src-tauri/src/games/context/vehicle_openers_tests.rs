use super::*;
use std::sync::mpsc;

#[test]
fn blocked_open_does_not_hold_up_another_integration_and_pending_scope_coalesces() {
    let scratch = Scratch::new();
    let platform = Arc::new(FakePlatform { context_supported: false, ..FakePlatform::new() });
    let runtime = fixture_runtime(Arc::new(|_| panic!("Passive open cannot submit output")),
        Paths::in_directory(&scratch.path), platform);
    let inner = &runtime.inner;
    let wakes = Arc::new(crate::game_vehicles::CaptureWakes::default());
    inner.update_vehicles(|vehicles| vehicles.watch_openers(wakes.clone()));
    let blocked = "assetto-corsa";
    let healthy = "assetto-corsa-rally";
    let queue = |integration, pid, epoch| {
        let game = json!({
            "id": integration, "integrationKey": integration, "integrationCompatible": true,
            "provider": crate::game_discovery::integration_provider(integration),
            "installed": true, "executable": "fixture.exe",
        });
        let process = ProcessIdentity { pid, epoch, path: "fixture.exe".into() };
        inner.update_vehicles(|vehicles| {
            let work = vehicles.begin_observation(&game, &json!({}), Some(process), "setup", true, epoch).unwrap();
            assert!(vehicles.defer_open(work).is_none());
        });
    };

    let stop = Arc::new(AtomicBool::new(false));
    let (opened, opened_rx) = mpsc::channel();
    let (released, released_rx) = mpsc::channel();
    let blocked_worker = {
        let inner = inner.clone();
        let stop = stop.clone();
        let wake = wakes.get(blocked).unwrap().clone();
        let opened = opened.clone();
        thread::spawn(move || {
            vehicle_openers::run(&inner, &stop, blocked, &wake, |work| {
                let pid = work.test_opening_pid();
                opened.send((blocked, pid)).unwrap();
                if pid == 10 {
                    let _ = released_rx.recv_timeout(Duration::from_secs(8));
                }
            })
        })
    };
    let healthy_worker = {
        let inner = inner.clone();
        let stop = stop.clone();
        let wake = wakes.get(healthy).unwrap().clone();
        thread::spawn(move || {
            vehicle_openers::run(&inner, &stop, healthy, &wake, |work| {
                opened.send((healthy, work.test_opening_pid())).unwrap();
            })
        })
    };
    let timeout = Duration::from_secs(2);
    queue(blocked, 10, 1);
    let first = opened_rx.recv_timeout(timeout);
    queue(blocked, 11, 2);
    queue(blocked, 12, 3);
    queue(healthy, 20, 4);
    let independent = opened_rx.recv_timeout(timeout);
    let _ = released.send(());
    let replacement = opened_rx.recv_timeout(timeout);
    stop.store(true, Ordering::Release);
    wakes.notify_all();
    blocked_worker.join().unwrap();
    healthy_worker.join().unwrap();

    assert_eq!(first, Ok((blocked, 10)));
    assert_eq!(independent, Ok((healthy, 20)));
    assert_eq!(replacement, Ok((blocked, 12)));
    runtime.shutdown();
}
