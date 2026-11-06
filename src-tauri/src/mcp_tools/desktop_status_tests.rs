use super::*;

struct DesktopStatus;

impl Backend for DesktopStatus {
    fn request(&self, request: Value) -> Value {
        if request["command"] == "profile_desktop_preview" {
            assert_eq!(request["clientId"], native_client("reader"));
            assert_eq!(request["frames"][0]["changes"][0]["sample"]["value"]["value"], true);
            assert_eq!(request["advances"], json!(["0","1000"]));
            return json!({"ok":true,"state":{"profileDesktopPreview":{
                "compilationId":request["compilationId"],"mode":"recording","nativeSubmission":false
            }}});
        }
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
fn desktop_preview_forwards_only_bounded_recording_frames_without_authority() {
    let name = "profile_desktop_preview";
    assert!(!is_mutation(name));
    let tools = Tools {
        backend: Arc::new(DesktopStatus),
        ledger: Mutex::new(Ledger::default()),
        drafts: Mutex::new(vec![]),
        workspace_uploads: Mutex::new(vec![]),
        integration: OnceLock::new(),
    };
    let args = json!({"compilationId":"00000000-0000-4000-8000-000000000001",
        "frames":[{"source":"button","generation":"1","sequence":"2","capturedUs":"0",
            "reason":"change","changes":[{"portId":"button","sample":{"status":"known",
                "value":{"kind":"scalar","value":true}}}]}],"advances":["0","1000"]});
    let result = tools.call(name, args.clone(), "reader");
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["data"]["nativeSubmission"], false);
    let mut invalid = args.clone();
    invalid["frames"][0]["changes"][0]["sample"]["value"]["kind"] = json!("action-pack");
    assert_eq!(tools.call(name, invalid, "reader")["error"]["code"], "invalid_arguments");
    let mut invalid = args;
    invalid["advances"] = json!(vec!["0"; crate::profile_desktop_preview::MAX_ADVANCES + 1]);
    assert_eq!(tools.call(name, invalid, "reader")["error"]["code"], "invalid_arguments");
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
