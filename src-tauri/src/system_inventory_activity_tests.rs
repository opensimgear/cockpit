use super::*;
use osg_streams::NativeWake;

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
