use super::*;
use std::os::windows::process::CommandExt;
use std::{
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let until = Instant::now() + Duration::from_secs(1);
        while Instant::now() < until {
            if self.0.try_wait().ok().flatten().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}
#[test]
fn own_process_identity_is_exact_and_rejects_stale_or_unrelated_executables() {
    let own = current().unwrap();
    let path = std::env::current_exe().unwrap();
    let process = Process::track(&own, &path).unwrap().unwrap();
    assert!(process.alive().unwrap());
    assert_eq!(identity(own.pid).unwrap(), own);
    let mut stale = own.clone();
    stale.created = (own.validate().unwrap() - 1).to_string();
    assert!(Process::track(&stale, &path).unwrap().is_none());
    let absent = tempfile::tempdir().unwrap();
    assert!(Process::track(&own, &absent.path().join("missing.exe")).is_err());
    for invalid in [
        Identity {
            pid: 0,
            created: "1".into(),
        },
        Identity {
            pid: 1,
            created: "0".into(),
        },
        Identity {
            pid: 1,
            created: "01".into(),
        },
        Identity {
            pid: 1,
            created: "-1".into(),
        },
        Identity {
            pid: 1,
            created: "x".into(),
        },
    ] {
        assert!(Process::track(&invalid, &path).is_err());
    }
    assert!(identity(0).is_err());
    assert!(created(ptr::null_mut()).is_err());
    assert!(image(ptr::null_mut()).is_err());
    assert!(user_sid(ptr::null_mut()).is_err());
    assert!(Process {
        handle: Handle(ptr::null_mut())
    }
    .alive()
    .is_err());
}
#[test]
fn a_retained_handle_observes_only_the_original_child_exit() {
    let python = std::env::var("OSG_TEST_PYTHON").unwrap_or_else(|_| "python".into());
    let mut child = ChildGuard(
        Command::new(python)
            .args(["-c", "import time;time.sleep(30)"])
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let id = identity(child.0.id()).unwrap();
    let raw = open(id.pid).unwrap().unwrap();
    let path = image(raw.0).unwrap();
    drop(raw);
    assert!(Process::track(&id, &std::env::current_exe().unwrap()).is_err());
    let process = Process::track(&id, &path).unwrap().unwrap();
    assert!(process.alive().unwrap());
    child.0.kill().unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while process.alive().unwrap() && Instant::now() < until {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(!process.alive().unwrap());
    assert!(Process::track(&id, &path).unwrap().is_none());
}
