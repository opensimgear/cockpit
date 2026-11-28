use super::*;
#[test]
fn retained_pages_expose_evictions_and_preserve_oversized_sample_wire() {
    let mut h = History::default();
    for n in 0..4100 {
        h.push(n, &[0, 42], Value::Null, None);
    }
    let page = h.page(0).unwrap();
    assert_eq!(page["gap"], true);
    assert_eq!(page["oldest"], "5");
    assert_eq!(page["reports"].as_array().unwrap().len(), 128);
    h.push(4101, &[0, 1], json!("s".repeat(513 * 1024)), None);
    let last = h.page(4100).unwrap();
    assert_eq!(last["reports"][0]["wireHex"], "0001");
    assert!(last["reports"][0]["samples"]["unavailable"].is_string());
    assert_eq!(last["gap"], false);
}
#[test]
fn translated_element_observations_have_no_wire_bytes() {
    let mut history = History::default();
    let source = json!({"source":"iohid-element","wireAvailable":false,"reportId":3});
    history.push(1, &[], json!({"cookie":17}), Some(source.clone()));
    let row = &history.page(0).unwrap()["reports"][0];
    assert!(row.get("wireHex").is_none());
    assert_eq!(row["provenance"], source);
    assert_eq!(row["reportId"], 3);
}
#[test]
fn explicit_reads_validate_deadline_scope_and_reply_fencing() {
    let mut req =
        json!({"deadlineMs":2001,"maxBytes":4,"kind":"feature","reportId":0,"requestId":"abc"});
    assert!(Get::parse(&req).is_err());
    req["deadlineMs"] = json!(2000);
    req["reportId"] = json!(256);
    assert!(Get::parse(&req).is_err());
    req["reportId"] = json!(0);
    req["maxBytes"] = json!(0);
    assert!(Get::parse(&req).is_err());
    for (closed, expected) in [(false, "timeout"), (true, "offline")] {
        let (_tx, rx) = mpsc::channel();
        let reply = Reply {
            rx,
            expires: Instant::now(),
            id: "r".into(),
            stop: Arc::new(AtomicBool::new(closed)),
        };
        assert_eq!(
            reply.wait("a", "g").unwrap()["result"]["error"]["kind"],
            expected
        );
    }
}
