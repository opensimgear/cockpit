use super::*;

#[test]
fn native_workspace_owner_preserves_connection_but_not_expired_claims_or_output_authority() {
    let (mut gateway, _context) = harness();assert_eq!(connect(&mut gateway)["ok"], true);
    let cache = Arc::new(workspace::Cache::new(empty_state()));cache.set_owned(true);gateway.workspace = Some(cache.clone());
    assert_eq!(request(&mut gateway, "claim", json!({"moduleId":MODULE}))["ok"], true);
    gateway.claims.get_mut(MODULE).unwrap().seen = Instant::now() - gateway.timing.claim_timeout - Duration::from_millis(1);
    let abandoned = Instant::now() - gateway.timing.ui_timeout - Duration::from_millis(1);
    gateway.maintain(abandoned, &mut Instant::now());
    assert!(gateway.connection.is_some());assert!(gateway.claims.is_empty());
    let output = request(&mut gateway, "action_invoke_owned", json!({"moduleId":MODULE,"actionId":"led.set_enabled","values":[true]}));
    assert_eq!(output["ok"], false);
    cache.set_owned(false);gateway.maintain(abandoned, &mut Instant::now());assert!(gateway.connection.is_none());
}

#[test]
fn passive_snapshot_and_completion_do_not_renew_claims_and_expiry_changes_owners() {
    let (mut gateway, _context) = harness();assert_eq!(connect(&mut gateway)["ok"], true);
    assert_eq!(request(&mut gateway, "claim", json!({"moduleId":MODULE}))["ok"], true);
    let cache = workspace::Cache::new(empty_state());let wake = Arc::new(osg_streams::NativeWake::default());cache.watch(wake.clone());
    let latest = Mutex::new((empty_state(), None));let claims = Mutex::new(Vec::new());
    gateway.cache_workspace(&latest, &claims, &cache);
    let seen = gateway.claims[MODULE].seen;let at = gateway.state["nativeSnapshotAtMs"].clone();
    for _ in 0..100 {let (state, owners) = cache.snapshot();assert_eq!(state["nativeSnapshotAtMs"], at);assert_eq!(owners[MODULE], CLIENT);}
    assert_eq!(gateway.claims[MODULE].seen, seen);
    let old_wake = wake.observed();
    gateway.claims.get_mut(MODULE).unwrap().seen = Instant::now() - gateway.timing.claim_timeout - Duration::from_millis(1);
    assert!(gateway.renew_workspace(CLIENT).unwrap().is_empty());
    gateway.cache_workspace(&latest, &claims, &cache);
    assert!(cache.snapshot().1.is_empty());assert!(wake.observed() > old_wake);
    assert_eq!(cache.snapshot().0["nativeSnapshotAtMs"], at);
}

#[test]
fn worker_renewal_only_refreshes_existing_own_claims_without_transport_or_capture() {
    let (_gateway, context) = harness();let mut timing = timing();timing.heartbeat = Duration::from_secs(3600);
    let service = Stm32Service::with_factory(Box::new(MockFactory(context.clone())), timing);
    assert_eq!(service.request(json!({"command":"connect","port":PATH,"clientId":CLIENT}))["ok"], true);
    assert_eq!(service.request(json!({"command":"claim","moduleId":MODULE,"clientId":CLIENT}))["ok"], true);
    let before = service.workspace_snapshot();let writes = context.lock().unwrap().writes.len();
    service.set_workspace_owned(true);
    assert_eq!(service.renew_workspace_client("other-cockpit-0001").unwrap(), Vec::<String>::new());
    assert_eq!(service.renew_workspace_client(CLIENT).unwrap(), vec![MODULE.to_string()]);
    assert_eq!(context.lock().unwrap().writes.len(), writes);
    assert_eq!(service.workspace_snapshot(), before);assert!(!service.output_activity());
    service.shutdown();assert!(service.workspace_snapshot().1.is_empty());
    assert!(service.renew_workspace_client(CLIENT).is_err());
}
