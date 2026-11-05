use super::*;

fn fixture() -> (Hub, Value) {
    let hub = Hub::new("pairing-host".into(), Arc::new(NullSourceSink));
    let frame = pending(&hub, "native");
    (hub, frame)
}

fn pending(hub: &Hub, companion: &str) -> Value {
    let offer = hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
    let reply = hub.handle_wire(json!({"type":"pair","secret":offer["secret"],
        "companionId":companion,"name":"Native panel"}), "requester", hub.now_ms());
    assert_eq!(reply["type"], "pair-pending");
    json!({"type":"pair-subscribe","requestId":reply["requestId"],"secret":offer["secret"]})
}

fn approve(hub: &Hub, frame: &Value) {
    hub.command(&json!({"command":"companion_approve","requestId":frame["requestId"]})).unwrap();
}

#[test]
fn approval_is_observed_before_or_after_subscription_without_authentication() {
    for approve_first in [false, true] {
        let (hub, frame) = fixture();
        if approve_first { approve(&hub, &frame); }
        let wake = Arc::new(NativeWake::default());
        let mut reader = hub.subscribe_pairing(&frame, "reader", wake.clone()).unwrap();
        let observed = reader.observed();
        if !approve_first {
            assert_eq!(reader.take(hub.now_ms()).unwrap()["type"], "pair-pending");
            approve(&hub, &frame);
            assert_ne!(reader.observed(), observed);
        }
        let paired = reader.take(hub.now_ms()).unwrap();
        assert_eq!(paired["type"], "paired");
        assert_eq!(paired["companionId"], "native");
        assert!(!paired["token"].as_str().unwrap().is_empty());
        assert!(hub.inner.lock().unwrap().connections.is_empty());
        assert_eq!(reader.expires_at_ms(), hub.next_deadline_ms().unwrap());
    }
}

#[test]
fn admission_checks_secret_exact_fields_duplicates_and_authenticated_connections() {
    let (hub, frame) = fixture();
    let subscribe = |value: &Value, connection: &str| hub.subscribe_pairing(value, connection, Arc::new(NativeWake::default()));
    let mut invalid = frame.clone();
    invalid["secret"] = json!("wrong-secret");
    assert!(subscribe(&invalid, "wrong").err().unwrap().contains("owner mismatch"));
    invalid = frame.clone();
    invalid["unexpected"] = json!(true);
    assert!(subscribe(&invalid, "extra").is_err());
    hub.inner.lock().unwrap().connections.insert("authenticated".into(), "native".into());
    assert!(subscribe(&frame, "authenticated").is_err());
    let reader = subscribe(&frame, "reader").unwrap();
    assert!(subscribe(&frame, "duplicate").err().unwrap().contains("already has a reader"));
    drop(reader);
    assert!(subscribe(&frame, "replacement").is_ok());
}

#[test]
fn rejection_cancellation_and_exact_expiry_wake_with_terminal_errors() {
    for (command, message) in [("companion_reject", "rejected"), ("companion_pair_cancel", "cancelled"), ("expiry", "expired")] {
        let (hub, frame) = fixture();
        let wake = Arc::new(NativeWake::default());
        let mut reader = hub.subscribe_pairing(&frame, "reader", wake.clone()).unwrap();
        let observed = reader.observed();
        if command == "expiry" {
            assert_eq!(reader.take(reader.expires_at_ms() - 1).unwrap()["type"], "pair-pending");
            hub.tick(reader.expires_at_ms());
        } else {
            hub.command(&json!({"command":command,"requestId":frame["requestId"]})).unwrap();
        }
        assert_ne!(reader.observed(), observed);
        assert!(reader.take(hub.now_ms()).unwrap_err().contains(message));
        assert!(hub.inner.lock().unwrap().pending.is_empty());
    }
}

#[test]
fn revoke_and_credential_rotation_cannot_resurrect_an_approval() {
    for rotate in [false, true] {
        let (hub, frame) = fixture();
        let mut reader = hub.subscribe_pairing(&frame, "reader", Arc::new(NativeWake::default())).unwrap();
        approve(&hub, &frame);
        let observed = reader.observed();
        if rotate {
            let replacement = pending(&hub, "native");
            approve(&hub, &replacement);
        } else {
            hub.command(&json!({"command":"companion_revoke","companionId":"native"})).unwrap();
        }
        assert_ne!(reader.observed(), observed);
        assert!(reader.take(hub.now_ms()).unwrap_err().contains("revoked"));
    }
}

#[test]
fn disconnect_and_stop_retire_unauthenticated_readers_with_exact_raii_cleanup() {
    for disconnect_all in [false, true] {
        let (hub, frame) = fixture();
        let mut old = hub.subscribe_pairing(&frame, "reader", Arc::new(NativeWake::default())).unwrap();
        let observed = old.observed();
        if disconnect_all { hub.disconnect_all(); } else { hub.disconnect_connection("reader", hub.now_ms()); }
        assert_ne!(old.observed(), observed);
        assert!(old.take(hub.now_ms()).unwrap_err().contains("ended"));
        let mut replacement = hub.subscribe_pairing(&frame, "reader", Arc::new(NativeWake::default())).unwrap();
        drop(old);
        assert_eq!(replacement.take(hub.now_ms()).unwrap()["type"], "pair-pending");
        drop(replacement);
        assert!(hub.inner.lock().unwrap().pairing_readers.is_empty());
    }
}

#[test]
fn reader_budget_is_bounded_and_raii_releases_capacity() {
    let hub = Hub::new("pairing-host".into(), Arc::new(NullSourceSink));
    let mut readers = Vec::new();
    for index in 0..16 {
        let frame = pending(&hub, &format!("native-{index}"));
        let reader = hub.subscribe_pairing(&frame, &format!("reader-{index}"), Arc::new(NativeWake::default())).unwrap();
        hub.command(&json!({"command":"companion_reject","requestId":frame["requestId"]})).unwrap();
        readers.push(reader);
    }
    let frame = pending(&hub, "last-native");
    assert!(hub.subscribe_pairing(&frame, "last-reader", Arc::new(NativeWake::default())).err().unwrap().contains("budget"));
    readers.pop();
    assert!(hub.subscribe_pairing(&frame, "last-reader", Arc::new(NativeWake::default())).is_ok());
}
