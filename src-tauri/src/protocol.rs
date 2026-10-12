//! The protocol-1 boundary is deliberately independent of Tauri and serial I/O.
use serde_json::Value;
use std::collections::HashSet;

pub const REQUEST_LIMIT: usize = 4_095;
pub const RESPONSE_LIMIT: usize = 65_536;
pub const INVALID_STATE: &str =
    "This serial device is not a supported OpenSimGear Pico gateway (protocol 1).";

pub fn identifier(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn printable(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        !value.trim().is_empty()
            && value.chars().count() <= 80
            && !value.chars().any(|ch| ch <= '\u{001f}' || ch == '\u{007f}')
    })
}

fn integer(value: &Value, min: u64, max: u64) -> bool {
    value
        .as_u64()
        .is_some_and(|value| (min..=max).contains(&value))
}

/// Validate before any firmware state is allowed to replace the trusted snapshot.
pub fn validate_state(state: &Value, expected_serial: Option<&str>) -> Result<(), String> {
    let invalid = || INVALID_STATE.to_owned();
    let serial = state["gatewaySerial"].as_str().ok_or_else(invalid)?;
    let suffix = serial.strip_prefix("OSG-PICO-").ok_or_else(invalid)?;
    if state["backend"] != "pico"
        || state["protocolVersion"] != 1
        || !identifier(suffix, 4, 64)
        || ["running", "needsReconnect", "reconnecting"]
            .iter()
            .any(|key| !state[key].is_boolean())
    {
        return Err(invalid());
    }
    let modules = state["modules"].as_array().ok_or_else(invalid)?;
    if modules.len() > 8 {
        return Err(invalid());
    }
    let mut ids = HashSet::new();
    let mut interfaces = HashSet::new();
    for module in modules {
        let id = module["id"].as_str().ok_or_else(invalid)?;
        let registered = module["registered"].as_bool().ok_or_else(invalid)?;
        let active = module["active"].as_bool().ok_or_else(invalid)?;
        if !identifier(id, 1, 40)
            || !ids.insert(id)
            || !printable(module["name"].as_str())
            || !matches!(
                module["kind"].as_str(),
                Some("shifter" | "pedals" | "wheel" | "handbrake" | "panel")
            )
            || !module["online"].is_boolean()
            || !integer(&module["buttonCount"], 0, 32)
            || !integer(&module["axisCount"], 0, 6)
            || module["buttonCount"].as_u64().unwrap() + module["axisCount"].as_u64().unwrap() == 0
            || module["serialNumber"] != format!("OSG-SIM-{id}")
            || module["usbSerialNumber"] != serial
            || !printable(module["settings"]["label"].as_str())
            || !integer(&module["settings"]["debounce"], 0, 100)
            || !module["settings"]["inverted"].is_boolean()
        {
            return Err(invalid());
        }
        let buttons = module["buttons"].as_array().ok_or_else(invalid)?;
        let button_count = module["buttonCount"].as_u64().unwrap();
        let mut pressed = HashSet::new();
        if buttons.len() > button_count as usize
            || buttons.iter().any(|button| {
                !integer(button, 1, button_count) || !pressed.insert(button.as_u64().unwrap())
            })
        {
            return Err(invalid());
        }
        let axes = module["axes"].as_array().ok_or_else(invalid)?;
        if axes.len() != module["axisCount"].as_u64().unwrap() as usize
            || axes.iter().any(|axis| !integer(axis, 0, 65_535))
        {
            return Err(invalid());
        }
        let interface = module.get("hidInterface").ok_or_else(invalid)?;
        if !interface.is_null() && !integer(interface, 2, 9)
            || registered == interface.is_null()
            || (registered && !interfaces.insert(interface.as_u64().unwrap()))
            || (active
                && (!registered || state["running"] != true || state["reconnecting"] != false))
        {
            return Err(invalid());
        }
    }
    if expected_serial.is_some_and(|expected| expected != serial) {
        return Err(
            "A different Pico gateway appeared on the serial port. Connect it explicitly.".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn state() -> Value {
        json!({"backend":"pico","protocolVersion":1,"firmwareVersion":"0.1.1",
            "gatewaySerial":"OSG-PICO-0123456789ABCDEF","running":false,"needsReconnect":false,"reconnecting":false,
            "modules":[{"id":"OSG-SQ-0042","name":"OSG Sequential shifter","kind":"shifter",
                "online":true,"registered":true,"active":false,"buttonCount":3,"axisCount":1,
                "buttons":[],"axes":[0],"serialNumber":"OSG-SIM-OSG-SQ-0042",
                "usbSerialNumber":"OSG-PICO-0123456789ABCDEF","hidInterface":2,
                "settings":{"label":"OSG Sequential shifter","debounce":10,"inverted":false}}]})
    }

    #[test]
    fn validates_identity_and_every_module_boundary() {
        let valid = state();
        assert!(validate_state(&valid, None).is_ok());
        assert!(validate_state(&valid, Some("OSG-PICO-other1234"))
            .unwrap_err()
            .contains("different Pico"));
        let mutations = [
            ("/backend", json!("unrelated")),
            ("/protocolVersion", json!(2)),
            ("/gatewaySerial", json!("OSG-PICO-")),
            ("/modules/0/id", json!("../../file")),
            ("/modules/0/name", json!("bad\nlabel")),
            ("/modules/0/settings/label", json!("x".repeat(81))),
            ("/modules/0/axisCount", json!(7)),
            ("/modules/0/axes", json!([65_536])),
            ("/modules/0/buttons", json!([1, 1])),
            ("/modules/0/buttons", json!([4])),
            ("/modules/0/registered", json!(false)),
            ("/modules/0/hidInterface", json!(10)),
            ("/modules/0/active", json!(true)),
            ("/modules/0/usbSerialNumber", json!("OSG-PICO-other1234")),
            ("/modules/0/settings/debounce", json!(101)),
            ("/modules/0/settings/inverted", json!(0)),
        ];
        for (path, value) in mutations {
            let mut malformed = valid.clone();
            *malformed.pointer_mut(path).unwrap() = value;
            assert!(validate_state(&malformed, None).is_err(), "accepted {path}");
        }
        let mut duplicates = valid.clone();
        duplicates["modules"]
            .as_array_mut()
            .unwrap()
            .push(valid["modules"][0].clone());
        assert!(validate_state(&duplicates, None).is_err());
    }

    #[test]
    fn accepts_utf8_and_pending_unregistered_module() {
        let mut value = state();
        value["modules"][0]["settings"]["label"] = json!("OSG Pédales 🏁");
        value["modules"][0]["registered"] = json!(false);
        value["modules"][0]["hidInterface"] = Value::Null;
        assert!(validate_state(&value, None).is_ok());
    }
}
