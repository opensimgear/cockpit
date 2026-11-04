use super::*;
const OWNER: &str = "11111111-1111-4111-8111-111111111111";

#[test]
fn visibility_cleanup_waits_for_exact_restoration_and_never_retries_uncertain_stop() {
    let calls = std::cell::RefCell::new(vec![]);
    let result = restore_visibility(OWNER, |request| {
        calls.borrow_mut().push("stop");
        assert_eq!(request, &json!({"owner":OWNER}));
        match calls.borrow().len() {
            1 => Ok(json!({"active":true,"phase":"restoring"})),
            3 => Ok(json!({"active":false,"phase":"restored"})),
            _ => panic!("unexpected restoration attempt"),
        }
    }, || {
        calls.borrow_mut().push("completion");
        Ok(true)
    }).unwrap();
    assert_eq!(result["phase"], "restored");
    assert_eq!(*calls.borrow(), ["stop", "completion", "stop"]);
    assert!(restore_visibility(OWNER, |_| Err("Lost cleanup reply".into()), || panic!("Uncertain Stop must not wait or retry")).unwrap_err().contains("Lost cleanup reply"));
    calls.borrow_mut().clear();
    assert!(restore_visibility(OWNER, |_| { calls.borrow_mut().push("stop"); Ok(json!({"active":true})) }, || Ok(false)).unwrap_err().contains("still pending"));
    assert_eq!(*calls.borrow(), ["stop"]);
    assert!(restore_visibility(OWNER, |_| Ok(json!({"active":false,"phase":"attention"})), || panic!("Inactive restoration needs no wait")).unwrap_err().contains("exact restoration"));
}

#[test]
fn retained_game_callback_has_only_exact_cleanup_and_physical_restore_paths() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
    let provider = Arc::new(osg_mapping_host::companions::CompanionProvider::new(Arc::new(|| 1)));
    let companions = Arc::new(crate::companion_host::Runtime::new(None, provider));
    let cleanup = panel(Arc::new(move |request| { count.fetch_add(1, Ordering::AcqRel); json!({"ok":true,"physicalRequest":request}) }), Arc::new(crate::profile_retired_recovery::Recovery::new(None, false)), controllers.clone(), companions.clone());
    for command in ["_game_mapping_snapshot", "_game_mapping_take", "_game_mapping_arm", "_game_mapping_resume", "_game_companion_snapshot", "_game_companion_take", "integration_start", "integration_get_state", "dcs_start", "dcs_status", "_retired_unknown"] {
        assert_eq!(cleanup(json!({"command":command,"owner":OWNER}))["ok"], false, "{command}");
    }
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert_eq!(cleanup(json!({"command":"_retired_mapping_inspect","owner":OWNER})), json!({"ok":true,"state":{"recovery":null}}));
    assert_eq!(cleanup(json!({"command":"_retired_mapping_cleanup","owner":OWNER,"recovery":null}))["ok"], true);
    assert_eq!(cleanup(json!({"command":"_retired_mapping_abandon","owner":OWNER}))["ok"], true);
    assert_eq!(cleanup(json!({"command":"_retired_mapping_cleanup","owner":OWNER}))["ok"], false);
    assert_eq!(cleanup(json!({"command":"_retired_mapping_cleanup","owner":OWNER,"recovery":{"owner":"forged"}}))["ok"], false);
    for command in ["_retired_mapping_inspect", "_retired_mapping_cleanup", "_retired_mapping_abandon", "_retired_mapping_visibility_cleanup"] {
        assert_eq!(cleanup(json!({"command":command,"owner":"forged","recovery":null}))["ok"], false);
    }
    assert_eq!(cleanup(json!({"command":"_game_companion_restore","recovery":false}))["ok"], false);
    assert_eq!(calls.load(Ordering::Acquire), 0);
    let request = json!({"command":"config_read","gatewayId":"native-gateway","moduleId":"recorded-module"});
    assert_eq!(cleanup(request.clone()), json!({"ok":true,"physicalRequest":request}));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    companions.shutdown();
    controllers.shutdown();
}
