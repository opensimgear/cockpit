use super::*;
use osg_mapping::engine::{Report, ReportReason};
use std::sync::Mutex;
#[derive(Default)]
struct State {
    spawned: usize,
    messages: Vec<Message>,
    terminated: Vec<usize>,
    dead: bool,
    fail: &'static str,
    fail_spawn: bool,
    observe: Option<std::path::PathBuf>,
    process_id: Option<u32>,
}
struct Child {
    state: Arc<Mutex<State>>,
    id: usize,
}
impl Channel for Child {
    fn request(&mut self, message: &Message) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if let Some(path) = &state.observe {
            let phase = crate::output_journal::inspect(path).unwrap().unwrap().phase;
            match message {
                Message::Start { .. } => assert_eq!(phase, Phase::Intent),
                Message::Stop { .. } | Message::Recover { .. } => {
                    assert_eq!(phase, Phase::Releasing)
                }
                _ => assert_eq!(phase, Phase::Confirmed),
            }
        }
        state.messages.push(message.clone());
        let command = match message {
            Message::Start { .. } => "start",
            Message::Renew { .. } => "renew",
            Message::Report { .. } => "report",
            Message::Stop { .. } => "stop",
            Message::Recover { .. } => "recover",
        };
        if state.fail == command {
            Err(format!("{command} failed"))
        } else {
            Ok(())
        }
    }
    fn alive(&mut self) -> bool {
        !self.state.lock().unwrap().dead
    }
    fn process_id(&self) -> Option<u32> {
        self.state.lock().unwrap().process_id
    }
    fn terminate(&mut self) {
        self.state.lock().unwrap().terminated.push(self.id);
    }
}
fn factory() -> (Factory, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    let shared = state.clone();
    (
        Arc::new(move || {
            let mut state = shared.lock().unwrap();
            if state.fail_spawn {
                return Err("spawn failed".into());
            }
            state.spawned += 1;
            Ok(Box::new(Child {
                state: shared.clone(),
                id: state.spawned,
            }))
        }),
        state,
    )
}
fn token() -> RunToken {
    RunToken {
        session: uuid::Uuid::new_v4().to_string(),
        generation: Counter(1),
    }
}
fn recording(token: &RunToken, count: u64) -> RecordingSink {
    let mut sink = RecordingSink::default();
    sink.record(
        (1..=count)
            .map(|n| {
                let binding = crate::vjoy::tests::binding();
                let mut state = binding.fallback();
                state.buttons[0] = n % 2 != 0;
                Report {
                    session: token.session.clone(),
                    run_generation: token.generation,
                    sequence: Counter(n),
                    at_us: Counter(n),
                    target: binding.target,
                    reason: ReportReason::Change,
                    state,
                }
            })
            .collect(),
    );
    sink
}
#[test]
fn handoff_proof_requires_the_same_live_confirmed_owner_without_sending_output() {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let path = dir.path().join("output.json");
    let (factory, state) = factory();
    state.lock().unwrap().process_id = Some(73);
    let run = token();
    let bindings = vec![crate::vjoy::tests::binding()];
    let mut session = Session::start_with_journal(
        factory,
        run.clone(),
        bindings.clone(),
        Counter(0),
        Some(path.clone()),
    )
    .unwrap();
    let original = sql_read(&path).unwrap();
    let proof = session.handoff_snapshot().unwrap().unwrap();
    assert_eq!(proof.token(), &run);
    assert_eq!(proof.process_id(), 73);
    assert_eq!(serde_json::to_vec(proof.bindings()).unwrap(), serde_json::to_vec(&bindings).unwrap());
    assert!(proof.clone().same_as(&session.handoff_snapshot().unwrap().unwrap()));

    session.token.generation = Counter(run.generation.0 + 1);
    assert!(session.handoff_snapshot().is_err());
    session.token = run;
    session.bindings[0].slot += 1;
    assert!(session.handoff_snapshot().is_err());
    session.bindings = bindings;
    state.lock().unwrap().process_id = Some(74);
    assert!(!proof.same_as(&session.handoff_snapshot().unwrap().unwrap()));
    for pid in [None, Some(0)] {
        state.lock().unwrap().process_id = pid;
        assert!(session.handoff_snapshot().unwrap().is_none());
    }
    state.lock().unwrap().process_id = Some(73);
    state.lock().unwrap().dead = true;
    assert!(session.handoff_snapshot().is_err());
    state.lock().unwrap().dead = false;
    assert!(proof.same_as(&session.handoff_snapshot().unwrap().unwrap()));
    assert_eq!(sql_read(&path).unwrap(), original);
    {
        let state = state.lock().unwrap();
        assert_eq!(state.spawned, 1);
        assert!(matches!(state.messages.as_slice(), [Message::Start { .. }]));
        assert!(state.terminated.is_empty());
    }
    session.persist_phase(Phase::Releasing).unwrap();
    assert!(session.handoff_snapshot().is_err());
    session.stop().unwrap();
    assert!(session.handoff_snapshot().unwrap().is_none());
}

#[test]
fn anonymous_unjournaled_or_unresolved_sessions_cannot_authorize_handoff() {
    struct Anonymous(Child);
    impl Channel for Anonymous {
        fn request(&mut self, message: &Message) -> Result<(), String> {
            self.0.request(message)
        }
        fn alive(&mut self) -> bool {
            self.0.alive()
        }
        fn terminate(&mut self) {
            self.0.terminate()
        }
    }
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let state = Arc::new(Mutex::new(State::default()));
    let shared = state.clone();
    let anonymous: Factory = Arc::new(move || {
        let mut state = shared.lock().unwrap();
        state.spawned += 1;
        Ok(Box::new(Anonymous(Child { state: shared.clone(), id: state.spawned })))
    });
    let mut session = Session::start_with_journal(
        anonymous, token(), vec![crate::vjoy::tests::binding()], Counter(0),
        Some(dir.path().join("anonymous.json")),
    ).unwrap();
    assert!(session.handoff_snapshot().unwrap().is_none());
    assert_eq!(state.lock().unwrap().messages.len(), 1);
    session.stop().unwrap();

    let (factory, state) = factory();
    state.lock().unwrap().process_id = Some(73);
    let mut session = Session::start(
        factory.clone(), token(), vec![crate::vjoy::tests::binding()], Counter(0),
    ).unwrap();
    assert!(session.handoff_snapshot().unwrap().is_none());
    assert_eq!(state.lock().unwrap().messages.len(), 1);
    session.stop().unwrap();
    let mut session = Session::start_with_journal(
        factory, token(), vec![crate::vjoy::tests::binding()], Counter(0),
        Some(dir.path().join("failed.json")),
    ).unwrap();
    state.lock().unwrap().dead = true;
    state.lock().unwrap().fail = "recover";
    assert!(session.keep_alive().is_err());
    assert!(session.recovery_required());
    let before = state.lock().unwrap().messages.len();
    assert!(session.handoff_snapshot().is_err());
    assert_eq!(state.lock().unwrap().messages.len(), before);
    state.lock().unwrap().fail = "";
    state.lock().unwrap().dead = false;
    session.recover().unwrap();
    assert!(session.handoff_snapshot().unwrap().is_none());
}

#[test]
fn handoff_rejects_changed_or_missing_confirmed_journal_without_mutation() {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let path = dir.path().join("output.json");
    let (factory, state) = factory();
    state.lock().unwrap().process_id = Some(73);
    let mut session = Session::start_with_journal(
        factory, token(), vec![crate::vjoy::tests::binding()], Counter(0), Some(path.clone()),
    ).unwrap();
    let original = sql_read(&path).unwrap();
    let record = crate::output_journal::inspect(&path).unwrap().unwrap();
    let mut changed = vec![record.clone(); 3];
    changed[0].token.generation = Counter(record.token.generation.0 + 1);
    changed[1].bindings[0].slot += 1;
    changed[2].phase = Phase::Clean;
    let mut candidates = vec![b"external".to_vec()];
    candidates.extend(changed.iter().map(|r| serde_json::to_vec(r).unwrap()));
    for bytes in candidates {
        sql_write(&path, &bytes).unwrap();
        assert!(session.handoff_snapshot().is_err());
        assert_eq!(sql_read(&path).unwrap(), bytes);
    }
    sql_delete(&path).unwrap();
    assert!(session.handoff_snapshot().is_err());
    assert!(!path.exists());
    assert!(matches!(state.lock().unwrap().messages.as_slice(), [Message::Start { .. }]));
    assert_eq!(state.lock().unwrap().spawned, 1);
    sql_write(&path, original).unwrap();
    assert!(session.stop().is_err()); // Equal bytes at a newer revision remain foreign.
}
#[test]
fn prepared_output_renews_without_reports_and_helper_loss_releases_it() {
    let (factory, state) = factory();
    let token = token();
    let mut session = Session::start(
        factory,
        token.clone(),
        vec![crate::vjoy::tests::binding()],
        Counter(0),
    )
    .unwrap();
    assert_eq!(session.token(), &token);
    session.renewed -= std::time::Duration::from_micros(RENEW_US);
    session.keep_alive().unwrap();
    assert!(state
        .lock()
        .unwrap()
        .messages
        .iter()
        .any(|m| matches!(m, Message::Renew { .. })));
    assert!(!state
        .lock()
        .unwrap()
        .messages
        .iter()
        .any(|m| matches!(m, Message::Report { .. })));
    state.lock().unwrap().dead = true;
    assert!(session.keep_alive().is_err());
    assert!(!session.active());
    assert!(state
        .lock()
        .unwrap()
        .messages
        .iter()
        .any(|m| matches!(m, Message::Recover { .. })));
}
#[test]
fn ordered_edges_are_bounded_and_quiet_runs_renew_without_ui() {
    let (factory, state) = factory();
    let token = token();
    let mut session = Session::start(
        factory,
        token.clone(),
        vec![crate::vjoy::tests::binding()],
        Counter(0),
    )
    .unwrap();
    let sink = recording(&token, 130);
    session.renewed -= std::time::Duration::from_micros(RENEW_US);
    session.poll(&sink).unwrap();
    assert!((1..=128).contains(&session.cursor().0));
    while session.cursor().0 < 130 {
        session.poll(&sink).unwrap();
    }
    session.poll(&sink).unwrap();
    assert!(session.recover().is_err());
    session.stop().unwrap();
    assert!(!session.active());
    session.stop().unwrap();
    session.recover().unwrap();
    assert!(session.poll(&sink).is_err());
    let messages = &state.lock().unwrap().messages;
    assert!(matches!(
        messages[1],
        Message::Renew {
            sequence: Counter(1),
            ..
        }
    ));
    let reports: Vec<_> = messages
        .iter()
        .filter_map(|m| match m {
            Message::Report { report, .. } => Some(report),
            _ => None,
        })
        .collect();
    assert_eq!(reports.len(), 130);
    for (i, report) in reports.iter().enumerate() {
        assert_eq!(report.sequence.0, i as u64 + 1);
        assert_eq!(report.state.buttons[0], i % 2 == 0);
    }
    assert!(matches!(messages.last(), Some(Message::Stop { .. })));
}
#[test]
fn failures_recover_only_acknowledged_runs_and_never_replay_reports() {
    for mode in [
        "report",
        "renew",
        "stop",
        "dead",
        "gap",
        "wrong-token",
        "counter",
        "spawn",
    ] {
        let (factory, shared) = factory();
        let run = token();
        let mut session = Session::start(
            factory,
            run.clone(),
            vec![crate::vjoy::tests::binding()],
            Counter(0),
        )
        .unwrap();
        let sink = recording(
            &if mode == "wrong-token" { token() } else { run },
            if mode == "gap" { 4100 } else { 1 },
        );
        {
            let mut state = shared.lock().unwrap();
            state.fail = if ["report", "renew", "stop"].contains(&mode) {
                mode
            } else {
                ""
            };
            state.dead = mode == "dead";
            state.fail_spawn = mode == "spawn";
        }
        session.renewed -= std::time::Duration::from_micros(RENEW_US);
        if mode == "counter" {
            session.renewal = u64::MAX;
        }
        if mode == "spawn" {
            shared.lock().unwrap().dead = true;
        }
        let error = if mode == "stop" {
            session.stop()
        } else {
            session.poll(&sink)
        }
        .unwrap_err();
        assert!(!session.active(), "{mode}: {error}");
        if mode == "spawn" {
            assert!(session.recovery_required());
            shared.lock().unwrap().fail_spawn = false;
            session.recover().unwrap();
        }
        assert!(!session.recovery_required());
        let state = shared.lock().unwrap();
        assert_eq!(state.spawned, 2);
        assert_eq!(state.terminated, [1, 2]);
        let Message::Recover { token, bindings } = state.messages.last().unwrap() else {
            panic!("Expected recovery for {mode}")
        };
        assert_ne!(token.session, session.token.session);
        assert_eq!(bindings.len(), 1);
        assert!(!error.is_empty());
    }
    let (factory, state) = factory();
    state.lock().unwrap().fail = "start";
    assert!(Session::start(
        factory.clone(),
        token(),
        vec![crate::vjoy::tests::binding()],
        Counter(0)
    )
    .is_err());
    assert_eq!(state.lock().unwrap().spawned, 1);
    assert_eq!(state.lock().unwrap().messages.len(), 1);
    state.lock().unwrap().fail_spawn = true;
    assert!(Session::start(
        factory,
        token(),
        vec![crate::vjoy::tests::binding()],
        Counter(0)
    )
    .is_err());
}
#[test]
fn unresolved_recovery_is_visible_and_drop_stops_owned_run() {
    let (factory, state) = factory();
    let mut session = Session::start(
        factory.clone(),
        token(),
        vec![crate::vjoy::tests::binding()],
        Counter(0),
    )
    .unwrap();
    state.lock().unwrap().dead = true;
    state.lock().unwrap().fail = "recover";
    assert!(session
        .poll(&RecordingSink::default())
        .unwrap_err()
        .contains("requires attention"));
    assert!(session.recovery_required());
    state.lock().unwrap().fail = "";
    session.recover().unwrap();
    assert!(!session.recovery_required());
    state.lock().unwrap().dead = false;
    let session = Session::start(
        factory,
        token(),
        vec![crate::vjoy::tests::binding()],
        Counter(0),
    )
    .unwrap();
    drop(session);
    assert!(matches!(
        state.lock().unwrap().messages.last(),
        Some(Message::Stop { .. })
    ));
}

#[test]
fn durable_activation_precedes_driver_calls_and_clean_stop_allows_a_new_run() {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let path = dir.path().join("output.json");
    let (factory, state) = factory();
    state.lock().unwrap().observe = Some(path.clone());
    let run = token();
    let mut session = Session::start_with_journal(
        factory.clone(),
        run.clone(),
        vec![crate::vjoy::tests::binding()],
        Counter(0),
        Some(path.clone()),
    )
    .unwrap();
    assert_eq!(
        crate::output_journal::inspect(&path)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Confirmed
    );
    session.poll(&recording(&run, 1)).unwrap();
    assert!(Session::start_with_journal(
        factory.clone(),
        token(),
        vec![crate::vjoy::tests::binding()],
        Counter(0),
        Some(path.clone())
    )
    .is_err());
    session.stop().unwrap();
    assert_eq!(
        crate::output_journal::inspect(&path)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Clean
    );
    drop(session);
    let mut session = Session::start_with_journal(
        factory.clone(),
        token(),
        vec![crate::vjoy::tests::binding()],
        Counter(0),
        Some(path.clone()),
    )
    .unwrap();
    state.lock().unwrap().dead = true;
    assert!(session.poll(&RecordingSink::default()).is_err());
    assert!(!session.recovery_required()); // Confirmed ownership allowed fallback.
    assert_eq!(
        crate::output_journal::inspect(&path)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Clean
    );
}
#[test]
fn lost_start_acknowledgement_and_external_storage_changes_never_guess_ownership() {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let path = dir.path().join("output.json");
    let (factory, state) = factory();
    state.lock().unwrap().fail = "start";
    assert!(Session::start_with_journal(
        factory.clone(),
        token(),
        vec![crate::vjoy::tests::binding()],
        Counter(0),
        Some(path.clone())
    )
    .is_err());
    let record = crate::output_journal::inspect(&path).unwrap().unwrap();
    assert_eq!(record.phase, Phase::Intent);
    assert!(Session::recover_stored(factory.clone(), path.clone()).is_err());
    assert_eq!(state.lock().unwrap().spawned, 1);
    Store::open(path.clone())
        .unwrap()
        .acknowledge(&record.token)
        .unwrap();
    state.lock().unwrap().fail = "";
    state.lock().unwrap().fail_spawn = true;
    assert!(Session::start_with_journal(
        factory.clone(),
        token(),
        vec![crate::vjoy::tests::binding()],
        Counter(0),
        Some(path.clone())
    )
    .is_err());
    assert_eq!(
        crate::output_journal::inspect(&path)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Clean
    );
    state.lock().unwrap().fail_spawn = false;
    let mut session = Session::start_with_journal(
        factory.clone(),
        token(),
        vec![crate::vjoy::tests::binding()],
        Counter(0),
        Some(path.clone()),
    )
    .unwrap();
    sql_write(&path, b"external").unwrap();
    assert!(session.stop().is_err());
    assert!(session.recovery_required());
    assert!(!session.active());
    assert!(session.recover().is_err());
    assert_eq!(sql_read(&path).unwrap(), b"external");
}
#[test]
fn restart_recovery_sends_only_fallback_and_refuses_ambiguous_or_missing_records() {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let path = dir.path().join("output.json");
    let (factory, state) = factory();
    assert!(Session::recover_stored(factory.clone(), path.clone()).is_err());
    let mut store = Store::open(path.clone()).unwrap();
    store
        .begin(token(), vec![crate::vjoy::tests::binding()])
        .unwrap();
    store.transition(Phase::Confirmed).unwrap();
    drop(store);
    state.lock().unwrap().observe = Some(path.clone());
    Session::recover_stored(factory.clone(), path.clone()).unwrap();
    assert!(matches!(
        state.lock().unwrap().messages.as_slice(),
        [Message::Recover { .. }]
    ));
    assert_eq!(
        crate::output_journal::inspect(&path)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Clean
    );
    assert!(Session::recover_stored(factory.clone(), path).is_err());
}

#[test]
fn exact_restart_recovery_rejects_changed_owner_binding_and_phase_before_opening_driver() {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let path = dir.path().join("output.json");
    let (factory, state) = factory();
    let mut store = Store::open(path.clone()).unwrap();
    store
        .begin(token(), vec![crate::vjoy::tests::binding()])
        .unwrap();
    store.transition(Phase::Confirmed).unwrap();
    let expected = store.record().unwrap().clone();
    drop(store);
    let original = sql_read(&path).unwrap();
    let mut changed = vec![expected.clone(); 4];
    changed[0].token.generation = Counter(2);
    changed[1].bindings[0].slot = 2;
    changed[2].phase = Phase::Releasing;
    changed[3].token.session = uuid::Uuid::new_v4().to_string();
    for stale in changed {
        assert!(
            Session::recover_stored_exact(factory.clone(), path.clone(), &stale)
                .unwrap_err()
                .contains("complete retained output journal changed")
        );
        assert_eq!(sql_read(&path).unwrap(), original);
    }
    assert_eq!(state.lock().unwrap().spawned, 0);
    assert!(state.lock().unwrap().messages.is_empty());
    Session::recover_stored_exact(factory, path.clone(), &expected).unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.spawned, 1);
    assert!(
        matches!(state.messages.as_slice(), [Message::Recover { bindings, .. }]
        if bindings[0].slot == expected.bindings[0].slot)
    );
    assert_eq!(state.terminated, vec![1]);
    assert_eq!(
        crate::output_journal::inspect(&path)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Clean
    );
}
#[test]
fn exact_restart_recovery_never_opens_driver_while_another_session_owns_the_journal() {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let path = dir.path().join("output.json");
    let (factory, state) = factory();
    let mut store = Store::open(path.clone()).unwrap();
    store
        .begin(token(), vec![crate::vjoy::tests::binding()])
        .unwrap();
    store.transition(Phase::Confirmed).unwrap();
    let expected = store.record().unwrap().clone();
    assert!(
        Session::recover_stored_exact(factory, path.clone(), &expected)
            .unwrap_err()
            .contains("Another Cockpit session owns")
    );
    assert_eq!(state.lock().unwrap().spawned, 0);
    assert_eq!(
        crate::output_journal::inspect(&path)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Confirmed
    );
}

fn sql_read(path:&std::path::Path)->Result<Vec<u8>,String>{osg_persistence::test_support::read_evidence(path,"recovery-output")}
fn sql_write(path:&std::path::Path,data:impl AsRef<[u8]>)->Result<(),String>{osg_persistence::test_support::write_evidence(path,"recovery-output",Some(data.as_ref()))}
fn sql_delete(path:&std::path::Path)->Result<(),String>{osg_persistence::test_support::write_evidence(path,"recovery-output",None)}
