use osg_logic::{
    wire::{self, Request},
    worker::Worker,
};
use osg_mapping::{engine::*, profile::Profile, runtime::Runtime, script::*, trace::*};
use std::{
    path::Path,
    time::{Duration, Instant},
};
fn helper() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_osg-logic"))
}
fn fixture() -> (Profile, Vec<Assignment>) {
    let mut p: Profile =
        serde_json::from_slice(include_bytes!("../../mapping/fixtures/direct-profile.json"))
            .unwrap();
    p.scripts = vec![Script {
        id: "pulse".into(),
        source: include_str!("../../../src/lib/script-examples/pulse.rhai").into(),
        inputs: vec![osg_mapping::profile::Input::Source {
            role: "left".into(),
            control: "trigger".into(),
        }],
        outputs: vec![OutputBinding {
            id: "result".into(),
            output: osg_mapping::profile::Output {
                target: "main".into(),
                control: osg_mapping::profile::OutputControl::Button { index: 1 },
            },
        }],
        parameters: empty(),
    }];
    let assignments = p
        .roles
        .iter()
        .map(|r| Assignment {
            role: r.id.clone(),
            source: synthetic(&r.id),
            controls: r
                .controls
                .iter()
                .map(|c| ControlAssignment {
                    control: c.id.clone(),
                    source_control: c.id.clone(),
                })
                .collect(),
        })
        .collect();
    (p, assignments)
}
fn baseline(role: &str) -> Frame {
    Frame {
        source: synthetic(role),
        generation: Counter(1),
        sequence: Counter(1),
        captured_us: Counter(1),
        reason: Reason::Baseline,
        changes: if role == "left" {
            vec![
                Change {
                    control: "trigger".into(),
                    value: osg_mapping::profile::Value::Button(false),
                },
                Change {
                    control: "x".into(),
                    value: osg_mapping::profile::Value::Axis(0),
                },
                Change {
                    control: "hat".into(),
                    value: osg_mapping::profile::Value::Hat(None),
                },
            ]
        } else {
            vec![Change {
                control: "trigger".into(),
                value: osg_mapping::profile::Value::Button(false),
            }]
        },
    }
}
fn callback(worker: &mut Worker, runtime: &mut Runtime) {
    for (id, request) in runtime.script_requests() {
        worker.submit(request).unwrap();
        let start = Instant::now();
        loop {
            if let Some((ticket, result)) = worker.poll() {
                runtime.script_reply(&id, &ticket, result.unwrap()).unwrap();
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(1));
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
#[test]
fn native_worker_pulse_replay_and_process_death() {
    let (profile, assignments) = fixture();
    let mut worker = Worker::spawn(helper(), &profile.scripts[0].source).unwrap();
    assert!(worker.id().is_some());
    assert!(!worker.source_hash.is_empty());
    let mut runtime = Runtime::default();
    let token = runtime.start(profile.clone(), assignments).unwrap();
    runtime.accept_for(&token, baseline("left")).unwrap();
    runtime.accept_for(&token, baseline("right")).unwrap();
    callback(&mut worker, &mut runtime);
    for (sequence, at, pressed) in [(2, 2, false), (3, 10, true)] {
        runtime
            .accept_for(
                &token,
                Frame {
                    source: synthetic("left"),
                    generation: Counter(1),
                    sequence: Counter(sequence),
                    captured_us: Counter(at),
                    reason: Reason::Change,
                    changes: vec![Change {
                        control: "trigger".into(),
                        value: osg_mapping::profile::Value::Button(pressed),
                    }],
                },
            )
            .unwrap();
        callback(&mut worker, &mut runtime);
    }
    assert_eq!(
        runtime.engine().unwrap().contributions()["pulse:result"],
        osg_mapping::profile::Value::Button(true)
    );
    runtime.advance_for(&token, Counter(5010)).unwrap();
    callback(&mut worker, &mut runtime);
    assert_eq!(
        runtime.engine().unwrap().contributions()["pulse:result"],
        osg_mapping::profile::Value::Button(false)
    );
    runtime.stop();
    let bytes = runtime.trace.export().unwrap();
    let path =
        std::env::temp_dir().join(format!("cockpit-logic-trace-{}.json", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let cli = std::process::Command::new(helper())
        .arg("replay")
        .arg(&path)
        .output()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert!(String::from_utf8_lossy(&cli.stdout).contains("sha256"));
    let trace = Trace::parse(&bytes).unwrap();
    let first = osg_logic::replay::replay(&trace, helper()).unwrap();
    assert_eq!(first, osg_logic::replay::replay(&trace, helper()).unwrap());
    let request = trace
        .steps
        .iter()
        .find_map(|s| match s {
            Step::Dispatch { requests } => Some(requests[0].1.clone()),
            _ => None,
        })
        .unwrap();
    let pid = worker.id().unwrap();
    #[cfg(unix)]
    assert!(std::process::Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .unwrap()
        .success());
    #[cfg(windows)]
    assert!(std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output()
        .unwrap()
        .status
        .success());
    worker.submit(request).unwrap();
    let start = Instant::now();
    loop {
        if let Some((_, result)) = worker.poll() {
            assert!(result.is_err());
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(worker.id().is_none());
}
#[test]
fn compiler_and_ipc_fail_closed() {
    assert!(!std::process::Command::new(helper())
        .output()
        .unwrap()
        .status
        .success());
    assert!(!std::process::Command::new(helper())
        .arg("replay")
        .output()
        .unwrap()
        .status
        .success());
    assert!(Worker::spawn(Path::new("no-such-cockpit-helper"), "code").is_err());
    assert!(Worker::spawn(helper(), "fn broken(").is_err());
    let mut bytes = Vec::new();
    wire::write(&Request::Compile { source: "x".into() }, &mut bytes).unwrap();
    assert!(matches!(
        wire::read::<Request>(&mut bytes.as_slice()).unwrap(),
        Request::Compile { .. }
    ));
    for bytes in [
        vec![],
        vec![255, 255, 255, 255],
        vec![0, 0, 0, 0],
        vec![1, 0, 0, 0, 255],
    ] {
        assert!(wire::read::<Request>(&mut bytes.as_slice()).is_err());
    }
    assert!(wire::write(
        &Request::Compile {
            source: "x".repeat(40000)
        },
        &mut Vec::new()
    )
    .is_err());
}

#[test]
fn framed_worker_exits_cleanly_on_eof_and_rejects_recompilation() {
    use osg_logic::wire::Reply;
    use std::process::{Command, Stdio};
    for recompile in [false, true] {
        let mut child = Command::new(helper())
            .arg(osg_logic::worker::ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let (profile, _) = fixture();
        wire::write(
            &Request::Compile {
                source: profile.scripts[0].source.clone(),
            },
            &mut input,
        )
        .unwrap();
        let handshake = wire::read::<Reply>(&mut output).unwrap();
        assert!(matches!(handshake, Reply::Compiled), "{handshake:?}");
        if recompile {
            wire::write(
                &Request::Compile {
                    source: "invalid".into(),
                },
                &mut input,
            )
            .unwrap();
        }
        drop(input);
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert_eq!(status.success(), !recompile);
                break;
            }
            if start.elapsed() > Duration::from_secs(2) {
                child.kill().unwrap();
                panic!("EOF did not stop helper");
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
