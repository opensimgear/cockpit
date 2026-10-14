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
}
struct Child {
    state: Arc<Mutex<State>>,
    id: usize,
}
impl Channel for Child {
    fn request(&mut self, message: &Message) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
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
