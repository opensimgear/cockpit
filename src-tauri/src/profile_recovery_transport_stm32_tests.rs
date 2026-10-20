use super::*;

#[test]
fn cold_recovery_connection_uses_only_the_production_snapshot_handshake() {
    let (mut gateway, context) = harness();
    let recovery = crate::profile_session::Status {
        state: crate::profile_session::State::RecoveryNeeded,
        applied_snapshot: None,
        owner: None,
        degradation: vec![],
        fault: Some("Interrupted profile".into()),
    };
    let request = json!({"command":"connect","clientId":CLIENT,
        "gatewayId":format!("stm32:{UID}"),"port":PATH});
    crate::gateways::recovery_transport::admit(&recovery, &request, None).unwrap();
    let reply = gateway.request(request);
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["state"]["gatewaySerial"], UID);
    assert_eq!(reply["state"]["connected"], true);
    let state = context.lock().unwrap();
    assert_eq!(state.opened.len(), 1);
    assert_eq!(state.writes.len(), 1);
    assert!(state.writes[0].ends_with(" snapshot\n"));
    assert_eq!(state.lines, vec![(true, false)]);
    assert!(gateway.claims.is_empty());
    assert!(gateway.actions.is_empty());
    drop(state);

    // The same connection cannot be opened again through the cleanup exception.
    let transport = crate::gateways::recovery_transport::Transport {
        connected: Some(true),
        configuration_claims: false,
        output_activity: false,
    };
    let request = json!({"command":"connect","clientId":CLIENT,
        "gatewayId":format!("stm32:{UID}"),"port":PATH});
    assert!(
        crate::gateways::recovery_transport::admit(&recovery, &request, Some(transport)).is_err()
    );
    assert_eq!(context.lock().unwrap().writes.len(), 1);
}
