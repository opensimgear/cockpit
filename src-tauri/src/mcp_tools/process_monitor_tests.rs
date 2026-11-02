use super::*;

#[derive(Default)]
struct MonitorBackend {
    requests: Mutex<Vec<Value>>,
    guarded_requests: Mutex<Vec<(Value, Value)>>,
}

impl Backend for MonitorBackend {
    fn request(&self, request: Value) -> Value {
        self.requests.lock().unwrap().push(request.clone());
        json!({"ok":true,"state":{"games":{"monitor":{"gameId":request["gameId"],"command":request["command"]}}}})
    }

    fn guarded(&self, request: Value, owner: Value) -> Value {
        self.guarded_requests.lock().unwrap().push((request, owner));
        json!({"ok":true,"state":{"guarded":true}})
    }
}

fn tools() -> (Tools, Arc<MonitorBackend>) {
    let backend = Arc::new(MonitorBackend::default());
    (
        Tools {
            backend: backend.clone(),
            ledger: Mutex::new(Ledger::default()),
            drafts: Mutex::new(vec![]),
            workspace_uploads: Mutex::new(vec![]),
            integration: OnceLock::new(),
        },
        backend,
    )
}

fn operation() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[test]
fn process_monitor_tasks_validate_and_forward_only_the_typed_game_identity() {
    let (tools, backend) = tools();
    let client = "authenticated-monitor-client";
    for command in ["game_process_monitor_enable", "game_process_monitor_cancel"] {
        let schema = catalogue()
            .into_iter()
            .find(|tool| tool["name"] == command)
            .expect("monitor task is discoverable");
        assert_eq!(schema["inputSchema"]["additionalProperties"], false);
        assert!(is_mutation(command));
        assert!(schema["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("operationId")));

        let game_id = "x-plane-12";
        let args = json!({"gameId":game_id,"operationId":operation()});
        let result = tools.call(command, args.clone(), client);
        assert_eq!(result["ok"], true, "{result}");
        assert_eq!(result["data"]["monitor"]["gameId"], game_id);
        assert_eq!(result["data"]["monitor"]["command"], command);
        assert_eq!(tools.call(command, args, client), result);
    }

    let requests = backend.requests.lock().unwrap();
    assert_eq!(requests.len(), 2, "retries replay the retained result");
    for request in requests.iter() {
        assert_eq!(request.as_object().unwrap().len(), 3);
        assert_eq!(
            request["clientId"],
            native_client("authenticated-monitor-client")
        );
        assert!(matches!(
            request["command"].as_str(),
            Some("game_process_monitor_enable" | "game_process_monitor_cancel")
        ));
        assert_eq!(request["gameId"], "x-plane-12");
    }
}

#[test]
fn process_monitor_arguments_fail_closed_before_native_dispatch() {
    let (tools, backend) = tools();
    let client = "authenticated-monitor-client";
    for args in [
        json!({"operationId":operation()}),
        json!({"gameId":"X-Plane 12","operationId":operation()}),
        json!({"gameId":"x-plane-12","operationId":operation(),"pid":42}),
        json!({"gameId":"x-plane-12"}),
    ] {
        let result = tools.call("game_process_monitor_enable", args, client);
        assert_eq!(result["ok"], false, "{result}");
        assert_eq!(result["error"]["code"], "invalid_arguments");
    }
    assert!(backend.requests.lock().unwrap().is_empty());
}

#[test]
fn native_error_text_maps_to_stable_mcp_failure_codes() {
    for (message, code) in [
        ("Windows authorization denied", "access_denied"),
        ("companion panel references changed", "resource_changed"),
        ("revision changed", "revision_conflict"),
        ("another owner owns this resource", "ownership_conflict"),
        ("request expired", "expired"),
        ("resource is read-only", "read_only"),
        ("operation cancelled", "cancelled"),
        ("game not found", "not_found"),
        ("native rejected the request", "native_rejected"),
    ] {
        let result = native_result(json!({"ok":false,"error":message}), Some("games"))
            .expect_err("native error should be translated");
        assert_eq!(result["error"]["code"], code, "{message}");
        assert_eq!(result["error"]["message"], message);
        assert_eq!(result["error"]["retryable"], false);
    }
}

#[test]
fn native_result_selects_requested_section_or_falls_back_to_the_state() {
    let response = json!({"ok":true,"state":{"games":{"status":"observed"},"other":7}});
    assert_eq!(
        native_result(response.clone(), Some("games")).unwrap(),
        json!({"status":"observed"})
    );
    assert_eq!(
        native_result(response.clone(), Some("missing")).unwrap(),
        response["state"]
    );
    assert_eq!(
        native_result(response.clone(), None).unwrap(),
        response["state"]
    );
}

#[test]
fn gateway_and_usb_setup_tasks_validate_config_and_keep_owner_guards_separate() {
    let (tools, backend) = tools();
    let client = "authenticated-monitor-client";
    let config = json!({
        "name":"Configured module",
        "expanderMask":0,
        "buttons":[],"axes":[],"hats":[],"encoders":[],"selectors":[]
    });
    let gateway = "stm32:0123456789abcdef01234567";
    let route = json!({"boot":7,"route":"current"});

    let read = tools.call(
        "gateway_config_read",
        json!({"gatewayId":gateway,"moduleId":"controller"}),
        client,
    );
    assert_eq!(read["ok"], true, "{read}");
    assert_eq!(
        backend.requests.lock().unwrap()[0],
        json!({"command":"config_read","gatewayId":gateway,"moduleId":"controller","clientId":native_client(client)})
    );

    for (action, supplied_config, expected_command) in [
        ("claim", None, "claim"),
        ("release", None, "release"),
        ("save", None, "config_save"),
        ("discard", None, "config_discard"),
        ("reset", None, "config_reset"),
        ("apply", Some(config.clone()), "config_apply"),
        ("apply_save", Some(config.clone()), "config_apply_save"),
    ] {
        let mut args = json!({"gatewayId":gateway,"moduleId":"controller","action":action,"expectedActiveOwner":null,"operationId":operation()});
        if let Some(config) = supplied_config {
            args["config"] = config;
        }
        let result = tools.call("gateway_config_change", args, client);
        assert_eq!(result["ok"], true, "{action}: {result}");
        let calls = backend.guarded_requests.lock().unwrap();
        let (request, owner) = calls.last().unwrap();
        assert_eq!(request["command"], expected_command);
        assert_eq!(*owner, Value::Null);
        assert_eq!(request.get("operationId"), None);
    }

    let invalid_apply = tools.call(
        "gateway_config_change",
        json!({"gatewayId":gateway,"moduleId":"controller","action":"apply","expectedActiveOwner":null,"operationId":operation()}),
        client,
    );
    assert_eq!(invalid_apply["error"]["code"], "invalid_arguments");
    let invalid_save = tools.call(
        "gateway_config_change",
        json!({"gatewayId":gateway,"moduleId":"controller","action":"save","config":config,"expectedActiveOwner":null,"operationId":operation()}),
        client,
    );
    assert_eq!(invalid_save["error"]["code"], "invalid_arguments");

    for (action, port, expected_command) in [
        ("connect", Some("COM3"), "module_usb_connect"),
        ("disconnect", None, "module_usb_disconnect"),
    ] {
        let mut args = json!({"key":"usb-module-1","action":action,"expectedActiveOwner":null,"operationId":operation()});
        if let Some(port) = port {
            args["port"] = json!(port);
        }
        let result = tools.call("usb_module_session", args, client);
        assert_eq!(result["ok"], true, "{action}: {result}");
        let calls = backend.guarded_requests.lock().unwrap();
        let (request, owner) = calls.last().unwrap();
        assert_eq!(request["command"], expected_command);
        assert_eq!(*owner, Value::Null);
    }
    let invalid_connect = tools.call(
        "usb_module_session",
        json!({"key":"usb-module-1","action":"connect","expectedActiveOwner":null,"operationId":operation()}),
        client,
    );
    assert_eq!(invalid_connect["error"]["code"], "invalid_arguments");
    let invalid_disconnect = tools.call(
        "usb_module_session",
        json!({"key":"usb-module-1","port":"COM3","action":"disconnect","expectedActiveOwner":null,"operationId":operation()}),
        client,
    );
    assert_eq!(invalid_disconnect["error"]["code"], "invalid_arguments");

    for (action, supplied_config, expected_command, expected_save) in [
        ("claim", None, "module_usb_claim", None),
        (
            "apply",
            Some(config.clone()),
            "module_usb_config_apply",
            Some(false),
        ),
        (
            "apply_save",
            Some(config.clone()),
            "module_usb_config_apply",
            Some(true),
        ),
        ("save", None, "module_usb_config_save", None),
    ] {
        let mut args = json!({"key":"usb-module-1","route":route,"action":action,"expectedActiveOwner":null,"operationId":operation()});
        if let Some(config) = supplied_config {
            args["config"] = config;
        }
        let result = tools.call("usb_module_config_change", args, client);
        assert_eq!(result["ok"], true, "{action}: {result}");
        let calls = backend.guarded_requests.lock().unwrap();
        let (request, owner) = calls.last().unwrap();
        assert_eq!(request["command"], expected_command);
        assert_eq!(request["key"], "usb-module-1");
        assert_eq!(request["route"], route);
        assert_eq!(*owner, Value::Null);
        if let Some(expected_save) = expected_save {
            assert_eq!(request["save"], expected_save);
        }
    }
    let invalid_usb_apply = tools.call(
        "usb_module_config_change",
        json!({"key":"usb-module-1","route":route,"action":"apply","expectedActiveOwner":null,"operationId":operation()}),
        client,
    );
    assert_eq!(invalid_usb_apply["error"]["code"], "invalid_arguments");
    let invalid_usb_claim = tools.call(
        "usb_module_config_change",
        json!({"key":"usb-module-1","route":route,"action":"claim","config":config,"expectedActiveOwner":null,"operationId":operation()}),
        client,
    );
    assert_eq!(invalid_usb_claim["error"]["code"], "invalid_arguments");
    assert_eq!(backend.guarded_requests.lock().unwrap().len(), 13);
}
