use super::*;

pub(super) fn recording() -> Value {
    seal(
        json!({"format":"osg-hid-recording","descriptor":{"interfaceId":"fixture","descriptor":{"fields":[]}},
        "interfaceId":"fixture","generation":"old-generation","startedUnixMs":"1000",
        "startedReceivedUs":"100","startingSequence":"0","lastSequence":"3",
        "status":"stopped","reason":{"kind":"stopped"},"recordCount":3,"bytes":999,
        "records":[
            {"sequence":"1","receivedUs":"110","wireHex":"01","samples":{"values":[]}},
            {"sequence":"2","receivedUs":"120","wireHex":"01","samples":{"values":[]}},
            {"kind":"gap","fromSequence":"3","toSequence":"3","receivedUs":"125","reason":{"kind":"loss"}},
            {"kind":"terminal","sequence":"3","receivedUs":"130","reason":{"kind":"stopped"}}
            ]}),
    )
}
pub(super) fn seal(mut archive: Value) -> Value {
    archive["recordCount"] = json!(archive["records"].as_array().unwrap().len());
    for _ in 0..8 {
        let bytes = archive.to_string().len();
        archive["bytes"] = json!(bytes);
    }
    archive
}
fn import(store: &mut Store) -> Value {
    store
        .request(
            "controller_hid_replay_import",
            &json!({"recording":recording()}),
        )
        .unwrap()
}

#[test]
fn replay_clock_and_entry_cursor_preserve_repeated_reports_and_markers() {
    let mut store = Store::default();
    let imported = import(&mut store);
    assert_eq!(imported["metadata"]["durationUs"], "30");
    assert_eq!(imported["descriptor"]["interfaceId"], "fixture");
    let id = &imported["replayId"];
    let first = store
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":id,"after":"0","elapsedUs":"20"}),
        )
        .unwrap();
    assert_eq!(first["records"].as_array().unwrap().len(), 2);
    assert_eq!(first["next"], "2");
    assert_eq!(first["records"][1]["sequence"], "2");
    assert_eq!(first["state"]["latest"], "2");
    let last = store
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":id,"after":"2"}),
        )
        .unwrap();
    assert_eq!(last["records"][0]["kind"], "gap");
    assert_eq!(last["records"][1]["kind"], "terminal");
    assert_eq!(last["state"]["status"], "offline");
    assert_eq!(last["complete"], true);
    let rewind = store
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":id,"after":"0","limit":1}),
        )
        .unwrap();
    assert_eq!(rewind["next"], "1");
    assert_eq!(rewind["state"]["latest"], "1");
    assert!(store
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":id,"after":"99"})
        )
        .is_err());
    store
        .request("controller_hid_replay_close", &json!({"replayId":id}))
        .unwrap();
    assert!(store
        .request("controller_hid_replay_read", &json!({"replayId":id}))
        .is_err());
}

#[test]
fn malformed_structure_and_order_are_rejected_without_retaining_archives() {
    let mut store = Store::default();
    let mut bad = recording();
    bad["records"][0]["wireHex"] = json!("xz");
    assert!(store
        .request("controller_hid_replay_import", &json!({"recording":bad}))
        .is_err());
    let mut bad = recording();
    bad["records"][1]["receivedUs"] = json!("109");
    assert!(store
        .request("controller_hid_replay_import", &json!({"recording":bad}))
        .is_err());
    let mut bad = recording();
    bad["records"][1]["sequence"] = json!("1");
    assert!(store
        .request("controller_hid_replay_import", &json!({"recording":bad}))
        .is_err());
    let deeply_nested = format!("{}0{}", "[".repeat(100), "]".repeat(100));
    assert!(store
        .request(
            "controller_hid_replay_import",
            &json!({"recording":deeply_nested})
        )
        .unwrap_err()
        .contains("structure"));
    assert!(store
        .request("controller_hid_replay_import", &json!({"recording":"{}"}))
        .is_err());
    for _ in 0..4 {
        import(&mut store);
    }
    assert!(store
        .request(
            "controller_hid_replay_import",
            &json!({"recording":recording()})
        )
        .is_err());
    store.clear();
    import(&mut store);
}

#[test]
fn chunk_upload_is_owned_ordered_and_round_trips_every_archive_field() {
    let mut store = Store::default();
    let text = recording().to_string();
    let started = store
        .request(
            "controller_hid_replay_upload_start",
            &json!({"clientId":"a","totalBytes":text.len()}),
        )
        .unwrap();
    let id = &started["uploadId"];
    let chunk = json!({"clientId":"b","uploadId":id,"offset":0,"chunk":"e30="});
    assert!(store
        .request("controller_hid_replay_upload_chunk", &chunk)
        .is_err());
    assert!(store
        .request(
            "controller_hid_replay_upload_finish",
            &json!({"clientId":"a","uploadId":id})
        )
        .is_err());
    let mut encoded = String::new();
    for (offset, bytes) in text.as_bytes().chunks(19).enumerate() {
        encoded.clear();
        encoded.push_str(&super::transfer::encode(bytes));
        store
            .request(
                "controller_hid_replay_upload_chunk",
                &json!({"clientId":"a","uploadId":id,"offset":offset*19,"chunk":encoded}),
            )
            .unwrap();
    }
    let imported = store
        .request(
            "controller_hid_replay_upload_finish",
            &json!({"clientId":"a","uploadId":id}),
        )
        .unwrap();
    assert!(imported.get("descriptor").is_none());
    let replay = &imported["replayId"];
    assert!(store
        .request(
            "controller_hid_replay_read",
            &json!({"clientId":"b","replayId":replay})
        )
        .is_err());
    let mut output = Vec::new();
    loop {
        let page = store.request("controller_hid_replay_read", &json!({"clientId":"a","replayId":replay,"part":"recording","byteOffset":output.len(),"maxBytes":17})).unwrap();
        output.extend(super::transfer::decode(page["chunk"].as_str().unwrap()).unwrap());
        if page["complete"] == true {
            break;
        }
    }
    assert_eq!(
        serde_json::from_slice::<Value>(&output).unwrap(),
        recording()
    );
    assert!(store
        .request(
            "controller_hid_replay_upload_finish",
            &json!({"clientId":"a","uploadId":id})
        )
        .is_err());
}

#[test]
fn upload_retries_must_match_previously_accepted_bytes_and_offsets() {
    let mut store = Store::default();
    let begin = store
        .request(
            "controller_hid_replay_upload_start",
            &json!({"totalBytes":5}),
        )
        .unwrap();
    let id = &begin["uploadId"];
    let req = json!({"uploadId":id,"offset":0,"chunk":"aGVs"}); // hel
    assert_eq!(
        store
            .request("controller_hid_replay_upload_chunk", &req)
            .unwrap()["nextOffset"],
        3
    );
    assert_eq!(
        store
            .request("controller_hid_replay_upload_chunk", &req)
            .unwrap()["nextOffset"],
        3
    );
    for req in [
        json!({"uploadId":id,"offset":0,"chunk":"aGFs"}),
        json!({"uploadId":id,"offset":4,"chunk":"bw=="}),
        json!({"uploadId":id,"offset":3,"chunk":"bG9uZw=="}),
    ] {
        assert!(store
            .request("controller_hid_replay_upload_chunk", &req)
            .is_err());
    }
    store
        .request(
            "controller_hid_replay_upload_chunk",
            &json!({"uploadId":id,"offset":3,"chunk":"bG8="}),
        )
        .unwrap();
    assert!(store
        .request(
            "controller_hid_replay_upload_finish",
            &json!({"uploadId":id})
        )
        .is_err());
    assert!(store
        .request("controller_hid_replay_upload_chunk", &req)
        .is_err());
    let begin = store
        .request(
            "controller_hid_replay_upload_start",
            &json!({"totalBytes":1}),
        )
        .unwrap();
    store
        .request(
            "controller_hid_replay_upload_cancel",
            &json!({"uploadId":begin["uploadId"]}),
        )
        .unwrap();
    assert!(store
        .request(
            "controller_hid_replay_upload_cancel",
            &json!({"uploadId":begin["uploadId"]})
        )
        .is_err());
    assert!(store
        .request(
            "controller_hid_replay_upload_start",
            &json!({"totalBytes":0})
        )
        .is_err());
    assert!(store
        .request(
            "controller_hid_replay_upload_start",
            &json!({"totalBytes":64*1024*1024+1})
        )
        .is_err());
}

#[test]
fn binary_transport_checks_padding_and_preserves_unicode_and_all_byte_values() {
    for (raw, encoded) in [
        (b"h".as_slice(), "aA=="),
        (b"he".as_slice(), "aGU="),
        (b"hel".as_slice(), "aGVs"),
        ("λ".as_bytes(), "zrs="),
    ] {
        assert_eq!(transfer::encode(raw), encoded);
        assert_eq!(transfer::decode(encoded).unwrap(), raw);
    }
    for invalid in [
        "", "x", "====", "a===", "aG=V", "aA=A", "aB==", "aGV=", "aA==aA==", "????",
    ] {
        assert!(transfer::decode(invalid).is_err(), "{invalid}");
    }
    let all = (0..=255).collect::<Vec<u8>>();
    assert_eq!(transfer::decode(&transfer::encode(&all)).unwrap(), all);
}

#[test]
fn replay_state_rebuilds_exact_absolute_values_and_never_integrates_relative_deltas() {
    let mut recording = recording();
    recording["records"][0]["samples"] = json!({"report_id":1,"elements":[{"cookie":1,"relative":false,"integer":"18446744073709551615"},{"cookie":2,"relative":true,"integer":"-1"}]});
    recording["records"][1]["samples"] =
        json!({"report_id":1,"elements":[{"cookie":1,"relative":false,"integer":"0"}]});
    let mut store = Store::default();
    let imported = store
        .request(
            "controller_hid_replay_import",
            &json!({"recording":seal(recording).to_string()}),
        )
        .unwrap();
    let read = |after, limit| json!({"replayId":imported["replayId"],"after":after,"limit":limit});
    let first = store
        .request("controller_hid_replay_read", &read("0", 1))
        .unwrap();
    assert_eq!(first["state"]["values"].as_array().unwrap().len(), 1);
    assert_eq!(
        first["state"]["values"][0]["sample"]["value"],
        "18446744073709551615"
    );
    let second = store
        .request("controller_hid_replay_read", &read("1", 1))
        .unwrap();
    assert_eq!(second["state"]["values"][0]["sample"]["value"], "0");
    let rewind = store
        .request("controller_hid_replay_read", &read("0", 1))
        .unwrap();
    assert_eq!(
        rewind["state"]["values"][0]["sample"]["value"],
        "18446744073709551615"
    );
    let gap = store
        .request("controller_hid_replay_read", &read("2", 1))
        .unwrap();
    assert_eq!(gap["state"]["values"], json!([]));
}

#[test]
fn read_validation_byte_boundaries_and_global_staging_budget_are_enforced() {
    let mut store = Store::default();
    let imported = import(&mut store);
    for req in [
        json!({"after":"-1"}),
        json!({"after":1}),
        json!({"elapsedUs":-1}),
        json!({"limit":0}),
        json!({"limit":129}),
        json!({"part":"unsupported"}),
        json!({"part":"recording","byteOffset":999999}),
        json!({"part":"recording","maxBytes":0}),
        json!({"part":"recording","maxBytes":24577}),
    ] {
        let mut req = req;
        req["replayId"] = imported["replayId"].clone();
        assert!(store.request("controller_hid_replay_read", &req).is_err());
    }
    let zero = store
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":imported["replayId"],"elapsedUs":0}),
        )
        .unwrap();
    assert_eq!(zero["records"], json!([]));
    assert_eq!(zero["complete"], false);
    store.clear();
    for _ in 0..2 {
        store
            .request(
                "controller_hid_replay_upload_start",
                &json!({"totalBytes":64*1024*1024}),
            )
            .unwrap();
    }
    assert!(store
        .request(
            "controller_hid_replay_upload_start",
            &json!({"totalBytes":1})
        )
        .is_err());
    store.clear();
    for _ in 0..4 {
        store
            .request(
                "controller_hid_replay_upload_start",
                &json!({"totalBytes":1}),
            )
            .unwrap();
    }
    assert!(store
        .request(
            "controller_hid_replay_upload_start",
            &json!({"totalBytes":1})
        )
        .is_err());
}
