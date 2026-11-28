mod fields;
mod state;

#[cfg(test)]
mod tests;

use fields::{decode_fields, report_name};
use serde_json::{json, Map, Value};

const IOCTL_HID_SET_FEATURE: u32 = 0x000b_0191;
const IOCTL_HID_WRITE_REPORT: u32 = 0x000b_0017;
const IOCTL_HID_GET_FEATURE: u32 = 0x000b_0195;
const IOCTL_HID_GET_INPUT_REPORT: u32 = 0x000b_01a1;
const IOCTL_HID_READ_REPORT: u32 = 0x000b_000b;
const MAX_EFFECT_BLOCK_INDEX: u8 = 100;

pub(super) use state::Effects;

/// Decode callback bytes using the vJoy 2.2.2 command and packed device/report nibble.
pub(super) fn decode(slot: u8, command: u32, bytes: &[u8]) -> Value {
    let mut out = Map::new();
    out.insert("slot".into(), json!(slot));
    out.insert("command".into(), json!(command));
    out.insert(
        "commandName".into(),
        json!(match command {
            IOCTL_HID_SET_FEATURE => "setFeature",
            IOCTL_HID_WRITE_REPORT => "writeReport",
            IOCTL_HID_GET_FEATURE => "getFeature",
            IOCTL_HID_GET_INPUT_REPORT => "getInputReport",
            IOCTL_HID_READ_REPORT => "readReport",
            _ => "unknown",
        }),
    );
    if bytes.is_empty() {
        out.insert("status".into(), json!("malformed"));
        out.insert("error".into(), json!("missing PID report ID"));
        return Value::Object(out);
    }
    let device_id = bytes[0] >> 4;
    let report_selector = bytes[0] & 0x0f;
    out.insert("deviceId".into(), json!(device_id));
    if command == IOCTL_HID_READ_REPORT {
        out.insert("reportSelector".into(), json!(report_selector));
        out.insert("status".into(), json!("unobserved"));
        out.insert("reportKind".into(), json!("input"));
        out.insert("observedByPinnedCallback".into(), json!(false));
        out.insert(
            "error".into(),
            json!("The pinned vJoy FFB callback does not forward IOCTL_HID_READ_REPORT"),
        );
        return Value::Object(out);
    }
    if bytes.len() < 2
        && (command == IOCTL_HID_SET_FEATURE
            || command == IOCTL_HID_WRITE_REPORT
            || command == IOCTL_HID_GET_FEATURE
            || command == IOCTL_HID_GET_INPUT_REPORT)
    {
        out.insert("status".into(), json!("malformed"));
        out.insert(
            "error".into(),
            json!("vJoy packet shorter than report selector and data"),
        );
        return Value::Object(out);
    }
    if command != IOCTL_HID_SET_FEATURE
        && command != IOCTL_HID_WRITE_REPORT
        && command != IOCTL_HID_GET_FEATURE
        && command != IOCTL_HID_GET_INPUT_REPORT
    {
        out.insert("reportSelector".into(), json!(report_selector));
        out.insert("reportName".into(), json!("unknown"));
        out.insert("status".into(), json!("unknown"));
        out.insert("error".into(), json!("unsupported vJoy HID command"));
        return Value::Object(out);
    }
    let feature = command == IOCTL_HID_SET_FEATURE || command == IOCTL_HID_GET_FEATURE;
    let input_response = command == IOCTL_HID_GET_INPUT_REPORT;
    let id = report_selector + if feature || input_response { 0x10 } else { 0 };
    out.insert("reportId".into(), json!(id));
    if device_id != slot {
        out.insert("status".into(), json!("malformed"));
        out.insert(
            "error".into(),
            json!("payload device ID does not match selected slot"),
        );
        return Value::Object(out);
    }
    let Some(report) = report_name(id) else {
        out.insert("status".into(), json!("unknown"));
        out.insert("reportName".into(), json!("unknown"));
        return Value::Object(out);
    };
    if (input_response && id != 0x14) || (command == IOCTL_HID_GET_FEATURE && id == 0x14) {
        out.insert("status".into(), json!("unknown"));
        out.insert("reportName".into(), json!(report));
        out.insert(
            "error".into(),
            json!("report ID does not match the pinned descriptor report type"),
        );
        return Value::Object(out);
    }
    out.insert("reportName".into(), json!(report));
    let Some(fields) = decode_fields(id, bytes) else {
        out.insert("status".into(), json!("malformed"));
        out.insert(
            "error".into(),
            json!(format!(
                "truncated {report} report: got {} bytes",
                bytes.len()
            )),
        );
        return Value::Object(out);
    };
    let feature_response = command == IOCTL_HID_GET_FEATURE;
    let unobserved_response = feature_response || input_response;
    out.insert(
        "status".into(),
        json!(if unobserved_response {
            "unobserved"
        } else {
            "ok"
        }),
    );
    out.insert(
        "reportKind".into(),
        json!(if input_response {
            "input"
        } else if feature_response {
            "featureResponse"
        } else if feature {
            "feature"
        } else {
            "output"
        }),
    );
    if unobserved_response {
        out.insert("observedByPinnedCallback".into(), json!(false));
    }
    if id == 0x01 {
        out.insert("sdkHelperValid".into(), json!(true));
        out.insert("descriptorComplete".into(), json!(bytes.len() >= 22));
    }
    if let Some(block) = fields.get("effectBlockIndex").cloned() {
        out.insert("effectBlockIndex".into(), block);
    }
    out.insert("fields".into(), Value::Object(fields));
    Value::Object(out)
}
