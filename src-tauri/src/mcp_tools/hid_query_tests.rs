use super::*;

fn value(id: &str, n: &str) -> Value {
    json!({"fieldId":id,"elementIndex":0,"semantics":"absolute","sample":{"state":"valid","kind":"integer","value":n},"logical":{"minimum":"0","maximum":"18446744073709551615"},"reportId":1,"sequence":"1"})
}
fn descriptor() -> Value {
    json!({"contractFingerprint":"same","detail":{"access":"receive-only","descriptor":{"fields":[
        {"collection":2,"kind":"input","globals":{"report_id":1,"logical_min":"0","logical_max":"18446744073709551615"},"primary":{"usages":[{"page":9,"id":1}]}},
        {"collection":3,"kind":"feature","globals":{"report_id":2},"primary":{"usages":[{"page":1,"id":57}]}}
    ]}}})
}
#[derive(Default)]
struct QueryBackend(Mutex<Vec<Value>>);
impl Backend for QueryBackend {
    fn request(&self, request: Value) -> Value {
        self.0.lock().unwrap().push(request.clone());
        let result = match request["command"].as_str().unwrap() {
            "controller_hid_inspect" | "controller_hid_snapshot" => descriptor(),
            "controller_hid_state" => {
                json!({"contractFingerprint":"same","result":{"status":"live","latest":"1","values":[value("descriptor:0","1"),value("descriptor:1","0")],"diagnostics":["exact diagnostic"]}})
            }
            "controller_hid_reports" => json!({"result":{"gap":true,"terminal":{"reason":"fixture"},"reports":[
                {"reportId":1,"sequence":"1","samples":{"values":[{"field":0,"element":0,"semantics":"Relative","sample":{"Integer":"-1"}}]}},
                {"kind":"gap","fromSequence":"2","reason":{"kind":"loss"}},
                {"kind":"terminal","sequence":"2","reason":{"kind":"fixture"}}
            ]}}),
            "controller_hid_replay_read" if request["part"] == "descriptor" => json!({"descriptor":descriptor()}),
            "controller_hid_replay_read" => {
                json!({"next":"3","hasMore":false,"state":{"status":"live","values":[value("descriptor:0","1")]},"records":[]})
            }
            _ => panic!("Unexpected request: {request}"),
        };
        json!({"ok":true,"state":{"controllers":result}})
    }
    fn guarded(&self, _: Value, _: Value) -> Value {
        panic!("Queries never acquire output ownership")
    }
}
fn tools(backend: Arc<QueryBackend>) -> Tools {
    Tools {
        backend,
        ledger: Mutex::new(Ledger::default()),
        drafts: Mutex::new(vec![]),
        workspace_uploads: Mutex::new(vec![]),
        integration: OnceLock::new(),
    }
}
#[test]
fn descriptor_search_is_bounded_searches_native_values_and_pages_without_skipping() {
    let backend = Arc::new(QueryBackend::default());
    let tools = tools(backend.clone());
    let first = tools
        .hid_query_tool(
            "controller_hid_descriptor_query",
            &json!({"source":"snapshot","interfaceId":"selected","search":"report_id","limit":1}),
            "reader",
        )
        .unwrap();
    assert_eq!(first["nodes"]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(first["nodes"]["total"], 2);
    assert_eq!(first["nodes"]["nextOffset"], 1);
    let second=tools.hid_query_tool("controller_hid_descriptor_query",&json!({"source":"snapshot","interfaceId":"selected","search":"report_id","limit":1,"offset":1,"queryFingerprint":first["queryFingerprint"]}),"reader").unwrap();
    assert_eq!(second["nodes"]["entries"][0]["value"], "2");
    assert!(second["nodes"]["nextOffset"].is_null());
    assert!(backend
        .0
        .lock()
        .unwrap()
        .iter()
        .all(|r| r["command"] == "controller_hid_snapshot"));
    let missing = tools
        .hid_query_tool(
            "controller_hid_descriptor_query",
            &json!({"source":"snapshot","interfaceId":"selected","offset":1}),
            "reader",
        )
        .unwrap_err();
    assert_eq!(missing["error"]["code"], "invalid_arguments");
}
#[test]
fn monitor_matches_ui_filters_keeps_native_evidence_and_never_opens_capture() {
    let backend = Arc::new(QueryBackend::default());
    let tools = tools(backend.clone());
    let mut args = json!({"source":"live","interfaceId":"selected","generation":"generation","kind":"input","report":"1","collection":"2","usage":"descriptor:0","fieldId":"descriptor:0","elementIndex":0,"active":true,"changed":true,"previous":[]});
    let reply = tools
        .hid_query_tool("controller_hid_monitor_query", &args, "reader")
        .unwrap();
    assert_eq!(reply["values"]["entries"][0]["presentation"]["kind"], "Button");
    assert_eq!(reply["values"]["entries"].as_array().unwrap().len(), 2);
    assert_eq!(reply["reports"]["total"], 3);
    assert_eq!(reply["evidence"]["gap"], true);
    assert_eq!(reply["state"]["diagnostics"], json!(["exact diagnostic"]));
    args["previous"] = json!([value("descriptor:0", "1")]);
    let changed = tools
        .hid_query_tool("controller_hid_monitor_query", &args, "reader")
        .unwrap();
    assert_eq!(changed["values"]["entries"].as_array().unwrap().len(), 1);
    args["report"] = json!("2");
    let filtered = tools
        .hid_query_tool("controller_hid_monitor_query", &args, "reader")
        .unwrap();
    assert_eq!(filtered["values"]["total"], 0);
    assert_eq!(filtered["reports"]["total"], 2);
    assert!(backend.0.lock().unwrap().iter().all(|r| matches!(
        r["command"].as_str(),
        Some("controller_hid_inspect" | "controller_hid_state" | "controller_hid_reports")
    )));
}
#[test]
fn replay_uses_only_authenticated_native_archive_sources_and_validates_clock() {
    let backend = Arc::new(QueryBackend::default());
    let tools = tools(backend.clone());
    let args = json!({"source":"replay","replayId":"archive","after":"0","elapsedUs":"3000","seek":true});
    let reply = tools
        .hid_query_tool("controller_hid_monitor_query", &args, "reader")
        .unwrap();
    assert_eq!(reply["sourceNext"], "3");
    assert_eq!(reply["values"]["total"], 1);
    let requests = backend.0.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["part"], "descriptor");
    assert_eq!(requests[1]["elapsedUs"], "3000");
    assert_eq!(requests[1]["seek"], true);
    assert_eq!(requests[0]["clientId"], native_client("reader"));
    drop(requests);
    for bad in [
        json!({"source":"replay","replayId":"archive","after":"01"}),
        json!({"source":"replay","replayId":"archive","after":"18446744073709551616"}),
        json!({"source":"replay","replayId":"archive","seek":true}),
        json!({"source":"replay","replayId":"archive","interfaceId":"other"}),
        json!({"source":"live","interfaceId":"selected","generation":"first","changed":true}),
    ] {
        assert_eq!(
            tools
                .hid_query_tool("controller_hid_monitor_query", &bad, "reader")
                .unwrap_err()["error"]["code"],
            "invalid_arguments"
        );
    }
    assert_eq!(backend.0.lock().unwrap().len(), 2);
}
#[test]
fn sample_presentations_preserve_wide_integers_null_relative_hats_and_button_activity() {
    let mut row = value("descriptor:0", "18446744073709551615");
    assert_eq!(model::presentation(&row, None)["percent"], 100.0);
    row["sample"]["value"] = json!("32768");
    row["logical"]["maximum"] = json!("65535");
    assert_eq!(model::presentation(&row, None)["percent"], 50.0);
    row["semantics"] = json!("relative");
    assert_eq!(model::presentation(&row, None)["kind"], "Relative delta");
    row["semantics"] = json!("absolute");
    row["usages"] = json!([{"page":1,"id":57}]);
    assert_eq!(model::presentation(&row, None)["kind"], "Hat");
    row["usages"] = Value::Null;
    row["usagePage"] = json!(9);
    row["sample"]["value"] = json!("0");
    assert_eq!(model::presentation(&row, None)["active"], false);
    row["sample"]["value"] = json!("1");
    assert_eq!(model::presentation(&row, None)["active"], true);
    row["sample"] = json!({"Null":"0"});
    let null = model::presentation(&row, None);
    assert_eq!(null["quality"], "null");
    assert!(null["percent"].is_null());
    assert_eq!(null["text"], "0");
}
#[test]
fn byte_paging_retains_every_match_and_explicitly_identifies_oversized_entries() {
    let entries = (0..4)
        .map(|n| json!({"sequence":n.to_string(),"wireHex":"a".repeat(90_000)}))
        .collect::<Vec<_>>();
    let first = page(&entries, 0, 128);
    assert_eq!(first["entries"].as_array().unwrap().len(), 2);
    assert_eq!(first["nextOffset"], 2);
    let second = page(&entries, 2, 128);
    assert_eq!(second["entries"][0]["sequence"], "2");
    assert!(second["nextOffset"].is_null());
    let huge = page(
        &[json!({"value":{"fieldId":"descriptor:1","elementIndex":2,"sample":{"bytes":"f".repeat(PAGE_BYTES+1)}}})],
        0,
        1,
    );
    assert_eq!(huge["entries"][0]["unavailable"], "query_entry_exceeds_byte_budget");
    assert_eq!(huge["entries"][0]["fieldId"], "descriptor:1");
    assert_eq!(
        fence(&json!({"valueOffset":1,"queryFingerprint":"old"}), "new").unwrap_err()["error"]["code"],
        "query_changed"
    );
}
#[test]
fn field_mapping_and_received_projection_preserve_all_native_backends() {
    let detail = json!({"capabilities":{"reports":[{"kind":"input","valueCaps":[{"reportId":1,"linkCollection":2,"usagePage":1,"usage":48}],"buttonCaps":[{"reportId":1,"linkCollection":3}]}]},"elements":[{"cookie":22,"parentCookie":1,"reportType":"input","usagePage":9,"usage":1}]});
    let fields = model::fields(&detail);
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[2]["id"], "iohid-cookie:22");
    let values = model::report_values(&[
        json!({"windowsReportId":1,"sequence":"9","samples":{"data":[{"dataIndex":3,"rawValue":"4294967295","values":[{"capabilityIndex":0,"semantics":"relative","quality":"valid","native":"-1"}]}]}}),
        json!({"sequence":"10","samples":{"elements":[{"cookie":22,"relative":true,"integer":"-7","usagePage":9,"usage":1}]}}),
    ]);
    assert_eq!(values[0]["fieldId"], "hidp-value:0:data:3");
    assert_eq!(values[0]["reportId"], 1);
    assert_eq!(
        model::presentation(&values[0], model::find_field(&fields, &values[0]))["text"],
        "-1"
    );
    assert_eq!(values[1]["fieldId"], "iohid-cookie:22");
    assert_eq!(
        model::presentation(&values[1], model::find_field(&fields, &values[1]))["text"],
        "-7"
    );
}
#[test]
fn traversal_keeps_only_its_page_and_bounds_expansion_of_large_imported_keys() {
    let source = json!((0..10_000).collect::<Vec<_>>());
    let mut tree = Tree {
        query: "",
        nodes: vec![],
        visited: 0,
        limited: false,
        offset: 2345,
        limit: 17,
        matched: 0,
        bytes: 0,
        page_full: false,
    };
    tree.walk(&source, "interface".into(), 0);
    assert_eq!(tree.matched, 10001);
    assert_eq!(tree.nodes.len(), 17);
    assert_eq!(tree.nodes[0]["path"], "interface.2344");
    assert!(tree.bytes <= PAGE_BYTES);
    let source = json!({"x".repeat(1024*1024):[1,2,3]});
    let mut tree = Tree {
        query: "",
        nodes: vec![],
        visited: 0,
        limited: false,
        offset: 0,
        limit: 128,
        matched: 0,
        bytes: 0,
        page_full: false,
    };
    tree.walk(&source, "interface".into(), 0);
    assert!(tree.limited);
    assert_eq!(tree.visited, 2);
    assert_eq!(tree.nodes.len(), 2);
    assert_eq!(tree.nodes[1]["pathTruncated"], true);
    assert!(tree.nodes[1]["path"].as_str().unwrap().len() <= 4096);
}
#[test]
fn monitor_shares_the_entry_quota_and_both_end_offsets_progress_without_loss() {
    let tools = tools(Arc::new(QueryBackend::default()));
    let mut args = json!({"source":"live","interfaceId":"selected","generation":"first","limit":1});
    let mut counts = (0, 0);
    let mut positions = (0, 0);
    loop {
        let response = tools
            .hid_query_tool("controller_hid_monitor_query", &args, "reader")
            .unwrap();
        let values = model::rows(&response["values"]["entries"]);
        let reports = model::rows(&response["reports"]["entries"]);
        assert!(values.len() + reports.len() <= 1);
        counts.0 += values.len();
        counts.1 += reports.len();
        let next = (
            response["values"]["endOffset"].as_u64().unwrap(),
            response["reports"]["endOffset"].as_u64().unwrap(),
        );
        assert_ne!(positions, next);
        positions = next;
        args["valueOffset"] = json!(next.0);
        args["reportOffset"] = json!(next.1);
        args["queryFingerprint"] = response["queryFingerprint"].clone();
        if response["values"]["nextOffset"].is_null() && response["reports"]["nextOffset"].is_null() {
            break;
        }
    }
    assert_eq!(counts, (3, 3));
    args["valueOffset"] = json!(999);
    assert_eq!(
        tools
            .hid_query_tool("controller_hid_monitor_query", &args, "reader")
            .unwrap_err()["error"]["code"],
        "invalid_arguments"
    );
    let first = tools
        .hid_query_tool(
            "controller_hid_descriptor_query",
            &json!({"source":"snapshot","interfaceId":"selected"}),
            "reader",
        )
        .unwrap();
    let invalid=tools.hid_query_tool("controller_hid_descriptor_query",&json!({"source":"snapshot","interfaceId":"selected","offset":50000,"queryFingerprint":first["queryFingerprint"]}),"reader").unwrap_err();
    assert_eq!(invalid["error"]["code"], "invalid_arguments");
}
