use super::*;
use serde_json::json;
use std::time::Duration;

fn state(stream: u32, head: u32, sampled: u32) -> Value {
    json!({"ok":true,"schemaVersion":1,"stream":stream,"head":head,"oldest":1,"overflow":0,
        "buttonCount":2,"held":[false,true],"contactButtonCount":2,"maintainedButtons":[1],
        "encoderButtons":[],"selectorButtons":[],"axisCount":1,"axes":[40000],
        "axisNeutral":[32767],"axisUsage":[48],"axisAvailable":[true],"sampledAtMs":sampled})
}
fn envelope(sequence: u32, kind: &str, body: Value) -> Value {
    json!({"boot":7,"sequence":sequence,"kind":kind,"state":body})
}
fn line(value: &Value) -> String { format!("@events {value}") }
fn decode(decoder: &mut Decoder, value: Value) -> Result<Frame, String> {
    decoder.decode(&line(&value), Instant::now())
}
fn baseline(decoder: &mut Decoder, stream: u32, head: u32) -> Frame {
    decode(decoder, envelope(1, "baseline", state(stream, head, 10))).unwrap()
}
fn page(stream: u32, head: u32, after: u32, count: u32, sampled: u32) -> Value {
    let mut body = state(stream, head, sampled);
    body["after"] = json!(after);
    body["next"] = json!(after + count);
    body["more"] = json!(after + count < head);
    body["events"] = json!((1..=count).map(|index| json!({"sequence":after+index,
        "control":0,"pressed":index%2==1,"capturedAtMs":sampled})).collect::<Vec<_>>());
    body
}
fn loss(sequence: u32, reason: &str, body: Value) -> Value {
    let mut frame = envelope(sequence, "loss", body);
    frame["reason"] = json!(reason);
    if reason == "streamExhausted" { frame["exhausted"] = json!(true); }
    frame
}

#[test]
fn baseline_anchors_at_current_head_and_bounded_pages_preserve_every_edge() {
    let mut decoder = Decoder::new(7);
    let first = baseline(&mut decoder, 3, 8);
    assert_eq!((first.boot, first.sequence, first.stream, first.head, first.cursor), (7, 1, 3, 8, 8));
    assert_eq!(first.kind, Kind::Baseline);
    assert_eq!(first.reason, None);
    assert!(!first.exhausted);
    let first_page = decode(&mut decoder, envelope(2, "update", page(3, 40, 8, 16, 11))).unwrap();
    assert_eq!((first_page.head, first_page.cursor), (40, 24));
    assert_eq!(first_page.state["more"], true);
    let second = decode(&mut decoder, envelope(3, "update", page(3, 40, 24, 16, 12))).unwrap();
    assert_eq!(second.cursor, 40);
    assert_eq!(second.state["events"][0]["sequence"], 25);
    assert_eq!(second.state["events"][15]["sequence"], 40);
    for reason in [None, Some("events"), Some("axes"), Some("health")] {
        let sequence = decoder.position.as_ref().unwrap().sequence + 1;
        let mut frame = envelope(sequence, "update", page(3, 40, 40, 0, 12));
        if let Some(reason) = reason { frame["reason"] = json!(reason); }
        assert_eq!(decode(&mut decoder, frame).unwrap().cursor, 40);
    }
}

#[test]
fn malformed_headers_framing_boots_and_initial_kinds_are_transactional() {
    let good = envelope(1, "baseline", state(3, 0, 1));
    let mut invalid = vec![json!(null), json!([]), json!({}), envelope(1, "update", page(3, 0, 0, 0, 1)),
        loss(1, "eventGap", state(3, 0, 1))];
    for (key, value) in [("boot", json!(0)), ("boot", json!(8)), ("boot", json!(-1)), ("boot", json!(4294967296u64)),
        ("sequence", json!(0)), ("sequence", json!(2)), ("sequence", json!(1.0)), ("kind", json!("snapshot")),
        ("reason", json!(null)), ("reason", json!("health")), ("exhausted", json!(null)), ("exhausted", json!(true)),
        ("unknown", json!(true)), ("state", json!(null))] {
        let mut patched = good.clone(); patched[key] = value; invalid.push(patched);
    }
    for invalid in invalid {
        let mut decoder = Decoder::new(7);
        assert!(decode(&mut decoder, invalid).is_err());
        assert!(decoder.position.is_none());
        assert!(decode(&mut decoder, good.clone()).is_ok());
    }
    for bad in ["", "@event {}", " @events {}", "@events not-json", "@events {} trailing", "@events {}\r\n",
        "@events {\"boot\":7,\"boot\":7}", "@events \t{}"] {
        assert!(Decoder::new(7).decode(bad, Instant::now()).is_err(), "{bad}");
    }
    assert!(decode(&mut Decoder::new(0), good).is_err());
}

#[test]
fn body_bounds_source_times_and_edge_timestamps_are_checked_before_commit() {
    for (key, value) in [("sampledAtMs", json!(null)), ("sampledAtMs", json!(-1)), ("sampledAtMs", json!(4294967296u64)),
        ("axisAvailable", json!([1])), ("axes", json!([65536])), ("held", json!([false])), ("stream", json!(0))] {
        let mut body = state(3, 0, 10); body[key] = value;
        assert!(decode(&mut Decoder::new(7), envelope(1, "baseline", body)).is_err());
    }
    let mut absent = state(3, 0, 10); absent.as_object_mut().unwrap().remove("sampledAtMs");
    assert!(decode(&mut Decoder::new(7), envelope(1, "baseline", absent)).is_err());
    for value in [json!(null), json!(-1), json!(4294967296u64), json!("10")] {
        let mut decoder = Decoder::new(7); baseline(&mut decoder, 3, 0);
        let mut body = page(3, 1, 0, 1, 11); body["events"][0]["capturedAtMs"] = value;
        assert!(decode(&mut decoder, envelope(2, "update", body)).is_err());
        assert_eq!(decoder.position.as_ref().unwrap().sequence, 1);
        assert!(decode(&mut decoder, envelope(2, "update", page(3, 1, 0, 1, 11))).is_ok());
    }
    let mut decoder = Decoder::new(7); baseline(&mut decoder, 3, 0);
    let mut body = page(3, 1, 0, 1, 11); body["events"][0].as_object_mut().unwrap().remove("capturedAtMs");
    assert!(decode(&mut decoder, envelope(2, "update", body)).is_err());
    assert!(decode(&mut decoder, envelope(2, "update", page(3, 17, 0, 17, 11))).is_err());
}

#[test]
fn malformed_pages_late_pages_and_replayed_wire_sequences_do_not_consume_position() {
    let mut decoder = Decoder::new(7); baseline(&mut decoder, 3, 0);
    decode(&mut decoder, envelope(2, "update", page(3, 2, 0, 2, 11))).unwrap();
    for invalid in [envelope(2, "update", page(3, 2, 2, 0, 11)), envelope(4, "update", page(3, 2, 2, 0, 11)),
        envelope(3, "baseline", state(3, 2, 11)), envelope(3, "update", page(4, 3, 2, 1, 12)),
        envelope(3, "update", page(3, 2, 0, 2, 11)), envelope(3, "update", page(3, 1, 1, 0, 10))] {
        assert!(decode(&mut decoder, invalid).is_err());
        assert_eq!(decoder.position.as_ref().unwrap().sequence, 2);
        assert_eq!(decoder.position.as_ref().unwrap().cursor, 2);
    }
    let mut boot_ahead = envelope(3, "update", page(3, 3, 2, 1, 12)); boot_ahead["boot"] = json!(8);
    assert!(decode(&mut decoder, boot_ahead).is_err());
    for (key, value) in [("next", json!(2)), ("more", json!(true)), ("after", json!(1)), ("events", json!([]))] {
        let mut body = page(3, 3, 2, 1, 12); body[key] = value;
        assert!(decode(&mut decoder, envelope(3, "update", body)).is_err());
    }
    assert!(decode(&mut decoder, envelope(3, "update", page(3, 3, 2, 1, 12))).is_ok());
}

#[test]
fn event_gap_is_an_atomic_baseline_and_never_replays_overflowed_history() {
    let mut decoder = Decoder::new(7); baseline(&mut decoder, 3, 2);
    let mut body = state(3, 10, 20); body["oldest"] = json!(9); body["overflow"] = json!(8);
    let gap = decode(&mut decoder, loss(2, "eventGap", body)).unwrap();
    assert_eq!(gap.kind, Kind::Loss); assert_eq!(gap.cursor, 10); assert!(!gap.exhausted);
    assert!(gap.state.get("events").is_none());
    let mut live = page(3, 11, 10, 1, 21); live["oldest"] = json!(9); live["overflow"] = json!(8);
    assert_eq!(decode(&mut decoder, envelope(3, "update", live)).unwrap().cursor, 11);
    let mut regressed = page(3, 11, 11, 0, 21); regressed["oldest"] = json!(8); regressed["overflow"] = json!(7);
    assert!(decode(&mut decoder, envelope(4, "update", regressed)).is_err());
}

#[test]
fn only_a_greater_scope_loss_can_change_stream_or_restore_after_exhaustion() {
    let mut decoder = Decoder::new(7); baseline(&mut decoder, 3, 2);
    for invalid in [loss(2, "scopeChanged", state(3, 0, 11)), loss(2, "scopeChanged", state(2, 0, 11)),
        loss(2, "eventGap", state(4, 2, 11)), loss(2, "streamExhausted", state(2, 2, 11)),
        loss(2, "health", state(3, 2, 11)), envelope(2, "loss", state(3, 2, 11))] {
        assert!(decode(&mut decoder, invalid).is_err());
    }
    let exhausted = decode(&mut decoder, loss(2, "streamExhausted", state(3, 2, 11))).unwrap();
    assert!(exhausted.exhausted);
    assert!(decode(&mut decoder, envelope(3, "update", page(3, 2, 2, 0, 12))).is_err());
    assert!(decode(&mut decoder, loss(3, "eventGap", state(3, 2, 12))).is_err());
    assert!(decode(&mut decoder, loss(3, "streamExhausted", state(3, 2, 12))).unwrap().exhausted);
    let scope = decode(&mut decoder, loss(4, "scopeChanged", state(4, 0, 0))).unwrap();
    assert_eq!((scope.stream, scope.cursor, scope.exhausted), (4, 0, false));
    assert!(decode(&mut decoder, envelope(5, "update", page(4, 1, 0, 1, 1))).is_ok());
}

#[test]
fn a_new_already_exhausted_scope_is_diagnostic_until_a_strictly_greater_healthy_scope() {
    let mut decoder = Decoder::new(7); baseline(&mut decoder, 3, 2);
    let exhausted = decode(&mut decoder, loss(2, "streamExhausted", state(4, 0, 12))).unwrap();
    assert_eq!((exhausted.stream, exhausted.cursor, exhausted.exhausted), (4, 0, true));
    assert_eq!(exhausted.reason.as_deref(), Some("streamExhausted"));
    assert!(exhausted.state.get("events").is_none());
    let mut missing_marker = loss(3, "streamExhausted", state(5, 0, 13));
    missing_marker["exhausted"] = json!(false);
    for rejected in [loss(3, "streamExhausted", state(3, 2, 13)), missing_marker,
        envelope(3, "update", page(4, 1, 0, 1, 13)),
        loss(3, "eventGap", state(4, 0, 13)), loss(3, "scopeChanged", state(4, 0, 13))] {
        assert!(decode(&mut decoder, rejected).is_err());
        let position = decoder.position.as_ref().unwrap();
        assert_eq!((position.sequence, position.stream, position.cursor, position.exhausted), (2, 4, 0, true));
    }
    assert!(decode(&mut decoder, loss(3, "streamExhausted", state(5, 0, 14))).unwrap().exhausted);
    assert!(decode(&mut decoder, envelope(4, "update", page(5, 1, 0, 1, 15))).is_err());
    let healthy = decode(&mut decoder, loss(4, "scopeChanged", state(6, 0, 16))).unwrap();
    assert_eq!((healthy.stream, healthy.cursor, healthy.exhausted), (6, 0, false));
    assert!(decode(&mut decoder, envelope(5, "update", page(6, 1, 0, 1, 17))).is_ok());
}

#[test]
fn simultaneous_ring_gap_and_exhaustion_accepts_terminal_metadata_without_replay() {
    let mut decoder = Decoder::new(7); baseline(&mut decoder, 3, 2);
    let mut body = state(3, 20, 30);
    body["oldest"] = json!(19); body["overflow"] = json!(18);
    let exhausted = decode(&mut decoder, loss(2, "streamExhausted", body.clone())).unwrap();
    assert_eq!((exhausted.cursor, exhausted.exhausted), (20, true));
    assert_eq!(*exhausted.state, body);
    assert!(exhausted.state.get("events").is_none());
    let mut live = page(3, 21, 20, 1, 31);
    live["oldest"] = json!(19); live["overflow"] = json!(18);
    assert!(decode(&mut decoder, envelope(3, "update", live)).is_err());
    assert!(decode(&mut decoder, loss(3, "eventGap", body)).is_err());
    assert_eq!(decoder.position.as_ref().unwrap().sequence, 2);
}

#[test]
fn explicit_loss_markers_and_metadata_only_frames_are_required() {
    for reason in ["eventGap", "scopeChanged", "streamExhausted"] {
        let mut decoder = Decoder::new(7); baseline(&mut decoder, 3, 0);
        let stream = if reason == "scopeChanged" { 4 } else { 3 };
        for key in ["events", "after", "next", "more"] {
            let mut body = state(stream, 0, 11); body[key] = if key == "events" { json!([]) } else { json!(0) };
            assert!(decode(&mut decoder, loss(2, reason, body)).is_err());
        }
        let mut bad = loss(2, reason, state(stream, 0, 11));
        bad["exhausted"] = json!(reason != "streamExhausted");
        assert!(decode(&mut decoder, bad).is_err());
        assert!(decode(&mut decoder, loss(2, reason, state(stream, 0, 11))).is_ok());
    }
    let mut baseline_events = state(3, 0, 10); baseline_events["events"] = json!([]);
    assert!(decode(&mut Decoder::new(7), envelope(1, "baseline", baseline_events)).is_err());
}

#[test]
fn clock_wrap_and_prelock_receipt_are_preserved_without_inventing_a_source_time() {
    let mut decoder = Decoder::new(7);
    decode(&mut decoder, envelope(1, "baseline", state(3, 0, u32::MAX))).unwrap();
    let mut body = page(3, 1, 0, 1, 0); body["events"][0]["capturedAtMs"] = json!(u32::MAX);
    let received = Instant::now() - Duration::from_secs(3);
    let frame = decoder.decode(&line(&envelope(2, "update", body.clone())), received).unwrap();
    assert_eq!(frame.received_at, received); assert_eq!(frame.sampled_at_ms, 0);
    assert_eq!(*frame.state, body); assert_eq!(frame.state["events"][0]["capturedAtMs"], u32::MAX);
    let clone = frame.clone(); assert!(Arc::ptr_eq(&frame.state, &clone.state));
}

#[test]
fn wire_and_stream_counters_cannot_wrap_or_reopen_the_retired_scope() {
    let mut decoder = Decoder::new(7); baseline(&mut decoder, u32::MAX, 0);
    assert!(decode(&mut decoder, loss(2, "scopeChanged", state(1, 0, 0))).is_err());
    decoder.position.as_mut().unwrap().sequence = u32::MAX - 1;
    assert_eq!(decode(&mut decoder, envelope(u32::MAX, "update", page(u32::MAX, 0, 0, 0, 1))).unwrap().sequence, u32::MAX);
    assert!(decode(&mut decoder, envelope(1, "update", page(u32::MAX, 0, 0, 0, 2))).is_err());
    assert!(decode(&mut decoder, envelope(u32::MAX, "baseline", state(u32::MAX, 0, 2))).is_err());
}

#[test]
fn byte_budgets_reject_large_envelopes_and_existing_event_body_limits_still_apply() {
    assert!(Decoder::new(7).decode(&format!("@events {}", " ".repeat(MAX_LINE_BYTES)), Instant::now()).is_err());
    let mut body = state(3, 0, 1); body["padding"] = json!("");
    let empty_len = body.to_string().len();
    body["padding"] = json!("x".repeat(4091 - empty_len));
    assert_eq!(body.to_string().len(), 4091);
    assert!(decode(&mut Decoder::new(7), envelope(1, "baseline", body.clone())).is_ok());
    body["padding"] = json!("x".repeat(4092 - empty_len));
    let encoded = line(&envelope(1, "baseline", body));
    assert!(encoded.len() < MAX_LINE_BYTES);
    assert!(Decoder::new(7).decode(&encoded, Instant::now()).is_err());
}
