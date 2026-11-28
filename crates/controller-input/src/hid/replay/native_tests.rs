use super::*;

#[test]
fn real_native_recording_export_import_preserves_capacity_loss_and_generation() {
    let mut recordings = super::super::recording::Store::default();
    let req = json!({"interfaceId":"fixture","generation":"source-generation","maxRecords":4});
    let handle=recordings.start(&req,json!({"interfaceId":"fixture","contractFingerprint":"native-fingerprint","descriptor":{"fields":[]}}),0,100).unwrap();
    for seq in 1..=3 {
        let row = json!({"sequence":seq.to_string(),"receivedUs":(100+seq).to_string(),"samples":{"values":[]},"wireHex":"00ff","provenance":{"wireAvailable":true,"source":"native-fixture"}});
        handle.observe(seq, u128::from(100 + seq), &row.to_string());
    }
    let export = recordings
        .request("controller_hid_record_export", &req)
        .unwrap();
    assert_eq!(export["result"]["recording"]["reason"]["kind"], "capacity");
    let mut replays = Store::default();
    let imported = replays
        .request("controller_hid_replay_import", &json!({"recording":export}))
        .unwrap();
    assert_eq!(imported["metadata"]["generation"], "source-generation");
    assert_eq!(
        imported["descriptor"]["contractFingerprint"],
        "native-fingerprint"
    );
    let page = replays
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":imported["replayId"]}),
        )
        .unwrap();
    assert_eq!(page["records"].as_array().unwrap().len(), 4);
    assert_eq!(page["records"][2]["kind"], "gap");
    assert_eq!(page["records"][3]["sequence"], "3");
}

#[test]
fn native_unknown_overflow_extent_replays_without_inventing_packets() {
    let mut recordings = super::super::recording::Store::default();
    let req = json!({"interfaceId":"fixture","generation":"generation"});
    let handle = recordings.start(&req, json!({}), 0, 100).unwrap();
    handle.end(json!({"kind":"overflow"}), 0, 110, true);
    let export = recordings
        .request("controller_hid_record_export", &req)
        .unwrap();
    let mut replays = Store::default();
    let imported = replays
        .request(
            "controller_hid_replay_import",
            &json!({"recording":export["result"].to_string()}),
        )
        .unwrap();
    let page = replays
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":imported["replayId"]}),
        )
        .unwrap();
    assert!(page["records"][0]["toSequence"].is_null());
    assert_eq!(page["state"]["values"], json!([]));
    assert_eq!(page["state"]["terminal"]["kind"], "overflow");
}

#[test]
fn seek_past_page_boundary_rebuilds_all_same_time_observations() {
    let mut recordings = super::super::recording::Store::default();
    let req = json!({"interfaceId":"fixture","generation":"generation"});
    let handle = recordings.start(&req, json!({}), 0, 100).unwrap();
    for seq in 1..=300u64 {
        let at = if seq == 251 || seq == 252 {
            350
        } else {
            100 + seq
        };
        let row = json!({"sequence":seq.to_string(),"receivedUs":at.to_string(),"samples":{"report_id":1,"elements":[{"cookie":seq%2,"relative":false,"integer":seq.to_string()}]}});
        handle.observe(seq, u128::from(at), &row.to_string());
    }
    handle.end(json!({"kind":"stopped"}), 300, 500, false);
    let export = recordings
        .request("controller_hid_record_export", &req)
        .unwrap();
    let mut replays = Store::default();
    let imported = replays
        .request(
            "controller_hid_replay_import",
            &json!({"recording":export["result"].to_string()}),
        )
        .unwrap();
    let read =
        |time| json!({"replayId":imported["replayId"],"after":"0","elapsedUs":time,"seek":true});
    let page = replays
        .request("controller_hid_replay_read", &read(250))
        .unwrap();
    assert_eq!(page["next"], "252");
    assert_eq!(page["hasMore"], false);
    assert_eq!(page["complete"], false);
    assert_eq!(page["records"].as_array().unwrap().len(), 128);
    assert_eq!(page["records"][0]["sequence"], "125");
    assert_eq!(page["state"]["latest"], "252");
    let values = page["state"]["values"].as_array().unwrap();
    assert_eq!(values.len(), 2);
    assert!(values.iter().any(|v| v["sample"]["value"] == "251"));
    assert!(values.iter().any(|v| v["sample"]["value"] == "252"));
    let rewind = replays
        .request("controller_hid_replay_read", &read(1))
        .unwrap();
    assert_eq!(rewind["next"], "1");
    assert_eq!(rewind["state"]["values"].as_array().unwrap().len(), 1);
    for mut invalid in [
        json!({"seek":true}),
        json!({"seek":"true"}),
        json!({"seek":true,"elapsedUs":1,"after":"1"}),
    ] {
        invalid["replayId"] = imported["replayId"].clone();
        assert!(replays
            .request("controller_hid_replay_read", &invalid)
            .is_err());
    }
}

#[test]
fn seek_returns_state_at_target_even_when_trailing_page_hits_byte_budget() {
    let mut archive = super::tests::recording();
    let mut rows = Vec::new();
    for seq in 1..=200 {
        rows.push(json!({"sequence":seq.to_string(),"receivedUs":(100+seq).to_string(),"samples":{"values":[],"opaque":"a".repeat(12000)}}));
    }
    rows.push(
        json!({"kind":"terminal","sequence":"200","receivedUs":"301","reason":{"kind":"stopped"}}),
    );
    archive["records"] = json!(rows);
    archive["lastSequence"] = json!("200");
    let mut store = Store::default();
    let imported = store
        .request(
            "controller_hid_replay_import",
            &json!({"recording":super::tests::seal(archive).to_string()}),
        )
        .unwrap();
    let page = store
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":imported["replayId"],"seek":true,"elapsedUs":200}),
        )
        .unwrap();
    assert_eq!(page["next"], "200");
    assert_eq!(page["hasMore"], false);
    assert_eq!(page["state"]["latest"], "200");
    assert!(page["records"].as_array().unwrap().len() < 128);
    assert!(page["records"].to_string().len() < 900 * 1024);
}
