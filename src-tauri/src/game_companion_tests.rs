use super::*;
fn capture() -> Value {
    json!({"schemaVersion":1,"assignments":[{"instanceId":"panel-a","companionId":"installation-a","publicationHash":format!("sha256:{}","a".repeat(64)),"capabilityFingerprint":format!("sha256:{}","b".repeat(64))}]})
}
#[derive(Default)]
struct CompanionMock {
    fail_snapshot: bool,
    fail_take: bool,
    fail_restore: bool,
    snapshot_phase: bool,
    snapshot_revision: bool,
    desired: Value,
    calls: Vec<String>,
}
fn companion_setup() -> (
    GamesRuntime,
    Arc<Mutex<CompanionMock>>,
    Arc<Mutex<Mock>>,
    Arc<FakePlatform>,
    PathBuf,
) {
    let (base, hardware) = mock();
    let shared = Arc::new(Mutex::new(CompanionMock::default()));
    let copy = shared.clone();
    let panel: Panel = Arc::new(move |r| {
        let command = r["command"].as_str().unwrap();
        if !command.starts_with("_game_companion_") {
            return base(r);
        }
        assert_eq!(r["clientId"], CLIENT);
        let mut m = copy.lock().unwrap();
        m.calls.push(command.into());
        match command {
            "_game_companion_snapshot" => {
                if m.fail_snapshot {
                    return json!({"ok":false,"error":"Captured revision is unavailable"});
                }
                m.desired = r["capture"].clone();
                let baseline = json!({"schemaVersion":1,"assignments":[{"instanceId":"panel-a","companionId":null,"publicationHash":null,"capabilityFingerprint":null}]});
                let mut snapshot = json!({"owner":uuid::Uuid::new_v4().to_string(),"desired":r["capture"],"baseline":baseline,"taken":m.snapshot_phase,"restored":false});
                if m.snapshot_revision {
                    snapshot["desired"]["assignments"][0]["publicationHash"] =
                        json!(format!("sha256:{}", "c".repeat(64)));
                }
                json!({"ok":true,"state":snapshot})
            }
            "_game_companion_take" if m.fail_take => {
                json!({"ok":false,"error":"Assignment acknowledgement lost"})
            }
            "_game_companion_restore" if m.fail_restore => {
                json!({"ok":false,"error":"External companion assignment changed; preserved"})
            }
            _ => json!({"ok":true,"state":{}}),
        }
    });
    let platform = Arc::new(FakePlatform::new());
    let path = path();
    (
        GamesRuntime::new(panel, path.clone(), platform.clone()),
        shared,
        hardware,
        platform,
        path,
    )
}
fn desired() -> Value {
    json!({"schemaVersion":3,"id":"custom","gameId":"dcs","name":"Companion game","devices":[],"integration":null,"companions":capture()})
}
#[test]
fn saving_does_not_touch_companions_and_active_capture_is_immutable_until_owned_restore() {
    let (runtime, comp, hardware, platform, path) = companion_setup();
    let mut p = desired();
    save(&runtime, p.clone());
    assert!(comp.lock().unwrap().calls.is_empty());
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    p["companions"]["assignments"][0]["publicationHash"] =
        json!(format!("sha256:{}", "c".repeat(64)));
    assert_eq!(comp.lock().unwrap().desired, capture());
    assert!(runtime
        .request(&json!({"command":"game_profile_save","gameProfile":p}))
        .is_err());
    for command in [
        "companion_assign",
        "companion_takeover",
        "companion_publish_begin",
        "companion_enable",
    ] {
        assert!(runtime.guard_request(&json!({"command":command})).is_err());
    }
    for action in ["save", "reconcile", "start"] {
        assert!(runtime
            .guard_request(
                &json!({"command":"mapping_companion_route","companion":{"action":action}})
            )
            .is_err());
    }
    for action in ["inspect", "recording", "stop", "recover"] {
        assert!(runtime
            .guard_request(
                &json!({"command":"mapping_companion_route","companion":{"action":action}})
            )
            .is_ok());
    }
    for action in ["inspect", "resync", "reset"] {
        assert!(runtime
            .guard_request(
                &json!({"command":"companion_feedback_routes","companion":{"action":action}})
            )
            .is_ok());
    }
    assert!(runtime
        .guard_request(
            &json!({"command":"companion_feedback_routes","companion":{"action":"save"}})
        )
        .is_err());
    for command in [
        "companion_status",
        "companion_game_capture",
        "companion_disable",
        "companion_disconnect",
        "companion_revoke",
    ] {
        assert!(runtime.guard_request(&json!({"command":command})).is_ok());
    }
    platform.alive.store(false, Ordering::Release);
    wait(&runtime, "idle");
    assert_eq!(
        comp.lock().unwrap().calls,
        vec![
            "_game_companion_snapshot",
            "_game_companion_take",
            "_game_companion_restore"
        ]
    );
    assert!(!path.with_extension("recovery.json").exists());
    assert!(!hardware
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|c| c == "config_apply"));
    runtime.shutdown();
    clean(&path);
}
#[test]
fn missing_companion_revision_fails_preflight_before_any_mutation_or_game_launch() {
    let (runtime, comp, hardware, platform, path) = companion_setup();
    save(&runtime, desired());
    comp.lock().unwrap().fail_snapshot = true;
    launch(&runtime, "dcs");
    wait(&runtime, "failed");
    assert!(!platform.launched.load(Ordering::Acquire));
    assert_eq!(comp.lock().unwrap().calls, vec!["_game_companion_snapshot"]);
    assert!(!hardware.lock().unwrap().calls.iter().any(|c| matches!(
        c.as_str(),
        "integration_stop" | "dcs_stop" | "config_apply" | "enable" | "disable"
    )));
    assert!(!path.with_extension("recovery.json").exists());
    runtime.shutdown();
    clean(&path);
}
#[test]
fn lost_companion_apply_reply_is_reconciled_by_owned_restore_without_replaying_take() {
    let (runtime, comp, _, platform, path) = companion_setup();
    save(&runtime, desired());
    comp.lock().unwrap().fail_take = true;
    launch(&runtime, "dcs");
    wait(&runtime, "failed");
    assert!(!platform.launched.load(Ordering::Acquire));
    assert_eq!(
        comp.lock().unwrap().calls,
        vec![
            "_game_companion_snapshot",
            "_game_companion_take",
            "_game_companion_restore"
        ]
    );
    assert!(!path.with_extension("recovery.json").exists());
    runtime.shutdown();
    clean(&path);
}
#[test]
fn external_conflict_keeps_journal_and_restart_requires_explicit_recovery_without_reactivation() {
    let (runtime, comp, _, platform, path) = companion_setup();
    save(&runtime, desired());
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    comp.lock().unwrap().fail_restore = true;
    platform.alive.store(false, Ordering::Release);
    wait(&runtime, "recovery-needed");
    let journal = load(&path.with_extension("recovery.json"))
        .unwrap()
        .unwrap();
    assert_eq!(journal["companions"]["taken"], true);
    assert_eq!(journal["companions"]["restored"], false);
    runtime.shutdown();
    drop(runtime);
    let (base, _) = mock();
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let copy = calls.clone();
    let panel: Panel = Arc::new(move |r| {
        let command = r["command"].as_str().unwrap();
        if command.starts_with("_game_companion_") {
            copy.lock().unwrap().push(command.into());
            return json!({"ok":true,"state":{}});
        }
        base(r)
    });
    let platform = Arc::new(FakePlatform::new());
    let restarted = GamesRuntime::new(panel, path.clone(), platform.clone());
    assert_eq!(restarted.status()["session"]["phase"], "recovery-needed");
    assert!(calls.lock().unwrap().is_empty());
    assert!(!platform.launched.load(Ordering::Acquire));
    restarted
        .request(&json!({"command":"game_recover"}))
        .unwrap();
    wait(&restarted, "idle");
    assert_eq!(*calls.lock().unwrap(), vec!["_game_companion_restore"]);
    assert!(!path.with_extension("recovery.json").exists());
    restarted.shutdown();
    clean(&path);
}
#[test]
fn schema_three_keeps_legacy_mapping_validation_and_rejects_unowned_recovery_scope() {
    let p = desired();
    assert_eq!(profile(&p).unwrap(), p);
    let mut bad = p.clone();
    bad["schemaVersion"] = json!(2);
    assert!(profile(&bad).is_err());
    for version in [1, 2] {
        let legacy = json!({"schemaVersion":version,"id":"legacy","gameId":"dcs","name":"Legacy","devices":[],"integration":null});
        assert_eq!(profile(&legacy).unwrap(), legacy);
    }
    let (runtime, _, _, _, path) = companion_setup();
    let snapshot = runtime.inner.snapshot(&p, true).unwrap();
    assert!(validate_journal(snapshot.clone()).is_ok());
    let mut bad = snapshot;
    bad["companions"]["baseline"]["assignments"][0]["instanceId"] = json!("external-panel");
    assert!(validate_journal(bad).is_err());
    runtime.shutdown();
    clean(&path);
}

#[test]
fn abandoning_conflicted_recovery_keeps_current_assignments_without_repeating_restore() {
    let (runtime, comp, _, platform, path) = companion_setup();
    save(&runtime, desired());
    launch(&runtime, "dcs");
    wait(&runtime, "running");
    comp.lock().unwrap().fail_restore = true;
    platform.alive.store(false, Ordering::Release);
    wait(&runtime, "recovery-needed");
    assert!(runtime.request(&json!({"command":"game_abandon"})).is_err());
    runtime
        .request(&json!({"command":"game_abandon","preserveCurrentSetup":true}))
        .unwrap();
    wait(&runtime, "idle");
    let calls = comp.lock().unwrap().calls.clone();
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.as_str() == "_game_companion_restore")
            .count(),
        1
    );
    assert_eq!(calls.last().unwrap(), "_game_companion_abandon");
    assert!(!path.with_extension("recovery.json").exists());
    runtime.shutdown();
    clean(&path);
}

#[test]
fn mismatched_or_prearmed_snapshot_is_rejected_before_any_side_effect() {
    for revision in [false, true] {
        let (runtime, comp, hardware, platform, path) = companion_setup();
        save(&runtime, desired());
        {
            let mut m = comp.lock().unwrap();
            m.snapshot_revision = revision;
            m.snapshot_phase = !revision;
        }
        launch(&runtime, "dcs");
        wait(&runtime, "failed");
        assert!(!platform.launched.load(Ordering::Acquire));
        assert_eq!(comp.lock().unwrap().calls, vec!["_game_companion_snapshot"]);
        assert!(!hardware
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c == "integration_stop"));
        runtime.shutdown();
        clean(&path);
    }
}
