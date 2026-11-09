use super::*;
use std::{sync::mpsc, time::Duration};

#[test]
fn inactive_editor_save_completes_while_input_admission_is_shared() {
    let manager = GatewayManager::mcp_test_manager();
    let active = manager.request(json!({
        "command": "profile_apply",
        "clientId": "save-admission-fixture",
        "id": osg_profiles::DEFAULT_PROFILE_ID,
        "expectedRevision": 1
    }));
    assert_eq!(active["ok"], true, "{active}");
    let owner = &active["state"]["profiles"]["profileSession"]["owner"];
    assert!(owner.is_object(), "{active}");
    let created = manager.request(json!({
        "command": "profile_create",
        "clientId": "save-admission-fixture",
        "name": "Inactive save"
    }));
    assert_eq!(created["ok"], true, "{created}");
    let mut document = created["state"]["profiles"]["document"].clone();
    document["name"] = json!("Saved while admitting input");

    std::thread::scope(|scope| {
        let admission = manager.profile_admission.read().unwrap();
        let (sent, received) = mpsc::channel();
        let manager = &manager;
        let save = scope.spawn(move || {
            let result = manager.request(json!({
                "command": "profile_editor_save",
                "clientId": "save-admission-fixture",
                "document": document,
                "assignments": [],
                "expectedRevision": 1
            }));
            sent.send(result).unwrap();
        });
        let response = received.recv_timeout(Duration::from_secs(3));
        drop(admission);
        save.join().unwrap();
        let response = response.expect("Inactive save waited for a shared input admission guard");
        assert_eq!(response["ok"], true, "{response}");
        let profiles = &response["state"]["profiles"];
        assert_eq!(profiles["document"]["revision"], 2);
        assert_eq!(profiles["saveApplication"]["state"], "inactive");
        assert_eq!(&profiles["profileSession"]["owner"], owner);
    });
}
