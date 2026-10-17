use super::*;
use serde_json::json;
fn action() -> Value {
    json!({"id":"led.set","endpoint":"led","label":"LED","readable":true,"semantics":"setter","parameters":[{"id":"on","label":"On","type":"boolean"}]})
}
fn request() -> Value {
    json!({"ownershipToken":"00112233-4455-6677-8899-aabbccddeeff","leaseId":42,"ttlMs":5000,"catalogueRevision":1,"actionId":"led.set","values":[false]})
}
#[test]
fn cached_activity_spans_pending_work_and_tracks_exact_release_and_monotonic_expiry() {
    let activity = LeaseActivity::default();
    let now = Instant::now();
    assert!(!activity.active_at(now));
    let pending = activity.begin();
    assert!(
        activity.active_at(now + Duration::from_secs(60)),
        "pending worker work cannot expire while an ACK is delayed"
    );
    pending.finish_at(
        "module:boot",
        "action_claim",
        None,
        Some(&json!({"leaseId":1,"remainingMs":1000})),
        now,
    );
    activity.begin().finish_at(
        "other:boot",
        "action_claim",
        None,
        Some(&json!({"leaseId":1,"remainingMs":2000})),
        now,
    );
    activity.begin().finish_at(
        "module:boot",
        "action_release",
        Some(1),
        Some(&json!({"leaseId":1,"released":true})),
        now + Duration::from_millis(10),
    );
    assert!(
        activity.active_at(now + Duration::from_millis(1500)),
        "release must not clear another physical endpoint's lease"
    );
    assert!(!activity.active_at(now + Duration::from_millis(2000)));
    activity.begin().finish_at(
        "module:boot",
        "action_claim",
        None,
        Some(&json!({"leaseId":2,"remainingMs":4000})),
        now,
    );
    activity.begin().finish_at(
        "module:boot",
        "action_renew",
        Some(2),
        Some(&json!({"leaseId":2,"remainingMs":5000})),
        now + Duration::from_millis(100),
    );
    activity.begin().finish_at(
        "module:boot",
        "action_invoke_owned",
        Some(2),
        None,
        now + Duration::from_millis(200),
    );
    assert!(activity.active_at(now + Duration::from_millis(5099)));
    assert!(!activity.active_at(now + Duration::from_millis(5100)));
}
#[test]
fn uncertain_lease_acknowledgements_stay_guarded_through_maximum_ttl_after_last_worker_completion()
{
    let now = Instant::now();
    let activity = LeaseActivity::default();
    let pending = activity.begin();
    assert!(activity.active_at(now + Duration::from_secs(60)));
    pending.finish_at(
        "module:boot",
        "action_claim",
        None,
        None,
        now + Duration::from_secs(60),
    );
    assert!(activity.active_at(now + Duration::from_millis(64999)));
    assert!(!activity.active_at(now + Duration::from_secs(65)));
    for (command, lease, receipt) in [
        (
            "action_renew",
            Some(2),
            json!({"leaseId":3,"remainingMs":5}),
        ),
        (
            "action_release",
            Some(2),
            json!({"leaseId":3,"released":true}),
        ),
        (
            "action_release",
            Some(2),
            json!({"leaseId":2,"released":false}),
        ),
        ("action_claim", None, json!({"leaseId":0,"remainingMs":5})),
        (
            "action_claim",
            None,
            json!({"leaseId":2,"remainingMs":TTL+1}),
        ),
        (
            "action_claim",
            None,
            json!({"leaseId":2,"remainingMs":5,"released":true}),
        ),
        ("unknown", None, json!({"leaseId":2,"remainingMs":5})),
    ] {
        activity
            .begin()
            .finish_at("module:boot", command, lease, Some(&receipt), now);
        assert!(activity.active_at(now + Duration::from_millis(4999)));
        assert!(!activity.active_at(now + Duration::from_millis(5000)));
    }
    activity
        .begin()
        .finish_at("module:boot", "action_invoke_owned", Some(99), None, now);
    assert!(activity.active_at(now));
    assert!(!activity.active_at(now + Duration::from_millis(TTL)));
    activity.begin().unsent();
    assert!(!activity.active());
    drop(activity.begin());
    assert!(activity.active());
}
#[test]
fn malformed_scope_and_bounded_tracker_overflow_preserve_uncertainty_without_storing_capabilities()
{
    let activity = LeaseActivity::default();
    let now = Instant::now();
    for scope in ["", "bad\nroute"] {
        activity.begin().finish_at(
            scope,
            "action_claim",
            None,
            Some(&json!({"leaseId":1,"remainingMs":10})),
            now,
        );
        assert!(activity.active_at(now));
        assert!(!activity.active_at(now + Duration::from_millis(TTL)));
    }
    for id in 1..=MAX_TRACKED_LEASES + 1 {
        activity.begin().finish_at(
            "module:boot",
            "action_claim",
            None,
            Some(&json!({"leaseId":id,"remainingMs":1})),
            now,
        );
    }
    assert_eq!(
        activity.state.lock().unwrap().leases.len(),
        MAX_TRACKED_LEASES
    );
    assert!(activity.active_at(now + Duration::from_millis(2)));
    assert!(!activity.active_at(now + Duration::from_millis(TTL)));
}
#[test]
fn bounded_tokens_commands_and_readable_setters() {
    let r = request();
    let a = vec![action()];
    assert_eq!(
        child("action_claim", &r, 1, &a).unwrap(),
        "claim 1 led.set 00112233-4455-6677-8899-aabbccddeeff 5000 0"
    );
    assert!(child("action_invoke_owned", &r, 1, &a)
        .unwrap()
        .starts_with("invoke_owned "));
    assert!(child("action_release", &r, 1, &a).unwrap().ends_with(" 42"));
    assert!(child("action_renew", &r, 1, &a)
        .unwrap()
        .ends_with("42 5000"));
    assert_eq!(
        child("action_ownership", &json!({}), 1, &a).unwrap(),
        "ownership 0"
    );
    for (key, value) in [
        ("ownershipToken", json!("bad")),
        ("leaseId", json!(0)),
        ("ttlMs", json!(5001)),
        ("ttlMs", json!(249)),
        ("catalogueRevision", json!(2)),
        ("actionId", json!("missing")),
        ("values", json!(["bad"])),
    ] {
        let mut x = r.clone();
        x[key] = value;
        assert!(child(
            if key == "leaseId" {
                "action_invoke_owned"
            } else {
                "action_claim"
            },
            &x,
            1,
            &a
        )
        .is_err());
    }
    let mut once = action();
    once["semantics"] = json!("oneShot");
    assert!(child("action_claim", &r, 1, &[once]).is_err());
    assert!(child("bad", &r, 1, &a).is_err());
    assert!(child("action_ownership", &json!({"offset":17}), 1, &a).is_err());
    assert!(token(&json!("00000000-0000-0000-0000-000000000000")).is_err());
}
#[test]
fn replies_cannot_leak_capabilities_or_change_lease() {
    let r = request();
    let mut reply = json!({"ok":true,"schemaVersion":1,"catalogueRevision":1,"leaseId":42,"endpoint":"led","remainingMs":5000});
    assert!(acknowledgement(&reply, 1, &r, "action_claim").is_ok());
    assert!(acknowledgement(&reply, 1, &r, "action_renew").is_ok());
    reply["released"] = json!(true);
    assert!(acknowledgement(&reply, 1, &r, "action_release").is_ok());
    for (key, value) in [
        ("leaseId", json!(0)),
        ("leaseId", json!(43)),
        ("endpoint", json!("bad name")),
        ("remainingMs", json!(5001)),
        ("ownershipToken", r["ownershipToken"].clone()),
        ("catalogueRevision", json!(2)),
    ] {
        let mut x = reply.clone();
        x[key] = value;
        assert!(acknowledgement(&x, 1, &r, "action_renew").is_err());
    }
}
#[test]
fn ownership_pages_have_exact_bounded_progression_and_no_tokens() {
    let empty = json!({"ok":true,"schemaVersion":1,"catalogueRevision":1,"total":0,"next":null,"ownership":[]});
    assert!(ownership_page(&empty, 1, 0).is_ok());
    let entry = json!({"endpoint":"led","actionId":"led.set","owned":true,"leaseId":1,"remainingMs":2000,"fallback":[false],"fallbackStatus":"none","token":"secret"});
    let good = json!({"ok":true,"schemaVersion":1,"catalogueRevision":1,"total":2,"next":1,"ownership":[entry]});
    let result = ownership_page(&good, 1, 0).unwrap();
    assert!(!result.to_string().contains("secret"));
    for (key, value) in [
        ("total", json!(17)),
        ("next", json!(0)),
        ("next", Value::Null),
        ("ownership", json!([])),
        ("catalogueRevision", json!(2)),
    ] {
        let mut bad = good.clone();
        bad[key] = value;
        assert!(ownership_page(&bad, 1, 0).is_err());
    }
    for (key, value) in [
        ("endpoint", json!("bad name")),
        ("owned", json!(1)),
        ("leaseId", json!(-1)),
        ("remainingMs", json!(5001)),
        ("fallback", json!([1, 2, 3, 4, 5])),
        ("fallbackStatus", json!("bad")),
    ] {
        let mut bad = good.clone();
        bad["ownership"][0][key] = value;
        assert!(ownership_page(&bad, 1, 0).is_err());
    }
    let mut dup = good;
    dup["ownership"] = json!([dup["ownership"][0], dup["ownership"][0]]);
    dup["next"] = Value::Null;
    assert!(ownership_page(&dup, 1, 0).is_err());
}
#[test]
fn opt_in_capability_requires_endpoint_scope_and_known_bounds() {
    assert!(capability(&json!({"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5000})).is_ok());
    for invalid in [
        Value::Null,
        json!({"schemaVersion":2,"scope":"endpoint","maxLeaseMs":5000}),
        json!({"schemaVersion":1,"scope":"client","maxLeaseMs":5000}),
        json!({"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5001}),
    ] {
        assert!(capability(&invalid).is_err());
    }
}
#[test]
fn restored_policy_and_preserved_release_are_explicit_bounded_and_secret_free() {
    let legacy = json!({"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5000});
    assert_eq!(capability(&legacy).unwrap(), legacy);
    let mut guarded = legacy.clone();
    guarded["restorePolicy"] = json!("if-unchanged");
    assert_eq!(capability(&guarded).unwrap(), guarded);
    for policy in [Value::Null, json!("always"), json!(true)] {
        let mut invalid = guarded.clone();
        invalid["restorePolicy"] = policy;
        assert!(capability(&invalid).is_err());
    }
    let mut reply = json!({"ok":true,"schemaVersion":1,"catalogueRevision":1,"ownership":guarded,"leaseId":42,"endpoint":"led","released":true,"fallbackStatus":"preserved"});
    acknowledgement(&reply, 1, &request(), "action_release").unwrap();
    let receipt = lease_receipt(&reply);
    assert_eq!(receipt["fallbackStatus"], "preserved");
    assert!(receipt.get("ownership").is_none());
    reply["fallbackStatus"] = json!("applied");
    assert!(acknowledgement(&reply, 1, &request(), "action_release").is_ok());
    reply.as_object_mut().unwrap().remove("fallbackStatus");
    assert!(acknowledgement(&reply, 1, &request(), "action_release").is_err());
    reply.as_object_mut().unwrap().remove("ownership");
    assert!(acknowledgement(&reply, 1, &request(), "action_release").is_ok());
    assert!(lease_receipt(&reply).get("fallbackStatus").is_none());
    for status in ["failed", "none", "unknown"] {
        reply["fallbackStatus"] = json!(status);
        assert!(acknowledgement(&reply, 1, &request(), "action_release").is_err());
    }
    reply["fallbackStatus"] = json!("preserved");
    reply["remainingMs"] = json!(5000);
    assert!(acknowledgement(&reply, 1, &request(), "action_renew").is_err());
    reply["ownershipToken"] = request()["ownershipToken"].clone();
    assert!(!lease_receipt(&reply).to_string().contains("00112233"));
    reply.as_object_mut().unwrap().remove("fallbackStatus");
    reply.as_object_mut().unwrap().remove("ownershipToken");
    reply["ownership"] =
        json!({"schemaVersion":1,"scope":"endpoint","maxLeaseMs":5000,"restorePolicy":"unknown"});
    assert!(acknowledgement(&reply, 1, &request(), "action_renew").is_err());
}
#[test]
fn preserved_public_ownership_is_unowned_and_never_exposes_a_token() {
    let mut reply = json!({"ok":true,"schemaVersion":1,"catalogueRevision":1,"total":1,"next":null,"ownership":[{"endpoint":"led","actionId":"led.set","owned":false,"leaseId":42,"remainingMs":0,"fallback":[false],"fallbackStatus":"preserved","ownershipToken":"private"}]});
    let page = ownership_page(&reply, 1, 0).unwrap();
    assert_eq!(page["ownership"][0]["fallbackStatus"], "preserved");
    assert!(!page.to_string().contains("private"));
    for (field, value) in [
        ("owned", json!(true)),
        ("leaseId", json!(0)),
        ("remainingMs", json!(1)),
    ] {
        let mut invalid = reply.clone();
        invalid["ownership"][0][field] = value;
        assert!(ownership_page(&invalid, 1, 0).is_err());
    }
    reply["ownership"][0]["fallbackStatus"] = json!("failed");
    assert!(ownership_page(&reply, 1, 0).is_ok());
}
