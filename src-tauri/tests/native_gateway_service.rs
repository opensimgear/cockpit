//! Exercise the actual browser service, rather than a mock bridge process.
#![cfg(debug_assertions)] // The browser service deliberately rejects release builds.

use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

#[test]
fn correlated_requests_reach_the_full_payload_limit_without_losing_the_service() {
    let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".research")
        .join(format!("native-service-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&scratch).unwrap();
    let mut service = Command::new(env!("CARGO_BIN_EXE_osg-gateway-service"))
        .env("OSG_LOCAL_GATEWAYS_ONLY", "1")
        .env("OSG_DCS_BIOS_CONFIG", scratch.join("dcs.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = service.stdin.take().unwrap();
    let mut output = BufReader::new(service.stdout.take().unwrap());
    let mut exchange = |id: &str, request: Value| {
        writeln!(input, "{}", json!({"requestId":id,"request":request})).unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        assert!(output.read_line(&mut line).unwrap() > 0);
        let envelope: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(envelope["requestId"], id, "Response retains correlation");
        envelope["response"].clone()
    };

    // Full bounded pages are legal synthetic provider input. Without an active
    // run, the runtime rejects the operation after parsing the telemetry; the
    // transport must not mistake it for an oversized or uncorrelated request.
    let mut graphics = vec![0u8; 4096];
    graphics[4] = 2;
    let request = json!({"command":"integration_simulate","clientId":"cockpit-service-boundary-test",
        "telemetryFrame":{"sourceId":"synthetic","sourceEpoch":1,
        "physics":vec![0u8;4096],"graphics":graphics,"staticPage":vec![0u8;4096]}});
    assert!(request.to_string().len() > 17_408);
    assert!(request.to_string().len() < 65_536);
    let response = exchange("large-frame", request);
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"], "Start a synthetic Rally profile first.");

    let mut request = json!({"command":"integration_get_state","clientId":"cockpit-service-boundary-test","padding":""});
    let overhead = request.to_string().len();
    request["padding"] = json!("x".repeat(65_536 - overhead));
    assert_eq!(request.to_string().len(), 65_536);
    assert_eq!(exchange("at-limit", request.clone())["ok"], true);

    request["padding"] = json!("x".repeat(65_537 - overhead));
    assert_eq!(request.to_string().len(), 65_537);
    let response = exchange("above-limit", request);
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"], "Invalid gateway request.");
    assert_eq!(
        exchange(
            "still-alive",
            json!({"command":"integration_get_state","clientId":"cockpit-service-boundary-test"})
        )["ok"],
        true
    );

    drop(exchange);
    drop(input);
    assert!(service.wait().unwrap().success());
    fs::remove_dir_all(scratch).unwrap();
}
