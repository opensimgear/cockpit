use super::*;

fn portable(field: usize) -> Value {
    json!({"field":field,"element":0,"semantics":"Absolute","raw_bits":"01","sample":{"Integer":"1"},"usages":[]})
}

#[test]
fn budget_failure_stops_constructing_rows_and_invalidates_the_whole_baseline() {
    let mut current = Current::default();
    let mut constructed = 0;
    let rows = std::iter::from_fn(|| {
        constructed += 1;
        assert!(
            constructed <= 16_385,
            "rows beyond capacity must not be constructed"
        );
        Some(
            json!({"fieldId":constructed,"elementIndex":0,"semantics":"absolute","sample":{"value":"1"}}),
        )
    });
    current.retain_rows(rows, &json!(1), 7, 9);
    assert_eq!(constructed, 16_385);
    assert_eq!(current.state(7, None)["status"], "stale");
    assert!(current.values.is_empty());
    assert_eq!(current.bytes, 0);
    assert_eq!(current.diagnostics[0]["kind"], "capacity");
}

#[test]
fn repeated_full_baselines_can_replace_keys_and_recover_after_capacity_failure() {
    let mut current = Current::default();
    let values: Vec<_> = (0..16_384).map(portable).collect();
    let projection = json!({"report_id":1,"values":values});
    for seq in 1..=2 {
        current.observe(seq, seq as u128, &projection, None);
    }
    assert_eq!(current.values.len(), 16_384);
    assert_eq!(current.status, Some("live"));
    let repeated = std::iter::repeat_n(
        json!({"fieldId":"same","elementIndex":0,"semantics":"absolute"}),
        20_000,
    );
    let mut replacement = Current::default();
    replacement.retain_rows(repeated, &json!(1), 1, 1);
    assert_eq!(replacement.values.len(), 1);
    let mut oversized = portable(0);
    oversized["raw_bits"] = json!("x".repeat(8 * 1024 * 1024));
    current.observe(3, 3, &json!({"report_id":1,"values":[oversized]}), None);
    assert_eq!(current.status, Some("stale"));
    assert_eq!(current.bytes, 0);
    current.observe(4, 4, &json!({"report_id":1,"values":[portable(2)]}), None);
    assert_eq!(current.status, Some("live"));
    assert_eq!(current.values.len(), 1);
    assert_eq!(current.state(4, None)["values"][0]["sequence"], "4");
}

#[test]
fn descriptor_bound_amplification_is_checked_between_rows_on_every_backend() {
    let big = "x".repeat(1024 * 1024);
    let descriptor = json!({"descriptor":{"fields":[{"globals":{"logical_min":big}}]},
        "elements":[{"cookie":1,"logicalMin":big}],
        "capabilities":{"reports":[{"kind":"input","valueCaps":[{"isAbsolute":true,"logicalMin":big}],"buttonCaps":[{"isAbsolute":true}]}]}});
    let cases = [
        json!({"report_id":1,"values":(0..32).map(|i| {let mut v=portable(0);v["element"]=json!(i);v}).collect::<Vec<_>>()}),
        json!({"reportId":1,"elements":(0..32).map(|i|json!({"cookie":1,"integer":i.to_string(),"relative":false})).collect::<Vec<_>>()}),
        json!({"kind":"input","reportId":1,"data":(0..32).map(|i|json!({"dataIndex":i,"rawValue":"1","values":[{"capabilityIndex":0,"semantics":"absolute","quality":"valid","native":"1"}]})).collect::<Vec<_>>()}),
    ];
    for (index, projection) in cases.into_iter().enumerate() {
        let mut current = Current::new(descriptor.clone());
        current.observe(1, 1, &projection, None);
        if index == 1 {
            // Repeated cookie observations replace one authoritative value.
            assert_eq!(current.status, Some("live"));
            assert_eq!(current.values.len(), 1);
        } else {
            assert_eq!(current.status, Some("stale"));
            assert_eq!(current.bytes, 0);
        }
    }
}

#[test]
fn relative_and_opaque_observations_do_not_construct_descriptor_expansions() {
    let mut current = Current::new(
        json!({"descriptor":{"fields":[{"globals":{"logical_min":"x".repeat(8*1024*1024)}}]}}),
    );
    let mut relative = portable(0);
    relative["semantics"] = json!("Relative");
    let mut opaque = portable(0);
    opaque["semantics"] = json!("Opaque");
    current.observe(
        1,
        1,
        &json!({"report_id":1,"values":[relative, opaque]}),
        None,
    );
    assert_eq!(current.status, Some("live"));
    assert_eq!(current.bytes, 0);
    assert!(current.values.is_empty());
}

#[test]
fn windows_stream_preserves_scalar_selector_button_and_packed_array_semantics() {
    let mut current = Current::new(json!({"capabilities":{"reports":[{"kind":"input",
        "valueCaps":[{"isAbsolute":true}],"buttonCaps":[{"isAbsolute":true}]}]}}));
    current.observe(1, 2, &json!({"kind":"input","reportId":7,
        "data":[{"dataIndex":3,"rawValue":"255","matchingButtonCaps":[0],"values":[
            {"capabilityIndex":0,"semantics":"absolute","quality":"valid","native":"-1"},
            {"capabilityIndex":0,"semantics":"relative","quality":"valid","native":"2"}]},
            {"dataIndex":4,"rawValue":"5","values":[{"capabilityIndex":0,"semantics":"array-selector","quality":"valid","native":"5"}]}],
        "valueArrays":[{"capabilityIndex":0,"bytes":"ff00","bitSize":8,"reportCount":2}]}), None);
    let state = current.state(1, None);
    let values = state["values"].as_array().unwrap();
    assert_eq!(values.len(), 4);
    let find = |prefix: &str| {
        values
            .iter()
            .find(|v| v["fieldId"].as_str().unwrap().starts_with(prefix))
            .unwrap()
    };
    assert_eq!(find("hidp-value:0:data:3")["sample"]["value"], "-1");
    assert_eq!(
        find("hidp-value:0:data:4")["sample"]["kind"],
        "array-selector"
    );
    assert_eq!(find("hidp-button")["sample"]["value"], "255");
    assert_eq!(find("hidp-array")["osProjectionHex"], "ff00");
    current.observe(
        2,
        3,
        &json!({"kind":"input","reportId":7,"data":[],"valueArrays":[]}),
        None,
    );
    assert_eq!(current.status, Some("live"));
    assert!(current.values.is_empty());
}
