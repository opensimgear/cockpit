use super::*;
use super::super::{Shared, lifetime::Exit};
use std::{sync::{atomic::{AtomicBool, Ordering}, mpsc}, thread, time::Duration};

struct Cleanup(Arc<AtomicBool>);
impl Drop for Cleanup {
    fn drop(&mut self) { self.0.store(true, Ordering::Release); }
}

#[test]
fn replacement_waits_for_exit_and_releases_registry_while_old_cleanup_is_pending() {
    let shared = Arc::new(Shared::new());
    let registry = Arc::new(Mutex::new(Registry { owner: Default::default(), shared: Arc::downgrade(&shared) }));
    let cleaned = Arc::new(AtomicBool::new(false));
    let (ready, started) = mpsc::sync_channel(1);
    let (release, cleanup) = mpsc::sync_channel(1);
    let observer = shared.clone();
    let resources = cleaned.clone();
    let native = thread::spawn(move || {
        let _exited = Exit::enter(observer.clone());
        let _resources = Cleanup(resources);
        observer.close();
        ready.send(()).unwrap();
        cleanup.recv().unwrap();
    });
    started.recv_timeout(Duration::from_secs(1)).unwrap();
    let (notice, waiting) = mpsc::sync_channel(1);
    shared.lifetime.notify_wait(notice);
    let replacing = registry.clone();
    let cleanup_complete = cleaned.clone();
    let (finished, answer) = mpsc::sync_channel(1);
    let replacement = thread::spawn(move || {
        finished.send(with_available(&replacing, Instant::now() + Duration::from_secs(2), |_, owner| {
            assert!(owner.is_none());
            assert!(cleanup_complete.load(Ordering::Acquire), "A replacement must never overlap old native resources");
            Ok(())
        })).unwrap();
    });
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(answer.try_recv().is_err());
    // The waiter has announced its actual Condvar wait. Holding this lock while
    // allowing cleanup proves retirement waiting does not retain the registry.
    let slot = registry.lock().unwrap();
    release.send(()).unwrap();
    native.join().unwrap();
    assert!(shared.lifetime.exited());
    assert!(answer.try_recv().is_err());
    drop(slot);
    answer.recv_timeout(Duration::from_secs(1)).unwrap().unwrap();
    replacement.join().unwrap();
}

#[test]
fn own_observer_cannot_wait_for_a_callback_to_return_before_reopening() {
    let shared = Arc::new(Shared::new());
    let _exited = Exit::enter(shared.clone());
    shared.close();
    let registry = Mutex::new(Registry { owner: Default::default(), shared: Arc::downgrade(&shared) });
    let started = Instant::now();
    let error = with_available(&registry, started + Duration::from_secs(5), |_, _| Ok(())).unwrap_err();
    assert!(error.contains("own retirement"));
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(!shared.lifetime.exited());
}

#[test]
fn stalled_retirement_returns_a_bounded_truthful_error_without_creating_an_owner() {
    let shared = Arc::new(Shared::new());
    shared.close();
    let registry = Mutex::new(Registry { owner: Default::default(), shared: Arc::downgrade(&shared) });
    let entered = AtomicBool::new(false);
    let started = Instant::now();
    let error = with_available(&registry, started + Duration::from_millis(5), |_, _| {
        entered.store(true, Ordering::Release);
        Ok(())
    }).unwrap_err();
    assert!(error.contains("retirement did not complete"));
    assert!(!entered.load(Ordering::Acquire));
    assert!(!shared.lifetime.exited());
    assert!(started.elapsed() < Duration::from_secs(1));
}
