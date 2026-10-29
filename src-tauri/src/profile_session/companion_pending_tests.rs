use super::*;

fn armed() -> (Fixture, CompanionSources, Phase, Value) {
    let fixture = Fixture::new();
    let mut adapter = fixture.prepared();
    fixture.acquire(&mut adapter, &[]);
    let open = fixture.connect();
    let delivery = fixture.delivery();
    adapter.bind_delivery_plan(&delivery).unwrap();
    let baseline = adapter.baseline(&fixture.hub, |_| Ok(()), 100).unwrap().unwrap();
    let mut phase = Phase::new(delivery.program, Counter(1), uuid::Uuid::new_v4().to_string()).unwrap();
    phase.accept_batch(baseline).unwrap();
    phase.drain();
    adapter.arm(phase.run_token()).unwrap();
    (fixture, adapter, phase, open)
}

fn change(fixture: &Fixture, open: &Value, control: &str, sequence: u64, value: bool, now: u64) {
    fixture.clock.store(now, Ordering::Release);
    let mut frame = fixture.frame(open, "input", &sequence.to_string());
    frame["changes"] = json!([{"controlId":control,"value":value}]);
    let reply = wire(&fixture.hub, frame);
    assert_eq!(reply["ok"], true, "{reply}");
}

#[test]
fn inventory_pending_pauses_inputs_once_without_clearing_feedback_or_rebinding() {
    let (fixture, mut adapter, phase, open) = armed();
    assert!(adapter.publish_feedback(&fixture.hub, &phase, |_| Ok(()), 100).is_empty());
    let receipt = adapter.instances[0].receipt.clone().unwrap();
    let token = fixture.hub.profile_feedback_token(&receipt).unwrap();
    let first = adapter.poll_live(&fixture.hub, |_| Ok(None), 100);
    assert_eq!(first.frames.len(), 1);
    assert_eq!(first.frames[0].reason, Reason::Gap);
    assert!(first.liveness.is_empty());
    for _ in 0..10 {
        assert!(adapter.poll_live(&fixture.hub, |_| Ok(None), 100).frames.is_empty());
        assert!(!adapter.needs_rebind_live(|_| Ok(None)));
        let rebound = adapter.rebind_live(&fixture.hub, &fixture.delivery(), |_| true, |_| Ok(None), 100);
        assert!(rebound.frames.is_empty());
        assert_eq!(fixture.hub.profile_feedback_token(&receipt).unwrap(), token);
    }
    assert!(adapter.instances[0].ready);
    assert!(adapter.instances[0].metadata_paused);
    assert!(adapter.instances[0].feedback_live);
    assert!(!adapter.instances[0].feedback_clear_pending);
    let reply = wire(&fixture.hub, fixture.frame(&open, "heartbeat", "1"));
    assert_eq!(reply["feedback"]["values"][0]["value"], false);
}

#[test]
fn resumed_metadata_requires_actual_neutral_and_never_replays_the_unchecked_press() {
    let (fixture, mut adapter, _, open) = armed();
    let receipt = adapter.instances[0].receipt.clone().unwrap();
    let token = fixture.hub.profile_feedback_token(&receipt).unwrap();
    let control = adapter.instances[0].inputs[0].observed.clone();
    let pause = adapter.poll_live(&fixture.hub, |_| Ok(None), 100);
    assert_eq!(pause.frames[0].reason, Reason::Gap);
    change(&fixture, &open, &control, 1, true, 200);
    let held = adapter.poll_live(&fixture.hub, |_| Ok(Some(())), 200);
    assert!(held.frames.is_empty());
    assert!(held.liveness.is_empty());
    assert!(adapter.instances[0].metadata_paused);
    change(&fixture, &open, &control, 2, false, 300);
    let neutral = adapter.poll_live(&fixture.hub, |_| Ok(Some(())), 300);
    assert!(neutral.diagnostics.is_empty(), "{:?}", neutral.diagnostics);
    assert_eq!(neutral.frames.len(), 1);
    assert_eq!(neutral.frames[0].reason, Reason::Baseline);
    assert_eq!(neutral.frames[0].changes[0].sample, SignalSample::scalar(PortValue::Boolean(false)));
    assert!(neutral.frames[0].sequence.0 > pause.frames[0].sequence.0);
    assert_eq!(neutral.liveness.len(), 1);
    assert!(!adapter.instances[0].metadata_paused);
    assert_eq!(fixture.hub.profile_feedback_token(&receipt).unwrap(), token);
    change(&fixture, &open, &control, 3, true, 400);
    let press = adapter.poll_live(&fixture.hub, |_| Ok(Some(())), 400);
    assert!(press.frames.iter().any(|frame| frame.reason == Reason::Change && frame.changes.iter()
        .any(|change| change.sample == SignalSample::scalar(PortValue::Boolean(true)))));
}

#[test]
fn actual_revocation_and_peer_loss_still_disconnect_and_clear_feedback() {
    for disconnect in [false, true] {
        let (fixture, mut adapter, phase, _) = armed();
        adapter.publish_feedback(&fixture.hub, &phase, |_| Ok(()), 100);
        let receipt = adapter.instances[0].receipt.clone().unwrap();
        let before = fixture.hub.profile_feedback_token(&receipt).unwrap();
        if disconnect { fixture.hub.disconnect_connection("connection", fixture.hub.now_ms()); }
        let lost = adapter.poll_live(&fixture.hub, |_| if disconnect { Ok(None) } else { Err("Revoked token".into()) }, 100);
        assert_eq!(lost.frames.len(), 1);
        assert_eq!(lost.frames[0].reason, Reason::Disconnect);
        assert!(lost.liveness.is_empty());
        assert!(!adapter.instances[0].ready);
        assert!(!adapter.instances[0].feedback_live);
        assert!(!adapter.instances[0].metadata_paused);
        if !disconnect {
            assert_ne!(fixture.hub.profile_feedback_token(&receipt).unwrap().feedback_generation, before.feedback_generation);
        }
    }
}

#[test]
fn deferred_clear_completes_before_any_fresh_same_owner_feedback_can_be_enqueued() {
    let (fixture, mut adapter, phase, _) = armed();
    adapter.instances[0].feedback_clear_pending = true;
    assert!(adapter.publish_feedback_background(&fixture.hub, &phase, |_| panic!("Pending clear queued feedback"), 100).is_empty());
    assert!(!adapter.feedback_worker.busy());
    let receipt = adapter.instances[0].receipt.clone().unwrap();
    let old = fixture.hub.profile_feedback_token(&receipt).unwrap();
    adapter.poll_live(&fixture.hub, |_| Ok(Some(())), 100);
    assert!(!adapter.instances[0].feedback_clear_pending);
    let current = fixture.hub.profile_feedback_token(&receipt).unwrap();
    assert_ne!(old.feedback_generation, current.feedback_generation);
    // No retained clear can reset a subsequent publication from this owner.
    adapter.instances[0].feedback_at = None;
    assert!(adapter.publish_feedback(&fixture.hub, &phase, |_| Ok(()), 100).is_empty());
    adapter.poll_live(&fixture.hub, |_| Ok(Some(())), 100);
    assert_eq!(fixture.hub.profile_feedback_token(&receipt).unwrap(), current);
}
