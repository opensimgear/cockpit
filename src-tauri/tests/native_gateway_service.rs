//! Exercise the actual browser service, rather than a mock bridge process.
#![cfg(debug_assertions)] // The browser service deliberately rejects release builds.

use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

include!("../../tests/fixtures/profiles/editor-boundary.rs");

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
    assert!(response["error"].as_str().unwrap().contains("retired"), "{response}");

    let mut request = json!({"command":"game_get_state","clientId":"cockpit-service-boundary-test","padding":""});
    let overhead = request.to_string().len();
    request["padding"] = json!("x".repeat(65_536 - overhead));
    assert_eq!(request.to_string().len(), 65_536);
    let at_limit = exchange("at-limit", request.clone());
    assert_eq!(at_limit["ok"], false);
    assert_ne!(at_limit["error"], "Invalid gateway request.", "The full payload reaches native schema validation: {at_limit}");

    request["padding"] = json!("x".repeat(65_537 - overhead));
    assert_eq!(request.to_string().len(), 65_537);
    let response = exchange("above-limit", request);
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"], "Invalid gateway request.");
    assert_eq!(
        exchange(
            "still-alive",
            json!({"command":"game_get_state","clientId":"cockpit-service-boundary-test"})
        )["ok"],
        true
    );

    let games = exchange(
        "games",
        json!({"command":"game_get_state","clientId":"cockpit-service-boundary-test"}),
    );
    assert_eq!(games["ok"], true);
    let catalogue = games["state"]["games"]["catalogue"].as_array().unwrap();
    let mut game_ids = catalogue.iter().map(|game| game["id"].as_str().unwrap()).collect::<Vec<_>>();
    game_ids.sort_unstable();
    assert_eq!(game_ids, [
        "assetto-corsa", "assetto-corsa-competizione", "assetto-corsa-evo",
        "assetto-corsa-rally", "dcs", "x-plane-11", "x-plane-12",
    ]);
    let game_state = games["state"]["games"].as_object().unwrap();
    for retired_field in ["profiles", "defaultProfiles", "session"] {
        assert!(!game_state.contains_key(retired_field), "{retired_field}");
    }
    assert!(game_state.contains_key("cleanup"));

    // CRUD uses the unified schema-v2 profile service. The transport retains
    // correlation for both supported commands and explicit retired rejections.
    let created = exchange(
        "create-profile",
        json!({"command":"profile_create","clientId":"cockpit-service-boundary-test","name":"Service test"}),
    );
    assert_eq!(created["ok"], true, "{created}");
    let profile = &created["state"]["profiles"]["document"];
    assert_eq!(profile["schemaVersion"], 2);
    let id = profile["id"].as_str().unwrap();
    let renamed = exchange(
        "rename-profile",
        json!({"command":"profile_rename","clientId":"cockpit-service-boundary-test",
            "id":id,"expectedRevision":1,"name":"Renamed service test"}),
    );
    assert_eq!(renamed["ok"], true, "{renamed}");
    assert_eq!(renamed["state"]["profiles"]["document"]["revision"], 2);
    let exported = exchange(
        "export-profile",
        json!({"command":"profile_export","clientId":"cockpit-service-boundary-test","id":id}),
    );
    assert_eq!(exported["ok"], true, "{exported}");
    assert_eq!(exported["state"]["profiles"]["document"]["name"], "Renamed service test");
    for command in ["game_profile_save", "mapping_import_begin", "integration_get_state"] {
        let rejected = exchange(
            command,
            json!({"command":command,"clientId":"cockpit-service-boundary-test"}),
        );
        assert_eq!(rejected["ok"], false, "{rejected}");
        assert!(rejected["error"].as_str().unwrap().contains("retired"), "{rejected}");
    }
    assert!(!scratch.join("games.json").exists());
    assert!(!scratch.join("mappings").exists());
    let mut editor = editor_boundary_request();
    editor["clientId"] = json!("cockpit-service-boundary-test");
    assert!(editor.to_string().len() > osg_cockpit::gateways::PROFILE_REQUEST_LIMIT);
    let saved_editor = exchange("large-editor-save", editor.clone());
    assert_eq!(saved_editor["ok"], true, "{}", saved_editor["error"]);
    assert_eq!(saved_editor["state"]["profiles"]["document"]["revision"], 1);
    assert_eq!(
        saved_editor["state"]["profiles"]["assignments"]
            .as_array()
            .unwrap()
            .len(),
        16
    );
    editor["command"] = json!("profile_import");
    assert_eq!(
        exchange("ordinary-profile-limit", editor)["error"],
        "Invalid gateway request."
    );
    drop(exchange);
    drop(input);
    assert!(service.wait().unwrap().success());
    fs::remove_dir_all(scratch).unwrap();
}
