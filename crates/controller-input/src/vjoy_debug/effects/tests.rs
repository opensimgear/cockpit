use super::{decode, Effects};
use serde_json::{json, Value};

fn event(bytes: &[u8]) -> serde_json::Value {
    decode(1, 0x000b_0017, bytes)
}

#[test]
fn decodes_set_effect_report_as_pid_fields() {
    let decoded = event(&[
        0x11, 0x02, 0x01, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0x03, 0x40,
        0x00, 0xc0, 0xff, 0xff, 0x00,
    ]);
    assert_eq!(decoded["status"], "ok");
    assert_eq!(decoded["reportName"], "setEffect");
    assert_eq!(decoded["effectBlockIndex"], 2);
    assert_eq!(decoded["fields"]["effectType"], "constant");
    assert_eq!(decoded["fields"]["durationMs"], "infinite");
    assert_eq!(decoded["fields"]["gainRaw"], 255);
    assert_eq!(
        decoded["fields"]["direction"],
        json!({"mode":"cartesian","x":64,"y":-64})
    );
}

#[test]
fn malformed_unknown_and_out_of_order_packets_are_observable_but_safe() {
    let mut state = Effects::default();
    let truncated = event(&[0x1a, 0x02]);
    assert_eq!(truncated["status"], "malformed");
    state.observe(&truncated);
    let unknown = event(&[0x1f, 0x01, 0x02]);
    assert_eq!(unknown["status"], "unknown");
    state.observe(&unknown);
    let unsupported_command = decode(1, u32::MAX, &[0x1d, 0xff]);
    assert_eq!(unsupported_command["status"], "unknown");
    state.observe(&unsupported_command);
    assert_eq!(state.snapshot()["gain"], Value::Null);
    state.observe(&event(&[0x1a, 0x02, 0x01, 0xff])); // start before definition
    let snapshot = state.snapshot();
    assert_eq!(snapshot["effects"][0]["blockIndex"], 2);
    assert_eq!(snapshot["effects"][0]["running"], true);
    assert_eq!(snapshot["effects"][0]["type"], "unknown");
}

#[test]
fn effect_create_update_start_stop_free_and_device_controls_follow_lifecycle() {
    let mut state = Effects::default();
    let created = decode(1, 0x000b_0191, &[0x11, 0x04, 0x03, 0x00]); // driver-rewritten EID=3
    assert_eq!(created["fields"]["allocationStatus"], "assigned");
    state.observe(&created);
    assert_eq!(state.snapshot()["effects"][0]["blockIndex"], 3);
    let failed = decode(1, 0x000b_0191, &[0x11, 0x04, 0x00, 0x00]); // EID=0 means no free block
    assert_eq!(failed["fields"]["allocationStatus"], "failed");
    state.observe(&failed);
    assert_eq!(state.snapshot()["effects"].as_array().unwrap().len(), 1);
    state.observe(&event(&[
        0x11, 0x03, 0x08, 0x10, 0x27, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x01, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00,
    ]));
    state.observe(&event(&[0x1a, 0x03, 0x01, 0x02])); // start twice
    state.observe(&event(&[0x1d, 0x80])); // global gain
    assert_eq!(state.snapshot()["gain"], 128);
    assert_eq!(state.snapshot()["running"], 1);
    state.observe(&event(&[0x1c, 0x05])); // pause
    assert_eq!(state.snapshot()["paused"], true);
    state.observe(&event(&[0x1a, 0x03, 0x03, 0x00])); // stop
    assert_eq!(state.snapshot()["running"], 0);
    state.observe(&event(&[0x1b, 0x03])); // block free
    assert_eq!(state.snapshot()["effects"], json!([]));
    state.observe(&event(&[0x1c, 0x04])); // device reset clears effects and pause
    assert_eq!(state.snapshot()["paused"], false);
    assert_eq!(state.snapshot()["actuatorsEnabled"], true);
}

#[test]
fn unobserved_device_state_is_unknown_and_parameter_reports_merge() {
    let mut state = Effects::default();
    assert_eq!(state.snapshot()["paused"], Value::Null);
    assert_eq!(state.snapshot()["actuatorsEnabled"], Value::Null);
    assert_eq!(state.snapshot()["gain"], Value::Null);
    state.observe(&event(&[
        0x13, 0x03, 0x01, 0x18, 0xfc, 0x0a, 0x00, 0xec, 0xff, 0xb8, 0x0b, 0xe8, 0x03, 0x14, 0x00,
    ]));
    state.observe(&event(&[
        0x14, 0x03, 0x10, 0x27, 0x18, 0xfc, 0x2c, 0x01, 0x88, 0x13, 0x00, 0x00,
    ]));
    state.observe(&event(&[
        0x12, 0x03, 0x64, 0x00, 0x05, 0x00, 0x08, 0x08, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00,
    ]));
    state.observe(&event(&[
        0x11, 0x03, 0x08, 0x10, 0x27, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x01, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00,
    ]));
    let snapshot = state.snapshot();
    let fields = &snapshot["effects"][0]["fields"];
    assert_eq!(fields["isY"], true);
    assert_eq!(fields["centerPointOffset"], -1000);
    assert_eq!(fields["periodMs"], 5000);
    assert_eq!(fields["attackTimeMs"], 2056);
    assert_eq!(fields["gainRaw"], 128);
}

#[test]
fn authored_raw_json_fixture_decodes_and_replays_in_order() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/hid-contract/vjoy/effects-lifecycle.json"
    ))
    .unwrap();
    let mut state = Effects::default();
    for packet in fixture["packets"].as_array().unwrap() {
        let bytes: Vec<u8> = packet["bytesHex"]
            .as_str()
            .unwrap()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let decoded = decode(
            fixture["slot"].as_u64().unwrap() as u8,
            packet["command"]
                .as_u64()
                .unwrap_or(fixture["command"].as_u64().unwrap()) as u32,
            &bytes,
        );
        assert_eq!(decoded["status"], packet["expect"]["status"]);
        if packet["expect"].get("reportName").is_some() {
            assert_eq!(decoded["reportName"], packet["expect"]["reportName"]);
        }
        if packet["expect"].get("effectType").is_some() {
            assert_eq!(
                decoded["fields"]["effectType"],
                packet["expect"]["effectType"]
            );
        }
        if packet["expect"].get("control").is_some() {
            assert_eq!(decoded["fields"]["control"], packet["expect"]["control"]);
        }
        state.observe(&decoded);
    }
    assert_eq!(state.snapshot(), fixture["expectedSnapshot"]);
}

#[test]
fn authored_report_layout_fixture_checks_field_projections_and_unobserved_responses() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/hid-contract/vjoy/report-layouts.json"
    ))
    .unwrap();
    let mut state = Effects::default();
    for packet in fixture["packets"].as_array().unwrap() {
        let bytes: Vec<u8> = packet["bytesHex"]
            .as_str()
            .unwrap()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let command = packet["command"]
            .as_u64()
            .unwrap_or(fixture["command"].as_u64().unwrap()) as u32;
        let decoded = decode(fixture["slot"].as_u64().unwrap() as u8, command, &bytes);
        assert_eq!(
            decoded["status"], packet["expect"]["status"],
            "{}",
            packet["name"]
        );
        assert_eq!(
            decoded["reportName"], packet["expect"]["reportName"],
            "{}",
            packet["name"]
        );
        if let Some(expected_fields) = packet["expect"]["fields"].as_object() {
            for (key, value) in expected_fields {
                assert_eq!(
                    decoded["fields"][key], *value,
                    "{}.fields.{key}",
                    packet["name"]
                );
            }
        }
        state.observe(&decoded);
    }
    assert_eq!(state.snapshot()["paused"], Value::Null);
    assert_eq!(state.snapshot()["gain"], 128);
    assert_eq!(state.snapshot()["actuatorsEnabled"], true);
}
