use super::*;

#[test]
fn activity_watch_is_weak_and_does_not_initialize_metadata_or_services() {
    let root = crate::test_directory();
    let path = root.path().join("absent");
    let runtime = Runtime::new(Some(path.clone()), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
    let wake = Arc::new(osg_streams::NativeWake::default());
    let before = wake.observed();
    runtime.watch_activity(&wake);
    assert_ne!(wake.observed(), before);
    assert_eq!(Arc::strong_count(&wake), 1);
    assert!(runtime.metadata.get().is_none());
    assert!(runtime.services.get().is_none());
    assert!(!path.exists());
    drop(wake);
    let hub = Hub::new("dead-watch".into(), Arc::new(osg_companion_service::NullSourceSink));
    runtime.register_activity_wake(&hub);
    assert_eq!(runtime.activity_wake.lock().unwrap().weak_count(), 0);
}

#[test]
fn activity_watch_registers_lazily_and_preserves_inventory_watch() {
    for initialized in [false, true] {
        let runtime = runtime();
        if initialized { runtime.metadata(true).unwrap(); }
        let inventory = Arc::new(osg_streams::NativeWake::default());
        let activity = Arc::new(osg_streams::NativeWake::default());
        runtime.watch_resource_inventory(&inventory);
        runtime.watch_activity(&activity);
        let hub = &runtime.metadata(true).unwrap().hub;
        assert!(runtime.services.get().is_none());
        assert_eq!(Arc::strong_count(&activity), 1);
        let before = (activity.observed(), inventory.observed());
        hub.notify_presence();
        assert_ne!(activity.observed(), before.0);
        assert_ne!(inventory.observed(), before.1);
        let before = (activity.observed(), inventory.observed());
        let mut upload: Value = serde_json::from_str(include_str!("../../../../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json")).unwrap();
        upload["command"] = json!("companion_publish");
        hub.command(&upload).unwrap();
        assert_ne!(activity.observed(), before.0);
        assert_ne!(inventory.observed(), before.1);
    }
}

#[test]
fn cached_roster_does_not_initialize_or_read_invalid_retained_state() {
    let root = crate::test_directory();
    let anchor = root.path().join("companion-host-state.dpapi");
    crate::persistence::test_support::record(&anchor, "secrets", "companion-host", b"bad cipher");
    let runtime = Runtime::new(Some(root.path().into()), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
    assert!(runtime.cached_resource_companions().is_none());
    assert!(runtime.metadata.get().is_none());
    assert!(runtime.services.get().is_none());
    assert!(runtime.hydrate_retained_metadata().is_err());
    assert!(runtime.cached_resource_companions().is_none());
    assert!(!runtime.retained_metadata_ready.load(Ordering::SeqCst));
    assert!(runtime.metadata.get().is_none());
    assert!(runtime.services.get().is_none());
}

#[test]
fn empty_hydration_commits_readiness_and_wakes_without_creating_state() {
    let root = crate::test_directory();
    for path in [None, Some(root.path().join("absent"))] {
        let runtime = Runtime::new(path.clone(), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
        let wake = Arc::new(osg_streams::NativeWake::default());
        runtime.watch_activity(&wake);
        let before = wake.observed();
        assert!(runtime.cached_resource_companions().is_none());
        runtime.hydrate_retained_metadata().unwrap();
        assert_ne!(wake.observed(), before);
        assert!(runtime.cached_resource_companions().unwrap().is_empty());
        assert!(runtime.metadata.get().is_none());
        assert!(runtime.services.get().is_none());
        assert!(path.is_none_or(|path| !path.exists()));
        runtime.shutdown();
        assert!(runtime.cached_resource_companions().is_none());
        assert!(runtime.hydrate_retained_metadata().is_err());
    }
}

#[test]
fn metadata_initialization_publishes_cached_readiness_and_wakes_activity() {
    let runtime = runtime();
    let wake = Arc::new(osg_streams::NativeWake::default());
    runtime.watch_activity(&wake);
    let before = wake.observed();
    assert!(runtime.cached_resource_companions().is_none());
    runtime.metadata(true).unwrap();
    assert_ne!(wake.observed(), before);
    assert!(runtime.cached_resource_companions().unwrap().is_empty());
    assert!(runtime.services.get().is_none());
}

#[cfg(windows)]
#[test]
fn hydration_restores_retained_roster_without_starting_transports() {
    let root = crate::test_directory();
    let hub = Hub::open(root.path(), Arc::new(osg_companion_service::NullSourceSink)).unwrap();
    let offer = hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
    let pending = hub.handle_wire(json!({"type":"pair","secret":offer["secret"],"companionId":"retained","name":"Retained screen"}), "pairing", hub.now_ms());
    assert_eq!(pending["type"], "pair-pending");
    hub.command(&json!({"command":"companion_pair_approve","requestId":pending["requestId"]})).unwrap();
    let runtime = Runtime::new(Some(root.path().into()), Arc::new(CompanionProvider::new(Arc::new(|| 1))));
    assert!(runtime.cached_resource_companions().is_none());
    let wake = Arc::new(osg_streams::NativeWake::default());
    runtime.watch_activity(&wake);
    let before = wake.observed();
    runtime.hydrate_retained_metadata().unwrap();
    assert_ne!(wake.observed(), before);
    let roster = runtime.cached_resource_companions().unwrap();
    assert_eq!(roster.len(), 1);
    assert_eq!(roster[0].companion_id, "retained");
    assert!(runtime.services.get().is_none());
    assert_eq!(runtime.metadata.get().unwrap().hub.host_id(), hub.host_id());
}
