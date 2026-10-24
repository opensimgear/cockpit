use super::*;
use osg_mapping::profile::Profile;
use osg_virtual_controller::{
    feeder::{Feeder, Message},
    session::{Channel, Factory},
    vjoy::{Axis, Binding, Driver, Position, Slot, Status},
};
use std::time::Instant;
#[path = "live_game_tests.rs"]
mod games;
#[path = "live_script_tests.rs"]
mod scripts;
#[derive(Default)]
struct State {
    calls: Vec<String>,
    dead: bool,
    busy: bool,
    blocked_recovery: bool,
    failed_stop: bool,
    delay_stop_once: Option<Duration>,
    stop_failures: Vec<String>,
    spawns: usize,
}
struct Backend {
    shared: Arc<Mutex<State>>,
    bindings: Vec<Binding>,
}
impl Driver for Backend {
    fn check(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn inspect(&mut self, id: u8) -> Result<Slot, String> {
        let b = self
            .bindings
            .iter()
            .find(|b| b.slot == id)
            .ok_or("Missing slot")?;
        Ok(Slot {
            id,
            status: if self.shared.lock().unwrap().busy {
                Status::Busy
            } else {
                Status::Free
            },
            owner_pid: None,
            buttons: b.layout.buttons,
            axes: b
                .axes
                .iter()
                .map(|u| Axis {
                    usage: *u,
                    min: 0,
                    max: 32767,
                })
                .collect(),
            continuous_hats: b.layout.hats,
            discrete_hats: 0,
            force_feedback: false,
        })
    }
    fn acquire(&mut self, id: u8) -> Result<(), String> {
        self.shared
            .lock()
            .unwrap()
            .calls
            .push(format!("acquire:{id}"));
        Ok(())
    }
    fn submit(&mut self, id: u8, p: &Position) -> Result<(), String> {
        self.shared
            .lock()
            .unwrap()
            .calls
            .push(format!("submit:{id}:{}", p.words[18]));
        Ok(())
    }
    fn release(&mut self, id: u8) {
        self.shared
            .lock()
            .unwrap()
            .calls
            .push(format!("release:{id}"));
    }
}
struct Child {
    feeder: Feeder<Backend>,
    shared: Arc<Mutex<State>>,
    clock: Instant,
    terminated: bool,
}
impl Channel for Child {
    fn request(&mut self, request: &Message) -> Result<(), String> {
        if matches!(request, Message::Recover { .. })
            && self.shared.lock().unwrap().blocked_recovery
        {
            return Err("External owner prevents recovery".into());
        }
        let stopping = matches!(request, Message::Stop { .. });
        if stopping {
            let delay = self.shared.lock().unwrap().delay_stop_once.take();
            if let Some(delay) = delay {
                std::thread::sleep(delay);
            }
        }
        let result = if stopping && self.shared.lock().unwrap().failed_stop {
            Err("Stop acknowledgement lost".into())
        } else {
            self.feeder
                .receive(request.clone(), self.clock.elapsed().as_micros() as u64 + 1)
        };
        if stopping {
            if let Err(error) = &result {
                self.shared
                    .lock()
                    .unwrap()
                    .stop_failures
                    .push(error.clone());
            }
        }
        result
    }
    fn alive(&mut self) -> bool {
        !self.terminated && !self.shared.lock().unwrap().dead
    }
    fn terminate(&mut self) {
        self.terminated = true;
        self.feeder.stop();
    }
}
fn setup() -> (
    Service,
    tempfile::TempDir,
    Profile,
    Vec<Assignment>,
    Arc<Mutex<State>>,
) {
    let (old, root, profile, assignments, _) = super::tests::setup();
    old.shutdown();
    drop(old);
    Store::new(root.path().into())
        .save_assignments(&profile.id, &assignments)
        .unwrap();
    let bindings: Vec<_> = profile
        .targets
        .iter()
        .enumerate()
        .map(|(i, t)| Binding {
            target: t.id.clone(),
            slot: i as u8 + 1,
            axes: (0..t.layout.axes.len()).map(|n| 0x30 + n as u32).collect(),
            layout: t.layout.clone(),
        })
        .collect();
    let state = Arc::new(Mutex::new(State::default()));
    let child_state = state.clone();
    let child_bindings = bindings.clone();
    let factory: Factory = Arc::new(move || {
        child_state.lock().unwrap().spawns += 1;
        Ok(Box::new(Child {
            feeder: Feeder::new(Backend {
                shared: child_state.clone(),
                bindings: child_bindings.clone(),
            }),
            shared: child_state.clone(),
            clock: Instant::now(),
            terminated: false,
        }))
    });
    let service = Service::with_output(
        Arc::new(ControllerService::new(None, false)),
        Some(root.path().into()),
        Arc::new(|| Ok(vec![])),
        factory,
        true,
    );
    service
        .request(
            &json!({"command":"mapping_targets_save","profileId":profile.id,"bindings":bindings}),
        )
        .unwrap();
    (service, root, profile, assignments, state)
}
fn wait_for(service: &Service, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let status = service
            .request(&json!({"command":"mapping_status"}))
            .unwrap();
        if predicate(&status) {
            return status;
        }
        assert!(Instant::now() < deadline, "Status: {status}");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn qualified_fixture_requires_opt_in_and_delivers_edges_without_ui_polling() {
    let (service, _root, profile, assignments, state) = setup();
    assert!(service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .unwrap_err()
        .contains("Enable"));
    assert_eq!(state.lock().unwrap().spawns, 0);
    service
        .request(&json!({"command":"mapping_virtual_enable","enabled":true}))
        .unwrap();
    let run = service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .unwrap();
    assert_eq!(run["preview"]["mode"], BACKEND);
    for command in [
        "mapping_preview_start",
        "mapping_virtual_start",
        "mapping_virtual_recover",
    ] {
        assert!(service
            .request(&json!({"command":command,"profileId":profile.id}))
            .is_err());
    }
    for pressed in [true, false, true, false] {
        service.request(&json!({"command":"mapping_simulate","token":run["token"],"source":assignments[0].source,
            "changes":[{"control":"trigger","value":{"kind":"button","value":pressed}}]})).unwrap();
    }
    // The lease lasts 250 ms. No UI/status calls occur during this interval.
    std::thread::sleep(Duration::from_millis(330));
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["mode"],
        BACKEND
    );
    let calls = state.lock().unwrap().calls.clone();
    let edges: Vec<_> = calls
        .iter()
        .filter(|c| c.starts_with("submit:1:"))
        .cloned()
        .collect();
    assert!(
        edges
            .windows(4)
            .any(|w| w == ["submit:1:1", "submit:1:0", "submit:1:1", "submit:1:0"]),
        "{edges:?}"
    );
    service
        .request(&json!({"command":"mapping_virtual_enable","enabled":false}))
        .unwrap();
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["running"],
        false
    );
    assert!(state.lock().unwrap().calls.iter().any(|c| c == "release:2"));
    service
        .request(&json!({"command":"mapping_virtual_recover"}))
        .unwrap();
    service.shutdown();
}
#[test]
fn child_death_stops_mapping_and_preserves_explicit_recovery_failure() {
    let (service, _root, profile, _assignments, state) = setup();
    service
        .request(&json!({"command":"mapping_virtual_enable","enabled":true}))
        .unwrap();
    service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .unwrap();
    {
        let mut state = state.lock().unwrap();
        state.dead = true;
        state.blocked_recovery = true;
    }
    let status = wait_for(&service, |s| s["recoveryRequired"] == true);
    assert_eq!(status["running"], false);
    assert!(status["fault"]
        .as_str()
        .unwrap()
        .contains("requires attention"));
    assert!(service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .is_err());
    assert!(service
        .request(&json!({"command":"mapping_virtual_recover"}))
        .is_err());
    {
        let mut state = state.lock().unwrap();
        state.dead = false;
        state.blocked_recovery = false;
    }
    service
        .request(&json!({"command":"mapping_virtual_recover"}))
        .unwrap();
    let status = service
        .request(&json!({"command":"mapping_status"}))
        .unwrap();
    assert_eq!(status["running"], false);
    assert_eq!(status["recoveryRequired"], false);
    service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .unwrap();
    service.request(&json!({"command":"mapping_stop"})).unwrap();
    service.shutdown();
}
#[test]
fn unavailable_slots_and_changed_profiles_cannot_replace_workspace_or_hide_devices() {
    let (service, root, mut profile, _assignments, state) = setup();
    service
        .request(&json!({"command":"mapping_virtual_enable","enabled":true}))
        .unwrap();
    assert!(service
        .request(&json!({"command":"mapping_virtual_recover"}))
        .is_err());
    state.lock().unwrap().busy = true;
    assert!(service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .is_err());
    assert!(state.lock().unwrap().calls.is_empty());
    state.lock().unwrap().busy = false;
    profile.targets[0].layout.buttons += 1;
    Store::new(root.path().into()).save(&profile).unwrap();
    let spawns = state.lock().unwrap().spawns;
    assert!(service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .is_err());
    assert_eq!(state.lock().unwrap().spawns, spawns);
    profile.targets[0].layout.buttons -= 1;
    profile.requirements.device_hiding = true;
    Store::new(root.path().into()).save(&profile).unwrap();
    assert!(service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .unwrap_err()
        .contains("hiding"));
    assert_eq!(state.lock().unwrap().spawns, spawns);
    service.shutdown();
}

#[test]
fn disabling_stays_disabled_when_stop_recovery_reports_a_failure() {
    let (service, root, profile, _, state) = setup();
    service
        .request(&json!({"command":"mapping_virtual_enable","enabled":true}))
        .unwrap();
    service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .unwrap();
    {
        let mut state = state.lock().unwrap();
        state.failed_stop = true;
        state.blocked_recovery = true;
    }
    assert!(service
        .request(&json!({"command":"mapping_virtual_enable","enabled":false}))
        .is_err());
    let status = service
        .request(&json!({"command":"mapping_status"}))
        .unwrap();
    assert_eq!(status["running"], false);
    assert_eq!(status["virtualDevices"]["enabled"], false);
    assert_eq!(status["recoveryRequired"], true);
    assert!(
        !crate::settings::Settings::open(Some(root.path().join("local-settings.json")))
            .value
            .virtual_devices
    );
    service.shutdown();
}

#[test]
fn restart_status_recovery_and_ambiguous_acknowledgement_never_resume_mapping() {
    use osg_virtual_controller::output_journal::{Phase, Store as RecoveryStore};
    let (service, root, profile, _, state) = setup();
    let path = root.path().join("output-recovery.json");
    let bindings = service.settings.lock().unwrap().value.targets[&profile.id].clone();
    let run = RunToken {
        session: uuid::Uuid::new_v4().to_string(),
        generation: Counter(1),
    };
    let mut journal = RecoveryStore::open(path.clone()).unwrap();
    journal.begin(run.clone(), bindings.clone()).unwrap();
    drop(journal);
    let status = service
        .request(&json!({"command":"mapping_status"}))
        .unwrap();
    assert_eq!(status["outputRecovery"]["phase"], "intent");
    assert!(service
        .request(&json!({"command":"mapping_virtual_recover"}))
        .is_err());
    assert!(service
        .request(&json!({"command":"mapping_virtual_acknowledge"}))
        .is_err());
    assert!(service
        .request(&json!({"command":"mapping_virtual_acknowledge","token":{}}))
        .is_err());
    assert_eq!(state.lock().unwrap().spawns, 0);
    service
        .request(&json!({"command":"mapping_preview_start","profileId":profile.id}))
        .unwrap();
    service.request(&json!({"command":"mapping_stop"})).unwrap();
    service
        .request(&json!({"command":"mapping_virtual_acknowledge","token":run}))
        .unwrap();
    let mut journal = RecoveryStore::open(path.clone()).unwrap();
    journal.begin(run.clone(), bindings).unwrap();
    journal.transition(Phase::Confirmed).unwrap();
    drop(journal);
    service
        .request(&json!({"command":"mapping_virtual_recover"}))
        .unwrap();
    let status = service
        .request(&json!({"command":"mapping_status"}))
        .unwrap();
    assert_eq!(status["running"], false);
    assert_eq!(status["outputRecovery"]["required"], false);
    assert!(state
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|c| c == "submit:1:0"));
    osg_persistence::test_support::write_evidence(&path,"recovery-output",Some(b"corrupt")).unwrap();
    assert!(service
        .request(&json!({"command":"mapping_status"}))
        .unwrap()["outputRecovery"]["error"]
        .is_string());
    service
        .request(&json!({"command":"mapping_preview_start","profileId":profile.id}))
        .unwrap();
    service.shutdown();
}

#[test]
fn unqualified_startup_keeps_restart_recovery_and_acknowledgement_behind_qualification() {
    let root = tempfile::tempdir().unwrap();
    let service = Service::with_output(
        Arc::new(ControllerService::new(None, false)),
        Some(root.path().into()),
        Arc::new(|| panic!("no inspection on recovery gate")),
        Arc::new(|| panic!("no output on recovery gate")),
        false,
    );
    for command in [
        "mapping_virtual_recover",
        "mapping_virtual_acknowledge",
        "mapping_virtual_start",
    ] {
        assert_eq!(
            service.request(&json!({"command":command})).unwrap_err(),
            QUALIFICATION
        );
    }
    assert_eq!(
        service
            .request(&json!({"command":"mapping_status"}))
            .unwrap()["outputRecovery"]["required"],
        false
    );
    assert!(!root.path().join("output-recovery.json").exists());
    assert!(!root.path().join("output-recovery.lock").exists());
}
