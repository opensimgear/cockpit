use super::*;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

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
fn start(service: &Service, profile: &Profile) -> Value {
    let started = service
        .request(&json!({"command":"mapping_virtual_start","profileId":profile.id}))
        .unwrap();
    let ready=wait_for(service, |v| {
        v["scripts"][0]["pending"] == 0 && v["scripts"][0]["inflight"] == false
    });
    assert!(ready["scripts"][0]["fault"].is_null(),"Script startup must be healthy before fixture input: {ready}");
    started["token"].clone()
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
    let token = start(&service, &profile);
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
        let token = start(&service, &profile);
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
fn losing_either_declared_source_releases_script_and_cancels_its_timers() {
    for source in 0..2 {
        let code = r#"
fn on_start(ctx) { transaction(#{held:true},[button("result",true)],[timer("later",500000)]) }
fn on_event(ctx) { transaction(ctx.state,[],[]) }
fn on_timer(ctx) { transaction(ctx.state,[button("result",true)],[]) }
"#;
        let (mut service, _root, profile, assignments, state) = scripted(code);
        service.script_live_qualified = true;
        let token = start(&service, &profile);
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
    let _token = start(&service, &profile);
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
