use super::*;

#[test]
fn routing_observation_is_lazy_weak_and_independent_of_activity() {
    for initialized in [false, true] {
        let runtime = runtime();
        if initialized { runtime.metadata(true).unwrap(); }
        let routing = Arc::new(osg_streams::NativeWake::default());
        let activity = Arc::new(osg_streams::NativeWake::default());
        runtime.watch_activity(&activity);
        runtime.watch_routing(&routing);
        assert_eq!(Arc::strong_count(&routing), 1);
        assert!(runtime.services.get().is_none());
        if !initialized { assert!(runtime.metadata.get().is_none()); }
        let hub = &runtime.metadata(true).unwrap().hub;
        let before = (routing.observed(), activity.observed());
        hub.notify_presence();
        assert_ne!(routing.observed(), before.0);
        assert_ne!(activity.observed(), before.1);
        let before = (routing.observed(), activity.observed());
        let mut upload: Value = serde_json::from_str(include_str!("../../../../../crates/companion-service/tests/fixtures/compiled-toggle-publication.json")).unwrap();
        upload["command"] = json!("companion_publish");
        hub.command(&upload).unwrap();
        assert_ne!(routing.observed(), before.0);
        assert_ne!(activity.observed(), before.1);
        drop(routing);
        runtime.register_routing_wake(hub);
        assert!(runtime.routing_wake.lock().unwrap().upgrade().is_none());
    }
}
