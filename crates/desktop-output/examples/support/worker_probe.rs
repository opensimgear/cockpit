//! Explicit real-worker acceptance. Never invoked by `cargo test` or startup.
use super::experiment::{foreground, wait, OBSERVATIONS};
use osg_desktop_output::{protocol::Scope, windows::now_us, Coordinator};
use osg_mapping::{
    desktop::{Control, Delivery, Intent, Key, MouseButton},
    engine::{typed::Token, Counter, RunToken},
};
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use windows_sys::Win32::Foundation::HWND;
fn run_token(n: u64) -> RunToken {
    RunToken {
        session: format!("native-receiver-{n}"),
        generation: Counter(1),
    }
}
fn token(n: u64, node: &str) -> Token {
    Token {
        run: run_token(n),
        node_id: node.into(),
        dependencies: vec![],
    }
}
fn key(k: u16) -> Control {
    Control::Key { key: Key(k) }
}
fn intent(n: u64, node: &str, op: u64, intent: Intent) -> Delivery {
    let now = now_us();
    Delivery {
        producer: token(n, node),
        action_id: "action".into(),
        operation_id: Counter(op),
        sequence: Counter(op),
        at_us: Counter(now),
        deadline_us: Some(Counter(now + 100_000)),
        intent,
    }
}
fn submit(
    c: &mut Coordinator,
    n: u64,
    node: &str,
    op: u64,
    value: Intent,
) -> Result<serde_json::Value, String> {
    let delivery = intent(n, node, op, value);
    let ack = c
        .submit(delivery, now_us())
        .map_err(|e| format!("Worker receiver run {n} node {node} operation {op}: {e}"))?;
    wait(Duration::from_millis(30));
    serde_json::to_value(ack).map_err(|e| e.to_string())
}
fn count(message: u32, scan: Option<u64>) -> usize {
    OBSERVATIONS
        .lock()
        .unwrap()
        .iter()
        .filter(|o| {
            o["message"] == message
                && o["injectedMarker"] == true
                && scan.is_none_or(|s| o["scanCode"] == s)
        })
        .count()
}
fn redacted(value: impl serde::Serialize) -> serde_json::Value {
    fn clean(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    if k == "executable" {
                        *v = "<receiver executable>".into()
                    } else if k == "pid" {
                        *v = 0.into()
                    } else if k == "created" {
                        *v = "<process creation identity>".into()
                    } else {
                        clean(v)
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for v in values {
                    clean(v)
                }
            }
            _ => {}
        }
    }
    let mut v = serde_json::to_value(value).unwrap();
    clean(&mut v);
    v
}
fn spawn(path: &Path, scope: &Scope, n: u64) -> Result<Coordinator, String> {
    if path.exists() {
        let old = Coordinator::inspect_journal(path)?;
        if old.uncertain || !old.held.is_empty() || old.in_flight.is_some() || !old.cleanup_complete
        {
            Coordinator::recover_journal(true, true, path.to_owned())?;
        }
    }
    let mut c = Coordinator::spawn_with_journal(true, true, path.to_owned())?;
    c.activate(run_token(n), scope.clone())?;
    Ok(c)
}
fn child(args: &[String]) -> Result<std::process::Child, String> {
    use std::os::windows::process::CommandExt;
    Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| e.to_string())
}
pub fn lifecycle(evidence: &str) -> Result<(), String> {
    if !osg_desktop_output::qualification_gate() {
        return Err(
            "Use an explicit Windows debug acceptance build for worker lifecycle acceptance."
                .into(),
        );
    }
    let directory = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join("crates/desktop-output/target/receiver-acceptance");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let mut checks = Vec::new();
    for cycle in 0..3 {
        let path=directory.join(format!("lifecycle-owned-{cycle}.json"));let owner=run_token(90+cycle);
        let mut coordinator = Coordinator::spawn_with_owned_journal(true, true, path.clone(),owner.clone(),Some(Scope::DesktopWide))?;
        let exclusive = Coordinator::spawn(true, true).is_err();
        if !exclusive {
            return Err("A concurrent same-session native worker was not excluded.".into());
        }
        coordinator.stop_all()?;
        let before = std::fs::read(&path).map_err(|e| e.to_string())?;
        coordinator.stop_all()?;
        if before != std::fs::read(&path).map_err(|e| e.to_string())? {
            return Err("Repeated Stop mutated a clean native journal.".into());
        }
        drop(coordinator);
        let record = Coordinator::inspect_owned_journal(&path,&owner,Some(&Scope::DesktopWide))?;
        if record.uncertain
            || !record.cleanup_complete
            || !record.held.is_empty()
            || record.in_flight.is_some()
        {
            return Err("Stop/drop left a spurious interrupted native journal.".into());
        }
        let lock = osg_desktop_output::windows::SessionLock::acquire()?;
        drop(lock);
        let wrong_owner_rejected=Coordinator::inspect_owned_journal(&path,&run_token(100),Some(&Scope::DesktopWide)).is_err();if !wrong_owner_rejected{return Err("A clean journal lost its immutable native session owner.".into())}
        checks.push(serde_json::json!({"cycle":cycle,"secondWorkerExcluded":exclusive,"repeatedStopInert":true,"cleanJournal":redacted(record),"sessionLockReacquiredAfterDrop":true,"wrongOwnerRejectedAfterCleanStop":wrong_owner_rejected}));
    }
    let value = serde_json::json!({"experiment":"same-executable native worker stop/drop/reacquire regression","nativeInputSubmitted":false,"productionQualified":osg_desktop_output::QUALIFIED,"checks":checks});
    std::fs::write(
        evidence,
        serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("Verified three native worker Stop/drop/reacquire cycles without submitting input: {evidence}");
    Ok(())
}
pub fn parent(args: &[String]) -> Result<(), String> {
    if !cfg!(debug_assertions) || !osg_desktop_output::qualification_gate() || args.len() != 3 {
        return Err(
            "Parent-loss acceptance requires an explicit qualified debug experiment.".into(),
        );
    }
    let scope: Scope = serde_json::from_str(&args[1]).map_err(|e| e.to_string())?;
    let path = PathBuf::from(&args[2]);
    let mut c = spawn(&path, &scope, 3)?;
    c.arm(token(3, "first"), "action".into())?;
    c.submit(
        intent(
            3,
            "first",
            1,
            Intent::Held {
                controls: vec![key(0x06)],
            },
        ),
        now_us(),
    )?;
    // Deliberate parent termination: bypass Drop, leaving the worker to EOF/lease.
    std::process::exit(87)
}
pub fn run(window: HWND, scope: Scope, evidence: &str) -> Result<(), String> {
    if !cfg!(debug_assertions) || !osg_desktop_output::qualification_gate() {
        return Err("Build Windows debug acceptance with OSG_DEV_DESKTOP_INPUT=1 first.".into());
    }
    let directory = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join("crates/desktop-output/target/receiver-acceptance");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let normal = directory.join("normal.json");
    let parent_path = directory.join("parent-loss.json");
    let death = directory.join("worker-loss.json");
    let focus_observations = directory.join("focus-observations.json");
    let mut checks = Vec::new();
    let mut c = spawn(&normal, &scope, 1)?;
    unsafe {
        foreground(window);
    }
    wait(Duration::from_millis(10));
    c.arm(token(1, "first"), "action".into()).map_err(|e| {
        format!(
            "Initial receiver arm: {e}; actual foreground {:?}",
            osg_desktop_output::windows::foreground_identity()
        )
    })?;
    c.arm(token(1, "second"), "action".into())?;
    let first = submit(
        &mut c,
        1,
        "first",
        1,
        Intent::Held {
            controls: vec![key(0xe0), key(0x0a)],
        },
    )?;
    let second = submit(
        &mut c,
        1,
        "second",
        1,
        Intent::Held {
            controls: vec![key(0xe0), key(0x0b)],
        },
    )?;
    let release_first = submit(&mut c, 1, "first", 2, Intent::Held { controls: vec![] })?;
    if !c.snapshot().held.contains(&key(0xe0)) || release_first["attempted"] != 1 {
        return Err("Shared Control ownership failed in the real worker.".into());
    }
    let release_second = submit(&mut c, 1, "second", 2, Intent::Held { controls: vec![] })?;
    checks.push(serde_json::json!({"case":"shared-chord-ownership","first":first,"second":second,"releaseFirst":release_first,"releaseSecond":release_second,"passed":true}));
    c.renew(run_token(1))?;
    for (i, button) in [
        MouseButton::Left,
        MouseButton::Right,
        MouseButton::Middle,
        MouseButton::X1,
        MouseButton::X2,
    ]
    .into_iter()
    .enumerate()
    {
        let down = submit(
            &mut c,
            1,
            "first",
            3 + 2 * i as u64,
            Intent::Held {
                controls: vec![Control::MouseButton { button }],
            },
        )?;
        let up = submit(
            &mut c,
            1,
            "first",
            4 + 2 * i as u64,
            Intent::Held { controls: vec![] },
        )?;
        checks.push(serde_json::json!({"case":"mouse-click","button":button,"down":down,"up":up}));
        c.renew(run_token(1))?;
    }
    let wheel = submit(
        &mut c,
        1,
        "first",
        13,
        Intent::Wheel {
            horizontal: 1,
            vertical: 1,
        },
    )?;
    checks.push(serde_json::json!({"case":"both-wheel-axes","ack":wheel}));
    for tick in 0..16 {
        let ack = submit(
            &mut c,
            1,
            "first",
            14 + tick,
            Intent::Pointer {
                dx: if tick < 8 { 1 } else { -1 },
                dy: 0,
            },
        )?;
        checks.push(serde_json::json!({"case":"pointer","tick":tick,"ack":ack}));
        c.renew(run_token(1))?;
    }
    let hold = submit(
        &mut c,
        1,
        "first",
        30,
        Intent::Held {
            controls: vec![
                key(4),
                Control::MouseButton {
                    button: MouseButton::Left,
                },
            ],
        },
    )?;
    c.renew(run_token(1))?;
    let mut focus = child(&[
        "--osg-desktop-focus-target".into(),
        focus_observations.to_string_lossy().into_owned(),
    ])?;
    let mut line = String::new();
    BufReader::new(
        focus
            .stdout
            .take()
            .ok_or("Missing focus target readiness pipe.")?,
    )
    .read_line(&mut line)
    .map_err(|e| e.to_string())?;
    wait(Duration::from_millis(25));
    c.renew(run_token(1))?;
    let paused = c.snapshot();
    if !paused.held.is_empty() {
        return Err("Focus loss retained native holds.".into());
    }
    unsafe {
        foreground(window);
    }
    wait(Duration::from_millis(20));
    let before = count(256, Some(0x1e));
    let rejected = c
        .submit(
            intent(
                1,
                "first",
                31,
                Intent::Held {
                    controls: vec![key(4)],
                },
            ),
            now_us(),
        )
        .is_err();
    wait(Duration::from_millis(20));
    if !rejected || before != count(256, Some(0x1e)) {
        return Err("Focus return resumed without neutral rearming.".into());
    }
    c.arm(token(1, "first"), "action".into())?;
    let rearmed = submit(
        &mut c,
        1,
        "first",
        32,
        Intent::Held {
            controls: vec![key(4)],
        },
    )?;
    let released = submit(&mut c, 1, "first", 33, Intent::Held { controls: vec![] })?;
    checks.push(serde_json::json!({"case":"real-focus-loss-and-neutral-rearm","hold":hold,"paused":redacted(paused),"resumedWithoutArm":!rejected,"rearmed":rearmed,"released":released}));
    c.renew(run_token(1))?;
    let before_up = count(257, Some(0x30));
    let lease_hold = submit(
        &mut c,
        1,
        "first",
        34,
        Intent::Held {
            controls: vec![key(5)],
        },
    )?;
    wait(Duration::from_millis(300));
    let lease_expired = c.renew(run_token(1)).is_err();
    let lease_snapshot = c.snapshot();
    if !lease_expired || !lease_snapshot.held.is_empty() || count(257, Some(0x30)) <= before_up {
        return Err("Autonomous worker lease cleanup failed.".into());
    }
    checks.push(serde_json::json!({"case":"no-input-no-ui-renewal-lease-expiry","hold":lease_hold,"expired":lease_expired,"snapshot":redacted(lease_snapshot),"receiverReleaseObserved":true}));
    c.stop_all()?;
    drop(c);
    let status = focus.wait().map_err(|e| e.to_string())?;
    let focus_events: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&focus_observations).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    checks.push(serde_json::json!({"case":"focus-target-received-owned-releases","targetExited":status.success(),"observations":focus_events}));
    // Terminate a distinct parent after its acknowledged hold. The receiving
    // worker remains responsible for parent EOF and independent lease cleanup.
    let before_up = count(257, Some(0x2e));
    let mut parent = child(&[
        "--osg-desktop-test-parent".into(),
        serde_json::to_string(&scope).unwrap(),
        parent_path.to_string_lossy().into_owned(),
    ])?;
    let parent_status = parent.wait().map_err(|e| e.to_string())?;
    wait(Duration::from_millis(300));
    let interrupted = Coordinator::inspect_journal(&parent_path)?;
    let blocked = Coordinator::spawn_with_journal(true, true, parent_path.clone()).is_err();
    if parent_status.code() != Some(87) || count(257, Some(0x2e)) <= before_up || !blocked {
        return Err(
            "Actual parent termination cleanup/interrupted activation guard failed.".into(),
        );
    }
    Coordinator::recover_journal(true, true, parent_path.clone())?;
    wait(Duration::from_millis(30));
    checks.push(serde_json::json!({"case":"actual-parent-exit-worker-cleanup","parentExitCode":87,"receiverReleaseObserved":true,"restartBlocked":blocked,"interrupted":redacted(interrupted),"explicitRecovery":redacted(Coordinator::inspect_journal(&parent_path)?)}));
    // The debug-only worker hook exits immediately after real SendInput and
    // before writing its ACK. The coordinator must preserve the possible hold.
    std::env::set_var("OSG_DESKTOP_TEST_EXIT_AFTER_SEND", "1");
    let mut dying = spawn(&death, &scope, 4)?;
    dying.arm(token(4, "first"), "action".into())?;
    let before_down = count(256, Some(0x20));
    let before_up = count(257, Some(0x20));
    let error = dying
        .submit(
            intent(
                4,
                "first",
                1,
                Intent::Held {
                    controls: vec![key(7)],
                },
            ),
            now_us(),
        )
        .err();
    std::env::remove_var("OSG_DESKTOP_TEST_EXIT_AFTER_SEND");
    wait(Duration::from_millis(30));
    let uncertain = dying.snapshot();
    let replay_blocked = dying
        .submit(
            intent(
                4,
                "first",
                2,
                Intent::Held {
                    controls: vec![key(7)],
                },
            ),
            now_us(),
        )
        .is_err();
    wait(Duration::from_millis(30));
    if error.is_none()
        || !uncertain.uncertain
        || uncertain.in_flight.is_none()
        || !uncertain.possible_held.contains(&key(7))
        || !replay_blocked
        || count(256, Some(0x20)) != before_down + 1
        || count(257, Some(0x20)) != before_up
    {
        return Err("Actual worker death did not preserve the uncertain submission gap.".into());
    }
    drop(dying);
    Coordinator::recover_journal(true, true, death.clone())?;
    wait(Duration::from_millis(30));
    let recovered = Coordinator::inspect_journal(&death)?;
    if recovered.uncertain
        || !recovered.held.is_empty()
        || !recovered.possible_held.is_empty()
        || count(257, Some(0x20)) <= before_up
    {
        return Err("Explicit possible-hold recovery failed.".into());
    }
    checks.push(serde_json::json!({"case":"actual-worker-death-after-sendinput-before-ack","error":error,"possibleHeldPreserved":true,"uncertain":redacted(uncertain),"replayBlocked":replay_blocked,"receiverDownCount":1,"receiverReleaseBeforeRecovery":0,"recovered":redacted(recovered),"receiverReleaseAfterRecovery":true}));
    let observations = OBSERVATIONS.lock().unwrap().clone();
    let report = serde_json::json!({"experiment":"J2 same-executable native coordinator/worker receiver","productionQualified":osg_desktop_output::QUALIFIED,"debugAcceptanceGate":true,"gameQualified":false,"checks":checks,"receiverObservations":observations});
    std::fs::write(
        evidence,
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("Recorded real coordinator/worker ownership, scope, lease, parent-death and worker-death acceptance to {evidence}.");
    Ok(())
}
