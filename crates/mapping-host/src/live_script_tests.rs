use super::*;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};
const STARTUP_ADMITTED: &str = "fixture-startup-admitted";
const STARTUP_ATTEMPTS: usize = 3;
const RESPONSE_TIMEOUT: &str =
    "Script helper response exceeded 100 ms; evaluation is limited to 5 ms.";
const CLEANED_HEARTBEAT: &str = "Mapping heartbeat expired; fallback submitted and ownership released. Fallback recovery completed; mapping remains stopped.";

pub(super) fn helper() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        if let Some(path) = std::env::var_os("OSG_TEST_LOGIC_WORKER") {
            let path = PathBuf::from(path);
            assert!(path.is_file());
            return path;
        }
        // Plain cargo test also runs the real helper. A distinct target avoids
        // waiting on the parent cargo process's build lock.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let target = std::env::temp_dir().join("opensimgear-mapping-script-test-helper");
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--locked", "--manifest-path"])
            .arg(root.join("crates/logic/Cargo.toml"))
            .args(["--bin", "osg-logic", "--target-dir"])
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success());
        target.join("debug").join(if cfg!(windows) {
            "osg-logic.exe"
        } else {
            "osg-logic"
        })
    })
}
fn scripted(
    source: &str,
) -> (
    Service,
    tempfile::TempDir,
    Profile,
    Vec<Assignment>,
    Arc<Mutex<State>>,
) {
    let (mut service, root, mut profile, assignments, state) = setup();
    // The marker is committed atomically with the original startup transaction.
    // A later timer fault must never be mistaken for a failed initial callback.
    let source = if source.contains("fn on_start(ctx)") {
        format!(
            "{}\nfn on_start(ctx) {{ let result = fixture_on_start(ctx); if !result.contains(\"logs\") {{ result.logs = []; }} result.logs.push(\"{STARTUP_ADMITTED}\"); result }}",
            source.replacen("fn on_start(ctx)", "fn fixture_on_start(ctx)", 1)
        )
    } else {
        source.into()
    };
    profile.scripts = vec![serde_json::from_value(json!({
        "id":"logic", "source":source,
        "inputs":[{"kind":"source","role":"left","control":"trigger"},{"kind":"source","role":"right","control":"trigger"}],
        "outputs":[{"id":"result","output":{"target":"main","control":{"kind":"button","index":1}}}]
    })).unwrap()];
    service.store().unwrap().save(&profile).unwrap();
    service.script_factory = Arc::new(|source| osg_logic::worker::Worker::spawn(helper(), source));
    service
        .request(&json!({"command":"mapping_virtual_enable","enabled":true}))
        .unwrap();
    (service, root, profile, assignments, state)
}
const DIRECT: &str = r#"
fn on_start(ctx) { transaction(#{}, [button("result",false)], []) }
fn on_event(ctx) { transaction(#{}, [button("result",ctx.inputs.left.trigger.value)], []) }
fn on_timer(ctx) { transaction(ctx.state,[],[]) }
"#;
fn start(service: &Service, profile: &Profile, state: &Arc<Mutex<State>>) -> Value {
    let mut failed_token = None;
    for attempt in 1..=STARTUP_ATTEMPTS {
        let started = service
            .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
            .unwrap();
        assert_ne!(failed_token.as_ref(), Some(&started["token"]),
            "A fresh fixture admission must have a different native owner");
        let ready = wait_for(service, |v| {
            v["scripts"][0]["pending"] == 0 && v["scripts"][0]["inflight"] == false
        });
        let script = &ready["scripts"][0];
        if script["fault"].is_null() {
            assert!(
                script["logs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|log| log == STARTUP_ADMITTED),
                "The actual startup transaction must be admitted before fixture input: {ready}"
            );
            return started["token"].clone();
        }
        assert!(
            script["fault"] == RESPONSE_TIMEOUT
                && script["logs"] == json!([])
                && script["started"] == true
                && script["timers"] == 0,
            "Only an uncommitted initial callback deadline may retry fixture admission: {ready}"
        );
        // Production correctly rejects even a queued reply after 100 ms. Before
        // any fixture input, Stop must fully release this failed run's output.
        if let Err(error) = service.request(&json!({"command":"mapping_stop"})) {
            // Feeder expiry may complete fallback/release before Stop reaches
            // it. Production still returns that diagnostic after successful
            // cleanup. No other Stop error authorizes a fixture retry.
            assert_eq!(error, CLEANED_HEARTBEAT, "Unresolved fixture cleanup");
        }
        let stopped = service
            .request(&json!({"command":"mapping_status"}))
            .unwrap();
        assert!(
            stopped["running"] == false
                && stopped["ready"] == false
                && stopped["recoveryRequired"] == false
                && stopped["outputRecovery"]["required"] == false
                && stopped["token"].is_null()
                && stopped["scripts"][0]["logs"] == json!([])
                && stopped["scripts"][0]["timers"] == 0,
            "Failed fixture cleanup remains unresolved: {stopped}"
        );
        let record =
            osg_virtual_controller::output_journal::inspect(&service.recovery_path().unwrap())
                .unwrap()
                .expect("A completed failed startup retains its clean native journal");
        assert_eq!(
            record.phase,
            osg_virtual_controller::output_journal::Phase::Clean
        );
        assert_eq!(
            serde_json::to_value(&record.token).unwrap(),
            started["token"]
        );
        let bindings = service.settings.lock().unwrap().value.targets[&profile.id].clone();
        assert_eq!(
            serde_json::to_value(&record.bindings).unwrap(),
            serde_json::to_value(&bindings).unwrap()
        );
        {
            let state = state.lock().unwrap();
            for binding in &bindings {
                let acquire = format!("acquire:{}", binding.slot);
                let release = format!("release:{}", binding.slot);
                let submit = format!("submit:{}:", binding.slot);
                let calls = state
                    .calls
                    .iter()
                    .filter(|call| {
                        **call == acquire || **call == release || call.starts_with(&submit)
                    })
                    .collect::<Vec<_>>();
                let acquired = calls.iter().filter(|call| ***call == acquire).count();
                assert!(acquired > 0);
                assert_eq!(
                    acquired,
                    calls.iter().filter(|call| ***call == release).count(),
                    "Every failed owner must release slot {} before fresh Start",
                    binding.slot
                );
                assert_eq!(*calls.last().unwrap(), &release);
                let fallback = format!(
                    "{submit}{}",
                    binding.report(&binding.fallback()).unwrap().words[18]
                );
                assert_eq!(
                    *calls[calls.len() - 2],
                    fallback,
                    "The released slot must have acknowledged its actual fallback"
                );
            }
        }
        let assignments = service
            .store()
            .unwrap()
            .load_assignments(&profile.id)
            .unwrap();
        assert!(
            service
                .request(
                    &json!({"command":"mapping_simulate","token":started["token"],
            "source":assignments[0].source,"changes":[]})
                )
                .is_err(),
            "A stopped fixture token must reject input before a fresh Start"
        );
        failed_token = Some(started["token"].clone());
        assert!(attempt < STARTUP_ATTEMPTS, "Initial script callback missed its strict deadline on every fresh fixture admission: {ready}");
    }
    unreachable!("Every bounded fixture admission either returned or failed")
}
fn press(service: &Service, token: &Value, assignment: &Assignment, pressed: bool) {
    service
        .request(
            &json!({"command":"mapping_simulate","token":token,"source":assignment.source,
        "changes":[{"control":"trigger","value":{"kind":"button","value":pressed}}]}),
        )
        .unwrap();
}
fn submitted(state: &Arc<Mutex<State>>, expected: &str) {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        let calls = state.lock().unwrap().calls.clone();
        if calls
            .iter()
            .filter(|call| call.starts_with("submit:1:"))
            .last()
            .is_some_and(|call| call == expected)
        {
            return;
        }
        assert!(Instant::now() < until, "Missing {expected}: {calls:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn production_script_gate_rejects_before_any_worker_or_output_and_syntax_cannot_acquire() {
    let (mut service, _root, profile, _, state) = scripted(DIRECT);
    service.script_factory = Arc::new(|_| panic!("unqualified script must not spawn"));
    assert!(!SCRIPT_LIVE_QUALIFIED);
    assert!(service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .unwrap_err()
        .contains("disabled"));
    assert_eq!(state.lock().unwrap().spawns, 0);
    service.shutdown();

    let (mut service, _root, profile, _, state) = scripted("fn broken(");
    service.script_live_qualified = true;
    assert!(service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .is_err());
    assert_eq!(state.lock().unwrap().spawns, 0);
    service.shutdown();
}
#[test]
fn isolated_script_transactions_reach_feeder_and_stop_rejects_late_input() {
    let (mut service, _root, profile, assignments, state) = scripted(DIRECT);
    service.script_live_qualified = true;
    let token = start(&service, &profile, &state);
    press(&service, &token, &assignments[0], true);
    submitted(&state, "submit:1:3");
    press(&service, &token, &assignments[0], false);
    submitted(&state, "submit:1:0");
    service.request(&json!({"command":"mapping_stop"})).unwrap();
    let calls = state.lock().unwrap().calls.clone();
    assert!(calls.iter().any(|c| c == "release:1"));
    assert!(service.request(&json!({"command":"mapping_simulate","token":token,"source":assignments[0].source,"changes":[]})).is_err());
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(state.lock().unwrap().calls, calls);
    service.shutdown();
}
#[test]
fn callback_failure_and_budget_exhaustion_release_script_output_without_partial_transactions() {
    for failure in ["throw \"fixture\";", "while true {}"] {
        let source = format!(
            r#"
fn on_start(ctx) {{ transaction(#{{}}, [button("result",true)], []) }}
fn on_event(ctx) {{ let partial = button("result",true); {failure} transaction(#{{}},[partial],[]) }}
fn on_timer(ctx) {{ transaction(ctx.state,[],[]) }}
"#
        );
        let (mut service, _root, profile, assignments, state) = scripted(&source);
        service.script_live_qualified = true;
        let token = start(&service, &profile, &state);
        submitted(&state, "submit:1:2");
        press(&service, &token, &assignments[0], true);
        wait_for(&service, |v| v["scripts"][0]["fault"].is_string());
        submitted(&state, "submit:1:1");
        assert_eq!(
            service
                .request(&json!({"command":"mapping_status"}))
                .unwrap()["mode"],
            BACKEND
        );
        service.request(&json!({"command":"mapping_stop"})).unwrap();
        service.shutdown();
    }
}

#[test]
fn delayed_output_setup_retries_only_a_cleaned_up_startup_without_replaying_held_output() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let source = r#"
fn on_start(ctx) { transaction(#{},[button("result",true)],[]) }
fn on_event(ctx) { transaction(ctx.state,[],[]) }
fn on_timer(ctx) { transaction(ctx.state,[],[]) }
"#;
    let (mut service, _root, profile, _, state) = scripted(source);
    service.script_live_qualified = true;
    // Keep the real script response deadline; backend lease expiry is covered
    // separately and must not add recovery acquisitions during slow SQL fsync.
    state.lock().unwrap().fixed_clock=Some(1);
    let factory = service.output_factory.clone();
    let checked = state.clone();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    service.output_factory = Arc::new(move || {
        let attempt = observed.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            // The actual on_start request is pending while output setup runs.
            // A queued healthy reply must still lose the production 100 ms race.
            std::thread::sleep(Duration::from_millis(120));
        } else {
            let calls = &checked.lock().unwrap().calls;
            for slot in [1, 2] {
                assert_eq!(
                    calls
                        .iter()
                        .filter(|call| **call == format!("release:{slot}"))
                        .count(),
                    attempt,
                    "Every failed fixture owner must be released before another output acquisition"
                );
            }
            assert!(
                !calls.iter().any(|call| call == "submit:1:2"),
                "An expired startup transaction cannot contribute held output"
            );
        }
        factory()
    });
    let _token = start(&service, &profile, &state);
    submitted(&state, "submit:1:2");
    let attempts = attempts.load(Ordering::SeqCst);
    assert!((2..=3).contains(&attempts));
    assert_eq!(
        state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|call| *call == "submit:1:2")
            .count(),
        1,
        "Only the fresh healthy startup may contribute held output"
    );
    service.request(&json!({"command":"mapping_stop"})).unwrap();
    for slot in [1, 2] {
        assert_eq!(
            state
                .lock()
                .unwrap()
                .calls
                .iter()
                .filter(|call| **call == format!("release:{slot}"))
                .count(),
            attempts
        );
    }
    service.shutdown();
}

fn delay_initial_startup(service: &mut Service) -> Arc<std::sync::atomic::AtomicUsize> {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let factory = service.output_factory.clone();
    let first = AtomicBool::new(true);
    service.output_factory = Arc::new(move || {
        if first.swap(false, Ordering::SeqCst) {
            // An actual healthy reply may already be queued, but output setup
            // crosses the unchanged initial callback response deadline.
            std::thread::sleep(Duration::from_millis(120));
        }
        factory()
    });
    let factory = service.script_factory.clone();
    let starts = Arc::new(AtomicUsize::new(0));
    let observed = starts.clone();
    service.script_factory = Arc::new(move |source| {
        observed.fetch_add(1, Ordering::SeqCst);
        factory(source)
    });
    starts
}

#[test]
fn expired_startup_stop_requires_completed_cleanup_before_a_fresh_owner() {
    use std::sync::atomic::Ordering;
    let source = r#"
fn on_start(ctx) { transaction(#{},[button("result",true)],[]) }
fn on_event(ctx) { transaction(ctx.state,[],[]) }
fn on_timer(ctx) { transaction(ctx.state,[],[]) }
"#;
    let (mut service, _root, profile, _, state) = scripted(source);
    service.script_live_qualified = true;
    state.lock().unwrap().delay_stop_once = Some(Duration::from_micros(
        osg_virtual_controller::feeder::LEASE_US + 50_000,
    ));
    let starts = delay_initial_startup(&mut service);
    let _token = start(&service, &profile, &state);
    submitted(&state, "submit:1:2");
    assert!((2..=STARTUP_ATTEMPTS).contains(&starts.load(Ordering::SeqCst)));
    let state = state.lock().unwrap();
    assert!(state
        .stop_failures
        .iter()
        .any(|error| error
            == "Mapping heartbeat expired; fallback submitted and ownership released."));
    assert_eq!(
        state
            .calls
            .iter()
            .filter(|call| *call == "submit:1:2")
            .count(),
        1,
        "Only the admitted fresh startup may contribute held output"
    );
    drop(state);
    service.request(&json!({"command":"mapping_stop"})).unwrap();
    service.shutdown();
}

#[test]
fn unresolved_or_unrelated_startup_stop_errors_never_admit_another_start() {
    use std::sync::atomic::Ordering;
    for unresolved in [true, false] {
        let source = r#"
fn on_start(ctx) { transaction(#{},[button("result",true)],[]) }
fn on_event(ctx) { transaction(ctx.state,[],[]) }
fn on_timer(ctx) { transaction(ctx.state,[],[]) }
"#;
        let (mut service, _root, profile, _, state) = scripted(source);
        service.script_live_qualified = true;
        {
            let mut state = state.lock().unwrap();
            state.delay_stop_once = Some(Duration::from_micros(
                osg_virtual_controller::feeder::LEASE_US + 50_000,
            ));
            state.blocked_recovery = unresolved;
            state.failed_stop = !unresolved;
        }
        let starts = delay_initial_startup(&mut service);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            start(&service, &profile, &state)
        }));
        assert!(result.is_err(), "Unsafe fixture admission must be rejected");
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            state.lock().unwrap().stop_failures,
            vec![if unresolved {
                "Mapping heartbeat expired; fallback submitted and ownership released."
            } else {
                "Stop acknowledgement lost"
            }],
            "The intended Stop fault must be observed before claiming admission was refused"
        );
        let status = service
            .request(&json!({"command":"mapping_status"}))
            .unwrap();
        assert_eq!(status["running"], false);
        assert!(status["token"].is_null());
        assert_eq!(status["recoveryRequired"], unresolved);
        assert!(!state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|call| call == "submit:1:2"));
        service.shutdown();
    }
}

#[test]
fn losing_either_declared_source_releases_script_and_cancels_its_timers() {
    for source in 0..2 {
        let code = r#"
fn on_start(ctx) { transaction(#{held:true},[button("result",true)],[timer("later",500000)]) }
fn on_event(ctx) { transaction(ctx.state,[],[]) }
fn on_timer(ctx) { transaction(ctx.state,[button("result",true)],[]) }
"#;
        let (mut service, _root, profile, assignments, state) = scripted(code);
        service.script_live_qualified = true;
        let token = start(&service, &profile, &state);
        submitted(&state, "submit:1:2");
        {
            let mut router = service.router.lock().unwrap();
            let time = service.capture.stream_page("", 0).captured_us;
            router
                .runtime
                .accept_for(
                    &serde_json::from_value(token).unwrap(),
                    osg_mapping::engine::Frame {
                        source: assignments[source].source.clone(),
                        generation: Counter(1),
                        sequence: Counter(2),
                        captured_us: Counter(time),
                        reason: osg_mapping::engine::Reason::Disconnect,
                        changes: vec![],
                    },
                )
                .unwrap();
            router.poll_scripts();
            router.flush_output().unwrap();
        }
        wait_for(&service, |v| v["scripts"][0]["fault"].is_string());
        let calls = state.lock().unwrap().calls.clone();
        assert_eq!(
            calls
                .iter()
                .filter(|c| c.starts_with("submit:1:"))
                .last()
                .unwrap(),
            "submit:1:0"
        );
        std::thread::sleep(Duration::from_millis(550));
        assert!(!state.lock().unwrap().calls[calls.len()..]
            .iter()
            .any(|c| c == "submit:1:2"));
        service.request(&json!({"command":"mapping_stop"})).unwrap();
        service.shutdown();
    }
}

#[test]
fn idle_helper_death_releases_held_output_without_waiting_for_another_input() {
    let code = r#"
fn on_start(ctx) { transaction(#{},[button("result",true)],[]) }
fn on_event(ctx) { transaction(ctx.state,[],[]) }
fn on_timer(ctx) { transaction(ctx.state,[],[]) }
"#;
    let (mut service, _root, profile, _, state) = scripted(code);
    service.script_live_qualified = true;
    let child_id = Arc::new(Mutex::new(None));
    let saved = child_id.clone();
    service.script_factory = Arc::new(move |source| {
        let worker = osg_logic::worker::Worker::spawn(helper(), source)?;
        *saved.lock().unwrap() = worker.id();
        Ok(worker)
    });
    let _token = start(&service, &profile, &state);
    submitted(&state, "submit:1:2");
    let pid = child_id.lock().unwrap().unwrap();
    #[cfg(unix)]
    let mut command = {
        let mut c = std::process::Command::new("/bin/kill");
        c.arg("-KILL").arg(pid.to_string());
        c
    };
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new(
            PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/taskkill.exe"),
        );
        c.args(["/PID", &pid.to_string(), "/F"])
            .creation_flags(0x08000000);
        c
    };
    let killed=command.output().unwrap();
    assert!(killed.status.success(),"Cannot terminate the owned test helper {pid}: status={:?}, stdout={}, stderr={}, runtime={}",killed.status,String::from_utf8_lossy(&killed.stdout),String::from_utf8_lossy(&killed.stderr),service.request(&json!({"command":"mapping_status"})).unwrap());
    wait_for(&service, |v| {
        v["scripts"][0]["fault"]
            .as_str()
            .is_some_and(|e| e.contains("terminated"))
    });
    submitted(&state, "submit:1:0");
    service.request(&json!({"command":"mapping_stop"})).unwrap();
    service.shutdown();
}
