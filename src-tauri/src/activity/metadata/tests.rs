use super::*;
use crate::activity::{Runtime, Snapshot};
use serde_json::json;
use std::sync::{atomic::{AtomicUsize, Ordering}, Mutex};
use std::time::Duration;

fn profile(name: &str) -> Snapshot {
    let mut snapshot = Snapshot::default();
    snapshot.profile_value("profile", &json!({"id":"profile","name":name,"revision":1}));
    snapshot
}

#[test]
fn scoped_capture_is_quiet_until_commit_and_keeps_racing_notifications() {
    let parent = Arc::new(NativeWake::default());
    let signal = Arc::new(NativeWake::child(parent.clone()));
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let change = signal.clone();
    let mut source = Source::new(Some(Group::Profiles), signal.clone(), Box::new(move || {
        if count.fetch_add(1, Ordering::SeqCst) == 0 { change.notify(); }
        profile("Current")
    }));
    let initial = source.take(false).unwrap();
    assert!(initial.states.contains_key("@profile"));
    assert!(!initial.states.contains_key("@controller"));
    assert_eq!(parent.observed(), 1);
    assert!(source.take(false).unwrap().events.is_empty(), "Commit racing capture must be drained again");
    assert!(source.take(false).is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    signal.notify();
    assert!(source.take(false).is_some());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn unrelated_source_cannot_acknowledge_a_profile_action_and_unavailable_metadata_is_retained() {
    let state = Arc::new(Mutex::new(profile("Saved")));
    let cache = state.clone();
    let mut source = Source::new(Some(Group::Profiles), Arc::new(NativeWake::default()),
        Box::new(move || cache.lock().unwrap().clone()));
    source.take(false).unwrap();
    let mut unavailable = Snapshot::default();
    unavailable.unavailable("profile");
    *state.lock().unwrap() = unavailable;
    let batch = source.take(true).unwrap();
    assert!(batch.events.is_empty());
    assert!(!batch.states.contains_key("@profile"));
    assert_eq!(source.previous.as_ref().unwrap().0.len(), 1);
    *state.lock().unwrap() = profile("Restored");
    assert_eq!(source.take(true).unwrap().events.len(), 1);
    let mut other = Source::new(Some(Group::Devices), Arc::new(NativeWake::default()), Box::new(Snapshot::default));
    let batch = other.take(false).unwrap();
    assert!(!batch.states.contains_key("@profile"));
    assert!(batch.states.contains_key("@module"));
}

#[test]
fn parked_writer_captures_only_the_changed_group_without_flush_or_interval_reads() {
    let directory = crate::test_directory();
    let runtime = Runtime::new(&directory.path().join("config.json"));
    let state = Arc::new(Mutex::new(profile("Original")));
    let profile_calls = Arc::new(AtomicUsize::new(0));
    let device_calls = Arc::new(AtomicUsize::new(0));
    let cache = state.clone();
    let calls = profile_calls.clone();
    runtime.observe_group(Group::Profiles, move || { calls.fetch_add(1, Ordering::SeqCst); cache.lock().unwrap().clone() });
    let calls = device_calls.clone();
    runtime.observe_group(Group::Devices, move || { calls.fetch_add(1, Ordering::SeqCst); Snapshot::default() });
    runtime.flush().unwrap();
    let before = (profile_calls.load(Ordering::SeqCst), device_calls.load(Ordering::SeqCst));
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!((profile_calls.load(Ordering::SeqCst), device_calls.load(Ordering::SeqCst)), before,
        "An idle writer must not capture metadata on the former 750ms interval");
    *state.lock().unwrap() = profile("Background commit");
    runtime.notify_metadata(Group::Profiles);
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        let rows = runtime.request(&json!({"command":"activity_query","committedOnly":true,
            "activityFilter":{"entityId":"profile","eventTypes":["profile_renamed"]}})).unwrap();
        if rows["activity"]["total"].as_u64() == Some(1) { break; }
        assert!(Instant::now() < deadline, "Background metadata was not committed: {rows}");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(device_calls.load(Ordering::SeqCst), before.1, "Unrelated devices must not be recaptured");
}
