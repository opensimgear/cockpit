//! Each native collector gets a fresh process-global owner and real local file.
use super::super::*;
use std::{fs, process::{Command, Stdio}};

const PROCESS_ROLE: &str = "OSG_TEST_AUDIT_PROCESS_ROLE";
const TEST: &str = "audit::collector::tests::native_process::native_parent_worker_files_checkpoint_and_finish";

fn command(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", TEST, "--nocapture"])
        .env(PROCESS_ROLE, role).stdin(Stdio::null());
    command
}

#[test]
fn native_parent_worker_files_checkpoint_and_finish() {
    if let Ok(role) = std::env::var(PROCESS_ROLE) {
        let worker = role == "worker";
        native::initialize(if worker { Role::Worker } else { Role::Parent });
        if option_env!("OSG_DEV_DESKTOP_INPUT") != Some("1") {
            assert!(!native::enabled());
            let report = native::checkpoint(Checkpoint::Manual, Duration::from_secs(1));
            assert!(report.configured && !report.enabled && !report.complete);
            return;
        }
        assert!(native::enabled());
        assert!(native::process_instance().unwrap() > 0);
        if worker {
            assert_eq!(native::channel(), Some(13));
            assert!(native::parent_instance().unwrap() > 0);
            assert_ne!(native::parent_pid().unwrap(), std::process::id() as u64);
        } else {
            assert_eq!(native::channel(), None);
            assert_eq!(native::parent_pid(), None);
            assert_eq!(native::parent_instance(), None);
        }
        assert!(native::next_id().unwrap() > 0);
        let alias = native::alias(AliasKind::Run, b"native-alias-key-must-not-be-written").unwrap();
        for sequence in 1..=3 {
            native::record_at(native::stamp().unwrap(), Kind::Marker, &[alias, sequence]);
        }
        for reason in [Checkpoint::Manual, Checkpoint::Stopped, Checkpoint::RunComplete] {
            let report = native::checkpoint(reason, Duration::from_secs(1));
            assert!(report.enabled && report.complete && report.valid && !report.finished);
            assert_eq!(report.through_sequence, 3);
        }
        if !worker {
            let mut child = command("worker");
            native::configure_child(&mut child, 13);
            let result = child.output().unwrap();
            assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        }
        let report = native::finish(if worker { ExitReason::Stop } else { ExitReason::Normal }, Duration::from_secs(1));
        assert!(report.complete && report.valid && report.finished);
        // Repeated finalization cannot emit another footer or resurrect the writer.
        assert!(native::finish(ExitReason::Error, Duration::from_secs(1)).finished);
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let result = command("parent")
        .env(ENV_DIRECTORY, directory.path()).env(ENV_RUN, "native-audit-capacity-probe")
        .env_remove(ENV_PARENT_PID).env_remove(ENV_PARENT_INSTANCE).env_remove(ENV_CHANNEL)
        .output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let files = fs::read_dir(directory.path()).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
    let enabled = option_env!("OSG_DEV_DESKTOP_INPUT") == Some("1");
    assert_eq!(files.len(), if enabled { 2 } else { 0 });
    for path in files {
        let text = fs::read_to_string(path).unwrap();
        assert!(!text.contains("native-alias-key-must-not-be-written"));
        let records = text.lines().map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()).collect::<Vec<_>>();
        assert_eq!(records.iter().filter(|record| record["type"] == "event").count(), 3);
        assert_eq!(records.iter().filter(|record| record["type"] == "footer").count(), 1);
        let report = &records.last().unwrap()["report"];
        assert_eq!(report["complete"], true);
        assert_eq!(report["valid"], true);
        assert_eq!(report["counters"]["ioErrors"], "0");
    }
}
