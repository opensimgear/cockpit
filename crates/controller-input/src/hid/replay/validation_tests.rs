use super::tests::{recording, seal};
use super::*;

#[test]
fn conflicting_archive_claims_and_implicit_sequence_loss_are_rejected() {
    for (pointer, value) in [
        ("/format", json!("unknown")),
        ("/interfaceId", json!("")),
        ("/descriptor", Value::Null),
        ("/status", json!("unknown")),
        ("/startedUnixMs", json!("-1")),
        ("/startingSequence", json!("100")),
        ("/lastSequence", json!("4")),
        ("/records/0/samples", json!([])),
        ("/records/0/sequence", json!("2")),
        ("/records/0/wireHex", json!(true)),
        ("/records/0/wireHex", json!("f")),
        ("/records/2/fromSequence", json!("4")),
        ("/records/2/toSequence", json!("2")),
        ("/records/2/toSequence", json!("4")),
        ("/records/2/reason", Value::Null),
        ("/records/3/kind", json!("mystery")),
        ("/records/3/kind", json!(null)),
        ("/records/3/sequence", json!("2")),
        ("/records/3/reason", Value::Null),
        ("/reason", json!({"kind":"other"})),
    ] {
        let mut archive = recording();
        *archive.pointer_mut(pointer).unwrap() = value;
        let archive = seal(archive);
        assert!(
            Store::default()
                .request(
                    "controller_hid_replay_import",
                    &json!({"recording":archive})
                )
                .is_err(),
            "accepted {pointer}"
        );
    }
    let mut archive = recording();
    archive["recordCount"] = json!(999);
    assert!(Store::default()
        .request(
            "controller_hid_replay_import",
            &json!({"recording":archive})
        )
        .unwrap_err()
        .contains("recordCount"));
    let mut archive = recording();
    archive["bytes"] = json!(999);
    assert!(Store::default()
        .request(
            "controller_hid_replay_import",
            &json!({"recording":archive})
        )
        .unwrap_err()
        .contains("bytes"));
    let mut archive = recording();
    archive["records"].as_array_mut().unwrap().push(json!({}));
    assert!(Store::default()
        .request(
            "controller_hid_replay_import",
            &json!({"recording":seal(archive)})
        )
        .is_err());
}

#[test]
fn raw_import_bounds_header_fields_nesting_and_individual_decoding_allocations() {
    let mut archive = recording();
    archive["descriptor"] = json!({"large":"a".repeat(bounds::DESCRIPTOR_BYTES)});
    assert!(Store::default()
        .request(
            "controller_hid_replay_import",
            &json!({"recording":seal(archive).to_string()})
        )
        .is_err());
    let mut archive = recording();
    archive["custom"] = json!("x".repeat(65537));
    assert!(Store::default()
        .request(
            "controller_hid_replay_import",
            &json!({"recording":seal(archive).to_string()})
        )
        .is_err());
    let mut archive = recording();
    archive["records"][0]["samples"] = json!({"large":"a".repeat(2*1024*1024)});
    assert!(Store::default()
        .request(
            "controller_hid_replay_import",
            &json!({"recording":seal(archive).to_string()})
        )
        .is_err());
    let mut nested = Value::Null;
    for _ in 0..60 {
        nested = json!([nested]);
    }
    assert!(Store::default()
        .request("controller_hid_replay_import", &json!({"recording":nested}))
        .unwrap_err()
        .contains("structure"));
    let fields = format!(
        "{{{}}}",
        (0..129)
            .map(|n| format!("\"{n}\":0"))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert!(Store::default()
        .request("controller_hid_replay_import", &json!({"recording":fields}))
        .is_err());
    assert!(Store::default()
        .request(
            "controller_hid_replay_import",
            &json!({"recording":"{\"format\":0,\"format\":1}"})
        )
        .is_err());
    assert!(Store::default()
        .request("controller_hid_replay_import", &json!({"recording":"{"}))
        .is_err());
}
