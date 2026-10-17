use super::*;
use osg_profiles::compiler_models::{BackendCapabilities, CompileSnapshot};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Probe {
    phases: Vec<ApplyPhase>,
    authority: Option<RunOwner>,
    held_polls: usize,
    restores: usize,
    epoch: Option<u64>,
    fail_install: bool,
    fail_start: bool,
    fail_health: bool,
}
struct Fake(Arc<Mutex<Probe>>);
fn compile(candidate: &Candidate) -> Prepared {
    let report = osg_profiles::compiler::compile(
        &candidate.document,
        &CompileSnapshot {
            inventory_session: "launch-fixture".into(),
            inventory_revision: "1".into(),
            assignments: vec![],
            resources: vec![],
            game_context: None,
            backend: BackendCapabilities::default(),
            inventory_diagnostics: vec![],
            read_only: false,
        },
    );
    Prepared {
        plan: report.plan.unwrap(),
        admission: Admission::default(),
        program_hash: report.program_hash.unwrap(),
        capture_fingerprint: report.capture_fingerprint,
    }
}
impl Host for Fake {
    fn prepare(&mut self, c: &Candidate, _: &RunOwner) -> Result<Prepared, String> {
        Ok(compile(c))
    }
    fn validate_prepared(&mut self, _: &Prepared, _: &RunOwner) -> Result<(), String> {
        Ok(())
    }
    fn capture(
        &mut self,
        _: &Prepared,
        _: &RunOwner,
        _: Option<&OwnedRun>,
    ) -> Result<Vec<Receipt>, String> {
        Ok(vec![])
    }
    fn set_authority(&mut self, o: Option<&RunOwner>) -> Result<(), String> {
        self.0.lock().unwrap().authority = o.cloned();
        Ok(())
    }
    fn neutralize(&mut self, _: &OwnedRun) -> Result<(), String> {
        Ok(())
    }
    fn enter(&mut self, p: ApplyPhase, _: &Prepared, _: &OwnedRun) -> Result<(), String> {
        let mut probe = self.0.lock().unwrap();
        probe.phases.push(p);
        if p == ApplyPhase::Start && probe.fail_start {
            Err("uncertain Start".into())
        } else {
            Ok(())
        }
    }
    fn restore(&mut self, _: &OwnedRun, _: bool) -> Result<(), String> {
        self.0.lock().unwrap().restores += 1;
        Ok(())
    }
    fn health(&mut self, _: &OwnedRun) -> Result<Health, String> {
        panic!("A waiting launch must never poll a running reducer")
    }
    fn supports_launch_hold(&self) -> bool {
        true
    }
    fn hold_health(&mut self, _: &OwnedRun) -> Result<Health, String> {
        let mut p = self.0.lock().unwrap();
        p.held_polls += 1;
        if p.fail_health {
            Err("owned visibility lost".into())
        } else {
            Ok(Health::default())
        }
    }
    fn bind_launch_epoch(&mut self, _: &OwnedRun, epoch: u64) -> Result<(), String> {
        self.0.lock().unwrap().epoch = Some(epoch);
        Ok(())
    }
    fn install_launch_context(&mut self, _: &OwnedRun, c: &Candidate) -> Result<Prepared, String> {
        if self.0.lock().unwrap().fail_install {
            return Err("native game proof changed".into());
        }
        c.stage.as_ref().ok_or("missing stage")?.take::<Prepared>(c)
    }
    fn discard_preparation(&mut self, _: &RunOwner) {}
}
fn fixture(path: Option<PathBuf>) -> (Coordinator<Fake>, Arc<Mutex<Probe>>) {
    let p = Arc::new(Mutex::new(Probe::default()));
    (Coordinator::new(Fake(p.clone()), path), p)
}
fn candidate() -> Candidate {
    Candidate {
        document: osg_profiles::default_document(),
        assignments: vec![],
        read_only: false,
        game_id: None,
        admission_epoch: Some(41),
        stage: None,
    }
}
fn context(c: &Coordinator<Fake>, owner: &RunOwner) -> Candidate {
    let mut candidate = c.launch_candidate(owner, "dcs", 42).unwrap();
    candidate.stage = Some(Stage::new(&candidate, compile(&candidate)).unwrap());
    candidate
}

#[test]
fn waiting_launch_keeps_one_journaled_owner_neutral_and_never_starts_graph_work() {
    let (mut c, p) = fixture(None);
    let owner = c.begin_launch(candidate()).unwrap();
    assert_eq!(c.status.state, State::AwaitingGame);
    assert!(!c.validates(&owner));
    assert!(c.manifest.active.is_none());
    assert_eq!(
        c.manifest
            .pending
            .as_ref()
            .unwrap()
            .incoming
            .as_ref()
            .unwrap()
            .attempted,
        &APPLY_PHASES[..4]
    );
    c.bind_launch_epoch(&owner, 42).unwrap();
    for _ in 0..3 {
        c.poll().unwrap();
    }
    let p = p.lock().unwrap();
    assert_eq!(p.phases, &APPLY_PHASES[..4]);
    assert_eq!(p.held_polls, 3);
    assert!(p.authority.is_none());
}
#[test]
fn verified_context_starts_once_without_reacquiring_or_changing_hardware_owner() {
    let (mut c, p) = fixture(None);
    let owner = c.begin_launch(candidate()).unwrap();
    c.bind_launch_epoch(&owner, 42).unwrap();
    let fresh = context(&c, &owner);
    c.finish_launch(&owner, fresh.clone()).unwrap();
    assert_eq!(c.status.owner, Some(owner.clone()));
    assert_eq!(c.status.state, State::Running);
    assert!(c.manifest.pending.is_none());
    assert_eq!(
        p.lock().unwrap().phases,
        vec![
            ApplyPhase::Overrides,
            ApplyPhase::Acquire,
            ApplyPhase::Baseline,
            ApplyPhase::Visibility,
            ApplyPhase::Baseline,
            ApplyPhase::Start
        ]
    );
    assert!(c.finish_launch(&owner, fresh).is_err());
    assert_eq!(
        p.lock()
            .unwrap()
            .phases
            .iter()
            .filter(|p| **p == ApplyPhase::Start)
            .count(),
        1
    );
}
#[test]
fn stop_or_pause_cancels_hold_and_late_context_cannot_stop_a_newer_owner() {
    for pause in [false, true] {
        let (mut c, p) = fixture(None);
        let owner = c.begin_launch(candidate()).unwrap();
        c.bind_launch_epoch(&owner, 42).unwrap();
        let late = context(&c, &owner);
        if pause {
            c.pause(true).unwrap();
            assert!(c.status.paused);
            c.pause(false).unwrap();
        } else {
            c.stop().unwrap();
        }
        assert!(c.finish_launch(&owner, late).is_err());
        c.apply(candidate()).unwrap();
        let newer = c.status.owner.clone().unwrap();
        assert_ne!(newer, owner);
        assert!(!c.cancel_launch(&owner).unwrap());
        assert!(c.validates(&newer));
        assert_eq!(
            p.lock()
                .unwrap()
                .phases
                .iter()
                .filter(|p| **p == ApplyPhase::Start)
                .count(),
            1
        );
    }
}
#[test]
fn pending_launch_rejects_replacement_or_modified_candidate_and_epoch_adoption() {
    let (mut c, _) = fixture(None);
    let owner = c.begin_launch(candidate()).unwrap();
    assert!(c.launch_candidate(&owner, "dcs", 42).is_err());
    c.bind_launch_epoch(&owner, 42).unwrap();
    assert!(c.bind_launch_epoch(&owner, 43).is_err());
    assert!(c.launch_candidate(&owner, "dcs", 43).is_err());
    assert!(c.apply(candidate()).is_err());
    let mut fresh = context(&c, &owner);
    fresh.document.revision += 1;
    assert!(c.finish_launch(&owner, fresh).is_err());
    assert_eq!(c.status.state, State::AwaitingGame);
    assert!(c.cancel_launch(&owner).unwrap());
}
#[test]
fn lost_health_or_deadline_compensates_without_start_or_replay() {
    for expired in [false, true] {
        let (mut c, p) = fixture(None);
        let owner = c.begin_launch(candidate()).unwrap();
        c.bind_launch_epoch(&owner, 42).unwrap();
        if expired {
            c.launch.as_mut().unwrap().deadline = Instant::now() - Duration::from_secs(1);
        } else {
            p.lock().unwrap().fail_health = true;
        }
        c.poll().unwrap();
        assert_eq!(c.status.state, State::Faulted);
        assert!(c.status.owner.is_none());
        assert!(c.launch.is_none());
        assert!(p.lock().unwrap().restores > 0);
        assert!(!p.lock().unwrap().phases.contains(&ApplyPhase::Start));
    }
}
#[test]
fn context_or_uncertain_start_failure_requires_explicit_recovery_and_never_replays() {
    for start in [false, true] {
        let (mut c, p) = fixture(None);
        let owner = c.begin_launch(candidate()).unwrap();
        c.bind_launch_epoch(&owner, 42).unwrap();
        let fresh = context(&c, &owner);
        if start {
            p.lock().unwrap().fail_start = true;
        } else {
            p.lock().unwrap().fail_install = true;
        }
        assert!(c.finish_launch(&owner, fresh).is_err());
        assert_eq!(c.status.state, State::RecoveryNeeded);
        let before = p.lock().unwrap().phases.len();
        c.recover().unwrap();
        assert_eq!(p.lock().unwrap().phases.len(), before);
        assert!(c.status.owner.is_none());
    }
}
#[test]
fn altered_context_program_is_compensated_before_start() {
    let (mut c, p) = fixture(None);
    let owner = c.begin_launch(candidate()).unwrap();
    c.bind_launch_epoch(&owner, 42).unwrap();
    let mut fresh = c.launch_candidate(&owner, "dcs", 42).unwrap();
    let mut prepared = compile(&fresh);
    prepared.program_hash = format!("sha256:{}", "b".repeat(64));
    fresh.stage = Some(Stage::new(&fresh, prepared).unwrap());
    assert!(c.finish_launch(&owner, fresh).is_err());
    assert_eq!(c.status.state, State::RecoveryNeeded);
    assert!(!p.lock().unwrap().phases.contains(&ApplyPhase::Start));
}
#[test]
fn crash_in_waiting_state_is_restore_only_and_journal_cannot_restart_launch() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let (mut c, _) = fixture(Some(path.clone()));
    let owner = c.begin_launch(candidate()).unwrap();
    c.bind_launch_epoch(&owner, 42).unwrap();
    drop(c);
    let (mut recovered, p) = fixture(Some(path));
    assert_eq!(recovered.status.state, State::RecoveryNeeded);
    assert!(recovered.launch.is_none());
    recovered.recover().unwrap();
    assert!(p.lock().unwrap().phases.is_empty());
    assert_eq!(recovered.status.state, State::Stopped);
}
