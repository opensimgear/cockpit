use super::*;
use osg_desktop_output::{
    coordinator::Channel,
    protocol::{Ack, Adapter, NativeEvent, Request},
    worker::Worker,
};

struct NoInput;
impl Adapter for NoInput {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> {
        Ok(true)
    }
    fn submit(&mut self, _: &[NativeEvent]) -> Result<usize, String> {
        panic!("No controls may be submitted by an empty profile")
    }
}
struct TestChannel(Worker<NoInput>);
impl Channel for TestChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        Ok(self.0.receive(request.clone(), 10))
    }
}
fn fixture() -> (Desktop, Phase) {
    let program = osg_mapping::program::Program {
        sources: vec![],
        nodes: vec![],
        sinks: vec![],
        controllers: vec![],
    };
    let mut phase = Phase::new(
        program.clone(),
        Counter(1),
        "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12".into(),
    )
    .unwrap();
    phase.drain();
    let run = phase.run_token();
    let mut desktop = Desktop {
        output: Coordinator::with_channel(Box::new(TestChannel(Worker::new(NoInput))), || 10),
        scheduler: Scheduler::from_program(run.clone(), &program).unwrap(),
        run,
        scope: Some(Scope::DesktopWide),
        armed: BTreeMap::new(),
        focused: false,
        started: false,
        registered: false,
        last_error: None,
        outcomes: VecDeque::new(),
        dropped_outcomes: 0,
        diagnostics: VecDeque::new(),
        scope_error: None,
        focus_boundary: None,
        waiting_neutral: BTreeSet::new(),
        rearm_evidence: BTreeMap::new(),
        shadow: None,
        audit_shadow: 0,
    };
    desktop.start_focused(&phase, true).unwrap();
    (desktop, phase)
}

#[test]
fn losing_and_returning_application_focus_is_healthy_and_retains_paused_status() {
    let (mut desktop, phase) = fixture();
    for _ in 0..2 {
        assert!(desktop
            .poll_checked(&[], &phase, Ok(false))
            .unwrap()
            .is_empty());
        assert_eq!(desktop.status()["state"], "paused");
    }
    assert!(desktop
        .poll_checked(&[], &phase, Ok(true))
        .unwrap()
        .is_empty());
    assert_eq!(desktop.status()["state"], "active");
    assert!(
        desktop.focus_boundary.is_some(),
        "Returning focus must retain the fresh neutral baseline fence."
    );
}

#[test]
fn application_scope_observation_failure_still_degrades_profile_health() {
    let (mut desktop, phase) = fixture();
    let diagnostics = desktop
        .poll_checked(
            &[],
            &phase,
            Err("Process identity could not be verified".into()),
        )
        .unwrap();
    assert_eq!(
        diagnostics,
        ["Keyboard and mouse output paused: Process identity could not be verified"]
    );
    assert_eq!(desktop.status()["state"], "paused");
    assert_eq!(
        desktop.status()["scopeError"],
        "Process identity could not be verified"
    );
    assert!(desktop
        .poll_checked(&[], &phase, Ok(true))
        .unwrap()
        .is_empty());
    assert_eq!(desktop.status()["scopeError"], serde_json::Value::Null);
}
