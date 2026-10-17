use super::*;
use crate::{
    module_console::UsbIdentity,
    module_usb::{UsbRouteToken, UsbSnapshot},
    transport::PortInfo,
};
use serde_json::json;
const SERIAL: &str = "00112233445566778899aabb";
const MODULE: &str = "4f53474400112233445566778899aabb";
fn slot() -> SlotSnapshot {
    SlotSnapshot {
        key: "selected-endpoint".into(),
        port: PortInfo {
            path: "COM8".into(),
            serial_number: Some(SERIAL.into()),
            vendor_id: Some("1209".into()),
            product_id: Some("0001".into()),
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
                board: "test".into(),
                profile: "Panel".into(),
                module_id: MODULE.into(),
                serial: SERIAL.into(),
                protocol: 3,
                button_count: 1,
                axis_count: 0,
                hat_count: 0,
            }),
            route: Some(UsbRouteToken {
                module_id: MODULE.into(),
                serial: SERIAL.into(),
                connection_generation: 1,
                boot: 100,
                configuration_generation: 2,
            }),
            config: Some(
                json!({"generation":2,"savedGeneration":1,"owner":0,"claimExpiresMs":0,"dirty":false,"pendingApply":false,"view":"active","valid":true,"config":{"name":"Panel","expanderMask":0,"buttons":[[0,0,0]],"axes":[],"hats":[],"encoders":[],"selectors":[]},"layout":{"buttons":1,"axes":0,"hats":0,"reportBytes":1},"descriptorHex":"0501"}),
            ),
            ..UsbSnapshot::default()
        },
    }
}
fn can(gateway: &str, boot: u32) -> (String, Value) {
    (
        gateway.into(),
        json!({"connected":true,"modules":[{"id":MODULE,"online":true,"boot":boot}]}),
    )
}

#[test]
fn verified_same_boot_ready_active_usb_is_preferred_without_mutating_sources() {
    let usb = vec![slot()];
    let can = vec![can("gateway-a", 100)];
    let before = usb[0].value();
    let before_can = can.clone();
    assert!(
        evaluate(&usb, &can, &MODULE.to_ascii_uppercase())
            .unwrap()
            .direct_usb_preferred
    );
    assert_eq!(usb[0].value(), before);
    assert_eq!(can, before_can);
    assert!(evaluate(&usb, &[], MODULE).unwrap().direct_usb_preferred);
}
#[test]
fn matching_unproven_module_ids_stay_isolated_and_never_prefer_a_route() {
    let gateways = vec![can("gateway-a", 100), can("gateway-b", 101)];
    assert!(
        !evaluate(&[], &gateways, MODULE)
            .unwrap()
            .direct_usb_preferred
    );
    for mutation in ["model", "serial", "port", "protocol", "derived"] {
        let mut usb = slot();
        match mutation {
            "model" => usb.snapshot.identity.as_mut().unwrap().model = "gateway".into(),
            "serial" => {
                usb.snapshot.identity.as_mut().unwrap().serial = "000000000000000000000000".into()
            }
            "port" => usb.port.serial_number = Some("00112233445566778899aacc".into()),
            "protocol" => usb.snapshot.identity.as_mut().unwrap().protocol = 2,
            _ => {
                usb.snapshot.identity.as_mut().unwrap().module_id =
                    "00112233445566778899aabbccddeeff".into()
            }
        }
        assert!(
            !evaluate(&[usb], &gateways, MODULE)
                .unwrap()
                .direct_usb_preferred,
            "{mutation}"
        );
    }
    assert!(evaluate(&[], &[], "bad").is_err());
}
#[test]
fn an_offline_verified_identity_can_detect_duplicate_can_routes_without_preferring_usb() {
    let mut usb = slot();
    usb.present = false;
    usb.admissible = false;
    usb.snapshot.connected = false;
    usb.snapshot.route = None;
    assert!(
        !evaluate(&[usb.clone()], &[can("gateway-a", 100)], MODULE)
            .unwrap()
            .direct_usb_preferred
    );
    assert!(evaluate(
        &[usb.clone()],
        &[can("gateway-a", 100), can("gateway-b", 100)],
        MODULE
    )
    .unwrap_err()
    .contains("Several live CAN"));
    let mut offline = can("gateway-b", 101);
    offline.1["connected"] = json!(false);
    assert!(evaluate(&[usb], &[can("gateway-a", 100), offline], MODULE).is_ok());
}
#[test]
fn conflicting_usb_can_boots_and_duplicate_usb_paths_block_all_new_claims() {
    assert!(evaluate(&[slot()], &[can("gateway-a", 101)], MODULE)
        .unwrap_err()
        .contains("boot"));
    assert!(evaluate(&[slot()], &[can("gateway-a", 0)], MODULE)
        .unwrap_err()
        .contains("boot"));
    let mut duplicate = slot();
    duplicate.key = "second-endpoint".into();
    assert!(evaluate(&[slot(), duplicate], &[], MODULE)
        .unwrap_err()
        .contains("Several live direct USB"));
    let mut blocked = slot();
    blocked.admissible = false;
    blocked.snapshot.connected = false;
    blocked.snapshot.route = None;
    assert!(evaluate(&[blocked], &[can("gateway-a", 100)], MODULE).is_err());
}
#[test]
fn usb_preference_requires_current_complete_active_configuration_and_layout() {
    for mutation in [
        "missing",
        "draft",
        "invalid",
        "pending",
        "generation",
        "layout",
        "identity-layout",
        "malformed",
        "route",
        "error",
        "disconnected",
    ] {
        let mut usb = slot();
        match mutation {
            "missing" => usb.snapshot.config = None,
            "draft" => usb.snapshot.config.as_mut().unwrap()["view"] = json!("draft"),
            "invalid" => usb.snapshot.config.as_mut().unwrap()["valid"] = json!(false),
            "pending" => usb.snapshot.config.as_mut().unwrap()["pendingApply"] = json!(true),
            "generation" => usb.snapshot.config.as_mut().unwrap()["generation"] = json!(3),
            "layout" => usb.snapshot.config.as_mut().unwrap()["layout"]["reportBytes"] = json!(3),
            "identity-layout" => usb.snapshot.identity.as_mut().unwrap().button_count = 2,
            "malformed" => usb.snapshot.config.as_mut().unwrap()["config"]["axes"] = json!(null),
            "route" => {
                usb.snapshot.route.as_mut().unwrap().serial = "00112233445566778899aacc".into()
            }
            "error" => usb.snapshot.error = Some("Read uncertain".into()),
            _ => usb.snapshot.connected = false,
        }
        assert!(
            !evaluate(&[usb], &[can("gateway-a", 100)], MODULE)
                .unwrap()
                .direct_usb_preferred,
            "{mutation}"
        );
    }
}

#[test]
fn opposite_transport_writer_guards_never_treat_a_matching_client_as_transfer_permission() {
    let pin = WriterPin {
        module_id: MODULE.into(),
        client_id: "same-client-identity".into(),
        boot: 100,
    };
    assert!(guard_usb_writer(MODULE, &[("gateway-a".into(), pin.clone())]).is_err());
    assert!(guard_can_writer(&MODULE.to_ascii_uppercase(), &[pin.clone()]).is_err());
    assert!(guard_usb_writer(MODULE, &[]).is_ok());
    assert!(guard_can_writer("4f53474400112233445566778899aacc", &[pin.clone()]).is_ok());
    let invalid = WriterPin {
        boot: 0,
        ..pin.clone()
    };
    assert!(guard_usb_writer(MODULE, &[("gateway-a".into(), invalid)])
        .unwrap_err()
        .contains("uncertain"));
    let invalid = WriterPin {
        client_id: String::new(),
        ..pin
    };
    assert!(guard_can_writer(MODULE, &[invalid])
        .unwrap_err()
        .contains("uncertain"));
}
