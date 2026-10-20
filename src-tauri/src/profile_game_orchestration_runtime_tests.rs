use super::*;
use crate::game_discovery::{Platform, ProcessIdentity};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize},
    mpsc, Arc, Barrier, Mutex,
};

type ProgressGate = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;
fn progress_gate() -> (ProgressGate, mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (entered, observed) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let released = Mutex::new(released);
    let gate = Arc::new(move || {
        entered.send(()).map_err(|_| "The progress observer closed.".to_owned())?;
        released.lock().unwrap().recv_timeout(Duration::from_secs(5))
            .map_err(|_| "The progress test did not release its native gate within five seconds.".to_owned())
    });
    (gate, observed, release)
}

struct NoPorts;
impl Provider for NoPorts {
    fn ports(&self) -> Result<Vec<PortInfo>, String> {
        Ok(vec![])
    }
    fn adapter(&self, _: &str) -> Arc<dyn Adapter> {
        panic!("Empty launch never opens hardware")
    }
}
#[derive(Default)]
struct Games {
    processes: Mutex<BTreeMap<String, ProcessIdentity>>,
    spawns: AtomicUsize,
    appear: AtomicBool,
    fail_spawn: AtomicBool,
    spawn_gate: Mutex<Option<ProgressGate>>,
}
fn executable(id: &str) -> String {
    format!("C:\\Games\\{id}\\game.exe")
}
impl Platform for Games {
    fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<Value> {
        ["dcs","x-plane-11","x-plane-12","assetto-corsa-rally"].iter().map(|id|json!({"id":id,"name":if *id=="dcs" {"DCS World"}else{id},"installed":true,"processMonitoringSupported":true,"launchSupported":true,"executable":executable(id)})).collect()
    }
    fn validate(&self, id: &str, path: &str) -> Result<(), String> {
        if path == executable(id) {
            Ok(())
        } else {
            Err("Installation changed".into())
        }
    }
    fn launch(&self, _: &Value) -> Result<ProcessIdentity, String> {
        panic!("No independent legacy launch")
    }
    fn spawn_process(&self, game: &Value) -> Result<(), String> {
        self.spawns.fetch_add(1, Ordering::AcqRel);
        if let Some(gate) = self.spawn_gate.lock().unwrap().clone() {
            gate()?;
        }
        if self.appear.load(Ordering::Acquire) {
            let id = game["id"].as_str().unwrap();
            self.processes.lock().unwrap().insert(
                id.into(),
                ProcessIdentity {
                    pid: 42,
                    path: executable(id),
                    epoch: 1,
                },
            );
        }
        if self.fail_spawn.load(Ordering::Acquire) {
            Err("Uncertain native spawn".into())
        } else {
            Ok(())
        }
    }
    fn find(&self, game: &Value) -> Result<Option<ProcessIdentity>, String> {
        Ok(self
            .processes
            .lock()
            .unwrap()
            .get(game["id"].as_str().unwrap())
            .cloned())
    }
    fn alive(&self, process: &ProcessIdentity) -> Result<bool, String> {
        Ok(self
            .processes
            .lock()
            .unwrap()
            .values()
            .any(|p| p == process))
    }
}
struct Fixture {
    manager: Arc<GatewayManager>,
    games: Arc<Games>,
    _directory: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self::with_installations(false)
    }
    fn with_installations(corrupt: bool) -> Self {
        let mut manager = GatewayManager::with_provider(Arc::new(NoPorts));
        let directory = crate::test_directory();
        if corrupt {
            std::fs::write(directory.path().join("games.installations-v1.json"),b"interrupted pinned executable metadata").unwrap();
        }
        let games = Arc::new(Games::default());
        games.appear.store(true, Ordering::Release);
        manager.games = Arc::new(crate::games::GamesRuntime::new(
            Arc::new(|request| {
                assert_eq!(request["command"], "integration_get_state");
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
        manager.profile_session = Arc::new(Mutex::new(crate::profile_session::Coordinator::new(
            host,
            Some(directory.path().join("session.json")),
        )));
        Self {
            manager: Arc::new(manager),
            games,
            _directory: directory,
        }
    }
    fn wait_game(&self, id: &str) {
        let deadline = Instant::now() + Duration::from_secs(4);
        while !self
            .manager
            .games
            .profile_activation_observations()
            .is_ok_and(|items| items.iter().any(|p| p.identity.game_id == id))
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
fn request(manager: &GatewayManager, command: &str, mut fields: Value) -> Value {
    fields["command"] = json!(command);
    fields["clientId"] = json!("native-launch-test");
    manager.request(fields)
}
fn ok(manager: &GatewayManager, command: &str, fields: Value) -> Value {
    let reply = request(manager, command, fields);
    assert_eq!(reply["ok"], true, "{reply}");
    reply["state"]["profiles"].clone()
}
fn launch(manager: &GatewayManager, id: &str) -> Result<Value, String> {
    let response = request(manager, "profile_game_launch", json!({"gameId":id}));
    if response["ok"] == true {
        Ok(response["state"]["profiles"].clone())
    } else {
        Err(response["error"]
            .as_str()
            .unwrap_or("Native launch failed")
            .into())
    }
}

fn activation_progress(manager: &GatewayManager, id: Option<&str>) -> Value {
    let mut fields = json!({});
    if let Some(id) = id { fields["activationId"] = json!(id); }
    let response = request(manager, if id.is_some() {"profile_activation_status"} else {"profile_activation_current"}, fields);
    assert_eq!(response["ok"], true, "{response}");
    response["state"]["profileActivation"].clone()
}

#[test]
fn tracked_game_launch_preflight_cancellation_preserves_the_active_profile() {
    let mut fixture = Fixture::new();
    let manager = Arc::get_mut(&mut fixture.manager).unwrap();
    ok(manager, "profile_apply", json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));
    let previous = manager.profile_session.lock().unwrap().status().owner.clone();
    let prepare = manager.profile_prepare.clone();
    let (gate, entered, release) = progress_gate();
    manager.profile_prepare = Arc::new(move |candidate| {
        let staged = prepare(candidate)?;
        gate()?;
        Ok(staged)
    });
    let launching = fixture.manager.clone();
    let worker = std::thread::spawn(move || launch(&launching, "dcs"));
    entered.recv_timeout(Duration::from_secs(5)).expect("Native game preparation did not enter its gate");
    let progress = activation_progress(&fixture.manager, None);
    let id = progress["activationId"].as_str().unwrap().to_owned();
    let cancelled = request(&fixture.manager, "profile_activation_cancel", json!({"activationId":id}));
    release.send(()).unwrap();
    let result = worker.join().unwrap();
    assert!(result.is_err());
    assert_eq!(progress["origin"], "game-launch");
    assert_eq!(progress["canCancel"], true);
    assert_eq!(cancelled["ok"], true, "{cancelled}");
    let completed = activation_progress(&fixture.manager, Some(&id));
    assert_eq!(completed["phase"], "cancelled", "{completed}");
    assert_eq!(completed["outcome"], "previous-active");
    assert_eq!(fixture.manager.profile_session.lock().unwrap().status().owner, previous);
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 0);
}

#[test]
fn game_launch_progress_keeps_one_operation_through_hardware_and_real_process_start() {
    let mut fixture = Fixture::new();
    let manager = Arc::get_mut(&mut fixture.manager).unwrap();
    let prepare = manager.profile_prepare.clone();
    let (preparation, preparing_entered, preparing_release) = progress_gate();
    manager.profile_prepare = Arc::new(move |candidate| {
        let staged = prepare(candidate)?;
        preparation()?;
        Ok(staged)
    });
    let (spawning, spawning_entered, spawning_release) = progress_gate();
    *fixture.games.spawn_gate.lock().unwrap() = Some(spawning);
    let launching = fixture.manager.clone();
    let worker = std::thread::spawn(move || launch(&launching, "dcs"));
    preparing_entered.recv_timeout(Duration::from_secs(5)).expect("Native game preparation did not enter its gate");
    let preparing = activation_progress(&fixture.manager, None);
    preparing_release.send(()).unwrap();
    spawning_entered.recv_timeout(Duration::from_secs(5)).expect("Native game spawn did not enter its gate");
    let progress = activation_progress(&fixture.manager, None);
    spawning_release.send(()).unwrap();
    let result = worker.join().unwrap().unwrap();
    assert_eq!(preparing["phase"], "preparing");
    assert_eq!(progress["activationId"], preparing["activationId"]);
    assert_eq!(progress["phase"], "launching", "{progress}");
    assert_eq!(progress["step"], "launching");
    assert_eq!(progress["message"], "Launching DCS World…");
    assert_eq!(progress["canCancel"], true, "Held launch epoch must remain cancellable");
    assert_eq!(result["launch"]["state"], "started");
    let completed = activation_progress(&fixture.manager, progress["activationId"].as_str());
    assert_eq!(completed["phase"], "succeeded", "{completed}");
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 1);
}

#[test]
fn tracked_cancel_during_game_process_wait_revokes_the_rebound_epoch_without_replay() {
    let fixture = Fixture::new();
    fixture.games.appear.store(false, Ordering::Release);
    let launching = fixture.manager.clone();
    let worker = std::thread::spawn(move || launch(&launching, "dcs"));
    let deadline = Instant::now() + Duration::from_secs(3);
    let progress = loop {
        let progress = activation_progress(&fixture.manager, None);
        if progress["phase"] == "launching" && progress["message"] == "Waiting for DCS World to start…" {
            break progress;
        }
        assert!(Instant::now() < deadline, "Launch never reached process wait: {progress}");
        std::thread::sleep(Duration::from_millis(10));
    };
    let id = progress["activationId"].as_str().unwrap().to_owned();
    let epoch = fixture.manager.profile_admission_epoch.load(Ordering::Acquire);
    let cancelled = request(&fixture.manager, "profile_activation_cancel", json!({"activationId":id}));
    assert_eq!(cancelled["ok"], true, "{cancelled}");
    assert_ne!(fixture.manager.profile_admission_epoch.load(Ordering::Acquire), epoch);
    assert!(worker.join().unwrap().is_err());
    let completed = activation_progress(&fixture.manager, Some(&id));
    assert_eq!(completed["phase"], "cancelled", "{completed}");
    assert_eq!(completed["outcome"], "inactive");
    assert!(fixture.manager.profile_session.lock().unwrap().status().owner.is_none());
    fixture.manager.poll_automatic();
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 1);
    ok(&fixture.manager, "profile_apply", json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));
    let owner = fixture.manager.profile_session.lock().unwrap().status().owner.clone();
    let newer_epoch = fixture.manager.profile_admission_epoch.load(Ordering::Acquire);
    let stale = request(&fixture.manager, "profile_activation_cancel", json!({"activationId":id}));
    assert_eq!(stale["ok"], true, "{stale}");
    assert_eq!(fixture.manager.profile_admission_epoch.load(Ordering::Acquire), newer_epoch);
    assert_eq!(fixture.manager.profile_session.lock().unwrap().status().owner, owner);
}

#[test]
fn explicit_game_launch_only_replaces_the_confirmed_owner() {
    let fixture = Fixture::new();
    let first = ok(&fixture.manager,"profile_apply",json!({
        "id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"expectedActiveOwner":null
    }));
    let owner = first["profileSession"]["owner"].clone();
    let unconfirmed = request(&fixture.manager,"profile_game_launch",json!({
        "gameId":"dcs","expectedActiveOwner":null
    }));
    assert_eq!(unconfirmed["ok"],false,"{unconfirmed}");
    assert!(unconfirmed["error"].as_str().unwrap().contains("active profile changed"));
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire),0);
    assert_eq!(ok(&fixture.manager,"profile_status",json!({}))["profileSession"]["owner"],owner);

    let confirmed = ok(&fixture.manager,"profile_game_launch",json!({
        "gameId":"dcs","expectedActiveOwner":owner
    }));
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire),1);
    let current = confirmed["profileSession"]["owner"].clone();
    assert_ne!(current,owner);
    assert_eq!(current["profileId"],osg_profiles::DEFAULT_PROFILE_ID);
    assert_eq!(confirmed["launch"]["selectedGameId"],"dcs");

    let stale = request(&fixture.manager,"profile_game_launch",json!({
        "gameId":"x-plane-12","expectedActiveOwner":owner
    }));
    assert_eq!(stale["ok"],false,"{stale}");
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire),1);
    assert_eq!(ok(&fixture.manager,"profile_status",json!({}))["profileSession"]["owner"],current);
}

#[test]
fn game_launch_rechecks_confirmed_owner_after_slow_preparation() {
    let mut fixture = Fixture::new();
    let manager = Arc::get_mut(&mut fixture.manager).unwrap();
    let first = ok(manager,"profile_apply",json!({
        "id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1,"expectedActiveOwner":null
    }));
    let owner = first["profileSession"]["owner"].clone();
    let prepare = manager.profile_prepare.clone();
    let gate = Arc::new(Barrier::new(2));
    let paused = gate.clone();
    manager.profile_prepare = Arc::new(move |candidate| {
        let result = prepare(candidate);
        paused.wait();
        paused.wait();
        result
    });
    let pending = fixture.manager.clone();
    let worker = std::thread::spawn(move || request(&pending,"profile_game_launch",json!({
        "gameId":"dcs","expectedActiveOwner":owner
    })));
    gate.wait();
    let epoch = fixture.manager.profile_admission_epoch.load(Ordering::Acquire);
    fixture.manager.profile_session.lock().unwrap().stop().unwrap();
    assert_eq!(fixture.manager.profile_admission_epoch.load(Ordering::Acquire),epoch);
    gate.wait();
    let rejected = worker.join().unwrap();
    assert_eq!(rejected["ok"],false,"{rejected}");
    assert!(rejected["error"].as_str().unwrap().contains("active profile changed"),"{rejected}");
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire),0);
    assert!(fixture.manager.profile_session.lock().unwrap().status().owner.is_none());
}

#[test]
fn manual_game_launch_uses_active_profile_and_defaults_after_deactivation() {
    let fixture = Fixture::new();
    let selected = ok(&fixture.manager,"profile_create",json!({"name":"Only when active"}))["document"]["id"].clone();
    let active = ok(&fixture.manager,"profile_apply",json!({
        "id":selected,"expectedRevision":1,"expectedActiveOwner":null
    }));
    let with_active = ok(&fixture.manager,"profile_game_launch",json!({
        "gameId":"dcs","expectedActiveOwner":active["profileSession"]["owner"]
    }));
    assert_eq!(with_active["launch"]["profileId"],selected);
    ok(&fixture.manager,"profile_stop",json!({}));
    assert_eq!(fixture.manager.profile_automatic.config().0.manual_profile_id,selected.as_str().unwrap());
    fixture.games.processes.lock().unwrap().clear();
    let without_active = ok(&fixture.manager,"profile_game_launch",json!({
        "gameId":"x-plane-12","expectedActiveOwner":null
    }));
    assert_eq!(without_active["launch"]["profileId"],osg_profiles::DEFAULT_PROFILE_ID);
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire),2);
}

#[test]
fn blocked_preflight_keeps_status_and_stop_responsive_and_never_admits_late_stage() {
    let mut fixture = Fixture::new();
    let manager = Arc::get_mut(&mut fixture.manager).unwrap();
    ok(
        manager,
        "profile_apply",
        json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}),
    );
    let prepare = manager.profile_prepare.clone();
    let gate = Arc::new(Barrier::new(2));
    let paused = gate.clone();
    manager.profile_prepare = Arc::new(move |candidate| {
        let result = prepare(candidate);
        paused.wait();
        paused.wait();
        result
    });
    let manager = fixture.manager.clone();
    let worker = std::thread::spawn(move || launch(&manager, "dcs"));
    gate.wait();
    assert_eq!(
        ok(&fixture.manager, "profile_status", json!({}))["profileSession"]["state"],
        "running"
    );
    ok(&fixture.manager, "profile_stop", json!({}));
    assert!(fixture
        .manager
        .profile_session
        .lock()
        .unwrap()
        .status()
        .owner
        .is_none());
    gate.wait();
    assert!(worker.join().unwrap().is_err());
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 0);
    assert!(fixture
        .manager
        .profile_automatic
        .emergency
        .load(Ordering::Acquire));
}

#[test]
fn completed_newer_stop_cannot_be_rebased_by_an_older_apply_or_launch_arrival() {
    for command in ["profile_apply", "profile_game_launch"] {
        let mut fixture = Fixture::new();
        let manager = Arc::get_mut(&mut fixture.manager).unwrap();
        ok(
            manager,
            "profile_apply",
            json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}),
        );
        let preparations = Arc::new(AtomicUsize::new(0));
        let counted = preparations.clone();
        manager.profile_prepare = Arc::new(move |_| {
            counted.fetch_add(1, Ordering::AcqRel);
            Err("Stale arrival must never prepare".into())
        });
        // Capture the native arrival before its writer is admitted. A complete
        // real Stop then leaves both pending counts back at one: counts alone
        // cannot distinguish it from a Stop preceding the launch request.
        manager.profile_pending.fetch_add(1, Ordering::AcqRel);
        let _pending = PendingProfileApply(&manager.profile_pending);
        let _control = manager.profile_automatic.begin_control(command).unwrap();
        let arrival = manager.invalidate_learning();
        ok(manager, "profile_stop", json!({}));
        let policy_after_stop = manager.profile_automatic.config();
        assert_eq!(manager.profile_pending.load(Ordering::Acquire), 1);
        assert_eq!(
            manager
                .profile_automatic
                .pending_controls
                .load(Ordering::Acquire),
            1
        );
        assert_ne!(
            manager.profile_admission_epoch.load(Ordering::Acquire),
            arrival
        );
        if command == "profile_apply" {
            // Also cover Stop arriving after Apply's first parse/check but
            // before its separate Manual policy-pinning boundary.
            assert!(manager
                .pin_staged_manual(osg_profiles::DEFAULT_PROFILE_ID, arrival, None)
                .unwrap_err()
                .contains("cancelled"));
        }
        let mut request = if command == "profile_apply" {
            json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1})
        } else {
            json!({"gameId":"dcs"})
        };
        request["command"] = json!(command);
        request["clientId"] = json!("native-arrival-test");
        let reply = manager.request_inner_at(request, Some(arrival));
        assert_eq!(reply["ok"], false, "{command}: {reply}");
        assert!(reply["error"].as_str().unwrap().contains("cancelled"));
        assert!(manager
            .profile_session
            .lock()
            .unwrap()
            .status()
            .owner
            .is_none());
        assert!(manager.profile_automatic.emergency.load(Ordering::Acquire));
        assert_eq!(
            manager.profile_automatic.config(),
            policy_after_stop,
            "Stale Apply must not repin policy"
        );
        assert_eq!(preparations.load(Ordering::Acquire), 0);
        assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 0);
    }
}
#[test]
fn public_commands_cannot_supply_the_native_arrival_sidechannel() {
    let fixture = Fixture::new();
    for command in ["profile_apply", "profile_game_launch"] {
        for field in [
            "epoch",
            "admissionEpoch",
            "nativeArrivalEpoch",
            "arrivalEpoch",
        ] {
            let mut value = if command == "profile_apply" {
                json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1})
            } else {
                json!({"gameId":"dcs"})
            };
            value[field] = json!(fixture
                .manager
                .profile_admission_epoch
                .load(Ordering::Acquire));
            assert_eq!(
                request(&fixture.manager, command, value)["ok"],
                false,
                "{command}:{field}"
            );
        }
    }
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 0);
    assert!(fixture
        .manager
        .profile_session
        .lock()
        .unwrap()
        .status()
        .owner
        .is_none());
}

#[test]
fn manual_launch_holds_then_starts_once_with_real_watcher_and_optional_integration_absent() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    let result = launch(manager, "dcs").unwrap();
    assert_eq!(
        result["launch"],
        json!({"gameId":"dcs","profileId":osg_profiles::DEFAULT_PROFILE_ID,"selectedGameId":"dcs","state":"started"})
    );
    let coordinator = manager.profile_session.lock().unwrap();
    assert_eq!(coordinator.status().state, SessionState::Running);
    assert!(coordinator
        .status()
        .owner
        .as_ref()
        .unwrap()
        .game_id
        .is_none());
    drop(coordinator);
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 1);
    manager.poll_automatic();
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 1);
    assert!(launch(manager, "dcs").is_err());
    assert_eq!(
        manager.profile_session.lock().unwrap().status().state,
        SessionState::Running
    );
}
#[test]
fn automatic_no_matching_rule_launches_default_without_claiming_a_game_context() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    ok(
        manager,
        "profile_selection_mode",
        json!({"mode":"automatic"}),
    );
    // Wait for an actual watcher scan, even though there are no running games.
    let deadline = Instant::now() + Duration::from_secs(2);
    while manager.games.profile_activation_observations().is_err() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let result = launch(manager, "dcs").unwrap();
    assert!(result["launch"]["selectedGameId"].is_null());
    let before = manager
        .profile_session
        .lock()
        .unwrap()
        .status()
        .owner
        .clone();
    manager.poll_automatic();
    assert_eq!(
        manager.profile_session.lock().unwrap().status().owner,
        before
    );
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 1);
}
#[test]
fn automatic_matching_rule_adopts_the_actual_started_process_without_reapply() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    let selected = ok(manager, "profile_create", json!({"name":"Cockpit"}))["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        manager,
        "profile_game_rule_save",
        json!({"gameId":"dcs","profileId":selected,"priority":3,"enabled":true}),
    );
    ok(
        manager,
        "profile_selection_mode",
        json!({"mode":"automatic"}),
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while manager.games.profile_activation_observations().is_err() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let result = launch(manager, "dcs").unwrap();
    assert_eq!(result["launch"]["profileId"], selected);
    assert_eq!(result["launch"]["selectedGameId"], "dcs");
    let before = manager
        .profile_session
        .lock()
        .unwrap()
        .status()
        .owner
        .clone();
    manager.poll_automatic();
    assert_eq!(
        manager.profile_session.lock().unwrap().status().owner,
        before
    );
    assert_eq!(result["automatic"]["mode"], "automatic");
}
#[test]
fn uncertain_spawn_restores_only_profile_hardware_and_inhibits_poll_replay() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    fixture.games.fail_spawn.store(true, Ordering::Release);
    assert!(launch(manager, "dcs")
        .unwrap_err()
        .contains("Uncertain native spawn"));
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 1);
    assert!(fixture.games.processes.lock().unwrap().contains_key("dcs"));
    assert!(manager
        .profile_session
        .lock()
        .unwrap()
        .status()
        .owner
        .is_none());
    fixture.wait_game("dcs");
    manager.poll_automatic();
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 1);
    assert!(manager.profile_automatic.emergency.load(Ordering::Acquire));
}
#[test]
fn higher_priority_running_game_remains_selected_when_launching_another_game() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    let selected = ok(manager, "profile_create", json!({"name":"Flight"}))["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        manager,
        "profile_game_rule_save",
        json!({"gameId":"x-plane-12","profileId":selected,"priority":9,"enabled":true}),
    );
    fixture.games.processes.lock().unwrap().insert(
        "x-plane-12".into(),
        ProcessIdentity {
            pid: 99,
            path: executable("x-plane-12"),
            epoch: 8,
        },
    );
    fixture.wait_game("x-plane-12");
    ok(
        manager,
        "profile_selection_mode",
        json!({"mode":"automatic"}),
    );
    let result = launch(manager, "dcs").unwrap();
    assert_eq!(result["launch"]["profileId"], selected);
    assert_eq!(result["launch"]["selectedGameId"], "x-plane-12");
    let before = manager
        .profile_session
        .lock()
        .unwrap()
        .status()
        .owner
        .clone();
    manager.poll_automatic();
    assert_eq!(
        manager.profile_session.lock().unwrap().status().owner,
        before
    );
}
#[test]
fn stopped_manual_launch_can_resume_but_automatic_inhibition_requires_explicit_resume() {
    let fixture = Fixture::new();
    let manager = &fixture.manager;
    ok(manager, "profile_stop", json!({}));
    assert!(manager.profile_automatic.emergency.load(Ordering::Acquire));
    launch(manager, "dcs").unwrap();
    assert!(!manager.profile_automatic.emergency.load(Ordering::Acquire));
    ok(manager, "profile_stop", json!({}));
    fixture.games.processes.lock().unwrap().clear();
    // Corrupt an optional policy only in-memory, mirroring a retained read-only
    // settings file: a Manual Default can still be explicitly launched.
    manager.profile_automatic.state.lock().unwrap().read_only = true;
    launch(manager, "dcs").unwrap();
    ok(manager, "profile_stop", json!({}));
    {
        let mut state = manager.profile_automatic.state.lock().unwrap();
        state.settings.mode = SelectionMode::Automatic;
        state.read_only = false;
    }
    assert!(launch(manager, "dcs").is_err());
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 2);
}
#[test]
fn stop_during_process_wait_cancels_only_the_exact_hold_without_killing_or_replaying() {
    let fixture = Fixture::new();
    let manager = fixture.manager.clone();
    fixture.games.appear.store(false, Ordering::Release);
    let worker = std::thread::spawn(move || launch(&manager, "dcs"));
    let deadline = Instant::now() + Duration::from_secs(3);
    while fixture.games.spawns.load(Ordering::Acquire) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    ok(&fixture.manager, "profile_stop", json!({}));
    assert!(worker.join().unwrap().is_err());
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 1);
    assert!(fixture
        .manager
        .profile_session
        .lock()
        .unwrap()
        .status()
        .owner
        .is_none());
    fixture.games.processes.lock().unwrap().insert(
        "dcs".into(),
        ProcessIdentity {
            pid: 42,
            path: executable("dcs"),
            epoch: 1,
        },
    );
    fixture.wait_game("dcs");
    fixture.manager.poll_automatic();
    assert!(fixture
        .manager
        .profile_session
        .lock()
        .unwrap()
        .status()
        .owner
        .is_none());
}
#[test]
fn failed_preflight_preserves_prior_owner_and_cannot_resume_a_previous_stop() {
    let mut fixture = Fixture::new();
    let manager = Arc::get_mut(&mut fixture.manager).unwrap();
    ok(
        manager,
        "profile_apply",
        json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}),
    );
    let before = manager
        .profile_session
        .lock()
        .unwrap()
        .status()
        .owner
        .clone();
    manager.profile_prepare = Arc::new(|_| Err("Catalogue failed before effects".into()));
    assert!(launch(manager, "dcs")
        .unwrap_err()
        .contains("Catalogue failed"));
    assert_eq!(
        manager.profile_session.lock().unwrap().status().owner,
        before
    );
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire), 0);
    ok(manager, "profile_stop", json!({}));
    assert!(launch(manager, "dcs").is_err());
    assert!(manager.profile_automatic.emergency.load(Ordering::Acquire));
}

#[test]
fn corrupt_installation_metadata_blocks_game_launch_and_auto_but_keeps_hardware_apply() {
    let fixture=Fixture::with_installations(true);
    let manager=&fixture.manager;
    let setup=request(manager,"game_integration_setup",json!({"gameId":"dcs"}));
    assert_eq!(setup["ok"],false);
    assert!(setup["error"].as_str().unwrap().contains("Repair game installation metadata"),"{setup}");
    ok(manager,"profile_apply",json!({"id":osg_profiles::DEFAULT_PROFILE_ID,"expectedRevision":1}));
    let owner=manager.profile_session.lock().unwrap().status().owner.clone();
    assert!(owner.is_some());
    assert!(launch(manager,"dcs").unwrap_err().contains("Repair game installation metadata"));
    ok(manager,"profile_selection_mode",json!({"mode":"automatic"}));
    let(decision,observed,_)=manager.automatic_decision().unwrap();
    assert!(matches!(decision,Decision::Hold(message) if message.contains("Repair game installation metadata")));
    assert!(observed.is_empty());
    manager.poll_automatic();
    assert_eq!(manager.profile_session.lock().unwrap().status().owner,owner);
    assert_eq!(fixture.games.spawns.load(Ordering::Acquire),0);
    assert_eq!(std::fs::read(fixture._directory.path().join("games.installations-v1.json")).unwrap(),b"interrupted pinned executable metadata");
}
