use super::*;
use osg_desktop_output::{
    coordinator::Channel,
    protocol::{native_count, Ack, Adapter, NativeEvent, Request},
    worker::Worker,
};
use osg_mapping::{
    desktop::{Action, Key},
    engine::{
        typed::{Change, Frame},
        Counter, Reason,
    },
    program::*,
    signal::*,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

struct Recording {
    focused: Arc<AtomicBool>,
    events: Arc<Mutex<Vec<NativeEvent>>>,
}
impl Adapter for Recording {
    fn eligible(&mut self, _: &Scope) -> Result<bool, String> {
        Ok(self.focused.load(Ordering::Acquire))
    }
    fn submit(&mut self, events: &[NativeEvent]) -> Result<usize, String> {
        self.events.lock().unwrap().extend_from_slice(events);
        Ok(native_count(events))
    }
}
struct RecordingChannel(Worker<Recording>);
impl Channel for RecordingChannel {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        Ok(self.0.receive(request.clone(), 10))
    }
}
fn clock() -> u64 {
    10
}
fn boolean(value: bool) -> SignalSample {
    SignalSample::scalar(PortValue::Boolean(value))
}
fn contract() -> PortContract {
    Action::KeyHold { key: Key(4) }.contract()
}
fn endpoint(node: &str, port: &str) -> Endpoint {
    Endpoint {
        node_id: node.into(),
        port_id: port.into(),
    }
}
fn source_program(action: Action, edge: bool) -> Program {
    let mut program = Program {
        sources: vec![SourcePort {
            endpoint: endpoint("source", "button"),
            contract: contract(),
        }],
        nodes: vec![],
        sinks: vec![],
        controllers: vec![],
    };
    let from = if edge {
        program.nodes.push(Node {
            id: "fallback".into(),
            inputs: vec![InputBinding {
                port_id: "input".into(),
                from: endpoint("source", "button"),
                contract: contract(),
                edge_id: "fallback-input".into(),
            }],
            outputs: vec![PortDescriptor {
                id: "value".into(),
                name: "Value".into(),
                direction: Direction::Output,
                contract: contract(),
            }],
            operation: Operation::Fallback {
                unknown: PortValue::Boolean(false),
                stale: PortValue::Boolean(false),
                unavailable: PortValue::Boolean(false),
            },
        });
        program.nodes.push(Node {
            id: "edge".into(),
            inputs: vec![InputBinding {
                port_id: "input".into(),
                from: endpoint("fallback", "value"),
                contract: contract(),
                edge_id: "edge-input".into(),
            }],
            outputs: vec![PortDescriptor {
                id: "event".into(),
                name: "Event".into(),
                direction: Direction::Output,
                contract: PortContract::Event {
                    schema: ScalarSchema::Boolean {},
                },
            }],
            operation: Operation::EventEdge { edge: Edge::Rising },
        });
        endpoint("edge", "event")
    } else {
        endpoint("source", "button")
    };
    program.sinks.push(Sink {
        id: "action".into(),
        endpoint: endpoint("desktop", "action"),
        from,
        contract: action.contract(),
        kind: SinkKind::Desktop { action },
    });
    program
}
fn frame(phase: &mut Phase, sequence: u64, at: u64, reason: Reason, value: bool) -> Vec<Effect> {
    phase
        .accept(Frame {
            source: "source".into(),
            generation: Counter(7),
            sequence: Counter(sequence),
            captured_us: Counter(at),
            reason,
            changes: if matches!(reason, Reason::Disconnect | Reason::Gap) {
                vec![]
            } else {
                vec![Change {
                    port_id: "button".into(),
                    sample: boolean(value),
                }]
            },
        })
        .unwrap();
    phase.drain()
}
fn prepared_fixture(
    action: Action,
    edge: bool,
) -> (
    Desktop,
    Phase,
    Arc<AtomicBool>,
    Arc<Mutex<Vec<NativeEvent>>>,
) {
    let program = source_program(action, edge);
    let session = "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12";
    let mut phase = Phase::new(program.clone(), Counter(1), session.into()).unwrap();
    phase.drain();
    frame(&mut phase, 1, 1, Reason::Baseline, false);
    let focused = Arc::new(AtomicBool::new(true));
    let events = Arc::new(Mutex::new(vec![]));
    let output = Coordinator::with_channel(
        Box::new(RecordingChannel(Worker::new(Recording {
            focused: focused.clone(),
            events: events.clone(),
        }))),
        clock,
    );
    let desktop = Desktop {
        output,
        scheduler: Scheduler::from_program(phase.run_token(), &program).unwrap(),
        run: phase.run_token(),
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
    };
    (desktop, phase, focused, events)
}
fn fixture(
    action: Action,
    edge: bool,
) -> (
    Desktop,
    Phase,
    Arc<AtomicBool>,
    Arc<Mutex<Vec<NativeEvent>>>,
) {
    let (mut desktop, phase, focused, events) = prepared_fixture(action, edge);
    desktop.start_focused(&phase, true).unwrap();
    (desktop, phase, focused, events)
}
#[test]
fn deferred_game_scope_never_registers_or_injects_before_start_and_failed_launch_cleans_neutrally()
{
    let (mut desktop, mut phase, _, events) =
        prepared_fixture(Action::KeyHold { key: Key(4) }, false);
    desktop.scope = None;
    let game = Scope::Application {
        identity: osg_desktop_output::ProcessIdentity {
            executable: "C:\\Fixture\\Game.exe".into(),
            pid: 42,
            created: "10".into(),
        },
    };
    desktop.bind_scope_before_start(game.clone()).unwrap();
    // Actual input may arrive during preparation, but the launch hold has no
    // desktop poll/arming. Renewing the prepared adapter cannot emit it.
    frame(&mut phase, 2, 2, Reason::Change, true);
    for _ in 0..4 {
        desktop.keep_alive().unwrap();
    }
    assert!(desktop.output.snapshot().runs.is_empty());
    assert!(desktop.output.snapshot().held.is_empty());
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(desktop.scope(), Some(&game));
    desktop.stop().unwrap();
    assert!(events.lock().unwrap().is_empty());
}
#[test]
fn started_scope_is_bound_to_the_captured_process_creation_identity_and_cannot_inherit_pid_reuse() {
    let (mut desktop, phase, _, events) = prepared_fixture(Action::KeyHold { key: Key(4) }, false);
    let first = Scope::Application {
        identity: osg_desktop_output::ProcessIdentity {
            executable: "C:\\Fixture\\Game.exe".into(),
            pid: 42,
            created: "10".into(),
        },
    };
    let replacement = Scope::Application {
        identity: osg_desktop_output::ProcessIdentity {
            executable: "C:\\Fixture\\Game.exe".into(),
            pid: 42,
            created: "11".into(),
        },
    };
    desktop.bind_scope_before_start(first.clone()).unwrap();
    desktop.start_focused(&phase, true).unwrap();
    assert!(desktop.bind_scope_before_start(replacement).is_err());
    assert_eq!(desktop.scope(), Some(&first));
    assert_eq!(desktop.output.snapshot().runs[0].scope, first);
    assert!(events.lock().unwrap().is_empty());
}
#[test]
fn shared_key_collision_is_visible_in_native_acknowledgements_and_inspector() {
    let (mut desktop, _, _, events) = fixture(Action::KeyHold { key: Key(0xe0) }, false);
    let mut program = source_program(Action::KeyHold { key: Key(0xe0) }, false);
    let mut second = program.sinks[0].clone();
    second.id = "second-action".into();
    second.endpoint.port_id = "second".into();
    program.sinks.push(second);
    let mut phase = Phase::new(
        program.clone(),
        desktop.run.generation,
        desktop.run.session.clone(),
    )
    .unwrap();
    phase.drain();
    frame(&mut phase, 1, 1, Reason::Baseline, false);
    desktop.scheduler = Scheduler::from_program(desktop.run.clone(), &program).unwrap();
    desktop.baseline(&phase).unwrap();
    desktop.flush(1).unwrap();
    let effects = frame(&mut phase, 2, 2, Reason::Change, true);
    let warnings = desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 1);
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("shares a held")));
    assert!(desktop.status()["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|outcome| outcome["acknowledgement"]["collision"] == true));
    let effects = frame(&mut phase, 3, 3, Reason::Change, false);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 2);
}
#[test]
fn expired_click_ack_cancels_queued_taps_and_is_recorded_without_injection() {
    let (mut desktop, mut phase, _, events) = fixture(
        Action::KeyTap {
            key: Key(4),
            down_ms: 30,
            up_ms: 30,
        },
        true,
    );
    for (sequence, value) in [(2, true), (3, false), (4, true)] {
        let effects = frame(&mut phase, sequence, sequence, Reason::Change, value);
        for effect in effects {
            desktop.scheduler.accept_effect(&effect).unwrap();
        }
    }
    desktop.flush(1_000_000).unwrap();
    assert!(events.lock().unwrap().is_empty());
    assert!(desktop.status()["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|outcome| outcome["acknowledgement"]["skipped"] == true));
    desktop.scheduler.advance(Counter(2_000_000)).unwrap();
    desktop.flush(2_000_000).unwrap();
    assert!(events.lock().unwrap().is_empty());
}
#[test]
fn failed_cleanup_remains_faulted_in_inspection_after_the_run_stops() {
    struct Failed;
    impl Channel for Failed {
        fn request(&mut self, _: &Request) -> Result<Ack, String> {
            Err("Worker cleanup acknowledgement was lost.".into())
        }
    }
    let (mut desktop, _, _, _) = prepared_fixture(Action::KeyHold { key: Key(4) }, false);
    desktop.output = Coordinator::with_channel(Box::new(Failed), clock);
    assert!(desktop.stop().is_err());
    assert!(!desktop.started);
    let status = desktop.status();
    assert_eq!(status["state"], "faulted");
    assert_eq!(status["backend"]["uncertain"], true);
    assert_eq!(status["backend"]["cleanupComplete"], false);
    assert!(status["error"].as_str().unwrap().contains("cleanup"));
}
#[test]
fn scope_inspection_error_pauses_with_the_actual_reason_and_releases_ownership() {
    let (mut desktop, mut phase, _, events) = fixture(Action::KeyHold { key: Key(4) }, false);
    let effects = frame(&mut phase, 2, 2, Reason::Change, true);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    phase.advance(Counter(3)).unwrap();
    let warnings = desktop
        .poll_checked(
            &[],
            &phase,
            Err(
                "The foreground target has higher integrity; choose a normal user application."
                    .into(),
            ),
        )
        .unwrap();
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("higher integrity")));
    assert!(desktop.status()["scopeError"]
        .as_str()
        .unwrap()
        .contains("higher integrity"));
    assert_eq!(events.lock().unwrap().len(), 2);
    assert!(desktop.output.snapshot().held.is_empty());
}
#[test]
fn delayed_prefocus_edges_and_liveness_cannot_rearm_a_returned_scope() {
    let (mut desktop, mut phase, focused, events) = fixture(Action::KeyHold { key: Key(4) }, false);
    focused.store(false, Ordering::Release);
    phase.advance(Counter(100)).unwrap();
    desktop.poll_focused(&[], &phase, false).unwrap();
    focused.store(true, Ordering::Release);
    phase.advance(Counter(200)).unwrap();
    desktop.poll_focused(&[], &phase, true).unwrap();
    // The reducer rebases processing to 200, but the immutable original native
    // observation proves this press was captured at 150 before scope return.
    let effects = frame(&mut phase, 2, 150, Reason::Change, true);
    assert_eq!(effects[0].at_us, Counter(200));
    assert_eq!(effects[0].observations[0].at_us, Counter(150));
    phase
        .observe_liveness("source", Counter(7), Counter(250))
        .unwrap();
    phase.advance(Counter(250)).unwrap();
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert!(events.lock().unwrap().is_empty());
    let effects = frame(&mut phase, 3, 160, Reason::Change, false);
    phase
        .observe_liveness("source", Counter(7), Counter(300))
        .unwrap();
    phase.advance(Counter(300)).unwrap();
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert!(desktop.waiting_neutral.contains("action"));
    let effects = frame(&mut phase, 4, 170, Reason::Change, true);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert!(events.lock().unwrap().is_empty());
    let effects = frame(&mut phase, 5, 301, Reason::Change, false);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert!(!desktop.waiting_neutral.contains("action"));
    let effects = frame(&mut phase, 6, 302, Reason::Change, true);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 1);
}
#[test]
fn event_actions_wait_for_a_real_neutral_observation_and_discard_earlier_same_tick_edges() {
    let (mut desktop, mut phase, focused, events) = fixture(
        Action::KeyTap {
            key: Key(4),
            down_ms: 30,
            up_ms: 30,
        },
        true,
    );
    focused.store(false, Ordering::Release);
    phase.advance(Counter(100)).unwrap();
    desktop.poll_focused(&[], &phase, false).unwrap();
    focused.store(true, Ordering::Release);
    phase.advance(Counter(200)).unwrap();
    desktop.poll_focused(&[], &phase, true).unwrap();
    let mut effects = frame(&mut phase, 2, 201, Reason::Change, true);
    effects.extend(frame(&mut phase, 3, 201, Reason::Change, false));
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert!(events.lock().unwrap().is_empty());
    assert!(!desktop.waiting_neutral.contains("action"));
    // Re-presenting an earlier captured occurrence at the same microsecond
    // cannot cross the neutral source-sequence fence from the later release.
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert!(events.lock().unwrap().is_empty());
    let effects = frame(&mut phase, 4, 202, Reason::Change, true);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 1);
}
#[test]
fn cached_neutral_at_the_exact_scope_return_microsecond_is_not_a_new_baseline(){
    let (mut desktop,mut phase,focused,events)=fixture(Action::KeyHold{key:Key(4)},false);
    focused.store(false,Ordering::Release);phase.advance(Counter(100)).unwrap();desktop.poll_focused(&[],&phase,false).unwrap();
    let effects=frame(&mut phase,2,200,Reason::Change,false);
    focused.store(true,Ordering::Release);desktop.poll_focused(&effects,&phase,true).unwrap();
    phase.observe_liveness("source",Counter(7),Counter(201)).unwrap();phase.advance(Counter(201)).unwrap();desktop.poll_focused(&[],&phase,true).unwrap();
    assert!(desktop.waiting_neutral.contains("action"));assert!(events.lock().unwrap().is_empty());
    let effects=frame(&mut phase,3,202,Reason::Change,false);desktop.poll_focused(&effects,&phase,true).unwrap();assert!(!desktop.waiting_neutral.contains("action"));
    let effects=frame(&mut phase,4,203,Reason::Change,true);desktop.poll_focused(&effects,&phase,true).unwrap();assert_eq!(events.lock().unwrap().len(),1);
}
#[test]
fn direct_hid_key_hold_starts_without_a_processing_node_and_stops_cleanly() {
    let (mut desktop, mut phase, _, events) = fixture(Action::KeyHold { key: Key(4) }, false);
    let effects = frame(&mut phase, 2, 2, Reason::Change, true);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        vec![NativeEvent::Key {
            usage: 4,
            down: true,
            repeat: false
        }]
    );
    desktop.stop().unwrap();
    assert_eq!(
        events.lock().unwrap().last(),
        Some(&NativeEvent::Key {
            usage: 4,
            down: false,
            repeat: false
        })
    );
    assert!(desktop.output.snapshot().held.is_empty());
}
#[test]
fn focus_return_does_not_resume_a_held_trigger_and_requires_neutral_then_press() {
    let (mut desktop, mut phase, focused, events) = fixture(Action::KeyHold { key: Key(4) }, false);
    let effects = frame(&mut phase, 2, 2, Reason::Change, true);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    focused.store(false, Ordering::Release);
    phase.advance(Counter(3)).unwrap();
    desktop.poll_focused(&[], &phase, false).unwrap();
    assert_eq!(events.lock().unwrap().len(), 2);
    focused.store(true, Ordering::Release);
    phase.advance(Counter(4)).unwrap();
    desktop.poll_focused(&[], &phase, true).unwrap();
    phase.advance(Counter(5)).unwrap();
    desktop.poll_focused(&[], &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 2);
    let effects = frame(&mut phase, 3, 6, Reason::Change, false);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    let effects = frame(&mut phase, 4, 7, Reason::Change, true);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 3);
    assert_eq!(
        events.lock().unwrap().last(),
        Some(&NativeEvent::Key {
            usage: 4,
            down: true,
            repeat: false
        })
    );
}
#[test]
fn source_loss_revokes_queued_taps_even_when_fallback_suppresses_the_sink_effect() {
    let (mut desktop, mut phase, _, events) = fixture(
        Action::KeyTap {
            key: Key(4),
            down_ms: 30,
            up_ms: 30,
        },
        true,
    );
    for (sequence, value) in [(2, true), (3, false), (4, true)] {
        let effects = frame(&mut phase, sequence, sequence, Reason::Change, value);
        desktop.poll_focused(&effects, &phase, true).unwrap();
    }
    assert_eq!(events.lock().unwrap().len(), 1);
    let effects = frame(&mut phase, 5, 5, Reason::Disconnect, false);
    assert!(
        effects.is_empty(),
        "A known fallback event without an occurrence is suppressed"
    );
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 2);
    phase.advance(Counter(100_005)).unwrap();
    let effects = phase.drain();
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 2);
    assert!(desktop.output.snapshot().held.is_empty());
}
#[test]
fn ten_native_encoder_edges_become_ten_serialized_taps_through_worker_recording() {
    let (mut desktop, mut phase, _, events) = fixture(
        Action::KeyTap {
            key: Key(4),
            down_ms: 30,
            up_ms: 30,
        },
        true,
    );
    let mut effects = vec![];
    for sequence in 2..=21 {
        effects.extend(frame(
            &mut phase,
            sequence,
            sequence,
            Reason::Change,
            sequence % 2 == 0,
        ));
    }
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 1);
    for step in 1..=20 {
        phase.advance(Counter(21 + step * 30_000)).unwrap();
        let effects = phase.drain();
        desktop.poll_focused(&effects, &phase, true).unwrap();
    }
    assert_eq!(events.lock().unwrap().len(), 20);
    for (index, event) in events.lock().unwrap().iter().enumerate() {
        assert_eq!(
            *event,
            NativeEvent::Key {
                usage: 4,
                down: index % 2 == 0,
                repeat: false
            }
        );
    }
}
