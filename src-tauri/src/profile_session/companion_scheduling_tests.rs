use super::*;

#[test]
fn native_sources_keep_exact_expiry_and_retry_only_observed_contention() {
    let wake = Arc::new(osg_streams::NativeWake::default());
    let fixture = Fixture::with_wake(Some(wake.clone()));
    let mut adapter = fixture.prepared();
    adapter.set_wake(Some(wake.clone()));
    assert!(!adapter.notification_driven());
    fixture.acquire(&mut adapter, &[]);
    let _open = fixture.connect();
    adapter.bind_delivery_plan(&fixture.delivery()).unwrap();
    adapter.baseline(&fixture.hub, |_| Ok(()), 100).unwrap().unwrap();
    assert!(adapter.notification_driven());
    adapter.poll_live(&fixture.hub, |_| Ok(Some(())), 100);
    adapter.scheduled_poll_complete(100);
    let expiry = adapter.next_deadline_us().unwrap();
    assert_eq!(expiry, 100 + osg_mapping_host::companions::LEASE_US);
    assert_eq!(adapter.next_deadline_us(), Some(expiry));
    assert!(!adapter.needs_delivery_retry());
    fixture.clock.store(200, Ordering::Release);
    let paused = adapter.poll_live(&fixture.hub, |_| Ok(None), 200);
    assert!(paused.frames.iter().any(|frame| frame.reason == Reason::Gap));
    adapter.scheduled_poll_complete(200);
    assert_eq!(adapter.next_deadline_us(), Some(5_200));
    assert_eq!(adapter.next_deadline_us(), Some(5_200), "Reading a retry cannot postpone it");
    fixture.clock.store(300, Ordering::Release);
    adapter.poll_live(&fixture.hub, |_| Ok(Some(())), 300);
    adapter.scheduled_poll_complete(300);
    assert_eq!(adapter.next_deadline_us(), Some(expiry), "Resolved contention cannot leave an acquisition tick");
    adapter.instances[0].feedback_clear_pending = true;
    adapter.scheduled_poll_complete(400);
    assert_eq!(adapter.next_deadline_us(), Some(5_400));
    adapter.instances[0].feedback_clear_pending = false;
    adapter.scheduled_poll_complete(500);
    adapter.stop(&fixture.hub).unwrap();
    assert!(!adapter.notification_driven());
    assert_eq!(adapter.next_deadline_us(), None);
    assert_eq!(adapter.feedback_deadline(), None);
}

#[test]
fn adapter_requires_both_owner_and_provider_notifications() {
    let fixture = Fixture::new();
    let mut adapter = fixture.prepared();
    adapter.set_wake(Some(Arc::default()));
    fixture.acquire(&mut adapter, &[]);
    assert!(!adapter.notification_driven(), "A local wake cannot replace an unnotified provider");
    assert!(CompanionSources::prepare(&empty(), vec![]).unwrap().notification_driven());
}

#[test]
fn held_neutral_recovery_and_retired_sources_do_not_schedule_an_immediate_loop() {
    let fixture = Fixture::with_wake(Some(Arc::default()));
    let mut adapter = fixture.prepared();
    adapter.set_wake(Some(Arc::default()));
    fixture.acquire(&mut adapter, &[]);
    let open = fixture.connect();
    adapter.bind_delivery_plan(&fixture.delivery()).unwrap();
    adapter.baseline(&fixture.hub, |_| Ok(()), 100).unwrap().unwrap();
    adapter.poll_live(&fixture.hub, |_| Ok(Some(())), 100);
    let button = fixture.publication.controls.iter().find(|control|
        matches!(control.kind, SourceKind::Button)).unwrap();
    let mut input = fixture.frame(&open, "input", "1");
    input["changes"] = json!([{"controlId":button.id,"value":true}]);
    assert_eq!(wire(&fixture.hub, input)["ok"], true);
    adapter.poll_live(&fixture.hub, |_| Ok(None), 100);
    adapter.scheduled_poll_complete(100);
    adapter.poll_live(&fixture.hub, |_| Ok(Some(())), 100);
    adapter.scheduled_poll_complete(100);
    assert!(adapter.instances[0].metadata_paused);
    assert!(!adapter.instances[0].validation_pending);
    assert_eq!(adapter.next_deadline_us(), Some(100 + osg_mapping_host::companions::LEASE_US),
        "A held button must wait for native release or expiry, not repeatedly recheck neutral");
    fixture.hub.disconnect_connection("connection", fixture.hub.now_ms());
    adapter.poll_live(&fixture.hub, |_| Ok(Some(())), 100);
    adapter.scheduled_poll_complete(100);
    assert!(!adapter.instances[0].ready);
    assert!(adapter.needs_delivery_retry());
    assert_eq!(adapter.next_deadline_us(), None, "Retired input leaves rebind retry ownership to its worker");
    adapter.stop(&fixture.hub).unwrap();
}

#[test]
fn feedback_rate_deadline_is_anchored_and_absent_without_eligible_outputs() {
    let fixture = Fixture::new();
    let mut adapter = fixture.prepared();
    assert_eq!(adapter.feedback_deadline(), None);
    fixture.acquire(&mut adapter, &[]);
    fixture.connect();
    adapter.bind_delivery_plan(&fixture.delivery()).unwrap();
    adapter.baseline(&fixture.hub, |_| Ok(()), 100).unwrap().unwrap();
    let at = Instant::now();
    adapter.instances[0].feedback_at = Some(at);
    assert_eq!(adapter.feedback_deadline(), Some(at + std::time::Duration::from_millis(34)));
    adapter.instances[0].feedback_clear_pending = true;
    assert_eq!(adapter.feedback_deadline(), None);
    adapter.instances[0].feedback_clear_pending = false;
    adapter.instances[0].ready = false;
    assert_eq!(adapter.feedback_deadline(), None);
    assert!(adapter.needs_delivery_retry());
    adapter.stop(&fixture.hub).unwrap();
    assert!(!adapter.needs_delivery_retry());
}
