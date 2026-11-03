use super::*;

fn frame(reader: &mut osg_streams::Reader) -> osg_streams::WireFrame {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if let Some(frame) = reader.try_recv().unwrap() { return frame; }
        assert!(std::time::Instant::now() < deadline, "Read model did not arrive");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn immutable_pages_and_mcp_reads_do_not_wait_for_profile_or_game_admission() {
    use crate::profile_read_models::{AutomaticChange, AutomaticSnapshot};
    let manager = Arc::new(GatewayManager::mcp_test_manager());
    let mut reader = manager.observations.broker().subscribe("page-test", "mcp", "profiles.runtime").unwrap();
    frame(&mut reader);
    let rules = vec![osg_profiles::activation::Rule {
        identity: osg_profiles::activation::GameIdentity { game_id: "dcs".into(), executable: format!("C:\\{}", "x".repeat(32760)) },
        profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(), priority: 0, enabled: true,
    }];
    manager.profile_read_models.automatic_observer()(AutomaticChange::Snapshot(AutomaticSnapshot {
        schema_version: 1, preferred_profiles: Default::default(), mode: osg_profiles::session::SelectionMode::Manual,
        manual_profile_id: osg_profiles::DEFAULT_PROFILE_ID.into(), rules, status: "manual".into(), message: String::new(),
        conflicts: vec![], last_attempt: None, target: None, read_only: false,
    }));
    let observed = frame(&mut reader).payload.unwrap();
    let id = observed["automatic"]["rules"]["pages"]["id"].as_str().unwrap().to_owned();
    let schema = crate::mcp_tools::catalogue().into_iter().find(|tool|tool["name"] == "profile_read_model_page").unwrap();
    assert_eq!(schema["annotations"]["readOnlyHint"], true);
    assert!(schema["inputSchema"]["properties"].get("operationId").is_none());
    let tools = Arc::new(crate::mcp_tools::Tools::new(manager.clone()));
    let profile_guard = manager.profile_admission.write().unwrap();
    let game_guard = manager.game_admission.write().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let caller = tools.clone();
    let worker = std::thread::spawn(move || {
        send.send(caller.call("profile_read_model_page", json!({"id":id,"offset":0}), "read-only-page-test-client")).unwrap();
    });
    let result = receive.recv_timeout(std::time::Duration::from_secs(1));
    drop(game_guard);
    drop(profile_guard);
    worker.join().unwrap();
    let result = result.expect("Page lookup waited for authority admission");
    assert_eq!(result["ok"], true, "{result}");
    assert!(result["data"]["chunk"].as_str().unwrap().len() <= 8192);
    assert_eq!(manager.profile_pending.load(Ordering::Acquire), 0);
    assert!(manager.profile_session.lock().unwrap().status().owner.is_none());
    manager.shutdown();
}
