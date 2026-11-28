use super::*;

fn request(id: &str) -> Value {
    json!({"interfaceId":id,"generation":"generation"})
}
fn begin(store: &mut Store, req: &Value) -> Handle {
    store
        .start(
            req,
            json!({"descriptorHex":"0001","provenance":{"source":"exact"},"records":[]}),
            0,
            17,
        )
        .unwrap()
}

#[test]
fn bounded_archive_preserves_prefix_and_marks_the_first_lost_sequence() {
    let mut store = Store::default();
    let mut req = request("first");
    req["maxRecords"] = json!(4);
    let handle = begin(&mut store, &req);
    let mut recording = handle.lock().unwrap();
    for sequence in 1..=4 {
        let row = json!({"sequence":sequence.to_string(),"receivedUs":sequence.to_string(),"wireHex":"0001","samples":{"wide":"18446744073709551615"}});
        recording.observe(sequence, sequence as u128, &row.to_string());
        if recording.active() {
            let (head, tail) = recording.make_parts();
            assert_eq!(
                recording.encoded_size(),
                head.len()
                    + tail.len()
                    + recording.row_bytes
                    + recording.records.len().saturating_sub(1)
                    - "{\"recording\":}".len()
            );
        }
    }
    let export = recording.export(&json!({})).unwrap();
    let archive = &export["recording"];
    let rows = archive["records"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["wireHex"], "0001");
    assert_eq!(rows[1]["sequence"], "2");
    assert_eq!(rows[2]["kind"], "gap");
    assert_eq!(rows[2]["fromSequence"], "3");
    assert_eq!(rows[2]["toSequence"], "3");
    assert_eq!(rows[3]["kind"], "terminal");
    assert_eq!(archive["reason"]["kind"], "capacity");
    assert_eq!(archive["descriptor"]["descriptorHex"], "0001");
    assert_eq!(
        archive["bytes"].as_u64().unwrap() as usize,
        archive.to_string().len()
    );
    assert!(archive.to_string().len() <= recording.charged);
}

#[test]
fn byte_limit_accounts_for_metadata_markers_and_oversized_records() {
    let mut store = Store::default();
    let mut req = request("first");
    req["maxBytes"] = json!(16384);
    let handle = begin(&mut store, &req);
    let mut recording = handle.lock().unwrap();
    recording.observe(1, 23, &json!({"samples":"x".repeat(20000)}).to_string());
    let export = recording.export(&json!({})).unwrap();
    assert!(export["recording"].to_string().len() <= 16384);
    assert_eq!(export["recording"]["recordCount"], 2);
    assert_eq!(export["recording"]["records"][0]["kind"], "gap");
    assert!(store
        .start(
            &request("huge"),
            json!({"descriptor":"x".repeat(MAX_BYTES)}),
            0,
            0
        )
        .is_err());
}

#[test]
fn stopped_archive_chunks_are_stable_and_discard_releases_global_memory() {
    let mut store = Store::default();
    let req = request("first");
    let handle = begin(&mut store, &req);
    assert!(store.request("controller_hid_record_export", &req).is_err());
    assert!(store
        .request("controller_hid_record_discard", &req)
        .is_err());
    handle.lock().unwrap().observe(
        1,
        19,
        &json!({"sequence":"1","receivedUs":"19","samples":{"opaque":"λ"}}).to_string(),
    );
    store.request("controller_hid_record_stop", &req).unwrap();
    let all = store.request("controller_hid_record_export", &req).unwrap()["result"].to_string();
    let mut chunk = req.clone();
    chunk["offset"] = json!(7);
    chunk["limit"] = json!(29);
    let exported = store
        .request("controller_hid_record_export", &chunk)
        .unwrap();
    assert_eq!(
        exported["result"]["chunk"],
        export::base64(&all.as_bytes()[7..36])
    );
    assert_eq!(exported["result"]["nextOffset"], 36);
    assert_eq!(exported["result"]["totalBytes"], all.len());
    drop(handle);
    assert!(*store.budget.lock().unwrap() > 0);
    store
        .request("controller_hid_record_discard", &req)
        .unwrap();
    assert_eq!(*store.budget.lock().unwrap(), 0);
    begin(&mut store, &req);
}

#[test]
fn storage_capacity_and_generation_keys_never_replace_existing_archives() {
    let mut store = Store::default();
    let req = request("first");
    begin(&mut store, &req);
    assert!(store.start(&req, Value::Null, 0, 0).is_err());
    let mut stale = req.clone();
    stale["generation"] = json!("other");
    assert!(store
        .request("controller_hid_record_status", &stale)
        .is_err());
    for id in ["two", "three", "four"] {
        begin(&mut store, &request(id));
    }
    assert!(store.start(&request("fifth"), Value::Null, 0, 0).is_err());
}

#[test]
fn global_byte_limit_is_shared_across_archives_and_unknown_loss_is_explicit() {
    let mut store = Store::default();
    let a = begin(&mut store, &request("a"));
    let b = begin(&mut store, &request("b"));
    *store.budget.lock().unwrap() = MAX_BYTES;
    a.lock().unwrap().observe(1, 20, "{\"sequence\":\"1\"}");
    assert_eq!(a.lock().unwrap().status()["reason"]["kind"], "capacity");
    // Restore the artificial accounting before handles drop.
    *store.budget.lock().unwrap() = a.lock().unwrap().charged + b.lock().unwrap().charged;
    b.lock()
        .unwrap()
        .end(json!({"kind":"overflow"}), 0, 21, true);
    let archive = b.lock().unwrap().export(&json!({})).unwrap();
    assert_eq!(archive["recording"]["records"][0]["fromSequence"], "1");
    assert!(archive["recording"]["records"][0]["toSequence"].is_null());
}

#[test]
fn chunk_validation_and_base64_padding_are_exact() {
    for (raw, expected) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(export::base64(raw.as_bytes()), expected);
    }
    let mut store = Store::default();
    let req = request("a");
    let handle = begin(&mut store, &req);
    handle
        .lock()
        .unwrap()
        .end(json!({"kind":"removed"}), 0, 21, false);
    for bad in [
        json!({"offset":-1}),
        json!({"offset":MAX_BYTES+1}),
        json!({"limit":0}),
        json!({"limit":32769}),
    ] {
        assert!(handle.lock().unwrap().export(&bad).is_err());
    }
    for (field, value) in [
        ("maxBytes", 1),
        ("maxRecords", 2),
        ("maxRecords", 1_000_001),
    ] {
        let mut req = request("invalid");
        req[field] = json!(value);
        assert!(store.start(&req, Value::Null, 0, 0).is_err());
    }
}

#[test]
fn every_chunk_boundary_matches_the_full_utf8_archive() {
    let mut store = Store::default();
    let handle = begin(&mut store, &request("a"));
    let mut recording = handle.lock().unwrap();
    for sequence in 1..50 {
        recording.observe(sequence, sequence as u128, &json!({"sequence":sequence.to_string(),"receivedUs":sequence.to_string(),"samples":{"opaque":"λ"}}).to_string());
    }
    recording.end(json!({"kind":"stopped"}), 49, 50, false);
    let full = recording.export(&json!({})).unwrap().to_string();
    for offset in (0..=full.len()).step_by(17) {
        let end = (offset + 53).min(full.len());
        let chunk = recording
            .export(&json!({"offset":offset,"limit":53}))
            .unwrap();
        assert_eq!(
            chunk["chunk"],
            export::base64(&full.as_bytes()[offset..end]),
            "offset {offset}"
        );
    }
}

#[test]
fn a_racing_stop_releases_capture_without_waiting_for_the_export_mutex() {
    let mut store = Store::default();
    let handle = begin(&mut store, &request("a"));
    let mut exporting = handle.lock().unwrap();
    let capture = handle.clone();
    let (done, wait) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        capture.observe(1, 21, "{\"sequence\":\"1\"}");
        capture.end(json!({"kind":"removed"}), 1, 22, false);
        done.send(()).unwrap();
    });
    exporting.end(json!({"kind":"stopped"}), 0, 20, false);
    // The mutex remains held for an arbitrarily long export operation.
    let finished = wait.recv_timeout(std::time::Duration::from_secs(2));
    drop(exporting);
    worker.join().unwrap();
    assert!(finished.is_ok());
    assert_eq!(handle.lock().unwrap().records.len(), 1);
}

#[test]
fn replay_imports_safe_rows_and_the_explicit_prefix_before_an_oversized_row() {
    let mut store = Store::default();
    let req = request("decode-budget");
    let handle = store
        .start(&req, json!({"descriptorHex":"01"}), 0, 0)
        .unwrap();
    let mut recording = handle.lock().unwrap();
    let mut safe =
        json!({"sequence":"1","receivedUs":"1","wireHex":"0001","samples":{"opaque":""}});
    let overhead = safe.to_string().len();
    safe["samples"]["opaque"] = json!("x".repeat(bounds::VALUE_BYTES - overhead));
    let encoded = safe.to_string();
    assert_eq!(encoded.len(), bounds::VALUE_BYTES);
    recording.observe_value(1, 1, &encoded, &safe);
    let oversized = json!({"sequence":"2","receivedUs":"2","wireHex":"0002","samples":{"opaque":"x".repeat(bounds::VALUE_BYTES)}});
    recording.observe_value(2, 2, &oversized.to_string(), &oversized);
    let exported = recording.export(&json!({})).unwrap();
    assert_eq!(exported["recording"]["reason"]["kind"], "oversized-row");
    assert_eq!(exported["recording"]["records"][1]["kind"], "gap");
    assert_eq!(exported["recording"]["records"][1]["fromSequence"], "2");
    assert_eq!(exported["recording"]["records"][1]["toSequence"], "2");
    assert_eq!(exported["recording"]["recordCount"], 3);
    assert!(exported.to_string().len() <= recording.max_bytes);
    let mut replay = crate::hid::replay::Store::default();
    for input in [exported.clone(), json!(exported.to_string())] {
        assert!(replay
            .request("controller_hid_replay_import", &json!({"recording":input}))
            .is_ok());
    }
}

#[test]
fn deep_native_shapes_are_rejected_or_sealed_before_the_unsafe_value() {
    let mut store = Store::default();
    let mut nested = json!({"value":1});
    for _ in 0..48 {
        nested = json!({"nested":nested});
    }
    assert!(store
        .start(&request("bad-descriptor"), nested.clone(), 0, 0)
        .is_err());
    assert!(store
        .start(
            &request("huge-descriptor"),
            json!({"opaque":"x".repeat(bounds::VALUE_BYTES)}),
            0,
            0
        )
        .is_err());
    let handle = store.start(&request("deep-row"), json!({}), 0, 0).unwrap();
    let row = json!({"sequence":"1","receivedUs":"1","samples":nested});
    handle
        .lock()
        .unwrap()
        .observe_value(1, 1, &row.to_string(), &row);
    let exported = handle.lock().unwrap().export(&json!({})).unwrap();
    assert_eq!(exported["recording"]["reason"]["kind"], "decode-budget");
    assert_eq!(exported["recording"]["records"][0]["fromSequence"], "1");
    assert_eq!(exported["recording"]["records"][0]["toSequence"], "1");
    assert!(crate::hid::replay::Store::default()
        .request(
            "controller_hid_replay_import",
            &json!({"recording":exported})
        )
        .is_ok());
}

#[test]
fn byte_accounting_includes_the_download_wrapper_and_reserved_markers() {
    let mut store = Store::default();
    let detail = json!({"opaque":"x".repeat(12000)});
    let probe = store
        .start(&request("probe"), detail.clone(), 0, 0)
        .unwrap();
    let old_limit = probe.lock().unwrap().initial_metadata_bytes + RESERVE;
    let mut constrained = Store::default();
    let mut too_small = request("probe");
    too_small["maxBytes"] = json!(old_limit);
    assert!(constrained.start(&too_small, detail.clone(), 0, 0).is_err());
    too_small["maxBytes"] = json!(old_limit + bounds::WRAPPER_BYTES);
    let exact = constrained.start(&too_small, detail, 0, 0).unwrap();
    assert_eq!(exact.lock().unwrap().charged, old_limit + bounds::WRAPPER_BYTES);
    let mut req = request("small");
    req["maxBytes"] = json!(16384);
    let handle = store.start(&req, json!({}), 0, 0).unwrap();
    let mut recording = handle.lock().unwrap();
    for seq in 1..100 {
        let row = json!({"sequence":seq.to_string(),"receivedUs":seq.to_string(),"samples":{"opaque":"x".repeat(1024)}});
        recording.observe_value(seq, seq as u128, &row.to_string(), &row);
        if !recording.active() {
            break;
        }
    }
    let export = recording.export(&json!({})).unwrap();
    assert_eq!(
        export["recording"]["bytes"].as_u64().unwrap() as usize,
        export["recording"].to_string().len()
    );
    assert_eq!(
        recording.encoded_size() + bounds::WRAPPER_BYTES,
        export.to_string().len()
    );
    assert!(export.to_string().len() <= recording.charged);
    assert!(recording.charged <= 16384);
    assert_eq!(export["recording"]["reason"]["kind"], "capacity");
    assert!(crate::hid::replay::Store::default()
        .request("controller_hid_replay_import", &json!({"recording":export}))
        .is_ok());
}
