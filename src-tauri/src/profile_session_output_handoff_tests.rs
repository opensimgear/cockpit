//! Hardware-free composition of the actual Session/Feeder with native handoff
//! helpers. FakeDriver owns only this test's memory; no native backend opens.
use super::*;
use crate::profile_session::{Coordinator, HandoffSource, State as SessionState};
use crate::profile_session_virtual::{self as output, PreparedHandoff};
use osg_mapping::{
    engine::{Report, ReportReason, RunToken},
    profile::AxisRange,
    program::{ControllerAxis, ControllerLayout, VirtualController},
};
use osg_profiles::{compiler_models::CompileSnapshot, default_document};
use osg_virtual_controller::{
    feeder::{Feeder, Message, LEASE_US},
    output_journal::{self, Phase as JournalPhase},
    session::{Channel, Factory, RunningOutput, Session},
    vjoy::{Axis, Binding, Driver, Position, Slot, Status as SlotStatus},
};
use std::{path::PathBuf, sync::Mutex, time::Duration};

struct DriverState {
    slot: Slot,
    calls: Vec<String>,
    messages: Vec<(u32, Message)>,
    next_pid: u32,
    fail_stop: bool,
    fail_recover: bool,
    foreign_after_release: bool,
}
struct FakeDriver {
    state: Arc<Mutex<DriverState>>,
    pid: u32,
}
impl Driver for FakeDriver {
    fn check(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn inspect(&mut self, _: u8) -> Result<Slot, String> {
        Ok(self.state.lock().unwrap().slot.clone())
    }
    fn acquire(&mut self, id: u8) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        if s.slot.id != id || s.slot.status != SlotStatus::Free {
            return Err("Fixture slot is not free".into());
        }
        s.calls.push(format!("acquire:{}:{id}", self.pid));
        s.slot.status = SlotStatus::Busy;
        s.slot.owner_pid = Some(self.pid);
        Ok(())
    }
    fn submit(&mut self, id: u8, position: &Position) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        if s.slot.id != id || s.slot.owner_pid != Some(self.pid) {
            return Err("Fixture output is not owned".into());
        }
        s.calls.push(format!(
            "submit:{}:{}:{}",
            self.pid, position.words[18], position.words[3]
        ));
        Ok(())
    }
    fn release(&mut self, id: u8) {
        let mut s = self.state.lock().unwrap();
        assert_eq!(s.slot.owner_pid, Some(self.pid));
        s.calls.push(format!("release:{}:{id}", self.pid));
        s.slot.status = SlotStatus::Free;
        s.slot.owner_pid = None;
        if s.foreign_after_release {
            s.foreign_after_release = false;
            s.slot.status = SlotStatus::Busy;
            s.slot.owner_pid = Some(9999);
        }
    }
}
struct Child {
    feeder: Feeder<FakeDriver>,
    state: Arc<Mutex<DriverState>>,
    pid: u32,
    alive: bool,
    clock: u64,
}
impl Channel for Child {
    fn request(&mut self, message: &Message) -> Result<(), String> {
        {
            let mut s = self.state.lock().unwrap();
            s.messages.push((self.pid, message.clone()));
            if matches!(message, Message::Stop { .. }) && s.fail_stop {
                return Err("Injected Stop acknowledgement failure".into());
            }
            if matches!(message, Message::Recover { .. }) && s.fail_recover {
                return Err("Injected cleanup failure".into());
            }
        }
        self.clock += 1;
        self.feeder.receive(message.clone(), self.clock)
    }
    fn alive(&mut self) -> bool {
        self.alive
    }
    fn process_id(&self) -> Option<u32> {
        Some(self.pid)
    }
    fn terminate(&mut self) {
        self.feeder.stop();
        self.alive = false;
    }
}
fn factory() -> (Factory, Arc<Mutex<DriverState>>) {
    let state = Arc::new(Mutex::new(DriverState {
        slot: Slot {
            id: 3,
            status: SlotStatus::Free,
            owner_pid: None,
            buttons: 16,
            axes: [48, 49, 50]
                .into_iter()
                .map(|usage| Axis {
                    usage,
                    min: 0,
                    max: 32767,
                })
                .collect(),
            discrete_hats: 0,
            continuous_hats: 2,
            force_feedback: false,
        },
        calls: vec![],
        messages: vec![],
        next_pid: 1000,
        fail_stop: false,
        fail_recover: false,
        foreign_after_release: false,
    }));
    let shared = state.clone();
    let factory: Factory = Arc::new(move || {
        let pid = {
            let mut s = shared.lock().unwrap();
            s.next_pid += 1;
            s.next_pid
        };
        Ok(Box::new(Child {
            feeder: Feeder::new(FakeDriver {
                state: shared.clone(),
                pid,
            }),
            state: shared.clone(),
            pid,
            alive: true,
            clock: 1,
        }))
    });
    (factory, state)
}
fn controller(name: &str, fallback: i32) -> VirtualController {
    VirtualController {
        id: name.into(),
        layout: ControllerLayout {
            buttons: (0..16).map(|i| format!("button-{i}")).collect(),
            axes: (0..3)
                .map(|i| ControllerAxis {
                    port_id: format!("axis-{i}"),
                    range: AxisRange::Unipolar,
                    fallback,
                })
                .collect(),
            hats: vec!["hat-0".into(), "hat-1".into()],
        },
    }
}
fn start(factory: &Factory, path: PathBuf, owner: &RunOwner, binding: Binding) -> Session {
    Session::start_with_journal(
        factory.clone(),
        token(owner),
        vec![binding],
        Counter(0),
        Some(path),
    )
    .unwrap()
}
fn token(owner: &RunOwner) -> RunToken {
    RunToken {
        session: owner.session.clone(),
        generation: Counter(owner.generation),
    }
}
fn candidate(revision: u64) -> Candidate {
    let mut document = default_document();
    document.revision = revision;
    Candidate {
        document,
        assignments: vec![],
        read_only: false,
        game_id: None,
        admission_epoch: Some(7),
        stage: None,
    }
}
fn owner(candidate: &Candidate) -> RunOwner {
    RunOwner {
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        profile_id: candidate.document.id.clone(),
        profile_revision: candidate.document.revision,
        game_id: None,
    }
}
fn run(candidate: &Candidate, owner: RunOwner) -> OwnedRun {
    OwnedRun {
        owner,
        snapshot: osg_profiles::session::AppliedSnapshot::capture(&candidate.document)
            .unwrap()
            .summary()
            .clone(),
        program_hash: format!("sha256:{}", "0".repeat(64)),
        capture_fingerprint: format!("sha256:{}", "0".repeat(64)),
        receipts: vec![],
        attempted: vec![ApplyPhase::Acquire],
    }
}
fn held(owner: &RunOwner, binding: &Binding) -> RecordingSink {
    let mut state = binding.fallback();
    state.buttons[0] = true;
    state.axes[0] = 23456;
    let mut sink = RecordingSink::default();
    sink.record(vec![Report {
        session: owner.session.clone(),
        run_generation: Counter(owner.generation),
        sequence: Counter(1),
        at_us: Counter(100),
        target: binding.target.clone(),
        reason: ReportReason::Change,
        state,
    }]);
    sink
}

struct FixtureHost {
    directory: PathBuf,
    factory: Factory,
    driver: Arc<Mutex<DriverState>>,
    outputs: BTreeMap<String, Session>,
    recordings: BTreeMap<String, RecordingSink>,
    bindings: BTreeMap<String, Vec<Binding>>,
    handoff: Option<PreparedHandoff>,
    authority: Option<RunOwner>,
}
impl Host for Arc<Mutex<FixtureHost>> {
    fn output_handoff(&mut self, run: &OwnedRun, _: u64) -> Result<Option<RunningOutput>, String> {
        let mut s = self.lock().unwrap();
        if s.authority.as_ref() != Some(&run.owner) {
            return Err("No outgoing authority".into());
        }
        s.outputs
            .get_mut(&key(&run.owner))
            .ok_or("No actual Session")?
            .handoff_snapshot()
    }
    fn prepare(&mut self, candidate: &Candidate, owner: &RunOwner) -> Result<Prepared, String> {
        let snapshot = CompileSnapshot {
            inventory_session: "fixture".into(),
            inventory_revision: "1".into(),
            assignments: vec![],
            resources: vec![],
            game_context: None,
            backend: Default::default(),
            inventory_diagnostics: vec![],
            read_only: false,
        };
        let report = osg_profiles::compiler::compile(&candidate.document, &snapshot);
        let mut s = self.lock().unwrap();
        let slots = vec![s.driver.lock().unwrap().slot.clone()];
        let proof = s.handoff.as_ref().and_then(|h| h.source.output.as_ref());
        let selected = output::select(
            &[controller(
                if candidate.document.revision == 1 {
                    "old-logical"
                } else {
                    "new-logical"
                },
                if candidate.document.revision == 1 {
                    0
                } else {
                    1234
                },
            )],
            &slots,
            proof,
        )?;
        s.bindings.insert(key(owner), selected.bindings);
        Ok(Prepared {
            plan: report.plan.ok_or("Empty fixture did not compile")?,
            admission: Default::default(),
            program_hash: report.program_hash.unwrap(),
            capture_fingerprint: report.capture_fingerprint,
        })
    }
    fn validate_prepared(&mut self, _: &Prepared, _: &RunOwner) -> Result<(), String> {
        Ok(())
    }
    fn capture(
        &mut self,
        _: &Prepared,
        owner: &RunOwner,
        previous: Option<&OwnedRun>,
    ) -> Result<Vec<Receipt>, String> {
        let mut s = self.lock().unwrap();
        if let Some(handoff) = s.handoff.clone() {
            let old = previous.ok_or("Missing outgoing run")?;
            let current = s
                .outputs
                .get_mut(&key(&old.owner))
                .unwrap()
                .handoff_snapshot()?;
            handoff.validate(
                previous,
                current.as_ref(),
                &s.bindings[&key(owner)],
                7,
                s.authority.as_ref(),
            )?;
        }
        Ok(vec![])
    }
    fn set_authority(&mut self, owner: Option<&RunOwner>) -> Result<(), String> {
        self.lock().unwrap().authority = owner.cloned();
        Ok(())
    }
    fn neutralize(&mut self, run: &OwnedRun) -> Result<(), String> {
        let mut state = self.lock().unwrap();
        let recording = state
            .recordings
            .remove(&key(&run.owner))
            .unwrap_or_default();
        let result = if let Some(output) = state.outputs.get_mut(&key(&run.owner)) {
            output::neutralize_output(output, &recording)
        } else {
            Ok(())
        };
        state.recordings.insert(key(&run.owner), recording);
        result
    }
    fn enter(&mut self, phase: ApplyPhase, _: &Prepared, run: &OwnedRun) -> Result<(), String> {
        if phase == ApplyPhase::Acquire {
            let mut s = self.lock().unwrap();
            let session = Session::start_with_journal(
                s.factory.clone(),
                token(&run.owner),
                s.bindings[&key(&run.owner)].clone(),
                Counter(0),
                Some(
                    s.directory
                        .join(format!("output-{}.json", run.owner.generation)),
                ),
            )?;
            s.outputs.insert(key(&run.owner), session);
            s.recordings
                .insert(key(&run.owner), RecordingSink::default());
        }
        Ok(())
    }
    fn restore(&mut self, run: &OwnedRun, recovery: bool) -> Result<(), String> {
        let mut state = self.lock().unwrap();
        if let Some(session) = state.outputs.get_mut(&key(&run.owner)) {
            output::restore_retained_output(session, &run.owner, recovery)?;
        } else if run.attempted.contains(&ApplyPhase::Acquire) {
            let path = state
                .directory
                .join(format!("output-{}.json", run.owner.generation));
            let factory = state.factory.clone();
            drop(state);
            output::recover_output(factory, path)?;
        }
        Ok(())
    }
    fn health(&mut self, _: &OwnedRun) -> Result<Health, String> {
        Ok(Health::default())
    }
    fn discard_preparation(&mut self, _: &RunOwner) {}
}

#[test]
fn coordinator_handoff_retires_the_exact_feeder_before_new_acquire_without_held_replay() {
    let directory = crate::test_directory();
    let (factory, driver) = factory();
    let shared = Arc::new(Mutex::new(FixtureHost {
        directory: directory.path().into(),
        factory,
        driver: driver.clone(),
        outputs: BTreeMap::new(),
        recordings: BTreeMap::new(),
        bindings: BTreeMap::new(),
        handoff: None,
        authority: None,
    }));
    let mut coordinator =
        Coordinator::new(shared.clone(), Some(directory.path().join("profile.json")));
    coordinator.apply(candidate(1)).unwrap();
    let old_owner = coordinator.status().owner.clone().unwrap();
    let mut incoming = candidate(2);
    // The native export asks for output proof only for a virtual-output candidate.
    // The actual selection/acquisition below uses its full native layout; the
    // Coordinator's empty graph isolates ownership phase ordering from graph I/O.
    let mut requested = incoming.clone();
    requested.document.virtual_outputs.push(
        serde_json::from_value(
            serde_json::from_str::<serde_json::Value>(include_str!(
                "../../tests/fixtures/profiles/complete-rig.json"
            ))
            .unwrap()["virtualOutputs"][0]
                .clone(),
        )
        .unwrap(),
    );
    let source = coordinator
        .native_handoff_source(&requested)
        .unwrap()
        .unwrap();
    {
        let mut host = shared.lock().unwrap();
        let binding = host.bindings[&key(&old_owner)][0].clone();
        let recording = held(&old_owner, &binding);
        host.outputs
            .get_mut(&key(&old_owner))
            .unwrap()
            .poll(&recording)
            .unwrap();
        host.recordings.insert(key(&old_owner), recording);
        host.handoff = Some(PreparedHandoff {
            source,
            slots: vec![driver.lock().unwrap().slot.clone()],
            observed: Instant::now(),
        });
    }
    incoming.document.id = old_owner.profile_id.clone();
    coordinator.apply(incoming).unwrap();
    let new_owner = coordinator.status().owner.clone().unwrap();
    assert_ne!(old_owner, new_owner);
    assert_eq!(coordinator.status().state, SessionState::Running);
    let calls = driver.lock().unwrap().calls.clone();
    let old_release = calls.iter().position(|c| c == "release:1001:3").unwrap();
    let new_acquire = calls.iter().position(|c| c == "acquire:1002:3").unwrap();
    assert!(old_release < new_acquire);
    assert!(calls[..old_release]
        .iter()
        .any(|c| c == "submit:1001:1:23456"));
    assert_eq!(calls[old_release - 1], "submit:1001:0:0");
    assert_eq!(calls[new_acquire + 1], "submit:1002:0:1234");
    coordinator.stop().unwrap();
    coordinator.stop().unwrap();
    coordinator.recover().unwrap();
    let s = driver.lock().unwrap();
    assert_eq!(
        s.messages
            .iter()
            .filter(|(_, m)| matches!(m, Message::Start { .. }))
            .count(),
        2
    );
    assert_eq!(
        s.messages
            .iter()
            .filter(|(_, m)| matches!(m, Message::Report { .. }))
            .count(),
        1
    );
    assert_eq!(
        s.messages
            .iter()
            .filter(|(_, m)| matches!(m, Message::Stop { .. }))
            .count(),
        2
    );
    assert_eq!(
        s.calls.iter().filter(|c| c.starts_with("release:")).count(),
        2
    );
    assert_eq!(s.slot.status, SlotStatus::Free);
    assert_eq!(coordinator.status().state, SessionState::Stopped);
}

#[test]
fn foreign_acquisition_after_retirement_is_preserved_by_the_fresh_feeder_start_guard() {
    let directory = crate::test_directory();
    let (factory, driver) = factory();
    let shared = Arc::new(Mutex::new(FixtureHost {
        directory: directory.path().into(),
        factory,
        driver: driver.clone(),
        outputs: BTreeMap::new(),
        recordings: BTreeMap::new(),
        bindings: BTreeMap::new(),
        handoff: None,
        authority: None,
    }));
    let mut coordinator =
        Coordinator::new(shared.clone(), Some(directory.path().join("profile.json")));
    coordinator.apply(candidate(1)).unwrap();
    let old = coordinator.status().owner.clone().unwrap();
    let mut incoming = candidate(2);
    incoming.document.id = old.profile_id.clone();
    let mut requested = incoming.clone();
    requested.document.virtual_outputs.push(
        serde_json::from_value(
            serde_json::from_str::<serde_json::Value>(include_str!(
                "../../tests/fixtures/profiles/complete-rig.json"
            ))
            .unwrap()["virtualOutputs"][0]
                .clone(),
        )
        .unwrap(),
    );
    let source = coordinator
        .native_handoff_source(&requested)
        .unwrap()
        .unwrap();
    shared.lock().unwrap().handoff = Some(PreparedHandoff {
        source,
        slots: vec![driver.lock().unwrap().slot.clone()],
        observed: Instant::now(),
    });
    driver.lock().unwrap().foreign_after_release = true;
    assert!(coordinator
        .apply(incoming)
        .unwrap_err()
        .contains("ownership"));
    assert_eq!(coordinator.status().state, SessionState::RecoveryNeeded);
    assert!(coordinator.status().owner.is_none());
    let state = driver.lock().unwrap();
    assert_eq!(state.slot.status, SlotStatus::Busy);
    assert_eq!(state.slot.owner_pid, Some(9999));
    assert!(state
        .calls
        .iter()
        .all(|c| !c.starts_with("acquire:1002:") && !c.starts_with("release:9999:")));
    assert_eq!(
        state
            .calls
            .iter()
            .filter(|c| c.starts_with("release:"))
            .count(),
        1
    );
    assert!(state
        .messages
        .iter()
        .all(|(_, m)| !matches!(m, Message::Report { .. })));
}

#[test]
fn handoff_fences_current_owner_epoch_worker_layout_and_inspection_age() {
    let directory = crate::test_directory();
    let (factory, driver) = factory();
    let candidate = candidate(1);
    let owner = owner(&candidate);
    let binding = output::bindings(
        &[controller("combined", 0)],
        &[driver.lock().unwrap().slot.clone()],
    )
    .unwrap()
    .remove(0);
    let mut session = start(
        &factory,
        directory.path().join("output.json"),
        &owner,
        binding,
    );
    let proof = session.handoff_snapshot().unwrap().unwrap();
    let run = run(&candidate, owner.clone());
    let source = HandoffSource {
        run: run.clone(),
        admission_epoch: 7,
        output: Some(proof.clone()),
    };
    let slots = vec![driver.lock().unwrap().slot.clone()];
    let selected = output::select(&[controller("renamed", 1234)], &slots, Some(&proof)).unwrap();
    assert!(selected.reused);
    assert_eq!(selected.bindings[0].slot, 3);
    let handoff = PreparedHandoff {
        source,
        slots: slots.clone(),
        observed: Instant::now(),
    };
    handoff
        .validate(
            Some(&run),
            Some(&proof),
            &selected.bindings,
            7,
            Some(&owner),
        )
        .unwrap();
    assert!(handoff
        .validate(
            Some(&run),
            Some(&proof),
            &selected.bindings,
            8,
            Some(&owner)
        )
        .is_err());
    assert!(handoff
        .validate(Some(&run), Some(&proof), &selected.bindings, 7, None)
        .is_err());
    assert!(handoff
        .validate(None, Some(&proof), &selected.bindings, 7, Some(&owner))
        .is_err());
    assert!(handoff
        .validate(Some(&run), None, &selected.bindings, 7, Some(&owner))
        .is_err());
    let mut foreign = handoff.clone();
    foreign.slots[0].owner_pid = Some(9999);
    assert!(foreign
        .validate(
            Some(&run),
            Some(&proof),
            &selected.bindings,
            7,
            Some(&owner)
        )
        .is_err());
    assert!(output::select(&[controller("renamed", 0)], &foreign.slots, Some(&proof)).is_err());
    foreign.slots[0].owner_pid = None;
    assert!(foreign
        .validate(
            Some(&run),
            Some(&proof),
            &selected.bindings,
            7,
            Some(&owner)
        )
        .is_err());
    foreign.slots.clear();
    assert!(foreign
        .validate(
            Some(&run),
            Some(&proof),
            &selected.bindings,
            7,
            Some(&owner)
        )
        .is_err());
    for change_session in [false, true] {
        let mut forged = handoff.clone();
        if change_session {
            forged.source.run.owner.session = uuid::Uuid::new_v4().to_string();
        } else {
            forged.source.run.owner.generation += 1;
        }
        assert!(forged
            .validate(
                Some(&forged.source.run),
                Some(&proof),
                &selected.bindings,
                7,
                Some(&forged.source.run.owner)
            )
            .is_err());
    }
    let mut lower_free = slots[0].clone();
    lower_free.id = 2;
    lower_free.status = SlotStatus::Free;
    lower_free.owner_pid = None;
    let preferred = output::select(
        &[controller("combined", 1234)],
        &[lower_free, slots[0].clone()],
        Some(&proof),
    )
    .unwrap();
    assert_eq!(preferred.bindings[0].slot, 3);
    assert!(preferred.reused);
    let mut changed = handoff.clone();
    changed.slots[0].buttons += 1;
    assert!(changed
        .validate(
            Some(&run),
            Some(&proof),
            &selected.bindings,
            7,
            Some(&owner)
        )
        .is_err());
    let mut aged = handoff.clone();
    aged.observed -= Duration::from_micros(LEASE_US);
    assert!(aged
        .validate(
            Some(&run),
            Some(&proof),
            &selected.bindings,
            7,
            Some(&owner)
        )
        .is_err());
    assert_eq!(driver.lock().unwrap().messages.len(), 1);
    output::neutralize_output(&mut session, &RecordingSink::default()).unwrap();
    output::neutralize_output(&mut session, &RecordingSink::default()).unwrap();
    assert!(session.handoff_snapshot().unwrap().is_none());
}

#[test]
fn retained_and_cold_recovery_are_cleanup_only_and_do_not_hold_a_nested_store_lock() {
    let directory = crate::test_directory();
    let path = directory.path().join("output.json");
    let (factory, driver) = factory();
    let candidate = candidate(1);
    let owner = owner(&candidate);
    let binding = output::bindings(
        &[controller("combined", 0)],
        &[driver.lock().unwrap().slot.clone()],
    )
    .unwrap()
    .remove(0);
    let mut session = start(&factory, path.clone(), &owner, binding.clone());
    driver.lock().unwrap().fail_stop = true;
    driver.lock().unwrap().fail_recover = true;
    assert!(output::neutralize_output(&mut session, &RecordingSink::default()).is_err());
    assert!(session.recovery_required());
    let before = driver.lock().unwrap().messages.len();
    output::neutralize_output(&mut session, &RecordingSink::default()).unwrap();
    assert_eq!(driver.lock().unwrap().messages.len(), before);
    assert!(output::restore_retained_output(&mut session, &owner, false).is_err());
    let mut foreign = owner.clone();
    foreign.generation += 1;
    assert!(output::restore_retained_output(&mut session, &foreign, true).is_err());
    assert_eq!(driver.lock().unwrap().messages.len(), before);
    driver.lock().unwrap().fail_recover = false;
    output::restore_retained_output(&mut session, &owner, true).unwrap();
    assert!(!session.active());
    assert!(!session.recovery_required());
    assert_eq!(
        output_journal::inspect(&path).unwrap().unwrap().phase,
        JournalPhase::Clean
    );
    drop(session);
    let before = driver.lock().unwrap().messages.len();
    output::recover_output(factory.clone(), path.clone()).unwrap();
    output::recover_output(factory.clone(), directory.path().join("missing.json")).unwrap();
    assert_eq!(driver.lock().unwrap().messages.len(), before);
    // A cold record has no live Session lock. Only the confirmed exact journal
    // may cause Recover; no Start/report can be reconstructed from it.
    let mut store = output_journal::Store::open(path.clone()).unwrap();
    store.begin(token(&owner), vec![binding.clone()]).unwrap();
    drop(store);
    assert_eq!(
        output::recover_output(factory.clone(), path.clone()).unwrap_err(),
        "Output ownership is ambiguous or already clean; inspect it before acknowledging recovery."
    );
    assert_eq!(driver.lock().unwrap().messages.len(), before);
    let mut store = output_journal::Store::open(path.clone()).unwrap();
    store.transition(JournalPhase::Confirmed).unwrap();
    drop(store);
    output::recover_output(factory, path.clone()).unwrap();
    assert!(matches!(
        driver.lock().unwrap().messages.last(),
        Some((_, Message::Recover { .. }))
    ));
    assert_eq!(
        output_journal::inspect(&path).unwrap().unwrap().phase,
        JournalPhase::Clean
    );
    assert_eq!(
        driver
            .lock()
            .unwrap()
            .messages
            .iter()
            .filter(|(_, m)| matches!(m, Message::Start { .. }))
            .count(),
        1
    );
    assert_eq!(driver.lock().unwrap().slot.status, SlotStatus::Free);
}

#[test]
fn actual_native_host_can_neutralize_an_already_stopped_session_repeatedly() {
    use crate::game_discovery::{Platform, ProcessIdentity};
    struct NoGame;
    impl Platform for NoGame {
        fn catalogue(&self, _: &BTreeMap<String, String>) -> Vec<serde_json::Value> {
            vec![]
        }
        fn validate(&self, _: &str, _: &str) -> Result<(), String> {
            Err("No game".into())
        }
        fn launch(&self, _: &serde_json::Value) -> Result<ProcessIdentity, String> {
            panic!("No launch")
        }
        fn find(&self, _: &serde_json::Value) -> Result<Option<ProcessIdentity>, String> {
            Ok(None)
        }
        fn alive(&self, _: &ProcessIdentity) -> Result<bool, String> {
            Ok(false)
        }
    }
    let directory = crate::test_directory();
    let resources = Arc::new(crate::profile_resources::Runtime::new(
        None,
        Arc::new(|_| vec![]),
        false,
    ));
    resources.refresh_native().unwrap();
    let controllers = Arc::new(osg_controller_input::ControllerService::new(None, false));
    let games = Arc::new(crate::games::GamesRuntime::new(
        Arc::new(|_| panic!("No physical cleanup I/O")),
        crate::games::Paths {
            installations: directory.path().join("installations.json"),
            provider_setup: directory.path().join("setup.json"),
            recovery: directory.path().join("games-recovery.json"),
        },
        Arc::new(NoGame),
    ));
    let companions = Arc::new(crate::companion_host::Runtime::new(
        None,
        Arc::new(osg_mapping_host::companions::CompanionProvider::new(
            Arc::new(|| 0),
        )),
    ));
    let mut host = NativeProfileHost::new(
        resources.clone(),
        controllers.clone(),
        games.clone(),
        companions.clone(),
        Arc::new(crate::gateways::ProfilePanelIo::empty()),
        false,
        Arc::new(AtomicU64::new(7)),
    );
    let mut candidate = candidate(1);
    candidate.stage = Some(
        NativeProfileHost::stage(
            &resources,
            &games,
            &companions,
            controllers.clone(),
            false,
            &candidate,
        )
        .unwrap(),
    );
    let owner = owner(&candidate);
    host.prepare(&candidate, &owner).unwrap();
    let (factory, driver) = factory();
    let binding = output::bindings(
        &[controller("combined", 0)],
        &[driver.lock().unwrap().slot.clone()],
    )
    .unwrap()
    .remove(0);
    let session = start(
        &factory,
        directory.path().join("owned-output.json"),
        &owner,
        binding.clone(),
    );
    {
        let state = host.run(&owner).unwrap();
        state.output = Some(session);
        state.bindings = vec![binding.clone()];
        state.recording = held(&owner, &binding);
    }
    let mut run = run(&candidate, owner.clone());
    run.receipts.push(Receipt {
        kind: "virtual-output".into(),
        resource_id: "virtual-output".into(),
        state: json!({"journal": output::journal_name(&owner).unwrap()}),
    });
    host.set_authority(Some(&owner)).unwrap();
    let source = HandoffSource {
        run: run.clone(),
        admission_epoch: 7,
        output: host.output_handoff(&run, 7).unwrap(),
    };
    let mut incoming = super::output_handoff_tests::candidate(2);
    let layout = controller("combined", 1234).layout;
    let ports = layout
        .buttons
        .iter()
        .cloned()
        .chain(layout.axes.iter().map(|axis| axis.port_id.clone()))
        .chain(layout.hats.iter().cloned())
        .map(|id| osg_profiles::Port {
            name: id.clone(),
            direction: osg_profiles::Direction::Input,
            contract: layout.virtual_port_contract(&id).unwrap(),
            id,
        })
        .collect();
    incoming
        .document
        .virtual_outputs
        .push(osg_profiles::VirtualOutput {
            id: "combined".into(),
            name: "Combined".into(),
            required: true,
            layout,
            ports,
        });
    let inspections = AtomicU64::new(0);
    incoming.stage = Some(
        NativeProfileHost::stage_internal(
            &resources,
            &games,
            &companions,
            controllers,
            true,
            &incoming,
            Some(run.clone()),
            Some(source),
            &|| {
                inspections.fetch_add(1, Ordering::SeqCst);
                Ok(vec![driver.lock().unwrap().slot.clone()])
            },
        )
        .unwrap(),
    );
    assert_eq!(inspections.load(Ordering::SeqCst), 2);
    let incoming_owner = RunOwner {
        session: uuid::Uuid::new_v4().to_string(),
        generation: 2,
        profile_revision: 2,
        ..owner.clone()
    };
    let prepared = host.prepare(&incoming, &incoming_owner).unwrap();
    // Passive proof tests do not qualify or activate the real backend.
    assert!(!prepared.admission.blockers.is_empty());
    assert_eq!(host.run(&incoming_owner).unwrap().bindings[0].slot, 3);
    host.capture(&prepared, &incoming_owner, Some(&run))
        .unwrap();
    assert!(host.run(&owner).unwrap().output.as_ref().unwrap().active());
    assert!(host.run(&incoming_owner).unwrap().output.is_none());
    assert_eq!(driver.lock().unwrap().messages.len(), 1);
    host.neutralize(&run).unwrap();
    // Later Overrides validation must not require the old, now-stopped worker.
    host.run(&incoming_owner)
        .unwrap()
        .output_handoff
        .as_mut()
        .unwrap()
        .observed -= Duration::from_micros(LEASE_US);
    host.validate_prepared(&prepared, &incoming_owner).unwrap();
    host.neutralize(&run).unwrap();
    host.neutralize(&run).unwrap();
    let output = host.run(&owner).unwrap().output.as_mut().unwrap();
    assert!(!output.active());
    assert!(!output.recovery_required());
    assert!(output.handoff_snapshot().unwrap().is_none());
    let s = driver.lock().unwrap();
    assert_eq!(
        s.messages
            .iter()
            .filter(|(_, m)| matches!(m, Message::Start { .. }))
            .count(),
        1
    );
    assert_eq!(
        s.messages
            .iter()
            .filter(|(_, m)| matches!(m, Message::Report { .. }))
            .count(),
        1
    );
    assert_eq!(
        s.messages
            .iter()
            .filter(|(_, m)| matches!(m, Message::Stop { .. }))
            .count(),
        1
    );
    assert_eq!(
        s.calls.iter().filter(|c| c.starts_with("release:")).count(),
        1
    );
    assert_eq!(s.slot.status, SlotStatus::Free);
}
