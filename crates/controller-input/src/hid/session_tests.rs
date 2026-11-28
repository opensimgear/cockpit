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
    assert_eq!(last["reports"][0]["samples"], "s".repeat(513 * 1024));
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
            history: Arc::new(Mutex::new(History::default())),
            token: String::new(),
            bounded: false,
        };
        assert_eq!(
            reply.wait("a", "g").unwrap()["result"]["error"]["kind"],
            expected
        );
    }
}

fn integer(field: usize, semantics: &str, value: &str) -> Value {
    json!({"field":field,"element":0,"raw_bits":"01","sample":{"Integer":value},
        "semantics":semantics,"physical":null,"usages":[]})
}

#[test]
fn current_state_preserves_exact_values_without_integrating_relative_events() {
    let mut h = History::default();
    assert_eq!(h.state()["status"], "waiting");
    for n in 1..=2 {
        h.push(
            n,
            &[1],
            json!({"kind":"input","report_id":1,"diagnostics":[],"values":[
            integer(0,"Absolute","9232379236109516801"), integer(1,"Relative","-1")]}),
            None,
        );
    }
    let state = h.state();
    assert_eq!(state["status"], "live");
    assert_eq!(state["latest"], "2");
    assert_eq!(state["values"].as_array().unwrap().len(), 1);
    assert_eq!(state["values"][0]["sample"]["value"], "9232379236109516801");
    assert_eq!(state["values"][0]["sequence"], "2");
    assert_eq!(state["values"][0]["rawBits"]["value"], "01");
    assert_eq!(h.page(0).unwrap()["reports"].as_array().unwrap().len(), 2);
}

#[test]
fn sample_quality_and_missing_reports_do_not_become_zero() {
    let mut h = History::default();
    for (sample, expected) in [
        (json!({"Integer":"0"}), "valid"),
        (json!({"Null":"255"}), "null"),
        (json!({"Invalid":"256"}), "invalid"),
        (
            json!({"Unknown":{"reason":"selector unknown","encoded":"7"}}),
            "unknown",
        ),
    ] {
        let mut value = integer(0, "Absolute", "0");
        value["sample"] = sample;
        h.push(
            1,
            &[0],
            json!({"report_id":1,"values":[value],"diagnostics":[]}),
            None,
        );
        assert_eq!(h.state()["values"][0]["sample"]["state"], expected);
    }
    h.push(
        2,
        &[1],
        json!({"report_id":1,"values":[],"diagnostics":[{"reason":"truncated"}]}),
        None,
    );
    assert_eq!(h.state()["status"], "stale");
    assert_eq!(h.state()["values"], json!([]));
    h.push(
        3,
        &[0],
        json!({"report_id":1,"unavailable":"unsupported descriptor"}),
        None,
    );
    assert_eq!(h.state()["status"], "unsupported");
    assert_eq!(h.state()["values"], json!([]));
}

#[test]
fn windows_native_data_indices_keep_scalar_quality_and_opaque_raw_separate() {
    let mut h = History::default();
    h.push(1, &[3,255], json!({"provenance":"windows-hidp-report-projection","kind":"input",
        "reportId":3,"data":[{"dataIndex":42,"rawValue":"255","matchingButtonCaps":[],
            "matchingValueCaps":[0],"quality":"os-projected","values":[{"capabilityIndex":0,
            "native":"-1","quality":"valid","semantics":"absolute"}]}],"valueArrays":[],"failures":[]}),None);
    let value = &h.state()["values"][0];
    assert_eq!(value["sample"]["value"], "-1");
    assert_eq!(value["encoded"], "255");
    assert_eq!(value["rawBits"]["state"], "unavailable");
    assert!(value["fieldId"].as_str().unwrap().contains("42"));
}

#[test]
fn mac_elements_keep_independent_cookie_identity_and_never_invent_wire() {
    let mut h = History::default();
    let source = Some(json!({"source":"iohid-element","wireAvailable":false,"reportId":0}));
    h.push(
        1,
        &[],
        json!({"elements":[{"cookie":17,"integer":"0","relative":false,
        "valueHex":"00","logicalMin":"-128","logicalMax":"127","array":false}]}),
        source.clone(),
    );
    h.push(
        2,
        &[],
        json!({"elements":[{"cookie":18,"integer":"-1","relative":true,"valueHex":"ff"}]}),
        source,
    );
    let state = h.state();
    assert_eq!(state["values"].as_array().unwrap().len(), 1);
    assert_eq!(state["values"][0]["sample"]["value"], "0");
    assert_eq!(state["values"][0]["logical"]["minimum"], "-128");
    assert_eq!(state["values"][0]["rawBits"]["state"], "unavailable");
}

#[test]
fn terminal_invalidates_current_authority_but_preserves_the_ordered_prefix() {
    let mut h = History::default();
    h.push(
        1,
        &[0],
        json!({"report_id":0,"values":[integer(0,"Absolute","1")]}),
        None,
    );
    h.end(json!({"kind":"overflow"}));
    assert_eq!(h.state()["status"], "offline");
    assert_eq!(h.state()["values"], json!([]));
    assert_eq!(h.page(0).unwrap()["reports"].as_array().unwrap().len(), 1);
    assert_eq!(h.state()["terminal"]["kind"], "overflow");
}

#[test]
fn a_complete_report_replaces_its_fields_but_preserves_other_report_ids() {
    let mut h = History::default();
    for report in [1, 2] {
        h.push(
            1,
            &[0],
            json!({"report_id":report,"values":[integer(0,"Absolute","1")]}),
            None,
        );
    }
    h.push(
        2,
        &[0],
        json!({"report_id":1,"values":[],"diagnostics":[]}),
        None,
    );
    let state = h.state();
    assert_eq!(state["values"].as_array().unwrap().len(), 1);
    assert_eq!(state["values"][0]["reportId"], 2);
}

#[test]
fn trailing_delivery_diagnostics_preserve_values_the_portable_decoder_validated() {
    let mut h = History::default();
    h.push(1,&[1,2],json!({"report_id":1,"values":[integer(0,"Absolute","1")],
        "diagnostics":[{"reason":"Trailing delivery bytes preserved outside the declared payload"}]}),None);
    assert_eq!(h.state()["values"][0]["sample"]["value"], "1");
    assert_eq!(h.state()["diagnostics"].as_array().unwrap().len(), 1);
}

#[test]
fn revoke_fences_a_report_already_waiting_in_the_native_poll() {
    struct Blocked {
        entered: mpsc::SyncSender<()>,
        resume: mpsc::Receiver<()>,
    }
    impl Reader for Blocked {
        fn poll(&mut self, _: Duration) -> Result<Option<Vec<u8>>, Value> {
            self.entered.send(()).unwrap();
            self.resume.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(Some(vec![1]))
        }
        fn project(&self, _: &[u8]) -> Value {
            json!({"report_id":1,"values":[integer(0,"Absolute","1")]})
        }
        fn get(&mut self, _: &Get, _: Duration) -> Result<Value, Value> {
            unreachable!()
        }
    }
    let (entered, at_poll) = mpsc::sync_channel(1);
    let (resume, paused) = mpsc::channel();
    let mut session = Session::start(move || {
        Ok(Box::new(Blocked {
            entered,
            resume: paused,
        }))
    })
    .unwrap();
    at_poll.recv_timeout(Duration::from_secs(2)).unwrap();
    session.revoke(json!({"kind":"stale-generation"}));
    resume.send(()).unwrap();
    session.worker.take().unwrap().join().unwrap();
    assert_eq!(session.state()["status"], "offline");
    assert_eq!(session.state()["latest"], "0");
    assert_eq!(session.state()["values"], json!([]));
    assert!(session.page(0).unwrap()["reports"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(session
        .submit(
            Get::parse(&json!({"deadlineMs":100,"maxBytes":1,
        "kind":"input","reportId":1,"requestId":"closed"}))
            .unwrap()
        )
        .is_err());
}

#[test]
fn descriptor_bounds_and_optional_exact_physical_values_remain_distinct() {
    let mut h = History {
        values: values::Current::new(json!({"descriptor":{"fields":[{"globals":{
            "logical_min":"0","logical_max":"255","physical_min":"-2","physical_max":"2"}}]}})),
        ..History::default()
    };
    let mut v = integer(0, "Absolute", "128");
    v["physical"] = json!({"numerator":"2","denominator":"255","unit":17,"exponent":-2});
    h.push(1, &[128], json!({"report_id":1,"values":[v]}), None);
    let row = &h.state()["values"][0];
    assert_eq!(row["sample"]["value"], "128");
    assert_eq!(row["logical"]["maximum"], "255");
    assert_eq!(row["physicalBounds"]["minimum"], "-2");
    assert_eq!(row["physical"]["denominator"], "255");
    h.push(
        2,
        &[0],
        json!({"report_id":1,"values":[integer(0,"Absolute","0")]}),
        None,
    );
    assert!(h.state()["values"][0].get("physical").is_none());
}

#[test]
fn current_value_expansion_exceeding_its_budget_invalidates_the_baseline() {
    let mut h = History::default();
    let mut value = integer(0, "Absolute", "0");
    value["sample"]["Integer"] = json!("1".repeat(8 * 1024 * 1024));
    h.push(1, &[1], json!({"report_id":1,"values":[value]}), None);
    assert_eq!(h.state()["status"], "stale");
    assert_eq!(h.state()["values"], json!([]));
    assert_eq!(h.state()["diagnostics"][0]["kind"], "capacity");
}

#[test]
fn revoke_fences_an_explicit_read_in_progress() {
    struct PendingGet {
        entered: mpsc::SyncSender<()>,
        resume: mpsc::Receiver<()>,
    }
    impl Reader for PendingGet {
        fn poll(&mut self, _: Duration) -> Result<Option<Vec<u8>>, Value> {
            thread::yield_now();
            Ok(None)
        }
        fn project(&self, _: &[u8]) -> Value {
            unreachable!()
        }
        fn get(&mut self, _: &Get, _: Duration) -> Result<Value, Value> {
            self.entered.send(()).unwrap();
            self.resume.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(json!({"wireHex":"01"}))
        }
    }
    let (entered, at_get) = mpsc::sync_channel(1);
    let (resume, paused) = mpsc::channel();
    let mut session = Session::start(move || {
        Ok(Box::new(PendingGet {
            entered,
            resume: paused,
        }))
    })
    .unwrap();
    let reply = session
        .submit(
            Get::parse(&json!({"deadlineMs":2000,"maxBytes":1,
        "kind":"input","reportId":1,"requestId":"old"}))
            .unwrap(),
        )
        .unwrap();
    at_get.recv_timeout(Duration::from_secs(2)).unwrap();
    session.revoke(json!({"kind":"stale-generation"}));
    resume.send(()).unwrap();
    assert_eq!(
        reply.wait("i", "g").unwrap()["result"]["error"]["kind"],
        "offline"
    );
    session.worker.take().unwrap().join().unwrap();
}

#[test]
fn element_without_declared_relative_flag_has_no_inferred_absolute_semantics() {
    let mut h = History::default();
    h.push(
        1,
        &[],
        json!({"elements":[{"cookie":1,"integer":"1","valueHex":"01"}]}),
        Some(json!({"source":"iohid-element","reportId":0,"wireAvailable":false})),
    );
    assert_eq!(h.state()["values"], json!([]));
}

#[test]
fn recording_precedes_display_eviction_and_projection_truncation() {
    let mut store = super::super::recording::Store::default();
    let req = json!({"interfaceId":"a","generation":"g"});
    let recording = store
        .start(&req, json!({"descriptorHex":"01"}), 0, 0)
        .unwrap();
    let mut history = History {
        recording: Some(Arc::downgrade(&recording)),
        ..History::default()
    };
    for micros in 0..4200 {
        history.push(micros, &[0, 1], json!({"delta":1}), None);
    }
    let exact = "s".repeat(513 * 1024);
    history.push(4201, &[0, 2], json!({"opaque":exact}), None);
    assert_eq!(history.page(0).unwrap()["gap"], true);
    history.end(json!({"kind":"removed"}));
    let archive = store.request("controller_hid_record_export", &req).unwrap();
    let rows = archive["result"]["recording"]["records"]
        .as_array()
        .unwrap();
    assert_eq!(rows.len(), 4202);
    assert_eq!(rows[0]["sequence"], "1");
    assert_eq!(rows[4200]["samples"]["opaque"], exact);
    assert_eq!(rows[4201]["kind"], "terminal");
    assert_eq!(rows[4201]["reason"]["kind"], "removed");
}

#[test]
fn an_oversized_recording_row_keeps_native_wire_history_and_an_importable_prefix() {
    let mut store = super::super::recording::Store::default();
    let req = json!({"interfaceId":"a","generation":"g"});
    let recording = store
        .start(&req, json!({"descriptorHex":"01"}), 0, 0)
        .unwrap();
    let mut history = History {
        recording: Some(Arc::downgrade(&recording)),
        ..History::default()
    };
    history.push(1, &[0, 1], json!({"value":1}), None);
    history.push(2, &[0, 2], json!({"opaque":"x".repeat(2*1024*1024)}), None);
    let native = history.page(1).unwrap();
    assert!(
        native["reports"][0]["reportSlice"]["totalBytes"]
            .as_u64()
            .unwrap()
            > 2 * 1024 * 1024
    );
    let exact: Value = serde_json::from_str(&history.entries.back().unwrap().json).unwrap();
    assert_eq!(exact["wireHex"], "0002");
    assert_eq!(exact["samples"]["opaque"], "x".repeat(2 * 1024 * 1024));
    assert!(history.terminal.is_none());
    let export = store.request("controller_hid_record_export", &req).unwrap();
    assert_eq!(
        export["result"]["recording"]["records"][0]["wireHex"],
        "0001"
    );
    assert_eq!(
        export["result"]["recording"]["reason"]["kind"],
        "oversized-row"
    );
    assert!(crate::hid::replay::Store::default()
        .request("controller_hid_replay_import", &json!({"recording":export}))
        .is_ok());
}
