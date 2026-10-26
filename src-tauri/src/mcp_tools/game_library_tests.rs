use super::*;
use std::sync::Mutex;

#[derive(Default)]
struct Fixture {
    requests: Mutex<Vec<Value>>,
    response: Mutex<Value>,
}
impl Backend for Fixture {
    fn request(&self, request: Value) -> Value {
        self.requests.lock().unwrap().push(request);
        self.response.lock().unwrap().clone()
    }
    fn guarded(&self, _: Value, _: Value) -> Value {
        panic!("Game catalogue tools use the native metadata request")
    }
}

fn fixture(response: Value) -> (Tools, Arc<Fixture>) {
    let backend = Arc::new(Fixture {
        requests: Mutex::new(vec![]),
        response: Mutex::new(response),
    });
    (
        Tools {
            backend: backend.clone(),
            ledger: Mutex::new(Default::default()),
            drafts: Mutex::new(vec![]),
            workspace_uploads: Mutex::new(vec![]),
            integration: OnceLock::new(),
        },
        backend,
    )
}

#[test]
fn discovery_start_uses_async_native_route_and_keeps_operation_id_out_of_request() {
    let (tools, backend) = fixture(
        json!({"ok":true,"state":{"games":{"discovery":{"scanId":"scan-1","status":"scanning","entries":[],"libraries":[],"diagnostics":[]}}}}),
    );
    let args = json!({"operationId":"a3ed4994-43c5-4ac3-b23b-a6ee03d51363","paths":["C:/Games"]});
    let result = tools.call("game_discovery_start", args, "writer");
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["data"]["scanId"], "scan-1");
    assert_eq!(
        result["operationId"],
        "a3ed4994-43c5-4ac3-b23b-a6ee03d51363"
    );
    let request = &backend.requests.lock().unwrap()[0];
    assert_eq!(request["command"], "game_discovery_start");
    assert_eq!(request["clientId"], native_client("writer"));
    assert_eq!(request["paths"][0], "C:/Games");
    assert!(request.get("operationId").is_none());
}

#[test]
fn status_reads_exact_scan_and_pages_discovery_entries() {
    let entries = (0..70)
        .map(|index| json!({"id":format!("entry-{index}"),"selected":true,"image":"data:image/jpeg;base64,AAAA"}))
        .collect::<Vec<_>>();
    let (tools, backend) = fixture(
        json!({"ok":true,"state":{"games":{"discovery":{"scanId":"scan-1","status":"complete","libraries":[{"id":"steam","status":"complete"}],"entries":entries,"diagnostics":["ok"]}}}}),
    );
    let result = tools
        .game_discovery_status(&json!({"scanId":"scan-1","offset":32,"limit":16}), "reader")
        .unwrap();
    assert_eq!(result["total"], 70);
    assert_eq!(result["offset"], 32);
    assert_eq!(result["nextOffset"], 48);
    assert_eq!(result["entries"][0]["id"], "entry-32");
    assert_eq!(result["entries"][0]["hasArtwork"], true);
    assert!(result["entries"][0].get("image").is_none());
    assert_eq!(result["libraries"][0]["id"], "steam");
    assert_eq!(
        backend.requests.lock().unwrap()[0]["command"],
        "game_get_state"
    );
}

#[test]
fn status_rejects_a_stale_scan_and_apply_routes_typed_actions() {
    let (tools, backend) = fixture(
        json!({"ok":true,"state":{"games":{"discovery":{"scanId":"new-scan","status":"complete","entries":[],"libraries":[],"diagnostics":[]} ,"catalogue":[]}}}),
    );
    let error = tools
        .game_discovery_status(&json!({"scanId":"old-scan"}), "reader")
        .unwrap_err();
    assert_eq!(error["error"]["code"], "stale_discovery");
    let args = json!({"scanId":"new-scan","actions":[{"id":"steam:4","selected":true,"gameId":"generic-game","executable":null}]});
    let applied = tools.game_discovery_apply(&args, "writer").unwrap();
    assert_eq!(applied["scanId"], "new-scan");
    assert_eq!(applied["status"], "complete");
    assert_eq!(applied["gameCount"], 0);
    // Verify the native route and field projection.
    let request = backend.requests.lock().unwrap().last().unwrap().clone();
    assert_eq!(request["command"], "game_discovery_apply");
    assert_eq!(request["actions"][0]["gameId"], "generic-game");
}

#[test]
fn stale_apply_error_is_propagated_without_rewriting_native_admission() {
    let (tools, backend) = fixture(json!({
        "ok": false,
        "error": "Discovery scan is no longer current."
    }));
    let error = tools
        .game_discovery_apply(
            &json!({"scanId":"old-scan","actions":[{"id":"candidate-1","selected":true}]}),
            "writer",
        )
        .unwrap_err();
    assert_eq!(error["error"]["code"], "expired");
    assert_eq!(
        backend.requests.lock().unwrap()[0]["command"],
        "game_discovery_apply"
    );
}

#[test]
fn catalogue_exposes_the_discovery_and_metadata_surface_with_mutation_replay() {
    let definitions = catalogue();
    for name in [
        "game_discovery_start",
        "game_discovery_status",
        "game_discovery_apply",
        "game_save",
        "game_remove",
        "game_installation_select",
    ] {
        assert!(
            definitions.iter().any(|tool| tool["name"] == name),
            "missing {name}"
        );
    }
    assert!(!definitions
        .iter()
        .any(|tool| tool["name"] == "game_library_scan" || tool["name"] == "game_library_import"));
    for name in [
        "game_discovery_start",
        "game_discovery_apply",
        "game_save",
        "game_remove",
        "game_installation_select",
    ] {
        let tool = definitions
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], false);
        assert_eq!(tool["annotations"]["idempotentHint"], true);
        assert!(tool["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("operationId")));
        assert!(is_mutation(name));
    }
    assert!(!is_mutation("game_discovery_status"));
    let (tools, backend) = fixture(
        json!({"ok":true,"state":{"games":{"discovery":{"scanId":"scan-1","status":"scanning","entries":[],"libraries":[],"diagnostics":[]}}}}),
    );
    let args = json!({"operationId":"a3ed4994-43c5-4ac3-b23b-a6ee03d51363"});
    let first = tools.call("game_discovery_start", args.clone(), "writer");
    assert_eq!(tools.call("game_discovery_start", args, "writer"), first);
    assert_eq!(backend.requests.lock().unwrap().len(), 1);
}

#[test]
fn metadata_mutations_forward_only_declared_fields() {
    let (tools, backend) = fixture(json!({"ok":true,"state":{"games":{"catalogue":[]}}}));
    tools
        .game_save(
            &json!({"gameId":"dcs","name":"DCS","unexpected":true}),
            "writer",
        )
        .unwrap();
    let request = &backend.requests.lock().unwrap()[0];
    assert_eq!(request["command"], "game_save");
    assert!(request.get("unexpected").is_none());
}

#[test]
fn metadata_select_and_remove_project_fields_and_replay_operations() {
    let (tools, backend) = fixture(json!({
        "ok":true,
        "state":{"games":{"catalogue":[{
            "id":"game-fixture","name":"Fixture Flight","selectedInstallationId":"install-b",
            "image":"data:image/jpeg;base64,AAAA"
        }]}}
    }));
    let select = json!({
        "operationId":"8a95989e-6eeb-47f6-b927-20af3a20e659",
        "gameId":"game-fixture",
        "installationId":"install-b"
    });
    let selected = tools.call("game_installation_select", select.clone(), "writer");
    assert_eq!(selected["ok"], true, "{selected}");
    assert_eq!(selected["data"]["gameId"], "game-fixture");
    assert_eq!(selected["data"]["name"], "Fixture Flight");
    assert_eq!(selected["data"]["selectedInstallationId"], "install-b");
    assert_eq!(selected["data"]["removed"], false);
    assert!(!selected.to_string().contains("data:image"));
    assert_eq!(
        tools.call("game_installation_select", select, "writer"),
        selected
    );

    let remove = json!({
        "operationId":"504a11d3-d6f7-44ed-a4f0-fb25cf8d1c33",
        "gameId":"game-fixture"
    });
    *backend.response.lock().unwrap() = json!({"ok":true,"state":{"games":{"catalogue":[]}}});
    let removed = tools.call("game_remove", remove.clone(), "writer");
    assert_eq!(removed["ok"], true, "{removed}");
    assert_eq!(removed["data"]["gameId"], "game-fixture");
    assert_eq!(removed["data"]["name"], Value::Null);
    assert_eq!(removed["data"]["selectedInstallationId"], Value::Null);
    assert_eq!(removed["data"]["removed"], true);
    assert_eq!(tools.call("game_remove", remove, "writer"), removed);

    let requests = backend.requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "Each distinct operation is dispatched once"
    );
    assert_eq!(requests[0]["command"], "game_installation_select");
    assert_eq!(requests[0]["gameId"], "game-fixture");
    assert_eq!(requests[0]["installationId"], "install-b");
    assert_eq!(requests[0]["clientId"], native_client("writer"));
    assert!(requests[0].get("operationId").is_none());
    assert_eq!(requests[1]["command"], "game_remove");
    assert_eq!(requests[1]["gameId"], "game-fixture");
    assert_eq!(requests[1]["clientId"], native_client("writer"));
    assert!(requests[1].get("operationId").is_none());
}

#[test]
fn malformed_metadata_arguments_do_not_reach_native_backend() {
    let (tools, backend) = fixture(json!({"ok":true,"state":{"games":{}}}));
    let invalid_game = tools.call(
        "game_remove",
        json!({
            "operationId":"504a11d3-d6f7-44ed-a4f0-fb25cf8d1c33",
            "gameId":"Bad Game"
        }),
        "writer",
    );
    assert_eq!(invalid_game["error"]["code"], "invalid_arguments");
    let missing_installation = tools.call(
        "game_installation_select",
        json!({
            "operationId":"8a95989e-6eeb-47f6-b927-20af3a20e659",
            "gameId":"game-fixture"
        }),
        "writer",
    );
    assert_eq!(missing_installation["error"]["code"], "invalid_arguments");
    assert!(backend.requests.lock().unwrap().is_empty());
}

#[test]
fn metadata_mutation_preserves_native_failure_and_replays_it() {
    let (tools, backend) = fixture(json!({
        "ok":false,
        "error":"The game metadata database is corrupt."
    }));
    let args = json!({
        "operationId":"504a11d3-d6f7-44ed-a4f0-fb25cf8d1c33",
        "gameId":"game-fixture"
    });
    let failed = tools.call("game_remove", args.clone(), "writer");
    assert_eq!(failed["ok"], false);
    assert_eq!(failed["error"]["code"], "native_rejected");
    assert_eq!(
        failed["error"]["message"],
        "The game metadata database is corrupt."
    );
    assert_eq!(failed["operationId"], args["operationId"]);
    assert_eq!(tools.call("game_remove", args, "writer"), failed);
    assert_eq!(backend.requests.lock().unwrap().len(), 1);
}

#[test]
fn status_reports_null_end_offset_and_rejects_missing_or_malformed_snapshots() {
    let (tools, _) = fixture(json!({
        "ok":true,
        "state":{"games":{"discovery":{
            "scanId":"scan-1","status":"complete","entries":[{"id":"entry-1"}],
            "libraries":[],"diagnostics":[]
        }}}
    }));
    let last = tools.call(
        "game_discovery_status",
        json!({"scanId":"scan-1","offset":1,"limit":8}),
        "reader",
    );
    assert_eq!(last["ok"], true, "{last}");
    assert_eq!(last["data"]["entries"], json!([]));
    assert_eq!(last["data"]["total"], 1);
    assert_eq!(last["data"]["nextOffset"], Value::Null);

    let (tools, backend) = fixture(json!({"ok":true,"state":{"games":{}}}));
    let missing = tools
        .game_discovery_status(&json!({}), "reader")
        .unwrap_err();
    assert_eq!(missing["error"]["code"], "invalid_arguments");
    let missing_snapshot = tools
        .game_discovery_status(&json!({"scanId":"scan-1"}), "reader")
        .unwrap_err();
    assert_eq!(missing_snapshot["error"]["code"], "invalid_native_result");
    assert_eq!(backend.requests.lock().unwrap().len(), 1);

    let (tools, _) = fixture(json!({
        "ok":true,
        "state":{"games":{"discovery":{"scanId":"scan-1","entries":"invalid"}}}
    }));
    let malformed_entries = tools
        .game_discovery_status(&json!({"scanId":"scan-1"}), "reader")
        .unwrap_err();
    assert_eq!(malformed_entries["error"]["code"], "invalid_native_result");
}

#[test]
fn discovery_start_rejects_a_successful_native_reply_without_snapshot() {
    let (tools, backend) = fixture(json!({"ok":true,"state":{"games":{}}}));
    let result = tools.call(
        "game_discovery_start",
        json!({"operationId":"be9eeb50-786f-41d7-9685-2404998229a3"}),
        "writer",
    );
    assert_eq!(result["ok"], false);
    assert_eq!(result["error"]["code"], "invalid_native_result");
    assert_eq!(
        result["operationId"],
        "be9eeb50-786f-41d7-9685-2404998229a3"
    );
    assert_eq!(backend.requests.lock().unwrap().len(), 1);
}
