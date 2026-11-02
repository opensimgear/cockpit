use super::*;
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
};
use tokio::{io::AsyncWriteExt, net::windows::named_pipe::ClientOptions};

fn request() -> Request {
    let own = security::current().unwrap();
    Request {
        nonce: security::nonce().unwrap(),
        parent_pid: own.pid,
        parent_epoch: own.epoch,
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn fixture_name(name: &str) -> String {
    format!("{}::{name}", module_path!().split_once("::").unwrap().1)
}

#[test]
fn arguments_are_bounded_canonical_and_offer_no_command_or_pipe_override() {
    let request = request();
    let args: Vec<_> = request
        .arguments()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert_eq!(Request::parse(&args).unwrap(), request);
    for replacement in ["..\\pipe\\other", "A", "00", "a".repeat(65).as_str()] {
        let mut invalid = args.clone();
        invalid[1] = replacement.into();
        assert!(Request::parse(&invalid).is_err());
    }
    let mut invalid = args.clone();
    invalid[2] = format!("0{}", invalid[2]);
    assert!(Request::parse(&invalid).is_err());
    let mut invalid = args;
    invalid.push("arbitrary.exe".into());
    assert!(Request::parse(&invalid).is_err());
}

#[test]
fn framing_rejects_oversize_truncation_unknown_fields_and_wrong_nonce() {
    runtime().block_on(async {
        for bytes in [
            (128u32 * 1024 + 1).to_le_bytes().to_vec(),
            [100u32.to_le_bytes().as_slice(), b"{}"].concat(),
            {
                let body = format!(
                    r#"{{"type":"Ready","nonce":"{}","command":"write"}}"#,
                    "a".repeat(64)
                );
                [
                    ((body.len() as u32).to_le_bytes()).as_slice(),
                    body.as_bytes(),
                ]
                .concat()
            },
        ] {
            let (mut writer, mut reader) = tokio::io::duplex(4096);
            writer.write_all(&bytes).await.unwrap();
            drop(writer);
            assert!(protocol::read(&mut reader, &"a".repeat(64)).await.is_err());
        }
        let (mut writer, mut reader) = tokio::io::duplex(4096);
        protocol::write(
            &mut writer,
            &Frame::Ready {
                nonce: "b".repeat(64),
            },
        )
        .await
        .unwrap();
        assert!(protocol::read(&mut reader, &"a".repeat(64)).await.is_err());
    });
}

#[test]
fn selected_identity_is_narrow_and_error_frames_are_utf8_bounded() {
    let mut expected = ProcessIdentity {
        pid: 1,
        epoch: 1,
        path: r"C:\Games\X-Plane.exe".into(),
    };
    assert!(protocol::validate_process(&expected).is_ok());
    for path in [
        r"C:\Games\cmd.exe",
        "X-Plane.exe",
        "C:\\Games\\X-Plane.exe\0",
    ] {
        expected.path = path.into();
        assert!(protocol::validate_process(&expected).is_err());
    }
    let Frame::Failed { error, .. } = protocol::failure(&"a".repeat(64), "🛫".repeat(3000))
    else {
        panic!()
    };
    assert!(error.len() <= 4000);
}

#[test]
fn pipe_is_first_instance_and_authenticates_actual_peer_pid_and_parent_epoch() {
    runtime().block_on(async {
        let request = request();
        let server = security::server(&request).unwrap();
        assert!(security::server(&request).is_err());
        let client = ClientOptions::new().open(request.pipe()).unwrap();
        server.connect().await.unwrap();
        let own = Handle::open(std::process::id()).unwrap();
        security::authenticate_child(&server, &own).unwrap();
        let selected = Selected::new();
        assert!(
            security::authenticate_child(&server, &Handle::open(selected.child.id()).unwrap())
                .is_err()
        );
        security::authenticate_parent(&request, &client).unwrap();
        let mut wrong = request.clone();
        wrong.parent_pid += 1;
        assert!(security::authenticate_parent(&wrong, &client).is_err());
        wrong = request;
        wrong.parent_epoch += 1;
        assert!(security::authenticate_parent(&wrong, &client).is_err());
    });
}

struct Selected {
    child: Child,
    directory: PathBuf,
    identity: ProcessIdentity,
}
impl Selected {
    fn new() -> Self {
        Self::with_pipe_client(None)
    }
    fn with_pipe_client(pipe: Option<&str>) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "osg-process-events-test-{}-{}",
            std::process::id(),
            security::nonce().unwrap()
        ));
        std::fs::create_dir(&directory).unwrap();
        let executable = directory.join("X-Plane.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let child = Command::new(&executable)
            .args([
                "--exact",
                &fixture_name("selected_child_fixture"),
                "--ignored",
            ])
            .env("OSG_PROCESS_EVENTS_SELECTED_FIXTURE", "1")
            .env("OSG_PROCESS_EVENTS_PIPE_CLIENT", pipe.unwrap_or(""))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let identity = Handle::open(child.id())
            .unwrap()
            .identity(child.id())
            .unwrap();
        Self {
            child,
            directory,
            identity,
        }
    }
}

#[test]
fn exact_launched_client_still_requires_the_same_executable_before_begin() {
    runtime().block_on(async {
        let request = request();
        let server = security::server(&request).unwrap();
        let selected = Selected::with_pipe_client(Some(&request.pipe()));
        timeout(Duration::from_secs(10), server.connect())
            .await
            .unwrap()
            .unwrap();
        let child = Handle::open(selected.child.id()).unwrap();
        assert!(security::authenticate_child(&server, &child)
            .unwrap_err()
            .contains("executable"));
    });
}
impl Drop for Selected {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(self.directory.join("X-Plane.exe"));
        let _ = std::fs::remove_dir(&self.directory);
    }
}
struct Helper(Child);
impl Helper {
    fn wait(&mut self) {
        let handle = Handle::open(self.0.id()).unwrap();
        runtime().block_on(async {
            timeout(
                Duration::from_secs(10),
                wait::Exit::new(handle).unwrap().wait(),
            )
            .await
            .unwrap()
            .unwrap();
        });
        assert!(self.0.wait().unwrap().success());
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn ordinary_launch(
    mode: &'static str,
) -> (
    impl FnOnce(&Request) -> Result<Handle, String> + Send + 'static,
    mpsc::Receiver<Child>,
) {
    let (sender, receiver) = mpsc::channel();
    let launch = move |request: &Request| {
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &fixture_name("ordinary_child_fixture"),
                "--ignored",
                "--nocapture",
            ])
            .env("OSG_PROCESS_EVENTS_TEST_REQUEST", request.arguments())
            .env("OSG_PROCESS_EVENTS_TEST_MODE", mode)
            .stdin(Stdio::null())
            .stdout(if matches!(mode, "stall" | "observe") {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(if mode == "stall" {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .spawn()
            .map_err(|e| e.to_string())?;
        let handle = Handle::open(child.id())?;
        sender.send(child).map_err(|e| e.to_string())?;
        Ok(handle)
    };
    (launch, receiver)
}

#[test]
fn ordinary_child_bridge_readiness_drop_and_native_child_exit_are_event_backed() {
    let selected = Selected::new();
    let (launch, receiver) = ordinary_launch("steady");
    let result = start_with(selected.identity.clone(), launch);
    let mut child = Helper(receiver.recv().unwrap());
    let (failure, worker) = result.unwrap();
    assert!(failure.borrow().is_none());
    drop(worker); // Closes the pipe; the ordinary child must stop without a timer.
    child.wait();
}

#[test]
fn ordinary_child_bridge_forwards_terminal_failure_after_ready() {
    let selected = Selected::new();
    let (launch, receiver) = ordinary_launch("failure");
    let result = start_with(selected.identity.clone(), launch);
    let mut child = Helper(receiver.recv().unwrap());
    let (mut failure, worker) = result.unwrap();
    let error = runtime().block_on(async {
        timeout(
            Duration::from_secs(10),
            super::super::wait_failure(&mut failure),
        )
        .await
        .unwrap()
    });
    assert!(error.contains("fixture terminal failure") || error.contains("exited"));
    drop(worker);
    let _ = child.0.wait();
}

#[test]
fn ordinary_child_bridge_rejects_failed_bootstrap_without_ready() {
    let selected = Selected::new();
    let (launch, receiver) = ordinary_launch("startup-error");
    let result = start_with(selected.identity.clone(), launch);
    let mut child = Helper(receiver.recv().unwrap());
    assert!(result.is_err());
    let _ = child.0.wait();
}

#[test]
fn closing_pipe_exits_child_even_when_monitor_factory_is_stalled() {
    let selected = Selected::new();
    runtime().block_on(async {
        let request = request();
        let mut pipe = security::server(&request).unwrap();
        let (launch, receiver) = ordinary_launch("stall");
        let child_handle = launch(&request).unwrap();
        let mut child = Helper(receiver.recv().unwrap());
        timeout(Duration::from_secs(10), pipe.connect())
            .await
            .unwrap()
            .unwrap();
        security::authenticate_child(&pipe, &child_handle).unwrap();
        protocol::write(&mut pipe, &Frame::begin(&request, &selected.identity))
            .await
            .unwrap();
        let stdout = child.0.stdout.take().unwrap();
        let (entered_sender, entered) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut sender = Some(entered_sender);
            for text in
                std::io::BufRead::lines(std::io::BufReader::new(stdout)).map_while(Result::ok)
            {
                if text.contains("factory-entered") {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(text);
                    }
                }
            }
        });
        assert!(entered
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .contains("factory-entered"));
        let started = std::time::Instant::now();
        drop(pipe);
        let mut exit = wait::Exit::new(child_handle).unwrap();
        timeout(Duration::from_secs(5), exit.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(child.0.wait().unwrap().success());
        assert!(started.elapsed() < Duration::from_secs(5));
    });
}

#[test]
#[ignore = "Only launched as an ordinary child by bridge tests; never requests UAC or WMI."]
fn ordinary_child_fixture() {
    let Some(args) = std::env::var_os("OSG_PROCESS_EVENTS_TEST_REQUEST") else {
        return;
    };
    let args: Vec<String> = args
        .to_string_lossy()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let request = Request::parse(&args).unwrap();
    let mode = std::env::var("OSG_PROCESS_EVENTS_TEST_MODE").unwrap();
    let result = runtime().block_on(helper(request, move |_| {
        if mode == "observe" {
            println!("factory-entered");
        }
        if mode == "stall" {
            println!("factory-entered");
            std::io::Write::flush(&mut std::io::stdout()).unwrap();
            std::thread::park();
            unreachable!();
        }
        if mode == "startup-error" {
            return Err("fixture startup failure".into());
        }
        let (sender, failure) = watch::channel(None);
        if mode == "failure" {
            let sender = sender.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(500));
                sender.send_replace(Some("fixture terminal failure".into()));
            });
        }
        Ok(Monitor {
            failure,
            _worker: super::super::Backend::Test(sender),
        })
    }));
    if let Err(error) = result {
        eprintln!("ordinary helper fixture: {error}");
        std::process::exit(1);
    }
}

#[path = "startup_tests.rs"]
mod startup;

#[test]
#[ignore = "Only launched as a temporary X-Plane.exe image by bridge tests."]
fn selected_child_fixture() {
    if std::env::var_os("OSG_PROCESS_EVENTS_SELECTED_FIXTURE").is_some() {
        let runtime = runtime();
        let _enter = runtime.enter();
        let pipe = std::env::var("OSG_PROCESS_EVENTS_PIPE_CLIENT").unwrap_or_default();
        let _pipe = if pipe.is_empty() {
            None
        } else {
            Some(ClientOptions::new().open(pipe).unwrap())
        };
        std::thread::park();
    }
}
