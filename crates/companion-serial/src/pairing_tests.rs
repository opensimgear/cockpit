use super::*;

fn fixture() -> (Hub, Wire, Value) {
    let hub = Hub::new("usb-pairing-host".into(), Arc::new(osg_companion_service::NullSourceSink));
    let mut wire = Wire::new(Arc::default());
    wire.reset(&hub, "usb-pairing");
    let subscribe = pending(&hub, &mut wire, "native-panel");
    (hub, wire, subscribe)
}

fn request(hub: &Hub, wire: &mut Wire, frame: Value) -> Value {
    let value = json!({"type":"serial-request","linkId":wire.link,
        "requestId":(wire.sequence + 1).to_string(),"frame":frame});
    wire.handle(hub, &value).unwrap()[0]["frame"].clone()
}

fn pending(hub: &Hub, wire: &mut Wire, companion: &str) -> Value {
    let offer = hub.command(&json!({"command":"companion_pair_offer"})).unwrap();
    let reply = request(hub, wire, json!({"type":"pair","secret":offer["secret"],
        "companionId":companion,"name":"USB panel"}));
    assert_eq!(reply["type"], "pair-pending");
    json!({"type":"pair-subscribe","requestId":reply["requestId"],"secret":offer["secret"]})
}

fn subscribe(hub: &Hub, wire: &mut Wire, frame: &Value) {
    assert_eq!(request(hub, wire, frame.clone()), json!({"ok":true,"type":"pair-subscribed"}));
}

fn approve(hub: &Hub, frame: &Value) {
    hub.command(&json!({"command":"companion_approve","requestId":frame["requestId"]})).unwrap();
}

#[test]
fn quiet_pairing_retains_one_reader_and_emits_one_identity_bound_terminal() {
    for approve_first in [false, true] {
        let (hub, mut wire, frame) = fixture();
        if approve_first { approve(&hub, &frame); }
        subscribe(&hub, &mut wire, &frame);
        if !approve_first {
            assert!(wire.take_feedback().unwrap().is_none());
            let observed = wire.pairing.observed;
            for _ in 0..8 {
                wire.wake.notify();
                assert!(wire.take_feedback().unwrap().is_none());
                assert_eq!(wire.pairing.observed, observed);
                assert!(wire.pairing.reader.is_some());
            }
            approve(&hub, &frame);
        }
        let terminal = wire.take_feedback().unwrap().unwrap();
        assert_eq!(terminal["command"], "serial-feedback");
        assert_eq!(terminal["linkId"], "usb-pairing");
        assert_eq!(terminal["frame"]["type"], "paired");
        assert_eq!(terminal["frame"]["companionId"], "native-panel");
        assert!(terminal["frame"]["token"].is_string());
        assert!(wire.pairing.reader.is_none());
        assert!(wire.take_feedback().unwrap().is_none());
        // Approval notification grants no authenticated serial authority.
        assert_eq!(request(&hub, &mut wire, json!({"type":"ping"}))["ok"], false);
    }
}

#[test]
fn pairing_reject_cancel_expiry_and_revocation_emit_one_error() {
    for command in ["companion_reject", "companion_pair_cancel", "expiry", "companion_revoke"] {
        let (hub, mut wire, frame) = fixture();
        subscribe(&hub, &mut wire, &frame);
        assert!(wire.take_feedback().unwrap().is_none());
        if command == "expiry" {
            hub.tick(wire.pairing.reader.as_ref().unwrap().expires_at_ms());
        } else if command == "companion_revoke" {
            approve(&hub, &frame);
            hub.command(&json!({"command":command,"companionId":"native-panel"})).unwrap();
        } else {
            hub.command(&json!({"command":command,"requestId":frame["requestId"]})).unwrap();
        }
        let terminal = wire.take_feedback().unwrap().unwrap();
        assert_eq!(terminal["frame"]["ok"], false, "{command}: {terminal}");
        assert!(terminal["frame"]["error"].is_string());
        assert!(terminal["frame"].get("token").is_none());
        assert!(wire.take_feedback().unwrap().is_none());
    }
}

#[test]
fn duplicate_and_wrong_owner_subscriptions_cannot_displace_pending_reader() {
    let (hub, mut wire, frame) = fixture();
    subscribe(&hub, &mut wire, &frame);
    assert_eq!(request(&hub, &mut wire, frame.clone())["ok"], false);
    let mut other = Wire::new(Arc::default());
    other.reset(&hub, "other-usb");
    assert_eq!(request(&hub, &mut other, frame.clone())["ok"], false);
    let mut wrong = frame.clone();
    wrong["secret"] = json!("wrong");
    assert_eq!(request(&hub, &mut other, wrong)["ok"], false);
    approve(&hub, &frame);
    assert_eq!(wire.take_feedback().unwrap().unwrap()["frame"]["type"], "paired");
    assert!(other.take_feedback().unwrap().is_none());
}

#[test]
fn reset_and_new_pairing_withdraw_old_link_delivery() {
    let (hub, mut wire, old) = fixture();
    subscribe(&hub, &mut wire, &old);
    approve(&hub, &old);
    wire.reset(&hub, "replacement-usb");
    assert!(wire.take_feedback().unwrap().is_none());
    let fresh = pending(&hub, &mut wire, "new-panel");
    subscribe(&hub, &mut wire, &fresh);
    approve(&hub, &fresh);
    let terminal = wire.take_feedback().unwrap().unwrap();
    assert_eq!(terminal["linkId"], "replacement-usb");
    assert_eq!(terminal["frame"]["companionId"], "new-panel");

    let late = pending(&hub, &mut wire, "late-panel");
    subscribe(&hub, &mut wire, &late);
    approve(&hub, &late);
    let _replacement = pending(&hub, &mut wire, "replacement-panel");
    assert!(wire.take_feedback().unwrap().is_none());
}

#[test]
fn auth_close_and_cancel_clear_pending_delivery_even_if_request_fails() {
    for boundary in [json!({"type":"auth","token":"invalid"}), json!({"type":"close"}), json!({"type":"cancel"})] {
        let (hub, mut wire, frame) = fixture();
        subscribe(&hub, &mut wire, &frame);
        approve(&hub, &frame);
        request(&hub, &mut wire, boundary);
        assert!(wire.take_feedback().unwrap().is_none());
        assert!(wire.pairing.reader.is_none());
    }
}
