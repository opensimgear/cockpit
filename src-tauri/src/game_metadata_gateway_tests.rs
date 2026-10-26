#[test]
fn discovery_apply_rechecks_captured_mcp_authorization_before_registry_commit() {
    let manager = GatewayManager::mcp_test_manager();
    let client = crate::mcp_tools::native_client("discovery-apply-revocation");
    manager.mcp_authorize_client(&client, true);
    let directory = crate::test_directory();
    let root = directory.path().join("Standalone Game");
    std::fs::create_dir_all(&root).unwrap();
    let executable = root.join("Standalone.exe");
    std::fs::write(&executable, b"fixture").unwrap();
    let ready = manager.games.stage_test_discovery(vec![crate::game_library::Candidate {
        id: "disk:standalone-fixture".into(),
        client: "disk".into(),
        name: "Standalone Game".into(),
        install_path: root.to_string_lossy().into_owned(),
        executable_candidates: vec![executable.to_string_lossy().into_owned()],
        preferred_executable: None,
        game_id: None,
        kind: "game".into(),
        app_id: "standalone-fixture".into(),
        launcher: None,
        artwork: None,
    }]);
    let scan_id = ready["scanId"].as_str().unwrap().to_owned();
    let entry = ready["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["library"] == "disk")
        .expect("the fixture candidate should be reconciled");
    let game_id = entry["gameId"].as_str().unwrap().to_owned();
    let request = json!({"command":"game_discovery_apply","clientId":client,"scanId":scan_id,
        "actions":[{"id":entry["id"],"selected":true}]});
    let captured = manager.capture_mcp_authorization(&client).unwrap();
    assert!(captured.is_some());
    manager.mcp_authorize_client(&client, false);
    let denied = manager.game_metadata_request(&request, captured.as_ref());
    assert_eq!(denied["ok"], false, "{denied}");
    assert!(denied["error"].as_str().unwrap().contains("authorization"));
    assert!(!manager.games.status()["catalogue"]
        .as_array()
        .unwrap()
        .iter()
        .any(|game| game["id"] == game_id));
    manager.mcp_authorize_client(&client, true);
    let accepted = manager.mcp_request(request);
    assert_eq!(accepted["ok"], true, "{accepted}");
    assert!(manager.games.status()["catalogue"]
        .as_array()
        .unwrap()
        .iter()
        .any(|game| game["id"] == game_id));
    manager.shutdown();
}

#[test]
fn metadata_gateway_accepts_bounded_high_resolution_preview_envelopes() {
    let manager = GatewayManager::mcp_test_manager();
    let client = crate::mcp_tools::native_client("large-background-preview");
    manager.mcp_authorize_client(&client, true);
    let request = json!({
        "command":"game_background_save",
        "clientId":client,
        "gameId":"game-fixture-123",
        "imageData":format!("data:image/jpeg;base64,{}", "A".repeat(90_000)),
        "crop":{"x":0.0,"y":0.0,"width":1.0,"height":1.0}
    });
    assert!(request.to_string().len() > 65_536);
    assert!(request.to_string().len() <= crate::gateways::GAME_METADATA_REQUEST_LIMIT);
    let response = manager.mcp_request(request);
    assert_eq!(response["ok"], false, "unknown game should still be rejected");
    assert_eq!(response["error"], "Choose an existing game background.", "{response}");
    manager.shutdown();
}

#[test]
fn artwork_gateway_rejects_invalid_envelopes_without_acquiring_control() {
    let manager = Arc::new(GatewayManager::mcp_test_manager());
    let client = crate::mcp_tools::native_client("artwork-inspection");
    manager.mcp_authorize_client(&client, true);
    let epoch = manager.profile_admission_epoch.load(Ordering::Acquire);
    let game = manager.game_admission.write().unwrap();
    let profile = manager.profile_admission.write().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let reading = manager.clone();
    let reader = std::thread::spawn(move || {
        let responses = [
            reading.mcp_request(json!({"command":"game_artwork_list","clientId":client,"gameId":"missing-game"})),
            reading.request(json!({"command":"game_artwork_list","gameId":"missing-game"})),
            reading.mcp_request(json!({"command":"game_artwork_preview","clientId":client,"gameId":"missing-game","artworkId":"x".repeat(9000)})),
        ];
        sender.send(responses).unwrap();
    });
    let responses = receiver.recv_timeout(Duration::from_secs(2));
    drop(game);drop(profile);reader.join().unwrap();
    let [missing, anonymous, oversized] = responses.expect("Artwork inspection must bypass control admission");
    assert_eq!(missing["error"], "Choose a discovered game.");
    assert_eq!(anonymous["error"], "Invalid game artwork request.");
    assert_eq!(oversized["error"], "Invalid game artwork request.");
    assert_eq!(manager.profile_admission_epoch.load(Ordering::Acquire), epoch);
    manager.shutdown();
}

#[test]
fn game_get_state_bypasses_manager_admission_and_keeps_envelope_validation() {
    struct CachedCatalogue;
    impl crate::game_discovery::Platform for CachedCatalogue {
        fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
            vec![]
        }
        fn validate(&self, _: &str, _: &str) -> Result<(), String> {
            panic!("status cannot validate installations")
        }
        fn launch(&self, _: &Value) -> Result<crate::game_discovery::ProcessIdentity, String> {
            panic!("status cannot launch")
        }
        fn find(
            &self,
            _: &Value,
        ) -> Result<Option<crate::game_discovery::ProcessIdentity>, String> {
            Ok(None)
        }
        fn alive(&self, _: &crate::game_discovery::ProcessIdentity) -> Result<bool, String> {
            Ok(false)
        }
    }
    let directory = crate::test_directory();
    let (mut manager, provider) = setup();
    Arc::get_mut(&mut manager).unwrap().games = Arc::new(crate::games::GamesRuntime::test_runtime(
        Arc::new(|_| panic!("status cannot address hardware")),
        crate::games::Paths::in_directory(directory.path()),
        Arc::new(CachedCatalogue),
    ));
    let profile = manager.profile_admission.write().unwrap();
    let game = manager.game_admission.write().unwrap();
    let epoch = manager.profile_admission_epoch.load(Ordering::Acquire);
    let reading = manager.clone();
    let (completed, responses) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        completed
            .send([
                request(&reading, "game_get_state", None, json!({})),
                request(
                    &reading,
                    "game_get_state",
                    None,
                    json!({"profileId":"forged"}),
                ),
                reading.request(json!({"command":"game_get_state"})),
                request(
                    &reading,
                    "game_get_state",
                    None,
                    json!({"extra":"x".repeat(65_536)}),
                ),
            ])
            .unwrap();
    });
    let response = responses.recv_timeout(Duration::from_secs(2));
    drop(game);
    drop(profile);
    reader.join().unwrap();
    let [valid, unknown, anonymous, oversized] =
        response.expect("Passive game state must bypass manager admission");
    assert_eq!(valid["ok"], true, "{valid}");
    assert!(valid["state"]["games"]["catalogue"].is_array());
    for rejected in [unknown, anonymous, oversized] {
        assert_eq!(rejected["ok"], false, "{rejected}");
    }
    assert_eq!(
        manager.profile_admission_epoch.load(Ordering::Acquire),
        epoch
    );
    assert!(provider.adapters.lock().unwrap().is_empty());
    manager.shutdown();
    assert_eq!(
        request(&manager, "game_get_state", None, json!({}))["ok"],
        false
    );
}
