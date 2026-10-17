#![allow(dead_code)]
#[path = "../../../src-tauri/src/game_activation.rs"]
mod game_activation;
#[path = "../../../src-tauri/src/game_discovery.rs"]
mod game_discovery;
#[path = "../../../src-tauri/src/xplane_installation.rs"]
mod xplane_installation;
use game_discovery::{NativePlatform, Platform, ProcessIdentity};
use serde_json::{json, Value};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

struct OwnedHelper {
    child: Child,
    stop: PathBuf,
    ready: PathBuf,
    stopped: bool,
}
impl OwnedHelper {
    fn start(executable: &Path, root: &Path, label: &str) -> Self {
        let stop = root.join(format!("{label}.stop"));
        let ready = root.join(format!("{label}.ready"));
        assert!(!stop.exists() && !ready.exists());
        let mut command = Command::new(executable);
        command.args([&stop, &ready]).current_dir(root);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let child = command
            .spawn()
            .expect("Start only the private GUI-subsystem helper");
        let mut helper = Self {
            child,
            stop,
            ready,
            stopped: false,
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(pid) = fs::read_to_string(&helper.ready) {
                assert_eq!(pid.parse::<u32>().unwrap(), helper.child.id());
                break;
            }
            assert!(
                helper.child.try_wait().unwrap().is_none(),
                "Private helper exited before readiness"
            );
            assert!(
                Instant::now() < deadline,
                "Private helper readiness timed out"
            );
            thread::sleep(Duration::from_millis(10));
        }
        helper
    }
    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        fs::write(&self.stop, b"stop owned helper").unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                self.child
                    .kill()
                    .expect("Terminate only this owned Child handle");
                self.child.wait().unwrap();
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.stopped = true;
    }
}
impl Drop for OwnedHelper {
    fn drop(&mut self) {
        self.stop();
    }
}
fn found(platform: &NativePlatform, game: &Value, expected: &OwnedHelper) -> ProcessIdentity {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(identity) = platform.find(game).unwrap() {
            assert_eq!(identity.pid, expected.child.id());
            assert!(identity.epoch > 0);
            assert!(platform.alive(&identity).unwrap());
            return identity;
        }
        assert!(
            Instant::now() < deadline,
            "Native Windows enumeration did not observe the private helper"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn process_json(identity: &ProcessIdentity) -> Value {
    json!({"pid":identity.pid,"creationEpoch":identity.epoch.to_string(),"executable":identity.path})
}
fn main() {
    assert!(
        cfg!(windows),
        "This smoke requires actual Windows process APIs"
    );
    let mut arguments = std::env::args_os().skip(1);
    let source = fs::canonicalize(arguments.next().expect("Compiled private stub path")).unwrap();
    let root = fs::canonicalize(arguments.next().expect("Private output directory")).unwrap();
    assert!(arguments.next().is_none());
    assert!(
        source.starts_with(root.join("artifacts")),
        "Use only the copied private smoke artifact"
    );
    let run = root.join("runs").join(uuid::Uuid::new_v4().to_string());
    let bin = run.join("DCS World/bin");
    fs::create_dir_all(&bin).unwrap();
    let run = fs::canonicalize(run).unwrap();
    assert!(run.starts_with(&root));
    let executable = bin.join("DCS.exe");
    fs::copy(&source, &executable).unwrap();
    let executable = fs::canonicalize(executable).unwrap();
    assert!(executable.starts_with(&run));
    let platform = NativePlatform;
    let game = json!({"id":"dcs","installed":true,"processMonitoringSupported":true,"executable":executable.to_str().unwrap()});
    platform
        .validate("dcs", executable.to_str().unwrap())
        .unwrap();
    assert!(platform.find(&game).unwrap().is_none());

    // A basename match in another installation must not create authority.
    let other_bin = run.join("Other installation/bin");
    fs::create_dir_all(&other_bin).unwrap();
    let other_executable = other_bin.join("DCS.exe");
    fs::copy(&source, &other_executable).unwrap();
    let mut other = OwnedHelper::start(&other_executable, &run, "unconfigured");
    assert!(platform.find(&game).unwrap().is_none());
    let observer = game_activation::Observer::default();
    observer.observe(&platform, &[game.clone()]);
    assert_eq!(observer.view("dcs")["status"], "not-running");
    assert!(observer.snapshot().unwrap().is_empty());

    let mut first = OwnedHelper::start(&executable, &run, "first");
    let first_identity = found(&platform, &game, &first);
    other.stop();
    observer.observe(&platform, &[game.clone()]);
    assert_eq!(observer.view("dcs")["status"], "settling");
    assert!(
        observer.snapshot().unwrap().is_empty(),
        "A single actual observation cannot confirm authority"
    );
    observer.observe(&platform, &[game.clone()]);
    let first_context = observer.snapshot().unwrap().remove(0);
    observer.validate(&platform, &game, &first_context).unwrap();
    let first_game = game_activation::identity(&platform, &game).unwrap();
    assert_eq!(
        observer
            .confirmed_process(&first_game, &first_identity)
            .unwrap(),
        first_context
    );
    let mut wrong_epoch = first_identity.clone();
    wrong_epoch.epoch += 1;
    assert!(!platform.alive(&wrong_epoch).unwrap());
    let mut wrong_path = first_identity.clone();
    wrong_path.path.push_str(".wrong");
    assert!(!platform.alive(&wrong_path).unwrap());
    first.stop();
    assert!(!platform.alive(&first_identity).unwrap());
    assert!(
        observer.validate(&platform, &game, &first_context).is_err(),
        "Actual exit rejects previously cached proof before another observer scan"
    );
    observer.observe(&platform, &[game.clone()]);
    assert!(observer.snapshot().unwrap().is_empty());
    assert_eq!(observer.view("dcs")["status"], "not-running");

    let mut second = OwnedHelper::start(&executable, &run, "replacement");
    let second_identity = found(&platform, &game, &second);
    assert_ne!(
        (first_identity.pid, first_identity.epoch),
        (second_identity.pid, second_identity.epoch)
    );
    assert!(observer.validate(&platform, &game, &first_context).is_err());
    observer.observe(&platform, &[game.clone()]);
    assert!(observer.snapshot().unwrap().is_empty());
    assert_eq!(observer.view("dcs")["status"], "settling");
    observer.observe(&platform, &[game.clone()]);
    let second_context = observer.snapshot().unwrap().remove(0);
    assert_ne!(
        first_context.process_session,
        second_context.process_session
    );
    observer
        .validate(&platform, &game, &second_context)
        .unwrap();
    assert!(observer
        .confirmed_process(&first_game, &first_identity)
        .is_none());
    assert_eq!(
        observer
            .confirmed_process(&first_game, &second_identity)
            .unwrap(),
        second_context
    );
    assert!(observer.validate(&platform, &game, &first_context).is_err());
    thread::sleep(Duration::from_millis(2100));
    assert!(
        observer.snapshot().is_err(),
        "Old observation timestamps cannot retain process authority"
    );
    assert!(observer
        .validate(&platform, &game, &second_context)
        .is_err());
    observer.observe(&platform, &[game.clone()]);
    observer
        .validate(&platform, &game, &second_context)
        .unwrap();
    second.stop();
    observer.observe(&platform, &[game.clone()]);
    assert!(observer.snapshot().unwrap().is_empty());
    assert!(platform.find(&game).unwrap().is_none());
    let mut concurrent_a = OwnedHelper::start(&executable, &run, "concurrent-a");
    let concurrent_a_identity = found(&platform, &game, &concurrent_a);
    observer.observe(&platform, &[game.clone()]);
    observer.observe(&platform, &[game.clone()]);
    let before_ambiguity = observer.snapshot().unwrap().remove(0);
    let mut concurrent_b = OwnedHelper::start(&executable, &run, "concurrent-b");
    let concurrent_error = platform.find(&game).unwrap_err();
    assert!(concurrent_error.contains("Multiple game processes"));
    assert!(observer
        .validate(&platform, &game, &before_ambiguity)
        .is_err());
    observer.observe(&platform, &[game.clone()]);
    observer.observe(&platform, &[game.clone()]);
    let concurrent_observer = observer.view("dcs");
    assert_eq!(concurrent_observer["status"], "unavailable");
    assert_eq!(concurrent_observer["message"], concurrent_error);
    assert!(concurrent_observer["processSession"].is_null());
    assert!(observer.snapshot().unwrap().is_empty());
    assert!(observer
        .confirmed_process(&first_game, &concurrent_a_identity)
        .is_none());
    concurrent_a.stop();
    // One surviving exact process still needs two fresh observations.
    observer.observe(&platform, &[game.clone()]);
    assert_eq!(observer.view("dcs")["status"], "settling");
    assert!(observer.snapshot().unwrap().is_empty());
    observer.observe(&platform, &[game.clone()]);
    assert_eq!(observer.view("dcs")["status"], "verified");
    assert_eq!(observer.snapshot().unwrap().len(), 1);
    concurrent_b.stop();
    observer.observe(&platform, &[game.clone()]);
    assert!(observer.snapshot().unwrap().is_empty());
    assert!(platform.find(&game).unwrap().is_none());

    // DCS may configure both runtimes; uniqueness spans every selected path.
    let alternate_bin = run.join("DCS World/bin-mt");
    fs::create_dir_all(&alternate_bin).unwrap();
    let alternate_executable = alternate_bin.join("DCS.exe");
    fs::copy(&source, &alternate_executable).unwrap();
    let mut configured_a = OwnedHelper::start(&executable, &run, "configured-a");
    let mut configured_b = OwnedHelper::start(&alternate_executable, &run, "configured-b");
    let configured_error = platform.find(&game).unwrap_err();
    assert!(configured_error.contains("Multiple game processes"));
    observer.observe(&platform, &[game.clone()]);
    observer.observe(&platform, &[game.clone()]);
    assert_eq!(observer.view("dcs")["status"], "unavailable");
    assert!(observer.snapshot().unwrap().is_empty());
    configured_a.stop();
    configured_b.stop();
    observer.observe(&platform, &[game.clone()]);
    assert!(observer.snapshot().unwrap().is_empty());
    assert!(platform.find(&game).unwrap().is_none());

    // The separate global X-Plane guard still rejects different installations.
    let mut xplane_paths = Vec::new();
    for installation in ["XPlane A", "XPlane B"] {
        let bin = run.join(installation).join("bin");
        fs::create_dir_all(bin.join("Resources/plugins")).unwrap();
        let executable = bin.join("X-Plane.exe");
        fs::copy(&source, &executable).unwrap();
        fs::write(
            bin.join("Resources/plugins/DataRefs.txt"),
            "2 1210 fixture\n\n",
        )
        .unwrap();
        platform
            .validate("x-plane-12", executable.to_str().unwrap())
            .unwrap();
        xplane_paths.push(executable);
    }
    let xplane_game = json!({"id":"x-plane-12","installed":true,"processMonitoringSupported":true,
        "executable":xplane_paths[0].to_str().unwrap()});
    let mut xplane_a = OwnedHelper::start(&xplane_paths[0], &run, "xplane-a");
    let xplane_identity = found(&platform, &xplane_game, &xplane_a);
    let mut xplane_b = OwnedHelper::start(&xplane_paths[1], &run, "xplane-b");
    assert_eq!(
        platform.find(&xplane_game).unwrap(),
        Some(xplane_identity.clone())
    );
    let xplane_error = game_discovery::exclusive_xplane(&xplane_identity).unwrap_err();
    assert!(xplane_error.contains("Multiple X-Plane processes"));
    xplane_a.stop();
    xplane_b.stop();
    assert!(platform.find(&xplane_game).unwrap().is_none());

    let result = json!({"result":"pass","scope":"WINDOWS PROCESS FIXTURE; not a game or GamesRuntime/Coordinator confirmation",
        "actualProductionSources":["game_discovery.rs","game_activation.rs","xplane_installation.rs"],
        "nativeApis":["CreateToolhelp32Snapshot","OpenProcess","GetProcessTimes","QueryFullProcessImageNameW","WaitForSingleObject"],
        "first":process_json(&first_identity),"replacement":process_json(&second_identity),
        "checks":["validated DCS.exe fixture path","actual external start and exit","two distinct native observations before confirmation",
            "exact PID creation epoch and image proof","wrong epoch and image rejected","same-path replacement uses a new session",
            "old process proof rejected","stale observer snapshot rejected","same basename at unconfigured path ignored",
            "same configured path ambiguity rejects Observer authority","all configured paths require a unique process",
            "remaining single process needs two new observations","global X-Plane exclusivity preserved across installations",
            "owned private stopfiles and Child handles only"],
        "concurrentSamePath":{"ownedPids":[concurrent_a.child.id(),concurrent_b.child.id()],"error":concurrent_error,"observerAfterTwoScans":concurrent_observer},
        "concurrentConfiguredPaths":{"ownedPids":[configured_a.child.id(),configured_b.child.id()],"error":configured_error},
        "globalXPlane":{"ownedPids":[xplane_a.child.id(),xplane_b.child.id()],"error":xplane_error},
        "ownedPids":[other.child.id(),first.child.id(),second.child.id(),concurrent_a.child.id(),concurrent_b.child.id(),configured_a.child.id(),configured_b.child.id(),xplane_a.child.id(),xplane_b.child.id()],
        "helperAutoExitSeconds":15,"helpersStopped":true,"runDirectory":run});
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}
