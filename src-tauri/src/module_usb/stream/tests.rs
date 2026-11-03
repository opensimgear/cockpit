use super::*;
use osg_streams::{Delivery, LossReason, RetainedBytes};
use serde_json::json;

fn route() -> UsbRouteToken {
    UsbRouteToken { module_id: "a".repeat(32), serial: "A".repeat(24),
        boot: 7, connection_generation: 2, configuration_generation: 3 }
}
fn capture() -> Arc<Capture> { Capture::create(route(), false).unwrap() }
fn state(head: u32, sampled: u32) -> Value {
    json!({"ok":true,"schemaVersion":1,"stream":3,"head":head,"oldest":1,"overflow":0,
        "buttonCount":2,"held":[false,true],"contactButtonCount":2,"maintainedButtons":[1],
        "encoderButtons":[],"selectorButtons":[],"axisCount":1,"axes":[40000],
        "axisNeutral":[32767],"axisUsage":[48],"axisAvailable":[true],"sampledAtMs":sampled})
}
fn envelope(sequence: u32, kind: &str, body: Value) -> Value {
    json!({"boot":7,"sequence":sequence,"kind":kind,"state":body})
}
fn page(sequence: u32, head: u32, after: u32, count: u32, sampled: u32) -> Value {
    let mut body = state(head, sampled);
    body["after"] = json!(after);
    body["next"] = json!(after + count);
    body["more"] = json!(after + count < head);
    body["events"] = json!((1..=count).map(|i| json!({"sequence":after+i,"control":0,
        "pressed":i%2==1,"capturedAtMs":sampled})).collect::<Vec<_>>());
    envelope(sequence, "update", body)
}
fn feed(capture: &Capture, value: Value, milliseconds: u64) {
    let at = capture.shared.origin + Duration::from_millis(milliseconds);
    capture.shared.accept(Input::Line { line: format!("@events {value}"), received_at: at }, at);
}
fn reader(capture: &Capture) -> NativeSubscription<Snapshot, Batch> {
    let mut reader = capture.subscribe().unwrap();
    assert!(matches!(reader.recv().unwrap(), Delivery::Snapshot(s) if !s.value.ready));
    reader
}
fn baseline(capture: &Capture, reader: &mut NativeSubscription<Snapshot, Batch>, sampled: u32) {
    feed(capture, envelope(1, "baseline", state(0, sampled)), 1);
    assert!(matches!(reader.recv().unwrap(), Delivery::Loss(l) if l.reason == LossReason::GenerationReplaced));
    assert!(matches!(reader.recv().unwrap(), Delivery::Snapshot(s) if s.value.ready && s.captured_us == 1000));
}

#[test]
fn inactive_baseline_is_not_an_occurrence_and_keeps_shared_metadata() {
    let capture = capture();
    let mut reader = reader(&capture);
    baseline(&capture, &mut reader, 10);
    assert!(reader.try_recv().unwrap().is_none());
    let first = capture.snapshot();
    assert!(Arc::ptr_eq(first.state.as_ref().unwrap(), capture.snapshot().state.as_ref().unwrap()));
    assert_eq!(first.route, route());
    assert_eq!(first.stream, 3);
    assert!(first.state.unwrap().get("events").is_none());
}

#[test]
fn batch_keeps_exact_fractional_sample_instant_while_wire_microseconds_stay_floored() {
    let capture = capture();
    let mut reader = reader(&capture);
    let initial_at = capture.shared.origin + Duration::from_nanos(100_900);
    capture.shared.accept(Input::Line { line: format!("@events {}", envelope(1, "baseline", state(0, 10))),
        received_at: initial_at }, initial_at);
    assert!(matches!(reader.recv().unwrap(), Delivery::Loss(_)));
    assert!(matches!(reader.recv().unwrap(), Delivery::Snapshot(value) if value.captured_us == 100
        && value.value.received_at == Some(initial_at)));
    let next_at = capture.shared.origin + Duration::from_nanos(200_100);
    capture.shared.accept(Input::Line { line: format!("@events {}", page(2, 0, 0, 0, 11)),
        received_at: next_at }, next_at);
    let Delivery::Event(event) = reader.recv().unwrap() else { panic!() };
    assert_eq!(event.captured_us, 200);
    assert_eq!(event.value.witnessed_at, next_at);
    assert_eq!(capture.snapshot().received_at, Some(next_at));
}

#[test]
fn complete_pages_are_atomic_and_preserve_every_capture_clock() {
    let capture = capture();
    let mut reader = reader(&capture);
    baseline(&capture, &mut reader, 10);
    feed(&capture, page(2, 32, 0, 16, 20), 2);
    assert!(reader.try_recv().unwrap().is_none());
    assert_eq!(capture.snapshot().state.unwrap()["head"], 0);
    assert_eq!(capture.snapshot().received_at, Some(capture.shared.origin + Duration::from_millis(1)));
    feed(&capture, page(3, 32, 16, 16, 20), 3);
    let Delivery::Event(event) = reader.recv().unwrap() else { panic!() };
    assert_eq!(event.captured_us, 2000); // Same sample's first page, not completion RX.
    assert_eq!(event.value.witnessed_at, capture.shared.origin + Duration::from_millis(2));
    assert_eq!(event.value.events.len(), 32);
    for (i, edge) in event.value.events.iter().enumerate() {
        assert_eq!(edge.sequence, i as u32 + 1);
        assert_eq!(edge.control, 0);
        assert_eq!(edge.pressed, i % 2 == 0);
        let page = if i < 16 { 2 } else { 3 };
        assert_eq!(edge.received_at, capture.shared.origin + Duration::from_millis(page));
        assert_eq!(edge.captured_at_ms, 20);
    }
    assert!(event.value.latest.state.get("events").is_none());
    assert_eq!(event.value.latest.cursor, 32);
    assert!(event.value.retained_bytes() <= LIMITS.max_frame_bytes);
    assert_eq!(capture.snapshot().state.unwrap()["head"], 32);
    let mut late = capture.subscribe().unwrap();
    assert!(matches!(late.recv().unwrap(), Delivery::Snapshot(s) if s.captured_us == 2000));
}

#[test]
fn staging_overflow_withdraws_without_delivering_partial_edges() {
    let capture = capture();
    let mut reader = reader(&capture);
    baseline(&capture, &mut reader, 10);
    for i in 0..5 { feed(&capture, page(i + 2, 80, i * 16, 16, 20 + i), i as u64 + 2); }
    assert!(matches!(reader.recv().unwrap(), Delivery::Loss(_)));
    assert!(matches!(reader.recv().unwrap(), Delivery::Snapshot(s) if !s.value.ready && s.value.state.is_none()));
    assert!(reader.try_recv().unwrap().is_none());
    assert!(capture.snapshot().error.unwrap().contains("overflow"));
    feed(&capture, page(7, 80, 80, 0, 30), 8);
    assert!(!capture.snapshot().ready);
}

#[test]
fn source_gap_replaces_baseline_and_discards_incomplete_history() {
    let capture = capture();
    let mut reader = reader(&capture);
    baseline(&capture, &mut reader, 10);
    feed(&capture, page(2, 32, 0, 16, 20), 2);
    let mut loss = envelope(3, "loss", state(40, 21));
    loss["reason"] = json!("eventGap");
    feed(&capture, loss, 3);
    assert!(matches!(reader.recv().unwrap(), Delivery::Loss(_)));
    assert!(matches!(reader.recv().unwrap(), Delivery::Snapshot(s) if s.value.ready && s.value.state.as_ref().unwrap()["head"] == 40));
    assert!(reader.try_recv().unwrap().is_none());
    feed(&capture, page(4, 41, 40, 1, 22), 4);
    assert!(matches!(reader.recv().unwrap(), Delivery::Event(e) if e.value.events.len() == 1 && e.value.events[0].sequence == 41));
}

#[test]
fn wrong_boot_scope_malformed_and_exhaustion_are_terminal() {
    for kind in ["boot", "scope", "malformed", "exhausted"] {
        let capture = capture();
        let mut reader = reader(&capture);
        baseline(&capture, &mut reader, 10);
        let mut value = page(2, 0, 0, 0, 11);
        match kind {
            "boot" => value["boot"] = json!(8),
            "scope" => {
                value = envelope(2, "loss", state(0, 11));
                value["reason"] = json!("scopeChanged");
                value["state"]["stream"] = json!(4);
            }
            "malformed" => value["state"]["events"] = json!([{}]),
            _ => {
                value = envelope(2, "loss", state(0, 11));
                value["reason"] = json!("streamExhausted");
                value["exhausted"] = json!(true);
            }
        }
        feed(&capture, value, 2);
        assert!(matches!(reader.recv().unwrap(), Delivery::Loss(_)), "{kind}");
        assert!(matches!(reader.recv().unwrap(), Delivery::Snapshot(s) if !s.value.ready && s.value.state.is_none()), "{kind}");
        feed(&capture, page(3, 0, 0, 0, 12), 3);
        assert!(!capture.snapshot().ready, "{kind}");
        assert_eq!(capture.snapshot().route, route());
    }
}

#[test]
fn repeated_samples_cannot_renew_and_u32_clock_wrap_is_valid() {
    let capture = capture();
    let mut reader = reader(&capture);
    baseline(&capture, &mut reader, u32::MAX - 2);
    let initial = capture.shared.lock().deadline;
    feed(&capture, page(2, 0, 0, 0, u32::MAX - 2), 350);
    assert_eq!(capture.shared.lock().deadline, initial);
    feed(&capture, page(3, 0, 0, 0, 2), 390);
    assert_eq!(capture.shared.lock().deadline, Some(capture.shared.origin + Duration::from_millis(790)));
    assert_eq!(capture.snapshot().received_at, Some(capture.shared.origin + Duration::from_millis(390)));
    feed(&capture, page(4, 0, 0, 0, 1), 391);
    assert!(!capture.snapshot().ready);
}

#[test]
fn stale_arrival_cannot_recover_and_admission_delay_does_not_renew() {
    for delayed in [false, true] {
        let capture = capture();
        let mut reader = reader(&capture);
        baseline(&capture, &mut reader, 10);
        let received_at = capture.shared.origin + Duration::from_millis(if delayed { 2 } else { 402 });
        let now = capture.shared.origin + Duration::from_millis(if delayed { 403 } else { 402 });
        capture.shared.accept(Input::Line { line: format!("@events {}", page(2, 0, 0, 0, 11)), received_at }, now);
        assert!(!capture.snapshot().ready);
        assert!(matches!(reader.recv().unwrap(), Delivery::Loss(_)));
    }
}

#[test]
fn repeated_health_cannot_fabricate_freshness_and_passive_reads_do_not_renew() {
    let capture = capture();
    let mut reader = reader(&capture);
    baseline(&capture, &mut reader, 10);
    let deadline = capture.shared.lock().deadline;
    for (sequence, at) in [(2, 100), (3, 200), (4, 300), (5, 400)] {
        feed(&capture, page(sequence, 0, 0, 0, 10), at);
        assert!(matches!(reader.recv().unwrap(), Delivery::Event(e) if e.captured_us == 1000
            && e.value.witnessed_at == capture.shared.origin + Duration::from_millis(1)));
        let mut late = capture.subscribe().unwrap();
        assert!(matches!(late.recv().unwrap(), Delivery::Snapshot(s) if s.captured_us == 1000));
        let _ = capture.snapshot();
        assert_eq!(capture.shared.lock().deadline, deadline);
        assert_eq!(capture.snapshot().received_at, Some(capture.shared.origin + Duration::from_millis(1)));
    }
    let at = capture.shared.origin + Duration::from_millis(402);
    capture.shared.expire(&mut capture.shared.lock(), at);
    assert!(matches!(reader.recv().unwrap(), Delivery::Loss(_)));
    assert!(!capture.snapshot().ready);
    feed(&capture, page(6, 0, 0, 0, 1000), 403);
    assert!(!capture.snapshot().ready);
}

#[test]
fn same_sample_gap_replacement_retains_evidence_time() {
    let capture = capture();
    let mut reader = reader(&capture);
    baseline(&capture, &mut reader, 10);
    let mut loss = envelope(2, "loss", state(0, 10));
    loss["reason"] = json!("eventGap");
    feed(&capture, loss, 100);
    assert!(matches!(reader.recv().unwrap(), Delivery::Loss(_)));
    assert!(matches!(reader.recv().unwrap(), Delivery::Snapshot(s) if s.value.ready && s.captured_us == 1000));
    assert_eq!(capture.shared.lock().deadline, Some(capture.shared.origin + Duration::from_millis(401)));
    let at = capture.shared.origin + Duration::from_millis(402);
    capture.shared.expire(&mut capture.shared.lock(), at);
    assert!(matches!(reader.recv().unwrap(), Delivery::Loss(_)));
    assert!(matches!(reader.recv().unwrap(), Delivery::Snapshot(s) if !s.value.ready && s.captured_us == 402000));
}

#[test]
fn raw_edge_age_and_rx_are_preserved_and_future_or_regressed_module_clocks_fail() {
    for invalid in [false, true] {
        let capture = capture();
        let mut reader = reader(&capture);
        baseline(&capture, &mut reader, 10);
        let mut value = page(2, 1, 0, 1, 400);
        value["state"]["events"][0]["capturedAtMs"] = json!(if invalid { 401 } else { 20 });
        feed(&capture, value, 400);
        if invalid {
            assert!(matches!(reader.recv().unwrap(), Delivery::Loss(_)));
            assert!(!capture.snapshot().ready);
        } else {
            let Delivery::Event(event) = reader.recv().unwrap() else { panic!() };
            let edge = &event.value.events[0];
            assert_eq!(edge.captured_at_ms, 20);
            assert_eq!(event.value.latest.sampled_at_ms.wrapping_sub(edge.captured_at_ms), 380);
            assert_eq!(edge.received_at, capture.shared.origin + Duration::from_millis(400));
            let mut backwards = page(3, 2, 1, 1, 401);
            backwards["state"]["events"][0]["capturedAtMs"] = json!(19);
            feed(&capture, backwards, 401);
            assert!(matches!(reader.recv().unwrap(), Delivery::Loss(_)));
        }
    }
}

#[test]
fn maximal_metadata_and_four_pages_fit_conservative_memory_bounds() {
    let capture = capture();
    let mut reader = reader(&capture);
    let maximize = |mut value: Value| {
        let body = &mut value["state"];
        body["buttonCount"] = json!(128);
        body["held"] = json!(vec![false; 128]);
        body["contactButtonCount"] = json!(128);
        body["maintainedButtons"] = json!((0..128).collect::<Vec<_>>());
        body["axisCount"] = json!(8);
        for (key, value) in [("axes", 40000), ("axisNeutral", 32767), ("axisUsage", 48)] {
            body[key] = json!(vec![value; 8]);
        }
        body["axisAvailable"] = json!(vec![true; 8]);
        value
    };
    feed(&capture, maximize(envelope(1, "baseline", state(0, 10))), 1);
    assert!(matches!(reader.recv().unwrap(), Delivery::Loss(_)));
    assert!(matches!(reader.recv().unwrap(), Delivery::Snapshot(s) if s.value.ready));
    for i in 0..4 {
        feed(&capture, maximize(page(i + 2, 64, i * 16, 16, 20 + i)), i as u64 + 2);
    }
    let Delivery::Event(event) = reader.recv().unwrap() else { panic!() };
    assert_eq!(event.value.events.len(), 64);
    assert!(event.value.retained_bytes() <= LIMITS.max_frame_bytes);
    assert!(capture.snapshot().retained_bytes() <= LIMITS.max_snapshot_bytes);
}

#[test]
fn generation_exhaustion_closes_instead_of_wrapping_or_reviving() {
    let capture = capture();
    let mut reader = reader(&capture);
    capture.shared.lock().generation = u64::MAX;
    feed(&capture, envelope(1, "baseline", state(0, 10)), 1);
    assert!(matches!(reader.recv().unwrap(), Delivery::Loss(l) if l.reason == LossReason::Closed));
    assert!(!capture.snapshot().ready);
    assert!(capture.snapshot().error.unwrap().contains("generation exhausted"));
    assert!(capture.subscribe().is_err());
}

#[test]
fn close_cancel_wake_reader_bounds_and_slow_reader_are_independent() {
    let capture = capture();
    let wake = Arc::new(NativeWake::default());
    let watch = capture.watch(wake.clone()).unwrap();
    let mut slow = reader(&capture);
    let mut fast = reader(&capture);
    let readers: Vec<_> = (0..5).map(|_| capture.subscribe().unwrap()).collect();
    assert!(capture.subscribe().is_err());
    drop(readers);
    baseline(&capture, &mut fast, 10);
    // Clear the slow reader's replacement boundary, then let only it fall behind.
    assert!(matches!(slow.recv().unwrap(), Delivery::Loss(_)));
    assert!(matches!(slow.recv().unwrap(), Delivery::Snapshot(_)));
    let before = wake.observed();
    for i in 0..65 {
        feed(&capture, page(i + 2, 0, 0, 0, i + 11), i as u64 + 2);
        assert!(matches!(fast.recv().unwrap(), Delivery::Event(_)));
    }
    assert!(wake.observed() > before);
    assert!(matches!(slow.recv().unwrap(), Delivery::Loss(l) if l.reason == LossReason::Overflow));
    assert!(!slow.is_continuous());
    fast.cancellation_handle().cancel();
    assert!(fast.recv().is_err());
    capture.close();
    assert!(watch.is_closed());
    assert!(!capture.snapshot().ready);
    assert!(capture.subscribe().is_err());
}

#[test]
fn real_deadline_withdraws_missing_first_baseline_without_polling() {
    let capture = Capture::new(route()).unwrap();
    let mut reader = reader(&capture);
    assert!(matches!(reader.recv_timeout(Duration::from_secs(2)).unwrap(), Some(Delivery::Loss(_))));
    assert!(matches!(reader.recv().unwrap(), Delivery::Snapshot(s) if !s.value.ready && s.value.error.is_some()));
    capture.accept(Input::Lost("Native read cancelled.".into()));
    assert!(capture.subscribe().is_err());
}
