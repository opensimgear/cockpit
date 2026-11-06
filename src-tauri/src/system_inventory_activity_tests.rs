use super::*;
use osg_streams::NativeWake;

#[test]
fn visibility_commit_is_retained_without_invalidating_system_inventory() {
    let runtime = Runtime::new(true);
    let observations = crate::observations::Runtime::new();
    runtime.observe(&observations).unwrap();
    let broker = observations.broker();
    let mut inventory = broker.subscribe("inventory-test", "ui", TOPIC).unwrap();
    let mut visibility = broker.subscribe("visibility-test", "mcp", VISIBILITY_TOPIC).unwrap();
    let _ = inventory.try_recv().unwrap().unwrap();
    let baseline = visibility.try_recv().unwrap().unwrap();
    let wake = runtime.wake().observed();

    runtime.visibility_committed();

    let changed = visibility.try_recv().unwrap().unwrap();
    assert!(changed.revision > baseline.revision);
    assert!(inventory.try_recv().unwrap().is_none());
    assert_eq!(runtime.wake().observed(), wake);
    let mut reopened = broker.subscribe("visibility-reopen", "browser", VISIBILITY_TOPIC).unwrap();
    assert_eq!(reopened.try_recv().unwrap().unwrap().revision, changed.revision);
}

#[test]
fn activity_watch_is_weak_independent_and_wakes_only_on_source_changes() {
    let runtime = Runtime::new(true);
    let worker = runtime.wake().observed();
    let activity = Arc::new(NativeWake::default());
    runtime.watch_activity(&activity);
    assert_eq!(activity.observed(), 1);
    assert_eq!(runtime.wake().observed(), worker);
    assert_eq!(Arc::strong_count(&activity), 1);
    runtime.commit(json!({"observedAt":1,"hiddenDevices":[]}));
    assert_eq!(activity.observed(), 1, "SDK snapshot commits are not Activity sources");
    runtime.commit(json!({"observedAt":2,"hiddenDevices":[]}));
    assert_eq!(activity.observed(), 1, "Identical inventory cannot wake Activity");
    runtime.changed();
    assert_eq!(activity.observed(), 2);
    assert_eq!(runtime.wake().observed(), worker + 1);
    let replacement = Arc::new(NativeWake::default());
    runtime.watch_activity(&replacement);
    runtime.commit(json!({"observedAt":3,"hiddenDevices":[{"id":"new"}]}));
    assert_eq!(replacement.observed(), 1);
    runtime.changed();
    assert_eq!(activity.observed(), 2);
    assert_eq!(replacement.observed(), 2);
    drop(replacement);
    runtime.changed();
    assert!(runtime.activity_wake.lock().unwrap().upgrade().is_none());
}
