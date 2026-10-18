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
fn frame(phase: &mut Phase, sequence: u64, at: u64, reason: Reason, value: bool) -> Vec<Frame> {
    let frame = Frame {
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
    };
    phase.accept(frame.clone()).unwrap();
    vec![frame]
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
    let (desktop, mut phase, focused, events) = prepared_program_fixture(program);
    frame(&mut phase, 1, 1, Reason::Baseline, false);
    phase.drain();
    (desktop, phase, focused, events)
}
fn prepared_program_fixture(
    program: Program,
) -> (
    Desktop,
    Phase,
    Arc<AtomicBool>,
    Arc<Mutex<Vec<NativeEvent>>>,
) {
    let session = "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12";
    let mut phase = Phase::new(program.clone(), Counter(1), session.into()).unwrap();
    phase.drain();
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
        shadow: None,
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
    desktop.shadow = Some(phase.fork_desktop().unwrap());
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
        frame(&mut phase, sequence, sequence, Reason::Change, value);
        for effect in phase.drain() {
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
    let produced = phase.drain();
    assert_eq!(produced[0].at_us, Counter(200));
    assert_eq!(produced[0].observations[0].at_us, Counter(150));
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
    let earlier = phase.drain();
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert!(events.lock().unwrap().is_empty());
    assert!(!desktop.waiting_neutral.contains("action"));
    // Re-presenting an earlier captured occurrence at the same microsecond
    // cannot cross the neutral source-sequence fence from the later release.
    desktop.deliver(&earlier, &phase).unwrap();
    assert!(events.lock().unwrap().is_empty());
    let effects = frame(&mut phase, 4, 202, Reason::Change, true);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 1);
}
#[test]
fn unchanged_fresh_neutral_then_press_in_one_native_batch_rearms_in_order() {
    let (mut desktop, mut phase, focused, events) = fixture(Action::KeyHold { key: Key(4) }, false);
    focused.store(false, Ordering::Release);
    phase.advance(Counter(100)).unwrap();
    desktop.poll_focused(&[], &phase, false).unwrap();
    focused.store(true, Ordering::Release);
    phase.advance(Counter(200)).unwrap();
    desktop.poll_focused(&[], &phase, true).unwrap();
    let mut frames = frame(&mut phase, 2, 201, Reason::Change, false);
    frames.extend(frame(&mut phase, 3, 202, Reason::Change, true));
    desktop.poll_focused(&frames, &phase, true).unwrap();
    assert!(!desktop.waiting_neutral.contains("action"));
    let count = events.lock().unwrap().len();
    assert_eq!(count, 1, "The actual neutral frame must precede the later press, even when its output value was unchanged");
}
#[test]
fn cached_neutral_at_the_exact_scope_return_microsecond_is_not_a_new_baseline() {
    let (mut desktop, mut phase, focused, events) = fixture(Action::KeyHold { key: Key(4) }, false);
    focused.store(false, Ordering::Release);
    phase.advance(Counter(100)).unwrap();
    desktop.poll_focused(&[], &phase, false).unwrap();
    let effects = frame(&mut phase, 2, 200, Reason::Change, false);
    focused.store(true, Ordering::Release);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    phase
        .observe_liveness("source", Counter(7), Counter(201))
        .unwrap();
    phase.advance(Counter(201)).unwrap();
    desktop.poll_focused(&[], &phase, true).unwrap();
    assert!(desktop.waiting_neutral.contains("action"));
    assert!(events.lock().unwrap().is_empty());
    let effects = frame(&mut phase, 3, 202, Reason::Change, false);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert!(!desktop.waiting_neutral.contains("action"));
    let effects = frame(&mut phase, 4, 203, Reason::Change, true);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 1);
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
fn scope_loss_cancels_preexisting_builtin_macro_even_after_fresh_root_neutral() {
    let mut program = source_program(Action::KeyHold { key: Key(4) }, false);
    program.nodes.push(Node {
        id: "macro".into(),
        inputs: vec![InputBinding {
            port_id: "input".into(),
            from: endpoint("source", "button"),
            contract: contract(),
            edge_id: "macro-input".into(),
        }],
        outputs: vec![PortDescriptor {
            id: "value".into(),
            name: "Value".into(),
            direction: Direction::Output,
            contract: contract(),
        }],
        operation: Operation::Builtin {
            inputs: vec!["input".into()],
            rule: osg_mapping::rules::Rule::Macro {
                steps: vec![
                    osg_mapping::rules::Step {
                        after_us: 0,
                        pressed: true,
                    },
                    osg_mapping::rules::Step {
                        after_us: 1_000,
                        pressed: false,
                    },
                    osg_mapping::rules::Step {
                        after_us: 10_000,
                        pressed: true,
                    },
                    osg_mapping::rules::Step {
                        after_us: 20_000,
                        pressed: false,
                    },
                ],
            },
        },
    });
    program.sinks[0].from = endpoint("macro", "value");
    program.controllers.push(VirtualController {
        id: "virtual".into(),
        layout: ControllerLayout {
            buttons: vec!["button".into()],
            axes: vec![],
            hats: vec![],
        },
    });
    program.sinks.push(Sink {
        id: "controller-action".into(),
        endpoint: endpoint("virtual", "button"),
        from: endpoint("macro", "value"),
        contract: contract(),
        kind: SinkKind::Virtual {
            controller_id: "virtual".into(),
            control: osg_mapping::profile::OutputControl::Button { index: 0 },
        },
    });
    let (mut desktop, mut phase, focused, events) = prepared_program_fixture(program);
    frame(&mut phase, 1, 1, Reason::Baseline, false);
    desktop.start_focused(&phase, true).unwrap();

    let effects = frame(&mut phase, 2, 10, Reason::Change, true);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 1);
    assert_eq!(phase.drain_reports().last().unwrap().state.buttons, [true]);
    focused.store(false, Ordering::Release);
    phase.advance(Counter(100)).unwrap();
    desktop.poll_focused(&[], &phase, false).unwrap();
    assert_eq!(events.lock().unwrap().len(), 2);

    // The macro's first release occurs during focus loss. Its later press
    // remains scheduled in Phase even though the desktop scheduler paused.
    phase.advance(Counter(2_000)).unwrap();
    assert_eq!(phase.drain_reports().last().unwrap().state.buttons, [false]);
    phase.drain();
    desktop.poll_focused(&[], &phase, false).unwrap();
    focused.store(true, Ordering::Release);
    phase.advance(Counter(3_000)).unwrap();
    desktop.poll_focused(&[], &phase, true).unwrap();
    let effects = frame(&mut phase, 3, 3_001, Reason::Change, false);
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert!(!desktop.waiting_neutral.contains("action"));
    assert_eq!(events.lock().unwrap().len(), 2);
    phase.drain();

    // A newer unchanged neutral report cannot legitimize an activation from
    // before scope loss. Requiring sequence > baseline alone is insufficient.
    let effects = frame(&mut phase, 4, 4_000, Reason::Change, false);
    assert!(phase.drain().is_empty());
    desktop.poll_focused(&effects, &phase, true).unwrap();

    phase.advance(Counter(10_010)).unwrap();
    let effects = phase.drain();
    assert_eq!(effects.len(), 2);
    let desktop_effect = effects
        .iter()
        .find(|effect| effect.sink_id == "action")
        .unwrap();
    assert_eq!(desktop_effect.observations[0].sequence, Counter(4));
    assert_eq!(
        phase.drain_reports().last().unwrap().state.buttons,
        [true],
        "The shared controller branch must retain its own original macro timer"
    );
    desktop.poll_focused(&[], &phase, true).unwrap();
    let event_count = events.lock().unwrap().len();
    assert_eq!(
        event_count, 2,
        "Scope loss must cancel the old macro; its timer must never resume native output"
    );
    phase.advance(Counter(20_010)).unwrap();
    desktop.poll_focused(&[], &phase, true).unwrap();
    let frames = frame(&mut phase, 5, 20_011, Reason::Change, true);
    desktop.poll_focused(&frames, &phase, true).unwrap();
    assert_eq!(
        events.lock().unwrap().len(),
        3,
        "A new physical press starts a new desktop macro"
    );
}
#[test]
fn returned_scope_requires_fresh_effective_rule_neutral_before_macro_or_pulse_activation() {
    let rules = [
        osg_mapping::rules::Rule::Macro {
            steps: vec![
                osg_mapping::rules::Step {
                    after_us: 0,
                    pressed: true,
                },
                osg_mapping::rules::Step {
                    after_us: 1_000,
                    pressed: false,
                },
                osg_mapping::rules::Step {
                    after_us: 10_000,
                    pressed: true,
                },
                osg_mapping::rules::Step {
                    after_us: 20_000,
                    pressed: false,
                },
            ],
        },
        osg_mapping::rules::Rule::Pulse {
            duration_us: 1_000,
            repeat_us: 10_000,
        },
    ];
    for rule in rules {
        for trace in 0..2 {
            let mut program = source_program(Action::KeyHold { key: Key(4) }, false);
            program.nodes.push(Node {
                id: "timed".into(),
                inputs: vec![InputBinding {
                    port_id: "input".into(),
                    from: endpoint("source", "button"),
                    contract: contract(),
                    edge_id: "timed-input".into(),
                }],
                outputs: vec![PortDescriptor {
                    id: "value".into(),
                    name: "Value".into(),
                    direction: Direction::Output,
                    contract: contract(),
                }],
                operation: Operation::Builtin {
                    inputs: vec!["input".into()],
                    rule: rule.clone(),
                },
            });
            program.sinks[0].from = endpoint("timed", "value");
            let (mut desktop, mut phase, focused, events) = prepared_program_fixture(program);
            frame(&mut phase, 1, 1, Reason::Baseline, false);
            desktop.start_focused(&phase, true).unwrap();
            focused.store(false, Ordering::Release);
            phase.advance(Counter(100)).unwrap();
            desktop.poll_focused(&[], &phase, false).unwrap();
            focused.store(true, Ordering::Release);
            phase.advance(Counter(200)).unwrap();
            desktop.poll_focused(&[], &phase, true).unwrap();
            let sequence = match trace {
                0 => {
                    // Cached neutral cannot arm the first post-return press.
                    let frames = frame(&mut phase, 2, 201, Reason::Change, true);
                    desktop.poll_focused(&frames, &phase, true).unwrap();
                    2
                }
                _ => {
                    // A delayed pre-return neutral cannot arm it either.
                    let frames = frame(&mut phase, 2, 150, Reason::Change, false);
                    desktop.poll_focused(&frames, &phase, true).unwrap();
                    let frames = frame(&mut phase, 3, 201, Reason::Change, true);
                    desktop.poll_focused(&frames, &phase, true).unwrap();
                    3
                }
            };
            for tick in [1_201, 10_201] {
                phase.advance(Counter(tick)).unwrap();
                desktop.poll_focused(&[], &phase, true).unwrap();
            }
            let count = events.lock().unwrap().len();
            assert_eq!(
                count, 0,
                "No activation without fresh effective neutral: rule {rule:?}, trace {trace}"
            );
            let frames = frame(&mut phase, sequence + 1, 12_000, Reason::Change, false);
            desktop.poll_focused(&frames, &phase, true).unwrap();
            let frames = frame(&mut phase, sequence + 2, 12_001, Reason::Change, true);
            desktop.poll_focused(&frames, &phase, true).unwrap();
            assert_eq!(
                events.lock().unwrap().len(),
                1,
                "A fresh neutral/press must activate {rule:?}"
            );
        }
    }
}
#[test]
fn scope_reset_discards_a_latched_toggle_hidden_behind_a_closed_gate() {
    let mut program = source_program(Action::KeyHold { key: Key(4) }, false);
    program.sources.push(SourcePort {
        endpoint: endpoint("gate", "open"),
        contract: contract(),
    });
    let binding = |id: &str, node: &str, port: &str| InputBinding {
        port_id: id.into(),
        from: endpoint(node, port),
        contract: contract(),
        edge_id: format!("{id}-{node}-{port}"),
    };
    let output = |id: &str| PortDescriptor {
        id: id.into(),
        name: id.into(),
        direction: Direction::Output,
        contract: contract(),
    };
    program.nodes = vec![
        Node {
            id: "toggle".into(),
            inputs: vec![binding("input", "source", "button")],
            outputs: vec![output("value")],
            operation: Operation::Builtin {
                inputs: vec!["input".into()],
                rule: osg_mapping::rules::Rule::Toggle,
            },
        },
        Node {
            id: "gated".into(),
            inputs: vec![
                binding("gate", "gate", "open"),
                binding("value", "toggle", "value"),
            ],
            outputs: vec![output("output")],
            operation: Operation::Gate {},
        },
        Node {
            id: "fallback".into(),
            inputs: vec![binding("input", "gated", "output")],
            outputs: vec![output("value")],
            operation: Operation::Fallback {
                unknown: PortValue::Boolean(false),
                stale: PortValue::Boolean(false),
                unavailable: PortValue::Boolean(false),
            },
        },
    ];
    program.sinks[0].from = endpoint("fallback", "value");
    let (mut desktop, mut phase, focused, events) = prepared_program_fixture(program);
    let gate_frame = |sequence, at, reason, value| Frame {
        source: "gate".into(),
        generation: Counter(8),
        sequence: Counter(sequence),
        captured_us: Counter(at),
        reason,
        changes: vec![Change {
            port_id: "open".into(),
            sample: boolean(value),
        }],
    };
    frame(&mut phase, 1, 1, Reason::Baseline, false);
    phase
        .accept(gate_frame(1, 1, Reason::Baseline, true))
        .unwrap();
    desktop.start_focused(&phase, true).unwrap();
    let frames = frame(&mut phase, 2, 2, Reason::Change, true);
    desktop.poll_focused(&frames, &phase, true).unwrap();
    let frames = frame(&mut phase, 3, 3, Reason::Change, false);
    desktop.poll_focused(&frames, &phase, true).unwrap();
    let closed = gate_frame(2, 4, Reason::Change, false);
    phase.accept(closed.clone()).unwrap();
    desktop.poll_focused(&[closed], &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 2);
    assert_eq!(
        phase.sample(&endpoint("toggle", "value")),
        Some(&boolean(true))
    );

    focused.store(false, Ordering::Release);
    phase.advance(Counter(5)).unwrap();
    desktop.poll_focused(&[], &phase, false).unwrap();
    focused.store(true, Ordering::Release);
    phase.advance(Counter(10)).unwrap();
    desktop.poll_focused(&[], &phase, true).unwrap();
    let mut frames = frame(&mut phase, 4, 11, Reason::Change, false);
    let closed = gate_frame(3, 11, Reason::Change, false);
    phase.accept(closed.clone()).unwrap();
    frames.push(closed);
    desktop.poll_focused(&frames, &phase, true).unwrap();
    assert!(!desktop.waiting_neutral.contains("action"));
    let opened = gate_frame(4, 12, Reason::Change, true);
    phase.accept(opened.clone()).unwrap();
    desktop.poll_focused(&[opened], &phase, true).unwrap();
    assert_eq!(
        phase.sample(&endpoint("fallback", "value")),
        Some(&boolean(true))
    );
    assert_eq!(
        desktop
            .shadow
            .as_ref()
            .unwrap()
            .sample(&endpoint("toggle", "value")),
        Some(&boolean(false))
    );
    assert_eq!(
        events.lock().unwrap().len(),
        2,
        "Opening the gate must not resume the old toggle"
    );
    let frames = frame(&mut phase, 5, 13, Reason::Change, true);
    desktop.poll_focused(&frames, &phase, true).unwrap();
    assert_eq!(
        events.lock().unwrap().len(),
        3,
        "A new press toggles the isolated desktop state on"
    );
}
#[test]
fn stationary_pointer_keeps_native_liveness_without_refreshing_capture_provenance() {
    let action = Action::Pointer {
        axis: osg_mapping::desktop::PointerAxis::X,
        speed: 1000,
        curve: 1.0,
    };
    let mut program = source_program(action.clone(), false);
    program.sources[0].contract = action.contract();
    let (mut desktop, mut phase, _, events) = prepared_program_fixture(program);
    let value_frame = |sequence, at, reason, value| Frame {
        source: "source".into(),
        generation: Counter(7),
        sequence: Counter(sequence),
        captured_us: Counter(at),
        reason,
        changes: vec![Change {
            port_id: "button".into(),
            sample: SignalSample::scalar(PortValue::Number(value)),
        }],
    };
    phase
        .accept(value_frame(1, 1, Reason::Baseline, 0.0))
        .unwrap();
    desktop.start_focused(&phase, true).unwrap();
    let moved = value_frame(2, 2, Reason::Change, 0.5);
    phase.accept(moved.clone()).unwrap();
    desktop.poll_focused(&[moved], &phase, true).unwrap();
    for tick in [500_000, 1_000_000, 1_500_000, 2_000_000] {
        phase
            .observe_liveness("source", Counter(7), Counter(tick))
            .unwrap();
        phase.advance(Counter(tick)).unwrap();
        desktop.poll_focused(&[], &phase, true).unwrap();
    }
    let shadow = desktop.shadow.as_ref().unwrap();
    assert_eq!(
        shadow.sample(&endpoint("source", "button")),
        Some(&SignalSample::scalar(PortValue::Number(0.5)))
    );
    let token = shadow
        .endpoint_token(&endpoint("source", "button"))
        .unwrap();
    let observations = shadow.source_observations(&token);
    assert_eq!(observations[0].at_us, Counter(2));
    assert_eq!(observations[0].sequence, Counter(2));
    assert_eq!(events.lock().unwrap().len(), 4);
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
    phase.drain();
    let effects = frame(&mut phase, 5, 5, Reason::Disconnect, false);
    assert!(
        phase.drain().is_empty(),
        "A known fallback event without an occurrence is suppressed"
    );
    desktop.poll_focused(&effects, &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), 2);
    phase.advance(Counter(100_005)).unwrap();
    phase.drain();
    desktop.poll_focused(&[], &phase, true).unwrap();
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
        phase.drain();
        desktop.poll_focused(&[], &phase, true).unwrap();
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
