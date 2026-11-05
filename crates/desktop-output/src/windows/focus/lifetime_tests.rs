use super::*;
use std::{sync::{atomic::{AtomicBool, Ordering}, mpsc}, time::Duration};

struct Resources(Arc<AtomicBool>);
impl Drop for Resources {
    fn drop(&mut self) { self.0.store(true, Ordering::Release); }
}

#[test]
fn exit_guard_notifies_after_failed_startup_resources_unwind() {
    let shared = Arc::new(Shared::new());
    let cleaned = Arc::new(AtomicBool::new(false));
    let observer = shared.clone();
    let resources = cleaned.clone();
    let (ready, arrival) = mpsc::sync_channel(1);
    let (release, failed) = mpsc::sync_channel(1);
    let thread = thread::spawn(move || {
        let _exited = Exit::enter(observer.clone());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _resources = Resources(resources);
            ready.send(()).unwrap();
            failed.recv().unwrap();
            panic!("failed initialization");
        }));
        assert!(result.is_err());
        observer.fail("Native startup failed.".into());
        assert!(!observer.lifetime.exited(), "Terminal failure is not cleanup completion");
    });
    arrival.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(shared.lifetime.wait(Instant::now() + Duration::from_millis(2)).is_err());
    assert!(!cleaned.load(Ordering::Acquire));
    release.send(()).unwrap();
    shared.lifetime.wait(Instant::now() + Duration::from_secs(1)).unwrap();
    assert!(cleaned.load(Ordering::Acquire));
    assert!(shared.terminal().unwrap().contains("startup failed"));
    thread.join().unwrap();
}

#[test]
fn completion_before_wait_is_retained_without_an_extra_notification() {
    let lifetime = Lifetime::new();
    lifetime.finish();
    lifetime.wait(Instant::now()).unwrap();
    assert!(lifetime.exited());
    assert!(!lifetime.with_live_resources(|| panic!("Stop posted after completion")));
}

#[test]
fn stop_post_and_teardown_are_serialized_before_native_identifiers_can_be_reused() {
    let lifetime = Arc::new(Lifetime::new());
    let posting = lifetime.clone();
    let (entered, observed_post) = mpsc::sync_channel(1);
    let (release, resume_post) = mpsc::sync_channel(1);
    let post = thread::spawn(move || {
        assert!(posting.with_live_resources(|| {
            entered.send(()).unwrap();
            resume_post.recv().unwrap();
        }));
    });
    observed_post.recv_timeout(Duration::from_secs(1)).unwrap();
    let retiring = lifetime.clone();
    let (started, observed_start) = mpsc::sync_channel(1);
    let (complete, observed_teardown) = mpsc::sync_channel(1);
    let teardown = thread::spawn(move || {
        started.send(()).unwrap();
        retiring.begin_teardown();
        complete.send(()).unwrap();
    });
    observed_start.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(lifetime.state.try_lock().is_err(), "Stop posting must retain the teardown guard");
    assert!(observed_teardown.try_recv().is_err());
    release.send(()).unwrap();
    post.join().unwrap();
    observed_teardown.recv_timeout(Duration::from_secs(1)).unwrap();
    teardown.join().unwrap();
    assert!(!lifetime.exited());
    assert!(!lifetime.with_live_resources(|| panic!("Stop posted during resource teardown")));
    lifetime.finish();
}
