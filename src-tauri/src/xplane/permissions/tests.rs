use super::*;
use std::{sync::mpsc, time::Duration};
fn identity() -> ProcessIdentity {
    ProcessIdentity {
        pid: 1,
        epoch: uuid::Uuid::new_v4().as_u128() as u64,
        path: format!("C:/permission-test/{}/X-Plane.exe", uuid::Uuid::new_v4()),
    }
}
fn observed(game: &str, wanted: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let state = status(game);
        if state["state"] == wanted {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "job did not reach {wanted}: {state}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn passive_status_and_failed_acquisition_never_repeat_the_permission_probe() {
    let game = format!("test-{}", uuid::Uuid::new_v4());
    let expected = identity();
    assert_eq!(status(&game)["state"], "waiting");
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = count.clone();
    begin_with(&game, expected.clone(), false, move |_, _| {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err("Native permission denied.".into())
    })
    .unwrap();
    observed(&game, "needs-permission");
    for _ in 0..5 {
        assert!(acquire(&game, &expected).is_err());
    }
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    cancel(&game);
    assert_eq!(status(&game)["state"], "disabled");
    assert!(acquire(&game, &expected).is_err());
}
#[test]
fn cancelled_or_replaced_permission_work_cannot_overwrite_the_new_job() {
    let game = format!("test-{}", uuid::Uuid::new_v4());
    let expected = identity();
    let (entered, entry) = mpsc::sync_channel(1);
    let (resume, wait) = mpsc::sync_channel(1);
    let original = begin_with(&game, expected.clone(), true, move |_, cancelled| {
        entered.send(()).unwrap();
        wait.recv().unwrap();
        assert!(*cancelled.borrow());
        Err("old failure must be discarded".into())
    })
    .unwrap()
    .unwrap();
    entry.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(status(&game)["state"], "starting");
    cancel(&game);
    assert_eq!(status(&game)["state"], "disabled");
    begin_with(&game, expected, true, |_, _| Err("new failure".into())).unwrap();
    let new = observed(&game, "failed");
    assert_eq!(new["details"], "new failure");
    resume.send(()).unwrap();
    original.join().unwrap();
    assert_eq!(status(&game)["details"], "new failure");
}

#[test]
fn a_startup_worker_panic_becomes_a_failed_job_without_authority() {
    let game = format!("test-{}", uuid::Uuid::new_v4());
    begin_with(&game, identity(), true, |_, _| {
        panic!("injected startup crash")
    })
    .unwrap()
    .unwrap()
    .join()
    .unwrap();
    assert!(observed(&game, "failed")["details"]
        .as_str()
        .unwrap()
        .contains("panicked"));
}

#[test]
fn acquisition_caches_a_direct_capability_failure_without_requesting_elevation() {
    let game = format!("test-{}", uuid::Uuid::new_v4());
    let mut expected = identity();
    expected.pid = u32::MAX;
    assert!(acquire(&game, &expected).err().unwrap().contains("Preparing"));
    let deadline = Instant::now() + Duration::from_secs(2);
    let failed = loop {
        let state = status(&game);
        if matches!(state["state"].as_str(), Some("failed" | "needs-permission")) {
            break state;
        }
        assert!(Instant::now() < deadline, "Capability probe did not fail: {state}");
        std::thread::yield_now();
    };
    assert!(failed["details"].as_str().is_some_and(|message| !message.is_empty()));
    assert!(acquire(&game, &expected).is_err());
    assert_eq!(status(&game), failed, "A failed capability check must not retry from reads");
    cancel(&game);
}

#[test]
fn ready_watchers_are_shared_without_new_permission_work_and_cancel_revokes_them() {
    let game = format!("test-{}", uuid::Uuid::new_v4());
    let expected = identity();
    let (monitor, failure) = crate::xplane::process_events::test_monitor();
    begin_with(&game, expected.clone(), true, |_, _| Ok(monitor)).unwrap().unwrap().join().unwrap();
    assert_eq!(status(&game)["state"], "ready");
    let first = acquire(&game, &expected).unwrap();
    let second = acquire(&game, &expected).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert!(enable("unsupported-game", expected.clone()).is_err());
    if cfg!(windows) { enable("x-plane-12", expected.clone()).unwrap(); }
    else { assert!(enable("x-plane-12", expected.clone()).is_err()); }
    // Native failure is reported without a status read renewing or restarting it.
    first.cancel();
    assert_eq!(status(&game)["state"], "failed");
    assert!(acquire(&game, &expected).is_err());
    cancel(&game);
    assert_eq!(status(&game)["state"], "disabled");
    assert!(failure.borrow().is_some());
}

#[test]
fn a_cancelled_late_success_is_revoked_without_replacing_the_new_ready_watcher() {
    let game = format!("test-{}", uuid::Uuid::new_v4());
    let expected = identity();
    let (old, old_failure) = crate::xplane::process_events::test_monitor();
    let (entered, entry) = mpsc::sync_channel(1);
    let (resume, wait) = mpsc::sync_channel(1);
    let original = begin_with(&game, expected.clone(), true, move |_, _| {
        entered.send(()).unwrap(); wait.recv().unwrap(); Ok(old)
    }).unwrap().unwrap();
    entry.recv_timeout(Duration::from_secs(2)).unwrap();
    if cfg!(windows) { assert!(enable("x-plane-12", expected.clone()).unwrap_err().contains("pending")); }
    cancel(&game);
    let (new, new_failure) = crate::xplane::process_events::test_monitor();
    begin_with(&game, expected.clone(), true, |_, _| Ok(new)).unwrap().unwrap().join().unwrap();
    resume.send(()).unwrap(); original.join().unwrap();
    assert!(old_failure.borrow().is_some());
    assert!(new_failure.borrow().is_none());
    assert_eq!(status(&game)["state"], "ready");
    assert!(acquire(&game, &expected).unwrap().error().is_none());
    cancel(&game);
}

#[cfg(windows)]
#[test]
fn assetto_permission_job_rejects_the_wrong_mapping_family_before_consent() {
    let game = "assetto-corsa-evo";
    let expected = ProcessIdentity {
        path: format!("C:/permission-test/{}/acs.exe", uuid::Uuid::new_v4()),
        ..identity()
    };
    enable(game, expected.clone()).unwrap();
    let failed = observed(game, "failed");
    assert!(failed["details"].as_str().unwrap().contains("monitoring scope"));
    assert!(acquire(game, &expected).is_err());
}
