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

    // A full bounded payload reaches the native context guard. Standalone
    // simulation is retired; this must be a context rejection rather than an
    // oversized or uncorrelated transport request.
    let mut graphics = vec![0u8; 4096];
    graphics[4] = 2;
    let request = json!({"command":"integration_simulate","clientId":"cockpit-service-boundary-test",
        "telemetryFrame":{"sourceId":"synthetic","sourceEpoch":1,
        "physics":vec![0u8;4096],"graphics":graphics,"staticPage":vec![0u8;4096]}});
    assert!(request.to_string().len() > 17_408);
    assert!(request.to_string().len() < 65_536);
    let response = exchange("large-frame", request);
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"], "Provider setup belongs to game details, and simulator bindings belong to the unified profile graph. Integration execution requires its applied profile and matching verified game context.");

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

    let games = exchange(
        "games",
        json!({"command":"game_get_state","clientId":"cockpit-service-boundary-test"}),
    );
    assert_eq!(games["ok"], true);
    let catalogue = games["state"]["games"]["catalogue"].as_array().unwrap();
    assert_eq!(catalogue.len(), 4);
    assert!(catalogue.iter().any(|g| g["id"] == "dcs"));
    assert!(catalogue.iter().any(|g| g["id"] == "assetto-corsa-rally"));
    assert!(catalogue.iter().any(|g| g["id"] == "x-plane-11"));
    assert!(catalogue.iter().any(|g| g["id"] == "x-plane-12"));
    let profile = json!({"schemaVersion":1,"id":"service-dcs","gameId":"dcs","name":"Service test","devices":[]});
    assert_eq!(
        exchange(
            "save-game",
            json!({"command":"game_profile_save","clientId":"cockpit-service-boundary-test","gameProfile":profile})
        )["ok"],
        true
    );
    let saved = exchange(
        "read-game",
        json!({"command":"game_get_state","clientId":"cockpit-service-boundary-test"}),
    );
    assert!(saved["state"]["games"]["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == "service-dcs"));

    // A maximum document uses bounded byte chunks, retaining native correlation.
    let mut document = include_bytes!("../../crates/mapping/fixtures/direct-profile.json").to_vec();
    document.resize(65_536, b' ');
    let begin = exchange(
        "mapping-begin",
        json!({"command":"mapping_import_begin",
        "clientId":"cockpit-service-boundary-test","size":document.len()}),
    );
    assert_eq!(begin["ok"], true);
    let upload = begin["state"]["mappings"]["uploadId"].clone();
    for (index, chunk) in document.chunks(8192).enumerate() {
        let response = exchange(
            &format!("mapping-chunk-{index}"),
            json!({
                "command":"mapping_import_chunk","clientId":"cockpit-service-boundary-test",
                "uploadId":upload,"offset":index * 8192,"bytes":chunk
            }),
        );
        assert_eq!(response["ok"], true);
    }
    let committed = exchange(
        "mapping-commit",
        json!({"command":"mapping_import_commit",
        "clientId":"cockpit-service-boundary-test","uploadId":upload}),
    );
    assert_eq!(committed["ok"], true, "{committed}");
    let profile = &committed["state"]["mappings"]["document"];
    assert!(profile.is_object());
    assert!(scratch
        .join("mappings")
        .join(format!("{}.json", profile["id"].as_str().unwrap()))
        .exists());
    assert!(committed.to_string().len() < 66_560);
    assert_eq!(
        exchange(
            "mapping-oversize",
            json!({"command":"mapping_import_begin",
        "clientId":"cockpit-service-boundary-test","size":65_537})
        )["ok"],
        false
    );
    drop(exchange);
    drop(input);
    assert!(service.wait().unwrap().success());
    fs::remove_dir_all(scratch).unwrap();
}
