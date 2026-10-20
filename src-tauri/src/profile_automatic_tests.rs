use super::*;
use crate::game_discovery::{Platform, ProcessIdentity};
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicUsize, Barrier},
    time::{Duration, Instant},
};

struct NoPorts;
impl Provider for NoPorts {
    fn ports(&self) -> Result<Vec<PortInfo>, String> {
        Ok(vec![])
    }
    fn adapter(&self, _: &str) -> Arc<dyn Adapter> {
        panic!("Automatic empty profiles must not open device endpoints")
    }
}
#[derive(Default)]
struct Games {
    processes: Mutex<BTreeMap<String, ProcessIdentity>>,
    invalid: AtomicBool,
    scans: AtomicUsize,
}
fn executable(id: &str) -> String {
    format!("C:\\Games\\{id}\\game.exe")
}
impl Platform for Games {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        ["dcs","assetto-corsa-rally","x-plane-11","x-plane-12"].iter().map(|id|json!({"id":id,"name":id,"installed":true,"processMonitoringSupported":true,"launchSupported":true,"executable":executable(id)})).collect()
    }
    fn validate(&self, id: &str, path: &str) -> Result<(), String> {
        if !self.invalid.load(Ordering::Acquire) && path == executable(id) {
            Ok(())
        } else {
            Err("Native installation changed".into())
        }
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        Err("Launch is outside these watcher/admission fixtures".into())
    }
    fn find(&self, game: &Value) -> Result<Option<ProcessIdentity>, String> {
        self.scans.fetch_add(1, Ordering::Relaxed);
        Ok(self
            .processes
            .lock()
            .unwrap()
            .get(game["id"].as_str().unwrap())
            .cloned())
    }
    fn alive(&self, identity: &ProcessIdentity) -> Result<bool, String> {
        Ok(self
            .processes
            .lock()
            .unwrap()
            .values()
            .any(|process| process == identity))
    }
}
struct Fixture {
    manager: GatewayManager,
    games: Arc<Games>,
    directory: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self { Self::with_legacy_stop(false) }
    fn with_legacy_stop(legacy_stop: bool) -> Self {
        let mut manager = GatewayManager::with_provider(Arc::new(NoPorts));
        let directory = crate::test_directory();
        let games = Arc::new(Games::default());
        manager.games = Arc::new(crate::games::GamesRuntime::new(
            Arc::new(|request| {
                assert_eq!(
                    request["command"], "integration_get_state",
                    "No provider or device output is authorized by this fixture"
                );
                json!({"ok":true,"state":{"integration":{"running":false}}})
            }),
            {
                let fixture_metadata_path = directory.path().join("games.json");
                crate::games::Paths {
                    installations: fixture_metadata_path.with_extension("installations-v1.json"),
                    provider_setup: fixture_metadata_path.with_extension("integration-setup.json"),
                    recovery: fixture_metadata_path.with_extension("recovery.json"),
                }
            },
            games.clone()));
        let resources = manager.profile_resources.clone();
        let runtime_games = manager.games.clone();
        let companions = manager.companion_host.clone();
        let controllers = manager.controllers.clone();
        manager.profile_prepare = Arc::new(move |candidate| {
            crate::profile_session_host::NativeProfileHost::stage(
                &resources,
                &runtime_games,
                &companions,
                controllers.clone(),
                false,
                candidate,
            )
        });
        let host = crate::profile_session_host::NativeProfileHost::new(
            manager.profile_resources.clone(),
            manager.controllers.clone(),
            manager.games.clone(),
            manager.companion_host.clone(),
            manager.profile_panels.clone(),
            false,
            manager.profile_admission_epoch.clone(),
        );
        if legacy_stop {
            std::fs::write(directory.path().join("session.json"), br#"{"schemaVersion":1,"paused":true,"active":null,"pending":null}"#).unwrap();
        }
        manager.profile_session = Arc::new(Mutex::new(crate::profile_session::Coordinator::new(
            host,
            Some(directory.path().join("session.json")),
        )));
        Self {
            manager,
            games,
            directory,
        }
    }
    fn process(&self, id: &str, epoch: u64) {
        self.games.processes.lock().unwrap().insert(
            id.into(),
            ProcessIdentity {
                pid: 42,
                path: executable(id),
                epoch,
            },
        );
    }
    fn wait_game(&self, id: &str) {
        let deadline = Instant::now() + Duration::from_secs(4);
        while !self
            .manager
            .games
            .profile_activation_observations()
            .is_ok_and(|games| {
                games.iter().any(|game| {
                    game.identity.game_id == id
                        && self.manager.games.validate_profile_activation(game).is_ok()
                })
            })
        {
            assert!(
                Instant::now() < deadline,
                "Game watcher did not confirm {id}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
fn request(manager: &GatewayManager, command: &str, mut fields: Value) -> Value {
    fields["command"] = json!(command);
    fields["clientId"] = json!("cockpit-automatic-test");
    manager.request(fields)
}
fn ok(manager: &GatewayManager, command: &str, fields: Value) -> Value {
    let reply = request(manager, command, fields);
    assert_eq!(reply["ok"], true, "{reply}");
    reply["state"]["profiles"].clone()
}
fn create(manager: &GatewayManager, name: &str) -> String {
    ok(manager, "profile_create", json!({"name":name}))["document"]["id"]
        .as_str()
        .unwrap()
        .into()
}
fn rule(manager: &GatewayManager, game: &str, id: &str, priority: i16) {
    ok(
        manager,
        "profile_game_rule_save",
        json!({"gameId":game,"profileId":id,"priority":priority,"enabled":true}),
    );
}
fn mode(manager: &GatewayManager, value: &str) {
    ok(manager, "profile_selection_mode", json!({"mode":value}));
}
fn owner(manager: &GatewayManager) -> Option<crate::profile_session::RunOwner> {
    manager
        .profile_session
        .lock()
        .unwrap()
        .status()
        .owner
        .clone()
}

#[test]
fn native_rules_are_strict_passive_and_stale_rules_can_be_removed_or_disabled() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    let id = create(manager, "Hardware");
    for extra in ["executable", "pid", "processSession", "sample", "resume"] {
        let mut fields = json!({"gameId":"dcs","profileId":id,"priority":7,"enabled":true});
        fields[extra] = json!("forged");
        assert_eq!(
            request(manager, "profile_game_rule_save", fields)["ok"],
            false
        );
    }
    assert_eq!(
        request(
            manager,
            "profile_game_rule_save",
            json!({"gameId":"dcs","profileId":id,"priority":32768,"enabled":true})
        )["ok"],
        false
    );
    rule(manager, "dcs", &id, 7);
    assert!(owner(manager).is_none());
    assert_eq!(
        ok(manager, "profile_automatic_status", json!({}))["automatic"]["rules"][0]["identity"]
            ["executable"],
        executable("dcs")
    );
    fixture.games.invalid.store(true, Ordering::Release);
    assert_eq!(
        request(
            manager,
            "profile_game_rule_save",
            json!({"gameId":"dcs","profileId":id,"priority":7,"enabled":true})
        )["ok"],
        false
    );
    ok(
        manager,
        "profile_game_rule_save",
        json!({"gameId":"dcs","profileId":id,"priority":7,"enabled":false}),
    );
    ok(manager, "profile_game_rule_remove", json!({"gameId":"dcs"}));
    assert_eq!(
        request(manager, "profile_game_rule_remove", json!({"gameId":"dcs"}))["ok"],
        false
    );
    assert!(owner(manager).is_none());
}
#[test]
fn automatic_apply_is_idempotent_and_editor_save_preserves_game_and_automatic_policy() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    let id = create(manager, "A");
    rule(manager, "dcs", &id, 0);
    fixture.process("dcs", 1);
    fixture.wait_game("dcs");
    manager.poll_automatic();
    assert!(owner(manager).is_none(), "Manual remains pinned");
    mode(manager, "automatic");
    manager.poll_automatic();
    let first = owner(manager).unwrap_or_else(|| panic!("{}",manager.profile_automatic.view()));
    assert_eq!(first.profile_id, id);
    assert_eq!(first.game_id.as_deref(), Some("dcs"));
    for _ in 0..4 {
        manager.poll_automatic();
    }
    assert_eq!(owner(manager), Some(first.clone()));
    ok(
        manager,
        "profile_rename",
        json!({"id":id,"expectedRevision":1,"name":"Edited saved profile"}),
    );
    manager.poll_automatic();
    assert_eq!(owner(manager), Some(first.clone()));
    let policy = manager.profile_automatic.config();
    let document = ok(manager,"profile_get",json!({"id":id}))["document"].clone();
    let saved = ok(manager,"profile_editor_save",json!({"document":document,"expectedRevision":2,"assignments":[]}));
    assert_eq!(saved["saveApplication"],json!({"state":"applied","error":null}));
    let current = owner(manager).unwrap();
    assert_ne!(current,first);
    assert_eq!(current.profile_revision,3);
    assert_eq!(current.game_id,first.game_id);
    assert_eq!(manager.profile_automatic.config(),policy);
    for _ in 0..4 {manager.poll_automatic();}
    assert_eq!(owner(manager),Some(current));
    fixture.games.processes.lock().unwrap().clear();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !manager
        .games
        .profile_activation_observations()
        .unwrap()
        .is_empty()
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    manager.poll_automatic();
    assert_eq!(
        owner(manager).unwrap().profile_id,
        osg_profiles::DEFAULT_PROFILE_ID
    );
}
#[test]
fn ties_manual_pin_emergency_stop_and_explicit_automatic_resume_are_authoritative() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    let a = create(manager, "A");
    let b = create(manager, "B");
    rule(manager, "dcs", &a, 1);
    rule(manager, "x-plane-12", &b, 1);
    fixture.process("dcs", 1);
    fixture.wait_game("dcs");
    mode(manager, "automatic");
    manager.poll_automatic();
    let first = owner(manager).unwrap();
    fixture.process("x-plane-12", 1);
    fixture.wait_game("x-plane-12");
    manager.poll_automatic();
    assert_eq!(owner(manager), Some(first));
    assert_eq!(manager.profile_automatic.view()["status"], "conflict");
    rule(manager, "x-plane-12", &b, 2);
    manager.poll_automatic();
    assert_eq!(owner(manager).unwrap().profile_id, b);
    mode(manager, "manual");
    assert_eq!(manager.profile_automatic.view()["manualProfileId"], b);
    manager.poll_automatic();
    assert_eq!(owner(manager).unwrap().profile_id, b);
    ok(
        manager,
        "profile_apply",
        json!({"id":a,"expectedRevision":1}),
    );
    let manual = owner(manager).unwrap();
    fixture.process("dcs", 2);
    manager.poll_automatic();
    assert_eq!(owner(manager), Some(manual));
    mode(manager, "automatic");
    manager.poll_automatic();
    assert_eq!(owner(manager).unwrap().profile_id, b);
    ok(manager, "profile_stop", json!({}));
    fixture.process("x-plane-12", 2);
    manager.poll_automatic();
    assert!(owner(manager).is_none());
    assert_eq!(manager.profile_automatic.view()["mode"], "manual");
    mode(manager, "automatic");
    fixture.wait_game("x-plane-12");
    manager.poll_automatic();
    assert!(owner(manager).is_some());
}
#[test]
fn delayed_automatic_stage_is_cancelled_by_stop_and_pending_apply_blocks_learning() {
    let mut fixture = Fixture::new();
    let id = create(&fixture.manager, "A");
    rule(&fixture.manager, "dcs", &id, 0);
    fixture.process("dcs", 1);
    fixture.wait_game("dcs");
    mode(&fixture.manager, "automatic");
    let barrier = Arc::new(Barrier::new(2));
    let gate = barrier.clone();
    let prepare = fixture.manager.profile_prepare.clone();
    fixture.manager.profile_prepare = Arc::new(move |candidate| {
        let staged = prepare(candidate)?;
        gate.wait();
        gate.wait();
        Ok(staged)
    });
    let manager = Arc::new(fixture.manager);
    let applying = manager.clone();
    let worker = std::thread::spawn(move || applying.poll_automatic());
    barrier.wait();
    let denied = request(
        &manager,
        "profile_input_learn_start",
        json!({"resourceId":"untrusted","generation":"0","capabilityRevision":"bad"}),
    );
    assert_eq!(denied["ok"], false);
    assert!(denied["error"]
        .as_str()
        .unwrap()
        .contains("pending profile activation"));
    ok(&manager, "profile_stop", json!({}));
    barrier.wait();
    worker.join().unwrap();
    assert!(owner(&manager).is_none());
    assert_eq!(manager.profile_automatic.view()["mode"], "manual");
    manager.shutdown();
}

#[test]
fn external_game_activation_has_tracked_progress_and_exact_cancel_without_launching_a_game() {
    let mut fixture = Fixture::new();
    ok(&fixture.manager, "profile_apply", json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));
    let previous = owner(&fixture.manager);
    let id = create(&fixture.manager, "Automatic flight");
    rule(&fixture.manager, "dcs", &id, 0);
    fixture.process("dcs", 1);
    fixture.wait_game("dcs");
    mode(&fixture.manager, "automatic");
    let (entered, observed) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let released = Mutex::new(released);
    let prepare = fixture.manager.profile_prepare.clone();
    fixture.manager.profile_prepare = Arc::new(move |candidate| {
        let staged = prepare(candidate)?;
        entered.send(()).map_err(|_| "The progress observer closed.".to_owned())?;
        released.lock().unwrap().recv_timeout(Duration::from_secs(5))
            .map_err(|_| "The automatic progress test did not release its native gate.".to_owned())?;
        Ok(staged)
    });
    let manager = Arc::new(fixture.manager);
    let applying = manager.clone();
    let worker = std::thread::spawn(move || applying.poll_automatic());
    observed.recv_timeout(Duration::from_secs(5)).expect("Automatic preparation did not enter its native gate");
    let current = request(&manager, "profile_activation_current", json!({}));
    let progress = current["state"]["profileActivation"].clone();
    let activation_id = progress["activationId"].as_str().unwrap().to_owned();
    let cancelled = request(&manager, "profile_activation_cancel", json!({"activationId":activation_id}));
    release.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(current["ok"], true, "{current}");
    assert_eq!(progress["origin"], "automatic");
    assert_eq!(progress["profileId"], id);
    assert_eq!(progress["profileName"], "Automatic flight");
    assert_eq!(progress["phase"], "preparing");
    assert_eq!(cancelled["ok"], true, "{cancelled}");
    let response = request(&manager, "profile_activation_status", json!({"activationId":activation_id}));
    let completed = &response["state"]["profileActivation"];
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(completed["phase"], "cancelled", "{completed}");
    assert_eq!(completed["outcome"], "previous-active");
    assert!(!completed["steps"].as_array().unwrap().iter().any(|step|step["step"] == "launching"));
    assert_eq!(owner(&manager), previous);
    manager.poll_automatic();
    assert_eq!(owner(&manager), previous, "Process polling must not replay a cancelled activation");
    manager.shutdown();
}
#[test]
fn final_admission_reverifies_process_even_before_cached_observer_catches_replacement() {
    let mut fixture = Fixture::new();
    let id = create(&fixture.manager, "A");
    rule(&fixture.manager, "dcs", &id, 0);
    fixture.process("dcs", 1);
    fixture.wait_game("dcs");
    mode(&fixture.manager, "automatic");
    let barrier = Arc::new(Barrier::new(2));
    let gate = barrier.clone();
    let prepare = fixture.manager.profile_prepare.clone();
    fixture.manager.profile_prepare = Arc::new(move |candidate| {
        let staged = prepare(candidate)?;
        gate.wait();
        gate.wait();
        Ok(staged)
    });
    let manager = Arc::new(fixture.manager);
    let applying = manager.clone();
    let worker = std::thread::spawn(move || applying.poll_automatic());
    barrier.wait();
    fixture.games.processes.lock().unwrap().insert(
        "dcs".into(),
        ProcessIdentity {
            pid: 42,
            path: executable("dcs"),
            epoch: 2,
        },
    );
    barrier.wait();
    worker.join().unwrap();
    assert!(owner(&manager).is_none());
    assert_eq!(manager.profile_automatic.view()["status"], "failed");
    manager.shutdown();
}
#[test]
fn startup_recovery_blocks_automatic_activation_without_replaying_then_explicit_recovery_reconciles(
) {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    let id = create(manager, "A");
    rule(manager, "dcs", &id, 0);
    fixture.process("dcs", 1);
    fixture.wait_game("dcs");
    mode(manager, "automatic");
    manager.poll_automatic();
    assert!(owner(manager).is_some());
    let journal = fixture.directory.path().join("session.json");
    let bytes = std::fs::read(&journal).unwrap();
    manager.profile_session.lock().unwrap().stop().unwrap();
    std::fs::write(&journal, bytes).unwrap();
    let host = || {
        crate::profile_session_host::NativeProfileHost::new(
            manager.profile_resources.clone(),
            manager.controllers.clone(),
            manager.games.clone(),
            manager.companion_host.clone(),
            manager.profile_panels.clone(),
            false,
            manager.profile_admission_epoch.clone(),
        )
    };
    let empty = crate::profile_session::Coordinator::new(host(), None);
    let previous = std::mem::replace(&mut *manager.profile_session.lock().unwrap(), empty);
    drop(previous);
    let recovered = crate::profile_session::Coordinator::new(host(), Some(journal));
    assert_eq!(
        recovered.status().state,
        crate::profile_session::State::RecoveryNeeded
    );
    *manager.profile_session.lock().unwrap() = recovered;
    manager.poll_automatic();
    assert!(owner(manager).is_none());
    assert_eq!(manager.profile_automatic.view()["status"], "blocked");
    ok(manager, "profile_recover", json!({}));
    manager.poll_automatic();
    assert!(owner(manager).is_some());
}
#[test]
fn stop_latch_does_not_wait_for_the_policy_mutex_and_background_worker_owns_no_manager_cycle() {
    let runtime = Arc::new(Runtime::new(None));
    let locked = runtime.state.lock().unwrap();
    let cloned = runtime.clone();
    let worker = std::thread::spawn(move || cloned.control("profile_stop"));
    worker.join().unwrap();
    assert!(runtime.emergency.load(Ordering::Acquire));
    drop(locked);
    let fixture = Fixture::new();
    let manager = Arc::new(fixture.manager);
    let weak = Arc::downgrade(&manager);
    manager.start_automatic();
    manager.shutdown();
    drop(manager);
    assert!(weak.upgrade().is_none());
}

#[test]
fn pending_operator_policy_cannot_be_overtaken_by_a_prepared_automatic_run() {
    let mut fixture = Fixture::new();
    let id = create(&fixture.manager, "A");
    rule(&fixture.manager, "dcs", &id, 0);
    fixture.process("dcs", 1);
    fixture.wait_game("dcs");
    mode(&fixture.manager, "automatic");
    let barrier = Arc::new(Barrier::new(2));
    let gate = barrier.clone();
    let prepare = fixture.manager.profile_prepare.clone();
    fixture.manager.profile_prepare = Arc::new(move |candidate| {
        let staged = prepare(candidate)?;
        gate.wait();
        gate.wait();
        Ok(staged)
    });
    let manager = Arc::new(fixture.manager);
    let applying = manager.clone();
    let worker = std::thread::spawn(move || applying.poll_automatic());
    barrier.wait();
    let pending = manager
        .profile_automatic
        .begin_control("profile_selection_mode")
        .unwrap();
    barrier.wait();
    worker.join().unwrap();
    assert!(owner(&manager).is_none());
    assert_eq!(manager.profile_automatic.view()["status"], "failed");
    drop(pending);
    manager.shutdown();
}
#[test]
fn actual_background_worker_switches_on_external_start_and_exit_without_ui_requests() {
    let fixture = Fixture::new();
    let id = create(&fixture.manager, "A");
    rule(&fixture.manager, "dcs", &id, 0);
    mode(&fixture.manager, "automatic");
    let manager = Arc::new(fixture.manager);
    manager.start_automatic();
    fixture.games.processes.lock().unwrap().insert(
        "dcs".into(),
        ProcessIdentity {
            pid: 42,
            path: executable("dcs"),
            epoch: 1,
        },
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !owner(&manager).is_some_and(|owner| owner.profile_id == id) {
        assert!(
            Instant::now() < deadline,
            "Background apply failed: {}",
            manager.profile_automatic.view()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let applied = owner(&manager).unwrap();
    std::thread::sleep(Duration::from_millis(550));
    assert_eq!(owner(&manager), Some(applied));
    fixture.games.processes.lock().unwrap().clear();
    let deadline = Instant::now() + Duration::from_secs(4);
    while !owner(&manager).is_some_and(|owner| owner.profile_id == osg_profiles::DEFAULT_PROFILE_ID)
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    manager.shutdown();
}

#[test]
fn malformed_optional_automation_storage_blocks_automatic_but_preserves_explicit_manual_use() {
    let mut fixture = Fixture::new();
    let path = fixture.directory.path().join("automatic.json");
    std::fs::write(&path, b"{").unwrap();
    fixture.manager.profile_automatic = Runtime::new(Some(path.clone()));
    let id = create(&fixture.manager, "Manual hardware");
    ok(
        &fixture.manager,
        "profile_apply",
        json!({"id":id,"expectedRevision":1}),
    );
    assert_eq!(owner(&fixture.manager).unwrap().profile_id, id);
    assert_eq!(std::fs::read(&path).unwrap(), b"{");
    assert_eq!(
        request(
            &fixture.manager,
            "profile_selection_mode",
            json!({"mode":"automatic"})
        )["ok"],
        false
    );
    fixture.manager.poll_automatic();
    assert_eq!(owner(&fixture.manager).unwrap().profile_id, id);
    ok(&fixture.manager, "profile_stop", json!({}));
    assert!(owner(&fixture.manager).is_none());
    assert_eq!(std::fs::read(path).unwrap(), b"{");
}

#[test]
fn external_activation_reports_enumeration_restart_only_for_successful_game_context_changes() {
    let runtime = Runtime::new(None);
    let mut target = Target::default();
    target.game_id = Some("dcs".into());
    target.process_session = Some("native-proof".into());
    runtime.finish(0, target.clone(), &Ok(()), true);
    assert!(runtime.view()["message"]
        .as_str()
        .unwrap()
        .contains("may need a restart"));
    runtime.finish(0, target.clone(), &Ok(()), false);
    assert!(!runtime.view()["message"]
        .as_str()
        .unwrap()
        .contains("restart"));
    runtime.finish(0, Target::default(), &Ok(()), true);
    assert!(!runtime.view()["message"]
        .as_str()
        .unwrap()
        .contains("restart"));
    runtime.finish(0, target, &Err("native preparation rejected".into()), true);
    assert_eq!(runtime.view()["message"], "native preparation rejected");
}

#[test]
fn failed_manual_pin_storage_inhibits_automatic_events_after_explicit_selection() {
    let mut fixture = Fixture::new();
    let path = fixture.directory.path().join("automatic.json");
    fixture.manager.profile_automatic = Runtime::new(Some(path.clone()));
    mode(&fixture.manager, "automatic");
    let id = create(&fixture.manager, "Manual choice");
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        request(
            &fixture.manager,
            "profile_select",
            json!({"id":id,"selectionSource":"manual"})
        )["ok"],
        false
    );
    fixture.manager.poll_automatic();
    assert!(owner(&fixture.manager).is_none());
    assert!(fixture
        .manager
        .profile_automatic
        .emergency
        .load(Ordering::Acquire));
    assert_eq!(
        fixture.manager.profile_automatic.view()["status"],
        "blocked"
    );
}

#[test]
fn legacy_pause_is_migrated_to_durable_stop_before_automatic_startup() {
    let mut fixture = Fixture::with_legacy_stop(true);
    let path = fixture.directory.path().join("automatic.json");
    fixture.manager.profile_automatic = Runtime::new(Some(path.clone()));
    mode(&fixture.manager, "automatic");
    fixture.manager.restore_automatic_selection();
    fixture.manager.poll_automatic();
    assert!(owner(&fixture.manager).is_none());
    assert_eq!(fixture.manager.profile_automatic.view()["mode"], "manual");
    assert_eq!(Store::new(Some(path)).load().unwrap().unwrap().mode, SelectionMode::Manual);
    let saved: Value = serde_json::from_slice(&std::fs::read(fixture.directory.path().join("session.json")).unwrap()).unwrap();
    assert!(saved.get("paused").is_none());
    ok(&fixture.manager, "profile_apply", json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));
    assert!(owner(&fixture.manager).is_some());
}

#[test]
fn legacy_stop_migration_preserves_the_old_intent_until_both_writes_succeed() {
    for fail_pin in [true, false] {
        let mut fixture = Fixture::with_legacy_stop(true);
        let path = fixture.directory.path().join("automatic.json");
        fixture.manager.profile_automatic = Runtime::new(Some(path.clone()));
        mode(&fixture.manager, "automatic");
        if fail_pin {
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
        } else {
            fixture.manager.profile_session.lock().unwrap().fail_journal_saves(true);
        }
        fixture.manager.restore_automatic_selection();
        fixture.manager.poll_automatic();
        assert!(owner(&fixture.manager).is_none());
        let saved: Value = serde_json::from_slice(&std::fs::read(fixture.directory.path().join("session.json")).unwrap()).unwrap();
        assert_eq!(saved["paused"], true);
        if fail_pin { std::fs::remove_dir(&path).unwrap(); }
        fixture.manager.profile_session.lock().unwrap().fail_journal_saves(false);
        fixture.manager.restore_automatic_selection();
        assert_eq!(Store::new(Some(path)).load().unwrap().unwrap().mode, SelectionMode::Manual);
        let saved: Value = serde_json::from_slice(&std::fs::read(fixture.directory.path().join("session.json")).unwrap()).unwrap();
        assert!(saved.get("paused").is_none());
    }
}

#[test]
fn desktop_stop_persists_manual_even_when_owned_cleanup_fails() {
    let mut fixture = Fixture::new();
    let path = fixture.directory.path().join("automatic.json");
    fixture.manager.profile_automatic = Runtime::new(Some(path.clone()));
    ok(&fixture.manager, "profile_apply", json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));
    mode(&fixture.manager, "automatic");
    fixture.manager.profile_session.lock().unwrap().fail_journal_saves(true);
    assert_eq!(request(&fixture.manager,"profile_desktop_stop",json!({}))["ok"], false);
    assert!(owner(&fixture.manager).is_none());
    assert_eq!(fixture.manager.profile_automatic.view()["mode"],"manual");
    assert_eq!(Store::new(Some(path)).load().unwrap().unwrap().mode, SelectionMode::Manual);
    fixture.manager.profile_session.lock().unwrap().fail_journal_saves(false);
    ok(&fixture.manager, "profile_recover", json!({}));
    fixture.manager.poll_automatic();
    assert!(owner(&fixture.manager).is_none());
}
