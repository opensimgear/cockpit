use super::*;

#[test]
fn cache_is_atomic_private_and_notifies_only_after_visible_or_owner_changes() {
    let cache = Cache::new(json!({"connected":false}));
    let first = Arc::new(osg_streams::NativeWake::default());let second = Arc::new(osg_streams::NativeWake::default());
    cache.watch(first.clone());cache.watch(first.clone());cache.watch(second.clone());
    let owners = BTreeMap::from([("module".into(), "private-client-authority".into())]);
    let state = json!({"connected":true,"nativeSnapshotAtMs":1,"moduleConfigs":{"module":{"owner":1234,"config":{"name":"Public"}}},
        "moduleActions":{"module":{"ownership":{"scope":"endpoint"},"lastLease":{"leaseId":7},"record":{"token":"private-token","ownershipToken":"private-ownership","clientId":"private-client"}}}});
    cache.commit(state.clone(), owners.clone());
    assert_eq!(first.observed(), 1);assert_eq!(second.observed(), 1);
    let (public, actual) = cache.snapshot();assert_eq!(actual, owners);
    let encoded = public.to_string();assert!(!encoded.contains("private-"));assert!(!encoded.contains("\"owner\""));
    assert_eq!(public["moduleConfigs"]["module"]["config"]["name"], "Public");
    assert_eq!(public["moduleActions"]["module"]["lastLease"]["leaseId"], 7);
    let visible = cache.visible_snapshot().0;
    assert!(visible.get("nativeSnapshotAtMs").is_none());
    let mut heartbeat = state;heartbeat["nativeSnapshotAtMs"] = json!(2);
    cache.commit(heartbeat, owners.clone());
    assert_eq!(first.observed(), 1);assert_eq!(cache.snapshot().0["nativeSnapshotAtMs"], 2);
    assert!(Arc::ptr_eq(&visible, &cache.visible_snapshot().0));
    cache.commit(public.as_ref().clone(), BTreeMap::new());
    assert_eq!(first.observed(), 2);assert!(cache.snapshot().1.is_empty());
    assert_eq!(cache.snapshot().0["connected"], true);
    assert!(Arc::ptr_eq(&visible, &cache.visible_snapshot().0), "owner changes preserve the same public state Arc");
}

#[test]
fn watcher_registry_is_weak_deduplicated_and_bounded() {
    let cache = Cache::new(json!({}));
    let discarded = Arc::new(osg_streams::NativeWake::default());cache.watch(discarded.clone());drop(discarded);
    let watchers = (0..MAX_WATCHERS+1).map(|_| Arc::new(osg_streams::NativeWake::default())).collect::<Vec<_>>();
    for wake in &watchers { cache.watch(wake.clone()); }
    cache.commit(json!({"connected":true}), BTreeMap::new());
    assert!(watchers[..MAX_WATCHERS].iter().all(|wake| wake.observed() == 1));
    assert_eq!(watchers[MAX_WATCHERS].observed(), 0);
    assert_eq!(cache.snapshot.lock().unwrap().wakes.len(), MAX_WATCHERS);
}
