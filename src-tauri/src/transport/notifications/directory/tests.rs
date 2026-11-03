use super::*;
use notify::event::{AccessKind, CreateKind};
use std::time::{Duration, Instant};

#[test]
fn callbacks_coalesce_bounded_work_and_ignore_unrelated_paths_and_closed_sources() {
    let target = PathBuf::from("/scope/advertisements");
    let pending = Pending::default();
    pending.event(Ok(Event::new(EventKind::Access(AccessKind::Any)).add_path(target.clone())), &target);
    pending.event(Ok(Event::new(EventKind::Create(CreateKind::File)).add_path("/unrelated/file".into())), &target);
    assert_eq!(pending.wake.observed(), 0);
    for _ in 0..1000 {
        pending.event(Ok(Event::new(EventKind::Create(CreateKind::File)).add_path(target.join("gateway.json"))), &target);
    }
    assert_eq!(pending.wake.observed(), 1);
    pending.dirty.store(false, Ordering::Release);
    pending.event(Err(notify::Error::generic("lost")), &target);
    assert!(pending.lost.load(Ordering::Acquire));
    assert_eq!(pending.wake.observed(), 2);
    pending.closed.store(true, Ordering::Release);
    pending.dirty.store(false, Ordering::Release);
    pending.event(Err(notify::Error::generic("closed")), &target);
    assert_eq!(pending.wake.observed(), 2);
}

fn await_change(wake: &NativeWake, before: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while wake.observed() == before {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "Native directory notification did not arrive");
        wake.wait_scheduled(before, Some(remaining));
    }
}

#[test]
fn native_notifications_rearm_missing_directories_parent_replacement_and_shutdown() {
    if matches!(RecommendedWatcher::kind(), WatcherKind::PollWatcher | WatcherKind::NullWatcher) { return; }
    let root = crate::test_directory();
    let parent = root.path().join("parent"); let target = parent.join("advertisements");
    let wake = Arc::new(NativeWake::default());
    wake.bind_thread(thread::current()).unwrap();
    let mut notifications = DirectoryNotifications::start(target.clone(), wake.clone()).unwrap();
    let before = wake.observed();
    std::fs::create_dir_all(&target).unwrap();
    await_change(&wake, before);
    let before = wake.observed();
    std::fs::write(target.join("gateway.json"), "advertisement").unwrap();
    await_change(&wake, before);
    // Advertisement directories are a Unix transport. Windows native directory
    // handles prevent renaming a watched parent; its device owner uses CM events.
    #[cfg(unix)] {
        let before = wake.observed();
        std::fs::rename(&parent, root.path().join("retired")).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        await_change(&wake, before);
    }
    assert!(notifications.error().is_none());
    notifications.shutdown();
    let before = wake.observed();
    std::fs::write(target.join("after-shutdown.json"), "ignored").unwrap();
    assert_eq!(wake.observed(), before);
    assert!(notifications.worker.is_none());
    notifications.shutdown();
}

#[test]
fn initialization_failure_never_leaves_a_worker_running() {
    let wake = Arc::new(NativeWake::default());
    assert!(DirectoryNotifications::start("relative/path".into(), wake.clone()).is_err());
    let root = crate::test_directory();
    let leaf = root.path().join("file"); std::fs::write(&leaf, "not a directory").unwrap();
    assert!(DirectoryNotifications::start(leaf.join("advertisements"), wake).is_err());
}
