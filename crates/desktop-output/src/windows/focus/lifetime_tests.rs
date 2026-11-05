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
}
