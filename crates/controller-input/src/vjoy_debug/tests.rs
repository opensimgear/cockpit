use super::*;
const SLOT: u8 = 1;
struct Fake(Option<String>);

#[test]
fn open_captures_the_generations_first_packet_and_only_inspect_borrows() {
    let service = Service::new(true);
    let args = json!({"slot":2,"clientId":"writer"});
    let caps = json!({"slotId":2,"axes":[]});
    let opened = service
        .request_with(
            "controller_vjoy_debug_open",
            &args,
            |_| panic!("unexpected duplicate inspection"),
            |slot, sink| {
                assert_eq!(slot, 2);
                sink(0xb0017, vec![0x2d, 42]);
                Ok((caps.clone(), Box::new(Fake(None))))
            },
        )
        .unwrap();
    let inspected = service
        .request_with(
            "controller_vjoy_debug_inspect",
            &args,
            |_| Ok(caps.clone()),
            |_, _| panic!("inspection acquired a slot"),
        )
        .unwrap();
    assert_eq!(inspected["captureGeneration"], opened["generation"]);
    let mut selected = opened;
    selected["clientId"] = json!("reader");
    let result = service
        .request("controller_vjoy_debug_read", &selected)
        .unwrap();
    assert_eq!(result["records"][0]["wireHex"], "2d2a");
    selected["generation"] = json!("previous");
    assert!(service
        .request("controller_vjoy_debug_read", &selected)
        .is_err());
    service.shutdown();
    let archive = json!({"format":"osg-vjoy-ffb","slot":2,"records":[{"sequence":u64::MAX.to_string(),"command":0,"wireHex":"00"}]});
    let end = history::replay(&json!({"recording":archive,"after":u64::MAX.to_string()})).unwrap();
    assert_eq!(end["records"], json!([]));
    assert_eq!(end["hasMore"], false);
}
impl CaptureHealth for Fake {
    fn failure(&self) -> Option<String> {
        self.0.clone()
    }
}
fn live(failure: Option<String>) -> (Service, Arc<Mutex<History>>) {
    let service = Service::new(true);
    let history = Arc::new(Mutex::new(History::default()));
    service.state.lock().unwrap().session = Some(Session {
        slot: SLOT,
        owner: "writer".into(),
        generation: "g".into(),
        capabilities: json!({"status":"free"}),
        history: history.clone(),
        capture: Box::new(Fake(failure)),
    });
    (service, history)
}
fn request(service: &Service, op: &str, client: &str) -> Result<Value, String> {
    service.request(
        &format!("controller_vjoy_debug_{op}"),
        &json!({"slot":SLOT,"generation":"g","clientId":client}),
    )
}
#[test]
fn recorded_callbacks_round_trip_with_exact_commands_bytes_and_state() {
    let (service, history) = live(None);
    assert!(request(&service, "export", "reader").is_err());
    assert!(request(&service, "record_start", "reader").is_err());
    request(&service, "record_start", "writer").unwrap();
    assert!(request(&service, "record_start", "writer").is_err());
    for data in [&[0x1d, 7, 0xff][..], &[0x1c, 2], &[0x1f, 0xfa, 0x03]] {
        history.lock().unwrap().observe(SLOT, 0xb000f, data);
    }
    let read = request(&service, "read", "reader").unwrap();
    assert_eq!(read["latest"], "3");
    request(&service, "record_stop", "writer").unwrap();
    let export = request(&service, "export", "reader").unwrap();
    let replay = Service::new(false)
        .request("controller_vjoy_debug_replay", &export)
        .unwrap();
    assert_eq!(replay["records"], read["records"]);
    assert_eq!(replay["effects"], read["effects"]);
    assert_eq!(replay["records"][2]["wireHex"], "1ffa03");
    let first = history::replay(&json!({"recording":export["recording"],"limit":1})).unwrap();
    assert_eq!(first["next"], "1");
    assert_eq!(first["hasMore"], true);
    let last = history::replay(&json!({"recording":export["recording"],"after":"1"})).unwrap();
    assert_eq!(last["effects"], read["effects"]);
    assert_eq!(last["records"][0], read["records"][1]);
}
#[test]
fn authorization_generation_and_shutdown_retire_the_handle() {
    let (service, _) = live(None);
    assert!(request(&service, "close", "other").is_err());
    assert!(request(&service, "open", "writer").is_err());
    assert!(request(&service, "invalid", "writer").is_err());
    service.release_client("other");
    assert!(request(&service, "read", "other").is_ok());
    service.release_client("writer");
    assert!(request(&service, "read", "writer").is_err());
    let (service, _) = live(Some("driver disconnected".into()));
    assert_eq!(
        request(&service, "read", "reader").unwrap()["status"],
        "ended"
    );
    assert!(request(&service, "record_start", "writer").is_err());
    assert_eq!(
        request(&service, "close", "writer").unwrap()["status"],
        "closed"
    );
    service.shutdown();
    assert!(request(&service, "inspect", "any").is_err());
    assert!(request(&Service::new(false), "inspect", "any").is_err());
}
#[test]
fn loss_and_recording_limits_remain_visible_when_the_reader_stalls() {
    let mut h = History::default();
    h.record().unwrap();
    for _ in 0..520 {
        h.observe(1, 1, &[0xff; 248])
    }
    assert_eq!(h.status()["reason"], "capacity");
    let r = h.read(&json!({"limit":1})).unwrap();
    assert_eq!(r["oldest"], "9");
    assert_eq!(r["gap"], true);
    assert_eq!(r["next"], "9");
    let archive = h.export(1, &json!({})).unwrap();
    assert!(archive["records"].as_array().unwrap().len() < 512);
    h.record().unwrap();
    assert_eq!(h.status()["records"], 0);
    h.observe(1,0xb0017,&[0x1d,42]);
    h.stop("stopped").unwrap();
    let archive=h.export(1,&json!({})).unwrap();
    assert_eq!(archive["startingSequence"],"520");
    let replay=history::replay(&json!({"recording":archive})).unwrap();
    assert_eq!(replay["priorCallbacksExcluded"],true);
    assert_eq!(replay["effects"]["gain"],42);
    for invalid in [
        json!({"limit":0}),
        json!({"after":"01"}),
        json!({"after":"522"}),
    ] {
        assert!(h.read(&invalid).is_err())
    }
}
#[test]
fn import_validates_the_entire_archive_even_before_a_short_first_page() {
    let base = json!({"format":"osg-vjoy-ffb","slot":1,"records":[{"sequence":"9007199254740993","command":1,"wireHex":"ab"}]});
    let page = history::replay(&json!({"recording":base})).unwrap();
    assert_eq!(page["next"], "9007199254740993");
    for (key, bad) in [
        ("sequence", json!("0")),
        ("sequence", json!("01")),
        ("command", json!(-1)),
        ("wireHex", json!("ab0")),
        ("wireHex", json!("xx")),
        ("wireHex", json!("ab".repeat(249))),
    ] {
        let mut broken = base.clone();
        broken["records"][0][key] = bad;
        assert!(history::replay(&json!({"recording":broken})).is_err());
    }
    let mut gap = base.clone();
    gap["records"]
        .as_array_mut()
        .unwrap()
        .push(json!({"sequence":"9007199254740995","command":1,"wireHex":"ab"}));
    assert!(history::replay(&json!({"recording":gap,"limit":1})).is_err());
    for (key, value) in [
        ("slot", json!(17)),
        ("format", json!("other")),
        ("capabilities", json!(false)),
        ("unknown", json!(true)),
    ] {
        let mut bad = base.clone();
        bad[key] = value;
        assert!(history::replay(&json!({"recording":bad})).is_err());
    }
}
