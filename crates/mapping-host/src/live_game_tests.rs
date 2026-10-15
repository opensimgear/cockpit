use super::*;
#[derive(Default)]
struct Visibility {
    phase: String,
    fail_start: bool,
    fail_stop: bool,
}
fn game(
    live: bool,
    hiding: bool,
) -> (
    Service,
    tempfile::TempDir,
    Value,
    Arc<Mutex<State>>,
    Arc<Mutex<Visibility>>,
) {
    let (mut service, root, mut profile, _, state) = setup();
    profile.requirements.virtual_devices = live;
    profile.requirements.device_hiding = hiding;
    service.store().unwrap().save(&profile).unwrap();
    service
        .request(&json!({"command":"mapping_virtual_enable","enabled":true}))
        .unwrap();
    let bindings = service.settings.lock().unwrap().value.targets[&profile.id].clone();
    let driver_state = state.clone();
    service.inspector = Arc::new(move || {
        let mut driver = Backend {
            shared: driver_state.clone(),
            bindings: bindings.clone(),
        };
        bindings.iter().map(|b| driver.inspect(b.slot)).collect()
    });
    let visibility = Arc::new(Mutex::new(Visibility::default()));
    let shared = visibility.clone();
    let calls = state.clone();
    service.game_visibility = Arc::new(move |operation, value| {
        calls
            .lock()
            .unwrap()
            .calls
            .push(format!("visibility:{operation}"));
        let mut visibility = shared.lock().unwrap();
        match operation {
            "capture" => Ok(value.clone()),
            "preflight" => Ok(json!({"intent":value,"policy":null})),
            "start" => {
                if visibility.fail_start {
                    return Err("Visibility activation failed".into());
                }
                visibility.phase = "active".into();
                Ok(json!({"active":true,"phase":"active"}))
            }
            "poll" => Ok(json!({"active":visibility.phase=="active","phase":visibility.phase})),
            "stop" => {
                if visibility.fail_stop {
                    return Err("External visibility policy changed; preserved".into());
                }
                visibility.phase = "restored".into();
                Ok(json!({"active":false,"phase":"restored"}))
            }
            "abandon" => Ok(json!({})),
            _ => panic!("Unknown operation {operation}"),
        }
    });
    let intent = hiding
        .then(|| json!({"mode":"hidhide-session","sourceIds":[uuid::Uuid::new_v4().to_string()]}));
    let capture = service.capture_saved(&profile.id, intent).unwrap();
    (service, root, capture, state, visibility)
}
#[test]
fn independent_optional_features_prepare_neutral_output_then_visibility_then_arm() {
    for (live, hiding) in [(false, false), (true, false), (false, true), (true, true)] {
        let (s, _root, capture, state, visibility) = game(live, hiding);
        let settings = serde_json::to_value(&s.settings.lock().unwrap().value).unwrap();
        let journal = s.game_snapshot(&capture).unwrap();
        assert!(!state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c.starts_with("acquire:")));
        s.game_take(&journal).unwrap();
        s.game_arm(&journal).unwrap();
        assert_eq!(s.router.lock().unwrap().live(), live);
        assert_eq!(
            s.router.lock().unwrap().runtime.token().unwrap().session,
            journal["owner"]
        );
        if hiding {
            assert_eq!(visibility.lock().unwrap().phase, "active");
        }
        if live && hiding {
            let calls = state.lock().unwrap().calls.clone();
            let at = |name| calls.iter().position(|c| c == name).unwrap();
            assert!(at("acquire:1") < at("visibility:start"));
            assert!(at("submit:1:0") < at("visibility:start"));
        }
        let token = s.router.lock().unwrap().runtime.token().unwrap();
        s.request(&json!({"command":"mapping_simulate","token":token,"source":capture["assignments"][0]["source"],"changes":[{"control":"trigger","value":{"kind":"button","value":true}}]})).unwrap();
        if live {
            let until = Instant::now() + Duration::from_secs(2);
            while !state
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|c| c == "submit:1:1")
            {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        s.game_stop(&journal).unwrap();
        if live && hiding {
            let calls = state.lock().unwrap().calls.clone();
            assert!(
                calls.iter().position(|c| c == "release:1").unwrap()
                    < calls.iter().position(|c| c == "visibility:stop").unwrap()
            );
        }
        s.game_resume(&journal, false).unwrap();
        assert!(!s.router.lock().unwrap().live());
        assert_eq!(
            serde_json::to_value(&s.settings.lock().unwrap().value).unwrap(),
            settings
        );
        s.shutdown();
    }
}
#[test]
fn visibility_failure_and_loss_release_prepared_or_running_output() {
    for during_start in [true, false] {
        let (s, _root, capture, state, visibility) = game(true, true);
        let journal = s.game_snapshot(&capture).unwrap();
        s.game_take(&journal).unwrap();
        if during_start {
            visibility.lock().unwrap().fail_start = true;
            assert!(s.game_arm(&journal).is_err());
        } else {
            s.game_arm(&journal).unwrap();
            visibility.lock().unwrap().phase = "restored".into();
            wait_for(&s, |status| status["running"] == false);
        }
        assert!(state.lock().unwrap().calls.iter().any(|c| c == "release:1"));
        assert!(!s.router.lock().unwrap().live());
        s.game_stop(&journal).unwrap();
        s.game_resume(&journal, false).unwrap();
        s.shutdown();
    }
}
#[test]
fn output_failure_stops_owned_visibility_and_conflicts_prevent_resume() {
    let (s, _root, capture, state, visibility) = game(true, true);
    let journal = s.game_snapshot(&capture).unwrap();
    s.game_take(&journal).unwrap();
    s.game_arm(&journal).unwrap();
    state.lock().unwrap().dead = true;
    wait_for(&s, |status| status["running"] == false);
    assert_eq!(visibility.lock().unwrap().phase, "restored");
    visibility.lock().unwrap().fail_stop = true;
    assert!(s.game_stop(&journal).unwrap_err().contains("preserved"));
    assert!(!s.router.lock().unwrap().live());
    visibility.lock().unwrap().fail_stop = false;
    s.game_stop(&journal).unwrap();
    s.game_resume(&journal, true).unwrap();
    assert!(!s.router.lock().unwrap().runtime.token().is_ok());
    s.shutdown();
}
#[test]
fn missing_optional_readiness_and_false_qualification_do_not_take_prior_run() {
    let (mut s, _root, capture, state, _visibility) = game(true, false);
    s.request(&json!({"command":"mapping_preview_start","profileId":capture["profile"]["id"]}))
        .unwrap();
    let prior = s.router.lock().unwrap().runtime.token().unwrap();
    s.qualified = false;
    assert!(s
        .game_snapshot(&capture)
        .unwrap_err()
        .contains("not qualified"));
    s.qualified = true;
    s.output_factory = Arc::new(|| Err("Optional output dependency missing".into()));
    assert!(s
        .game_snapshot(&capture)
        .unwrap_err()
        .contains("dependency missing"));
    assert_eq!(s.router.lock().unwrap().runtime.token().unwrap(), prior);
    assert!(!state
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|c| c.starts_with("acquire:")));
    assert!(s.game_lease.lock().unwrap().is_none());
    s.shutdown();
}
#[test]
fn explicit_restart_recovery_checks_output_owner_and_never_rearms() {
    for matching in [false, true] {
        let (s, _root, capture, state, _visibility) = game(true, false);
        let journal = s.game_snapshot(&capture).unwrap();
        let owner = if matching {
            journal["owner"].as_str().unwrap().to_owned()
        } else {
            uuid::Uuid::new_v4().to_string()
        };
        let path = s.recovery_path().unwrap();
        let mut store = osg_virtual_controller::output_journal::Store::open(path.clone()).unwrap();
        store
            .begin(
                RunToken {
                    session: owner,
                    generation: Counter(1),
                },
                serde_json::from_value(capture["targets"].clone()).unwrap(),
            )
            .unwrap();
        store
            .transition(osg_virtual_controller::output_journal::Phase::Confirmed)
            .unwrap();
        drop(store);
        let before = std::fs::read(&path).unwrap();
        if matching {
            s.game_stop(&journal).unwrap();
            s.game_resume(&journal, true).unwrap();
        } else {
            assert!(s
                .game_stop(&journal)
                .unwrap_err()
                .contains("Another output owner"));
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        assert!(s.router.lock().unwrap().runtime.token().is_err());
        if matching {
            assert!(state
                .lock()
                .unwrap()
                .calls
                .iter()
                .filter(|c| c.starts_with("submit:"))
                .all(|c| c.ends_with(":0")));
            assert!(!osg_virtual_controller::output_journal::inspect(&path)
                .unwrap()
                .unwrap()
                .pending());
        } else {
            assert!(!state
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|c| c.starts_with("acquire:")));
        }
        s.shutdown();
    }
}
#[test]
fn emergency_stop_during_visibility_preparation_cannot_arm_later() {
    let (mut s, _root, capture, state, _visibility) = game(true, true);
    let (entered, notice) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let once = std::sync::atomic::AtomicBool::new(false);
    let resume = Mutex::new(resume);
    let original = s.game_visibility.clone();
    s.game_visibility = Arc::new(move |operation, value| {
        if operation == "poll" && !once.swap(true, Ordering::AcqRel) {
            entered.send(()).unwrap();
            resume
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
        }
        original(operation, value)
    });
    let s = Arc::new(s);
    let journal = s.game_snapshot(&capture).unwrap();
    s.game_take(&journal).unwrap();
    let worker = s.clone();
    let saved = journal.clone();
    let thread = std::thread::spawn(move || worker.game_arm(&saved));
    notice.recv_timeout(Duration::from_secs(3)).unwrap();
    s.request(&json!({"command":"mapping_stop"})).unwrap();
    release.send(()).unwrap();
    assert!(thread.join().unwrap().unwrap_err().contains("cancelled"));
    assert!(state.lock().unwrap().calls.iter().any(|c| c == "release:1"));
    assert!(s.router.lock().unwrap().runtime.token().is_err());
    s.game_stop(&journal).unwrap();
    s.game_resume(&journal, false).unwrap();
    assert!(s.router.lock().unwrap().runtime.token().is_err());
    s.shutdown();
}
#[test]
fn captured_script_uses_real_isolated_worker_and_feeder_after_gate_checks() {
    let (mut s, _root, mut capture, state, _visibility) = game(true, false);
    capture["profile"]["scripts"] = json!([{
        "id":"logic","source":"fn on_start(ctx) { transaction(#{},[],[]) } fn on_event(ctx) { transaction(#{},[button(\"result\",ctx.inputs.left.trigger.value)],[]) } fn on_timer(ctx) { transaction(ctx.state,[],[]) }",
        "inputs":[{"kind":"source","role":"left","control":"trigger"}],
        "outputs":[{"id":"result","output":{"target":"main","control":{"kind":"button","index":1}}}]
    }]);
    assert!(s.game_snapshot(&capture).unwrap_err().contains("disabled"));
    s.script_live_qualified = true;
    s.script_factory =
        Arc::new(|source| osg_logic::worker::Worker::spawn(super::scripts::helper(), source));
    let journal = s.game_snapshot(&capture).unwrap();
    s.game_take(&journal).unwrap();
    s.game_arm(&journal).unwrap();
    let ready = wait_for(&s, |status| {
        status["scripts"][0]["pending"] == 0 && status["scripts"][0]["inflight"] == false
    });
    assert!(ready["scripts"][0]["fault"].is_null(), "{ready}");
    let token = s.router.lock().unwrap().runtime.token().unwrap();
    s.request(&json!({"command":"mapping_simulate","token":token,"source":capture["assignments"][0]["source"],"changes":[{"control":"trigger","value":{"kind":"button","value":true}}]})).unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    while !state
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|c| c == "submit:1:3")
    {
        assert!(
            Instant::now() < until,
            "status={} calls={:?}",
            s.request(&json!({"command":"mapping_status"})).unwrap(),
            state.lock().unwrap().calls
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    s.game_stop(&journal).unwrap();
    s.game_resume(&journal, false).unwrap();
    assert!(!s.router.lock().unwrap().live());
    s.shutdown();
}
#[test]
fn resuming_a_preview_never_activates_optional_features_from_its_profile_flags() {
    let (mut s, _root, mut desired, state, _visibility) = game(false, false);
    let mut prior: Profile = serde_json::from_value(desired["profile"].clone()).unwrap();
    prior.requirements.virtual_devices = true;
    prior.requirements.device_hiding = true;
    s.store().unwrap().save(&prior).unwrap();
    s.request(&json!({"command":"mapping_preview_start","profileId":prior.id}))
        .unwrap();
    s.qualified = false;
    // Deliberately leave saved target bindings absent: a recording preview never
    // required them, and normal resume must not begin requiring an optional SDK.
    s.settings.lock().unwrap().value.targets.clear();
    desired["targets"] = json!([]);
    let journal = s.game_snapshot(&desired).unwrap();
    assert_eq!(
        journal["baseline"]["profile"]["requirements"]["virtualDevices"],
        true
    );
    s.game_take(&journal).unwrap();
    s.game_arm(&journal).unwrap();
    s.game_stop(&journal).unwrap();
    s.game_resume(&journal, false).unwrap();
    assert!(s.router.lock().unwrap().runtime.token().is_ok());
    assert!(!s.router.lock().unwrap().live());
    assert_eq!(state.lock().unwrap().spawns, 0);
    assert!(!state
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|c| c == "visibility:start"));
    s.shutdown();
}
