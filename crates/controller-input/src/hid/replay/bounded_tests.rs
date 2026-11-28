use super::*;
#[test]
fn bounded_replay_preserves_large_rows_in_archive_slices_and_exact_state_at_cursor() {
    let mut recording = super::tests::recording();
    let exact = "9".repeat(1_100_000);
    recording["records"][0]["samples"] = json!({"report_id":0,"elements":[{
        "cookie":1,"relative":false,"integer":exact}]});
    let recording = super::tests::seal(recording);
    let mut store = Store::default();
    let imported = store
        .request(
            "controller_hid_replay_import",
            &json!({"recording":recording}),
        )
        .unwrap();
    let id = &imported["replayId"];
    let bounded = store
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":id,"limit":1,"bounded":true}),
        )
        .unwrap();
    assert!(bounded.to_string().len() < 1024 * 1024);
    assert_eq!(bounded["next"], "1");
    assert_eq!(bounded["state"]["stateSlice"], true);
    let reference = &bounded["records"][0]["recordSlice"];
    let begin = reference["byteOffset"].as_u64().unwrap();
    let total = reference["totalBytes"].as_u64().unwrap();
    let mut row = Vec::new();
    let mut offset = begin;
    while offset < begin + total {
        let slice = store
            .request(
                "controller_hid_replay_read",
                &json!({"replayId":id,"part":"recording",
            "byteOffset":offset,"maxBytes":(begin+total-offset).min(24576)}),
            )
            .unwrap();
        row.extend(transfer::decode(slice["chunk"].as_str().unwrap()).unwrap());
        offset = slice["nextByteOffset"].as_u64().unwrap();
    }
    let restored: Value = serde_json::from_slice(&row).unwrap();
    assert_eq!(restored["samples"]["elements"][0]["integer"], exact);
    let state = store
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":id,"part":"state","after":"1"}),
        )
        .unwrap();
    assert_eq!(state["state"]["latest"], "1");
    assert_eq!(state["state"]["values"][0]["sample"]["value"], exact);
    let ui = store
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":id,"limit":1}),
        )
        .unwrap();
    assert_eq!(ui["records"][0]["samples"]["elements"][0]["integer"], exact);
    let descriptor = store
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":id,"part":"descriptor"}),
        )
        .unwrap();
    assert_eq!(descriptor["descriptor"], imported["descriptor"]);
}
#[test]
fn bounded_replay_rejects_nonboolean_projection_flag() {
    let mut store = Store::default();
    let imported = store
        .request(
            "controller_hid_replay_import",
            &json!({"recording":super::tests::recording()}),
        )
        .unwrap();
    assert!(store
        .request(
            "controller_hid_replay_read",
            &json!({"replayId":imported["replayId"],"bounded":"true"})
        )
        .is_err());
}
