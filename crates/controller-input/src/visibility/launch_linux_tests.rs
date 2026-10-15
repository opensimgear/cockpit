use super::*;
fn request(code: &str) -> Request {
    Request {
        launch_kind: "direct-child".into(),
        executable: "/usr/bin/python3".into(),
        directory: "/usr/bin".into(),
        arguments: vec!["-c".into(), code.into()],
    }
}
fn fixture(path: &str, actual: &str, group: &str, id: &str, virtual_device: bool) -> Node {
    let m = std::fs::metadata(actual).unwrap();
    Node {
        path: path.into(),
        group: group.into(),
        identity: id.into(),
        virtual_device,
        device: m.rdev(),
        inode: m.ino(),
        filesystem: m.dev(),
    }
}
fn process(code: &str) -> Process {
    let nodes = vec![
        fixture(
            "/dev/input/event900",
            "/dev/null",
            "selected",
            "selected",
            false,
        ),
        fixture("/dev/input/event901", "/dev/zero", "virtual", "", true),
    ];
    let snap = nodes.clone();
    Process::spawn(
        request(code),
        trusted_program(Path::new(BWRAP)).unwrap(),
        nodes.clone(),
        vec![nodes[1].clone()],
        Arc::new(move || Ok(snap.clone())),
        |_| File::open("/dev/zero").map_err(|e| e.to_string()),
    )
    .unwrap()
}
fn complete(child: &mut Process) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.poll().unwrap() {
            Poll::Exited(code) => return code,
            _ => {
                assert!(Instant::now() < deadline, "sandbox did not finish");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
#[test]
fn real_namespace_denies_selected_nodes_exposes_only_snapshot_and_closes_ambient_handles() {
    let ambient = tempfile::NamedTempFile::new().unwrap();
    unsafe {
        assert_eq!(libc::fcntl(ambient.as_raw_fd(), libc::F_SETFD, 0), 0);
    }
    let code = format!(
        r#"
import os, socket, errno
assert not os.path.exists('/dev/input/event900')
assert not os.path.exists('/dev/hidraw900')
assert not os.path.exists('/dev/bus/usb')
assert os.listdir('/dev/input') == ['event901']
assert open('/dev/input/event901','rb',buffering=0).read(4) == b'\0'*4
assert not os.listdir('/sys') and not os.listdir('/run')
assert os.environ['HOME'] == '/home/player'
for fd in os.listdir('/proc/self/fd'):
    try: assert {ambient:?} not in os.readlink('/proc/self/fd/'+fd)
    except FileNotFoundError: pass
try:
    socket.socket(socket.AF_UNIX)
    raise AssertionError('host broker socket allowed')
except PermissionError: pass
pid=os.fork()
if pid==0:
    assert not os.path.exists('/dev/input/event900')
    assert open('/dev/input/event901','rb',buffering=0).read(1)==b'\0'
    os._exit(0)
assert os.waitpid(pid,0)[1]==0
"#,
        ambient = ambient.path().to_str().unwrap()
    );
    let mut child = process(&code);
    assert_eq!(complete(&mut child), 0);
    child.stop().unwrap();
    child.stop().unwrap();
    assert!(Path::new("/dev/null").exists());
    assert!(Path::new("/dev/zero").exists());
}
#[test]
fn device_change_stops_owned_namespace_and_never_adds_a_new_path() {
    let mut child = process("import time; time.sleep(60)");
    let until = Instant::now() + Duration::from_secs(3);
    while !matches!(child.poll().unwrap(), Poll::Running) {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    let pid = child.child.id();
    child.inventory = Arc::new(|| Ok(vec![]));
    assert!(child.poll().err().unwrap().contains("topology changed"));
    child.stop().unwrap();
    assert!(child.child.try_wait().unwrap().is_some());
    assert!(!Path::new(&format!("/proc/{pid}")).exists());
    let mut child = process("raise SystemExit(7)");
    assert_eq!(complete(&mut child), 7);
}
#[test]
fn preflight_refuses_swapped_nodes_bad_applications_and_missing_approval() {
    assert!(start(request("pass"), vec![])
        .err()
        .unwrap()
        .contains("disabled"));
    assert!(trusted_program(Path::new("/missing/bwrap")).is_err());
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("helper");
    std::fs::write(&path, b"test").unwrap();
    assert!(trusted_program(&path).is_err());
    let mut helper = trusted_program(Path::new(BWRAP)).unwrap();
    assert!(approve_helper(&mut helper, None).is_err());
    assert!(approve_helper(&mut helper, Some("bad")).is_err());
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(BWRAP).unwrap();
    let mut helper = trusted_program(Path::new(BWRAP)).unwrap();
    approve_helper(&mut helper, Some(&format!("{:x}", Sha256::digest(bytes)))).unwrap();
    for (program, bundle) in [
        ("/missing/game", "/usr"),
        ("/usr/bin/true", "/"),
        ("/usr/bin/true", "/dev"),
        ("/usr/bin/true", "/tmp"),
        ("/dev/null", "/dev"),
        ("/usr/share", "/usr"),
    ] {
        let mut r = request("");
        r.executable = program.into();
        r.directory = bundle.into();
        assert!(executable(&r).is_err());
    }
    let nodes = vec![fixture(
        "/dev/input/event901",
        "/dev/zero",
        "virtual",
        "",
        true,
    )];
    let original = nodes.clone();
    assert!(Process::spawn(
        request("pass"),
        trusted_program(Path::new(BWRAP)).unwrap(),
        nodes.clone(),
        nodes.clone(),
        Arc::new(move || Ok(original.clone())),
        |_| File::open("/dev/null").map_err(|e| e.to_string())
    )
    .is_err());
    assert!(Process::spawn(
        request("pass"),
        trusted_program(Path::new(BWRAP)).unwrap(),
        nodes.clone(),
        nodes.clone(),
        Arc::new(|| Ok(vec![])),
        |_| File::open("/dev/zero").map_err(|e| e.to_string())
    )
    .is_err());
    assert!(Process::spawn(
        request("pass"),
        trusted_program(Path::new(BWRAP)).unwrap(),
        nodes.clone(),
        nodes,
        Arc::new(|| Err("inventory unavailable".into())),
        |_| Err("permission denied".into())
    )
    .is_err());
    let _ = inventory();
    // Exercise native ancestry on actual kernel sysfs objects without accessing input.
    let device = udev::Device::from_syspath(Path::new("/sys/devices/virtual/mem/null")).unwrap();
    assert!(!group(&device).unwrap().is_empty());
    assert!(phys(&device).is_empty());
}
#[test]
fn malformed_status_deadline_and_inventory_error_are_failures() {
    let mut child = process("import time; time.sleep(60)");
    child.status_bytes = vec![b'x'; 8193];
    assert!(child.poll().is_err());
    child.stop().unwrap();
    let mut child = process("import time; time.sleep(60)");
    child.status_bytes = b"garbage\n".to_vec();
    assert!(child.poll().is_err());
    child.stop().unwrap();
    let mut child = process("import time; time.sleep(60)");
    child.inventory = Arc::new(|| Err("capture topology unavailable".into()));
    assert!(child.poll().is_err());
    child.stop().unwrap();
    // A disposable nonblocking pipe isolates the startup deadline from runner speed.
    let mut child = process("import time; time.sleep(60)");
    let (reader, _writer) = pipe().unwrap();
    child.status = reader;
    child.started = Instant::now() - Duration::from_secs(4);
    assert!(child.poll().err().unwrap().contains("three seconds"));
    child.stop().unwrap();
}

#[test]
fn cancel_before_release_never_executes_application() {
    let mut child = process("raise SystemExit(73)");
    std::thread::sleep(Duration::from_millis(50));
    assert!(child.child.try_wait().unwrap().is_none());
    child.stop().unwrap();
    assert_ne!(child.child.try_wait().unwrap().unwrap().code(), Some(73));
}

fn descendants(pid: u32) -> Vec<u32> {
    let mut result = vec![pid];
    let path = format!("/proc/{pid}/task/{pid}/children");
    if let Ok(children) = std::fs::read_to_string(path) {
        for child in children
            .split_whitespace()
            .filter_map(|p| p.parse::<u32>().ok())
        {
            result.extend(descendants(child));
        }
    }
    result
}
fn live(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| s.rsplit_once(") ").map(|(_, rest)| rest.starts_with('Z')))
        .is_some_and(|zombie| !zombie)
}
#[test]
fn parent_death_cleans_namespace_children_without_drop() {
    const KEY: &str = "OSG_LAUNCH_TEST_PARENT_EXIT";
    if let Some(path) = std::env::var_os(KEY) {
        let mut child = process("import os,time; pid=os.fork(); time.sleep(60)");
        let until = Instant::now() + Duration::from_secs(3);
        while !matches!(child.poll().unwrap(), Poll::Running) {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        // Wait for the authored fork, then record only this owned process tree.
        let mut pids = descendants(child.child.id());
        while pids.len() < 4 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
            pids = descendants(child.child.id());
        }
        assert!(pids.len() >= 4);
        std::fs::write(path, serde_json::to_vec(&pids).unwrap()).unwrap();
        unsafe { libc::_exit(0) };
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("children.json");
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "visibility::launch_linux::tests::parent_death_cleans_namespace_children_without_drop",
            "--test-threads=1",
        ])
        .env(KEY, &file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let pids: Vec<u32> = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while pids.iter().any(|p| live(*p)) {
        assert!(
            Instant::now() < deadline,
            "owned child survived parent exit: {pids:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
