use super::*;

struct DesktopStatus;

impl Backend for DesktopStatus {
    fn request(&self, request: Value) -> Value {
        assert_eq!(request, json!({"command":"profile_desktop_status","clientId":native_client("reader")}));
        json!({"ok":true,"state":{"desktop":{
            "enabled":true,"qualified":true,"state":"waiting","reason":null,
            "scope":{"kind":"application"},"error":null,
            "application":{"path":"private-application"},"recentApplication":{"pid":32},
            "runtime":{"state":"waiting","waitingNeutral":["hold-a"],"dispatchPending":false,
                "scope":{"executable":"private-application"},"helperScope":{"pid":32},
                "backend":{"private":"backend journal"}}
        }}})
    }

    fn guarded(&self, _: Value, _: Value) -> Value {
        panic!("Reading desktop status must not acquire authority")
    }
}

#[test]
fn desktop_status_is_discoverable_and_projects_native_readiness_without_identities() {
    let name = "profile_desktop_status";
    assert!(catalogue().iter().any(|tool| tool["name"] == name));
    assert!(!is_mutation(name));
    let tools = Tools {
        backend: Arc::new(DesktopStatus),
        ledger: Mutex::new(Ledger::default()),
        drafts: Mutex::new(vec![]),
        workspace_uploads: Mutex::new(vec![]),
        integration: OnceLock::new(),
    };
    let reply = tools.call(name, json!({}), "reader");
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"], json!({
        "enabled":true,"qualified":true,"state":"waiting","reason":null,
        "scope":{"kind":"application"},"error":null,
        "runtime":{"state":"waiting","waitingNeutral":["hold-a"],"dispatchPending":false}
    }));
    assert_eq!(tools.call(name, json!({"enabled":false}), "reader")["error"]["code"], "invalid_arguments");
}
