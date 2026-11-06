use super::*;

#[test]
fn legacy_native_owner_bypasses_only_ui_timeout_and_expired_claim_is_not_revived() {
    let (mut gateway, context) = harness();gateway.timing.claim_timeout = Duration::from_secs(60);assert_eq!(connect(&mut gateway)["ok"], true);
    let cache = Arc::new(crate::stm32::workspace::Cache::new(empty_state()));cache.set_owned(true);gateway.workspace = Some(cache.clone());
    assert_eq!(request(&mut gateway, "claim", json!({"moduleId":MODULE}), CLIENT_A)["ok"], true);
    let before = gateway.claims[MODULE].seen;let writes = context.lock().unwrap().writes.len();
    assert!(gateway.renew_workspace(CLIENT_B).unwrap().is_empty());assert_eq!(gateway.claims[MODULE].seen, before);
    assert_eq!(gateway.renew_workspace(CLIENT_A).unwrap(), vec![MODULE.to_string()]);
    assert_eq!(context.lock().unwrap().writes.len(), writes);
    gateway.claims.get_mut(MODULE).unwrap().seen = Instant::now() - gateway.timing.claim_timeout - Duration::from_millis(1);
    assert!(gateway.renew_workspace(CLIENT_A).unwrap().is_empty());
    let abandoned = Instant::now() - gateway.timing.ui_timeout - Duration::from_millis(1);
    gateway.maintain_local(abandoned);assert!(gateway.connection.is_some());assert!(gateway.claims.is_empty());
    assert_eq!(gateway.state["running"], false);
    cache.set_owned(false);gateway.maintain_local(abandoned);assert!(gateway.connection.is_none());
}

#[test]
fn legacy_worker_renewal_does_not_write_or_manufacture_telemetry() {
    let (_gateway, context) = harness();let mut timing = timing();timing.heartbeat = Duration::from_secs(3600);timing.claim_timeout = Duration::from_secs(60);
    let cache = Arc::new(crate::stm32::workspace::Cache::new(empty_state()));
    let service = LegacyPicoService::with_workspace(Box::new(MockFactory(context.clone())), timing, false, cache.clone());
    assert_eq!(service.request(json!({"command":"connect","port":PATH,"clientId":CLIENT_A}))["ok"], true);
    assert_eq!(service.request(json!({"command":"claim","moduleId":MODULE,"clientId":CLIENT_A}))["ok"], true);
    let before = cache.snapshot();let writes = context.lock().unwrap().writes.len();
    assert_eq!(service.renew_workspace_client(CLIENT_A).unwrap(), vec![MODULE.to_string()]);
    assert!(service.renew_workspace_client(CLIENT_B).unwrap().is_empty());
    assert_eq!(cache.snapshot(), before);assert_eq!(context.lock().unwrap().writes.len(), writes);
    service.shutdown();assert!(cache.snapshot().1.is_empty());assert!(service.renew_workspace_client(CLIENT_A).is_err());
}

#[test]
fn lazy_selection_uses_the_same_cache_and_does_not_block_passive_readers() {
    let (_gateway, context) = harness();
    let service = PicoService::with_factory(Box::new(MockFactory(context.clone())));
    let wake = Arc::new(osg_streams::NativeWake::default());service.watch_workspace(wake.clone());service.set_workspace_owned(true);
    assert_eq!(service.request(json!({"command":"connect","port":PATH,"clientId":CLIENT_A}))["ok"], true);
    assert!(wake.observed() > 0);assert_eq!(service.workspace_snapshot().0["connected"], true);
    let selection = service.selected.lock().unwrap();
    assert!(service.workspace.owned());assert_eq!(service.workspace_snapshot().0["connected"], true);
    assert!(service.renew_workspace_client(CLIENT_A).unwrap_err().contains("busy"));
    drop(selection);assert!(!service.output_activity());
    service.shutdown();assert_eq!(service.workspace_snapshot().0["connected"], false);
}
