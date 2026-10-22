#[test]
fn game_metadata_commit_rechecks_captured_mcp_authorization() {
    let manager = GatewayManager::mcp_test_manager();
    let client = crate::mcp_tools::native_client("custom-game-metadata-revocation");
    manager.mcp_authorize_client(&client, true);
    let captured = manager.capture_mcp_authorization(&client).unwrap();
    assert!(captured.is_some());
    let request = json!({"command":"game_custom_save","clientId":client,
        "name":"Revoked registration","executable":null});
    manager.mcp_authorize_client(&client, false);
    let denied = manager.game_metadata_request(&request, captured.as_ref());
    assert_eq!(denied["ok"], false, "{denied}");
    assert!(denied["error"].as_str().unwrap().contains("authorization"));
    assert!(!manager.games.status()["catalogue"].as_array().unwrap().iter()
        .any(|game| game["name"] == "Revoked registration"));
    manager.mcp_authorize_client(&client, true);
    let accepted = manager.mcp_request(request);
    assert_eq!(accepted["ok"], true, "{accepted}");
    assert!(accepted["state"]["games"]["addedGameId"].as_str().unwrap().starts_with("custom-"));
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
    Arc::get_mut(&mut manager).unwrap().games = Arc::new(crate::games::GamesRuntime::new(
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

#[test]
fn slow_game_metadata_staging_keeps_profile_stop_responsive() {
    struct SlowCatalogue {
        calls: AtomicUsize,
        entered: mpsc::Sender<()>,
        released: Mutex<mpsc::Receiver<()>>,
    }
    impl crate::game_discovery::Platform for SlowCatalogue {
        fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
            if self.calls.fetch_add(1, Ordering::AcqRel) != 0 {
                self.entered.send(()).unwrap();
                self.released
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap();
            }
            vec![
                json!({"id":"dcs","name":"DCS","installed":false,"provider":"dcs-bios","processMonitoringSupported":false}),
            ]
        }
        fn validate(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn launch(&self, _: &Value) -> Result<crate::game_discovery::ProcessIdentity, String> {
            panic!("metadata cannot launch")
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
    let (entered, capture_entered) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let (mut manager, _) = setup();
    Arc::get_mut(&mut manager).unwrap().games = Arc::new(crate::games::GamesRuntime::new(
        Arc::new(|_| panic!("metadata cannot address hardware")),
        crate::games::Paths::in_directory(directory.path()),
        Arc::new(SlowCatalogue {
            calls: AtomicUsize::new(0),
            entered,
            released: Mutex::new(released),
        }),
    ));
    assert_eq!(
        request(
            &manager,
            "profile_apply",
            None,
            json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1})
        )["ok"],
        true
    );
    let editing = manager.clone();
    let metadata = std::thread::spawn(move || {
        request(
            &editing,
            "game_custom_save",
            None,
            json!({"name":"Slow metadata","executable":null}),
        )
    });
    capture_entered
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let stopping = manager.clone();
    let (stopped, capture_stopped) = mpsc::channel();
    let stop = std::thread::spawn(move || {
        stopped
            .send(request(&stopping, "profile_stop", None, json!({})))
            .unwrap();
    });
    let response = capture_stopped.recv_timeout(Duration::from_secs(2));
    // Always release/join the private blocked stage before asserting a timeout.
    release.send(()).unwrap();
    let edited = metadata.join().unwrap();
    stop.join().unwrap();
    let response = response.expect("Stop must complete while metadata discovery remains blocked");
    assert_eq!(response["ok"], true, "{response}");
    assert!(response["state"]["profiles"]["profileSession"]["owner"].is_null());
    assert_eq!(edited["ok"], true, "{edited}");
}
