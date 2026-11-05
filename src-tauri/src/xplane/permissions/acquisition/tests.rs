use super::*;
use std::{sync::{atomic::{AtomicBool, Ordering}, mpsc}, time::Duration};

fn identity() -> ProcessIdentity {
    ProcessIdentity { pid: 1, epoch: uuid::Uuid::new_v4().as_u128() as u64,
        path: format!("C:/permission-wait/{}/acs.exe", uuid::Uuid::new_v4()) }
}

#[test]
fn setup_wait_receives_quiet_startup_completion_without_restarting_the_monitor() {
    let game = format!("wait-{}", uuid::Uuid::new_v4());
    let expected = identity();
    let (resume, paused) = mpsc::sync_channel(0);
    let (monitor, failure) = crate::xplane::process_events::test_monitor();
    let startup = begin_with(&game, expected.clone(), false, move |_, _| {
        paused.recv().unwrap(); Ok(monitor)
    }).unwrap().unwrap();
    let wake = Arc::new(NativeWake::default());
    let peer = wake.clone();
    let waiting_game = game.clone();
    let waiting = std::thread::spawn(move || {
        peer.bind_thread(std::thread::current()).unwrap();
        acquire_until(&waiting_game, &expected, peer, Instant::now() + Duration::from_secs(2), || false)
    });
    resume.send(()).unwrap();
    startup.join().unwrap();
    let acquired = waiting.join().unwrap().unwrap();
    assert!(acquired.error().is_none());
    assert!(failure.borrow().is_none());
    assert_eq!(status(&game)["state"], "ready");
    cancel(&game);
    assert!(acquired.error().is_some());
}

#[test]
fn cancellation_wakes_pending_setup_without_granting_or_cancelling_shared_authority() {
    let game = format!("wait-cancel-{}", uuid::Uuid::new_v4());
    let expected = identity();
    let (resume, paused) = mpsc::sync_channel(0);
    let (monitor, failure) = crate::xplane::process_events::test_monitor();
    let startup = begin_with(&game, expected.clone(), false, move |_, _| {
        paused.recv().unwrap(); Ok(monitor)
    }).unwrap().unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let wake = Arc::new(NativeWake::default());
    let peer = wake.clone();
    let waiting_game = game.clone();
    let (finished, result) = mpsc::sync_channel(1);
    let waiting = std::thread::spawn(move || {
        peer.bind_thread(std::thread::current()).unwrap();
        let result = acquire_until(&waiting_game, &expected, peer,
            Instant::now() + Duration::from_secs(30), || flag.load(Ordering::Acquire));
        finished.send(result.err()).unwrap();
    });
    assert!(result.recv_timeout(Duration::from_millis(30)).is_err());
    cancelled.store(true, Ordering::Release);
    wake.notify();
    assert!(result.recv_timeout(Duration::from_secs(2)).unwrap().unwrap().contains("cancelled"));
    waiting.join().unwrap();
    assert_eq!(status(&game)["state"], "checking");
    resume.send(()).unwrap();
    startup.join().unwrap();
    assert!(failure.borrow().is_none(), "Cancelling setup cannot revoke another reader's monitor");
    cancel(&game);
}

#[test]
fn deadline_and_terminal_denial_do_not_report_pending_startup_as_a_result() {
    let game = format!("wait-deadline-{}", uuid::Uuid::new_v4());
    let expected = identity();
    let (resume, paused) = mpsc::sync_channel(0);
    let startup = begin_with(&game, expected.clone(), false, move |_, _| {
        paused.recv().unwrap(); Err("Native permission denied.".into())
    }).unwrap().unwrap();
    let wake = Arc::new(NativeWake::default());
    wake.bind_thread(std::thread::current()).unwrap();
    let error = acquire_until(&game, &expected, wake.clone(), Instant::now(), || false).err().unwrap();
    assert!(error.contains("timed out"));
    assert_eq!(status(&game)["state"], "checking");
    resume.send(()).unwrap();
    startup.join().unwrap();
    let error = acquire_until(&game, &expected, wake, Instant::now(), || false).err().unwrap();
    assert_eq!(error, "Native permission denied.");
    assert_eq!(status(&game)["state"], "needs-permission");
    cancel(&game);
}
