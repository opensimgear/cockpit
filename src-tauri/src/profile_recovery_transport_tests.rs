use super::*;
use serde_json::json;

const GATEWAY: &str = "stm32:0123456789abcdef01234567";
const MODULE: &str = "0123456789abcdef0123456789abcdef";

fn status() -> Status {
    Status {
        state: State::RecoveryNeeded,
        applied_snapshot: None,
        owner: None,
        degradation: vec![],
        fault: Some("Interrupted owner".into()),
    }
}
fn request(command: &str) -> Value {
    let mut request =
        json!({"command":command,"clientId":"recovery-read-fixture","gatewayId":GATEWAY});
    if command == "connect" {
        request["port"] = json!("COM7");
    } else {
        request["moduleId"] = json!(MODULE);
    }
    request
}
fn transport(connected: Option<bool>) -> Transport {
    Transport {
        connected,
        configuration_claims: false,
        output_activity: false,
    }
}

#[test]
fn only_unapplied_recovery_state_can_receive_the_exception() {
    let request = request("connect");
    for state in [
        State::Stopped,
        State::Running,
        State::Degraded,
        State::Faulted,
        State::Transitioning,
        State::AwaitingGame,
    ] {
        let mut status = status();
        status.state = state;
        assert!(admit(&status, &request, None).is_err(), "{state:?}");
    }
    let mut status = status();
    assert!(admit(&status, &request, None).is_ok());
    status.owner = Some(crate::profile_session::RunOwner {
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        profile_id: uuid::Uuid::new_v4().to_string(),
        profile_revision: 1,
        game_id: None,
    });
    assert!(admit(&status, &request, None).is_err());
    status.owner = None;
    status.applied_snapshot = Some(osg_profiles::session::SnapshotSummary {
        profile_id: uuid::Uuid::new_v4().to_string(),
        revision: 1,
        semantic_hash: format!("sha256:{}", "a".repeat(64)),
    });
    assert!(admit(&status, &request, None).is_err());
}

#[test]
fn connection_requires_exact_stm32_selection_and_an_idle_disconnected_adapter() {
    let request = request("connect");
    assert!(admit(&status(), &request, None).is_ok());
    assert!(admit(&status(), &request, Some(transport(Some(false)))).is_ok());
    for connected in [None, Some(true)] {
        assert!(admit(&status(), &request, Some(transport(connected))).is_err());
    }
    for state in [
        Transport {
            configuration_claims: true,
            ..transport(Some(false))
        },
        Transport {
            output_activity: true,
            ..transport(Some(false))
        },
    ] {
        assert!(admit(&status(), &request, Some(state)).is_err());
    }
    for (key, value) in [
        ("gatewayId", json!("pico:OSG-PICO-0123456789abcdef")),
        ("gatewayId", json!("stm32:0123456789ABCDEF01234567")),
        ("gatewayId", Value::Null),
        ("backend", json!("pico")),
        ("port", Value::Null),
        ("port", json!("COM7\n")),
        ("ownershipToken", json!("not-authority")),
        ("_directUsbPreferred", json!(true)),
    ] {
        let mut invalid = request.clone();
        invalid[key] = value;
        assert!(admit(&status(), &invalid, None).is_err(), "{key}");
    }
}

#[test]
fn passive_action_reads_require_the_exact_connected_transport_and_bounded_module() {
    for command in ["action_describe", "action_state", "action_ownership"] {
        let request = request(command);
        assert!(admit(&status(), &request, Some(transport(Some(true)))).is_ok());
        assert!(admit(&status(), &request, None).is_err());
        assert!(admit(&status(), &request, Some(transport(Some(false)))).is_err());
        for (key, value) in [
            ("backend", json!("pico")),
            ("gatewayId", json!("pico:OSG-PICO-0123456789abcdef")),
            ("ownershipToken", json!("never-authority")),
        ] {
            let mut invalid = request.clone();
            invalid[key] = value;
            assert!(admit(&status(), &invalid, Some(transport(Some(true)))).is_err());
        }
        for value in [
            Value::Null,
            json!("wrong"),
            json!("0".repeat(32)),
            json!("f".repeat(32)),
        ] {
            let mut invalid = request.clone();
            invalid["moduleId"] = value;
            assert!(admit(&status(), &invalid, Some(transport(Some(true)))).is_err());
        }
    }
    let mut ownership_request = request("action_ownership");
    for offset in [json!(0), json!(16)] {
        ownership_request["offset"] = offset;
        assert!(admit(&status(), &ownership_request, Some(transport(Some(true)))).is_ok());
    }
    for offset in [json!(17), json!(-1), json!("0"), Value::Null] {
        ownership_request["offset"] = offset;
        assert!(admit(&status(), &ownership_request, Some(transport(Some(true)))).is_err());
    }
    let mut request = request("action_state");
    request["values"] = json!([true]);
    assert!(admit(&status(), &request, Some(transport(Some(true)))).is_err());
}

#[test]
fn neither_control_writes_nor_other_reads_gain_recovery_authority() {
    for command in [
        "claim",
        "release",
        "disconnect",
        "reconnect",
        "apply",
        "enable",
        "disable",
        "config_apply",
        "config_save",
        "config_discard",
        "action_claim",
        "action_claim_current",
        "action_renew",
        "action_release",
        "action_invoke",
        "action_invoke_owned",
        "debug_invoke",
        "profile_apply",
        "profile_recover",
        "events_read",
        "module_usb_connect",
    ] {
        assert!(operation(command).is_none(), "{command}");
        assert!(
            admit(&status(), &request(command), None).is_err(),
            "{command}"
        );
    }
}
