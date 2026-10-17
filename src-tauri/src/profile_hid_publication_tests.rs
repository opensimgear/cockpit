use super::*;
use serde_json::json;

fn can_state() -> Value {
    json!({"gatewaySerial":"0123456789abcdef01234567","connected":true,"canProtocol":3,
        "layoutPending":false,"nativeSnapshotAtMs":now_ms(),"publication":{"phase":"idle","outcome":"no_change"},
        "modules":[{"online":true,"directUsb":false,"published":false,"boot":1,"configurationGeneration":2}]})
}
const GATEWAY: &str = "stm32:0123456789abcdef01234567";

#[test]
fn can_requires_actual_settled_disabled_publication_and_fresh_observation() {
    let state = can_state();
    assert_eq!(
        can_status(GATEWAY, &state, &state["modules"][0]),
        "disabled"
    );
    for (path, value) in [
        ("/connected", json!(false)),
        ("/canProtocol", json!(2)),
        ("/layoutPending", json!(true)),
        ("/nativeSnapshotAtMs", json!(now_ms() - 4000)),
        ("/nativeSnapshotAtMs", json!(now_ms() + 60000)),
        ("/modules/0/online", json!(false)),
        ("/modules/0/directUsb", json!(true)),
        ("/modules/0/boot", json!(0)),
        ("/modules/0/configurationGeneration", json!(0)),
        ("/modules/0/published", Value::Null),
        ("/publication/phase", json!("restoring")),
        ("/publication/outcome", json!("preserved")),
    ] {
        let mut changed = state.clone();
        *changed.pointer_mut(path).unwrap() = value;
        assert_eq!(
            can_status(GATEWAY, &changed, &changed["modules"][0]),
            "unknown",
            "{path}"
        );
    }
    assert_eq!(
        can_status(
            "stm32:999999999999999999999999",
            &state,
            &state["modules"][0]
        ),
        "unknown"
    );
    let mut changed = state;
    changed["modules"][0]["published"] = json!(true);
    assert_eq!(
        can_status(GATEWAY, &changed, &changed["modules"][0]),
        "published"
    );
    changed["modules"][0]["published"] = json!(false);
    changed["publication"] = json!({"phase":"owned","outcome":"ok","expectedReconnect":false});
    assert_eq!(
        can_status(GATEWAY, &changed, &changed["modules"][0]),
        "lease-disabled"
    );
    changed["publication"]["expectedReconnect"] = json!(true);
    assert_eq!(
        can_status(GATEWAY, &changed, &changed["modules"][0]),
        "unknown"
    );
}

pub(crate) fn usb() -> SlotSnapshot {
    use crate::{
        module_console::UsbIdentity,
        module_usb::{UsbRouteToken, UsbSnapshot},
        transport::PortInfo,
    };
    SlotSnapshot {
        key: "endpoint".into(),
        port: PortInfo {
            path: "COM8".into(),
            serial_number: None,
            vendor_id: None,
            product_id: None,
            manufacturer: None,
            friendly_name: None,
        },
        present: true,
        automatic: false,
        admissible: true,
        diagnostics: vec![],
        snapshot: UsbSnapshot {
            connected: true,
            port: Some("COM8".into()),
            identity: Some(UsbIdentity {
                model: "device-template".into(),
                board: "board".into(),
                profile: "profile".into(),
                module_id: "module".into(),
                serial: "serial".into(),
                protocol: 3,
                button_count: 0,
                axis_count: 0,
                hat_count: 0,
            }),
            route: Some(UsbRouteToken {
                module_id: "module".into(),
                serial: "serial".into(),
                connection_generation: 7,
                boot: 8,
                configuration_generation: 9,
            }),
            config: Some(
                json!({"view":"active","valid":true,"pendingApply":false,"generation":9,"layout":{"reportBytes":0}}),
            ),
            draft: None,
            actions: None,
            events: None,
            claimed_by: None,
            observed_at_ms: Some(now_ms()),
            error: None,
        },
    }
}
#[test]
fn usb_uses_active_descriptor_not_draft_flags_or_absent_hid_inventory() {
    let mut slot = usb();
    assert_eq!(usb_status(&slot), "disabled");
    slot.snapshot.draft = Some(json!({"layout":{"reportBytes":0}}));
    slot.snapshot.config.as_mut().unwrap()["layout"]["reportBytes"] = json!(1);
    assert_eq!(usb_status(&slot), "published");
    slot.snapshot.config.as_mut().unwrap()["layout"]["reportBytes"] = json!(0);
    slot.snapshot.config.as_mut().unwrap()["generation"] = json!(10);
    assert_eq!(usb_status(&slot), "unknown");
    slot.snapshot.config.as_mut().unwrap()["generation"] = json!(9);
    slot.snapshot.observed_at_ms = Some(now_ms() - 4000);
    assert_eq!(usb_status(&slot), "unknown");
    slot.snapshot.observed_at_ms = Some(now_ms());
    slot.present = false;
    assert_eq!(usb_status(&slot), "unknown");
    slot.present = true;
    slot.snapshot.config.as_mut().unwrap()["view"] = json!("draft");
    assert_eq!(usb_status(&slot), "unknown");
}
