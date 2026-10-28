use super::*;
use crate::{
    engine::{typed::Dependency, Reason},
    program::*,
    signal::*,
};

fn run() -> RunToken {
    RunToken {
        session: "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12".into(),
        generation: Counter(1),
    }
}
fn token(generation: u64) -> Token {
    Token {
        run: run(),
        node_id: "source".into(),
        dependencies: vec![Dependency {
            endpoint: Endpoint {
                node_id: "source".into(),
                port_id: "button".into(),
            },
            generation: Counter(generation),
            epoch: Counter(generation),
        }],
    }
}
fn boolean(value: bool) -> SignalSample {
    SignalSample::scalar(PortValue::Boolean(value))
}
fn scheduler(action: Action) -> Scheduler {
    Scheduler::new(
        run(),
        vec![ActionDefinition {
            id: "action".into(),
            action,
        }],
    )
    .unwrap()
}
fn accept(s: &mut Scheduler, value: bool, at: u64, baseline: bool) {
    s.accept("action", token(1), boolean(value), Counter(at), baseline)
        .unwrap();
}

#[test]
fn physical_keys_reject_text_media_pause_and_unknown_usages() {
    for usage in [4, 0x1d, 0x47, 0x49, 0x65, 0xe0, 0xe7] {
        Key(usage).validate().unwrap();
    }
    for usage in [0, 3, 0x48, 0x66, 0xff, 0x100] {
        assert!(Key(usage).validate().is_err());
    }
    assert!(Action::Chord {
        keys: vec![Key(4), Key(4)]
    }
    .validate()
    .is_err());
    assert!(Action::Pointer {
        axis: PointerAxis::X,
        speed: 10,
        curve: f64::NAN
    }
    .validate()
    .is_err());
}
#[test]
fn held_baseline_requires_neutral_then_releases_only_its_owner() {
    let mut s = scheduler(Action::KeyHold { key: Key(4) });
    accept(&mut s, true, 0, true);
    accept(&mut s, true, 1, false);
    assert!(s
        .drain()
        .iter()
        .all(|delivery| matches!(delivery.intent, Intent::Cleanup)));
    accept(&mut s, false, 2, false);
    accept(&mut s, true, 3, false);
    let held = s.drain();
    assert_eq!(held.len(), 1);
    assert_eq!(
        held[0].intent,
        Intent::Held {
            controls: vec![Control::Key { key: Key(4) }]
        }
    );
    accept(&mut s, false, 4, false);
    let up = s.drain();
    assert_eq!(up[0].producer, token(1));
    assert_eq!(up[0].intent, Intent::Held { controls: vec![] });
    assert_eq!(up[0].deadline_us, None);
}
#[test]
fn chord_contribution_orders_modifiers_before_other_keys() {
    let mut s = scheduler(Action::Chord {
        keys: vec![Key(0x16), Key(0xe1), Key(0xe0)],
    });
    accept(&mut s, false, 0, true);
    s.drain();
    accept(&mut s, true, 1, false);
    assert_eq!(
        s.drain()[0].intent,
        Intent::Held {
            controls: vec![
                Control::Key { key: Key(0xe0) },
                Control::Key { key: Key(0xe1) },
                Control::Key { key: Key(0x16) }
            ]
        }
    );
}
#[test]
fn ten_encoder_detents_serialize_exactly_ten_taps_with_both_dwells() {
    let mut s = scheduler(Action::KeyTap {
        key: Key(4),
        down_ms: 30,
        up_ms: 30,
    });
    let mut output = Vec::new();
    for _ in 0..10 {
        accept(&mut s, true, 0, false);
        output.extend(s.drain());
    }
    assert_eq!(output.len(), 1);
    for step in 1..=20 {
        s.advance(Counter(step * 30_000)).unwrap();
        output.extend(s.drain());
    }
    assert_eq!(output.len(), 20);
    for (index, delivery) in output.iter().enumerate() {
        let Intent::Held { controls } = &delivery.intent else {
            panic!("Expected distinct tap edge")
        };
        assert_eq!(controls.is_empty(), index % 2 == 1);
        assert_eq!(delivery.at_us, Counter(index as u64 * 30_000));
        assert_eq!(delivery.sequence, Counter(index as u64 + 1));
        assert_eq!(delivery.operation_id, delivery.sequence);
    }
}
#[test]
fn repeat_skips_missed_intervals_and_never_releases_to_repeat() {
    let mut s = scheduler(Action::Repeat {
        key: Key(4),
        delay_ms: 100,
        interval_ms: 50,
    });
    accept(&mut s, false, 0, true);
    s.drain();
    accept(&mut s, true, 1, false);
    s.drain();
    s.advance(Counter(200_001)).unwrap();
    let events = s.drain();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].intent, Intent::Repeat { key: Key(4) });
    s.advance(Counter(10_000_000)).unwrap();
    assert!(s.drain().is_empty());
    s.advance(Counter(10_050_000)).unwrap();
    assert_eq!(s.drain().len(), 1);
    assert!(s
        .decisions()
        .iter()
        .any(|decision| decision.kind == "repeat" && decision.detail.contains("missed")));
}
#[test]
fn pointer_stationary_deflection_retains_fraction_and_clamps_elapsed() {
    let mut s = scheduler(Action::Pointer {
        axis: PointerAxis::X,
        speed: 10,
        curve: 1.0,
    });
    s.accept(
        "action",
        token(1),
        SignalSample::scalar(PortValue::Number(0.0)),
        Counter(0),
        true,
    )
    .unwrap();
    s.drain();
    s.accept(
        "action",
        token(1),
        SignalSample::scalar(PortValue::Number(0.5)),
        Counter(1),
        false,
    )
    .unwrap();
    let mut events = Vec::new();
    // 5 px/sec over 400 ms. Sub-tick advances must not discard elapsed time.
    for tick in 1..=400 {
        s.advance(Counter(1 + tick * 1000)).unwrap();
        events.extend(s.drain());
    }
    let total = events
        .iter()
        .map(|event| {
            if let Intent::Pointer { dx, .. } = event.intent {
                dx
            } else {
                0
            }
        })
        .sum::<i32>();
    assert_eq!(total, 2);
    s.advance(Counter(20_000_000)).unwrap();
    let late = s.drain();
    assert!(late
        .iter()
        .all(|event| !matches!(event.intent,Intent::Pointer{dx,..}if dx>1)));
}
#[test]
fn pause_discards_pending_edges_and_requires_neutral_without_catchup() {
    let mut s = scheduler(Action::Pointer {
        axis: PointerAxis::Y,
        speed: 1000,
        curve: 1.0,
    });
    s.accept(
        "action",
        token(1),
        SignalSample::scalar(PortValue::Number(0.0)),
        Counter(0),
        true,
    )
    .unwrap();
    s.drain();
    s.accept(
        "action",
        token(1),
        SignalSample::scalar(PortValue::Number(1.0)),
        Counter(1),
        false,
    )
    .unwrap();
    s.advance(Counter(8_001)).unwrap();
    s.pause("foreground-lost");
    assert!(s
        .drain()
        .iter()
        .all(|event| event.intent == Intent::Cleanup));
    s.advance(Counter(10_000_000)).unwrap();
    assert!(s.drain().is_empty());
    s.accept(
        "action",
        token(1),
        SignalSample::scalar(PortValue::Number(1.0)),
        Counter(10_000_001),
        false,
    )
    .unwrap();
    s.advance(Counter(10_008_001)).unwrap();
    assert!(s.drain().is_empty());
    s.accept(
        "action",
        token(1),
        SignalSample::scalar(PortValue::Number(0.0)),
        Counter(10_008_002),
        true,
    )
    .unwrap();
    s.drain();
    s.accept(
        "action",
        token(1),
        SignalSample::scalar(PortValue::Number(1.0)),
        Counter(10_008_003),
        false,
    )
    .unwrap();
    s.advance(Counter(10_016_003)).unwrap();
    assert_eq!(s.drain()[0].intent, Intent::Pointer { dx: 0, dy: 8 });
}
#[test]
fn generation_change_and_source_loss_cancel_taps_before_running_timers() {
    let mut s = scheduler(Action::KeyTap {
        key: Key(4),
        down_ms: 30,
        up_ms: 30,
    });
    accept(&mut s, true, 0, false);
    s.drain();
    s.accept("action", token(2), boolean(false), Counter(60_000), true)
        .unwrap();
    assert!(s
        .drain()
        .iter()
        .all(|event| event.intent == Intent::Cleanup));
    assert!(s
        .accept("action", token(1), boolean(true), Counter(60_001), false)
        .is_err());
    s.accept("action", token(2), boolean(true), Counter(60_002), false)
        .unwrap();
    s.drain();
    s.source_lost("source");
    assert_eq!(s.drain()[0].intent, Intent::Cleanup);
    s.advance(Counter(120_002)).unwrap();
    assert!(s.drain().is_empty());
}
#[test]
fn queue_overflow_faults_visibly_and_cleanup_does_not_expire() {
    let mut s = scheduler(Action::MouseClick {
        button: MouseButton::Left,
        down_ms: 30,
        up_ms: 30,
    });
    for _ in 0..MAX_TAPS {
        accept(&mut s, true, 0, false);
        s.drain();
    }
    assert!(s
        .accept("action", token(1), boolean(true), Counter(0), false)
        .is_err());
    assert!(!s.active());
    assert!(s.fault().unwrap().contains("queue overflow"));
    assert!(s
        .drain()
        .iter()
        .all(|event| event.intent == Intent::Cleanup && event.deadline_us.is_none()));
}
#[test]
fn expired_click_cancels_without_late_click_or_catchup() {
    let mut s = scheduler(Action::MouseClick {
        button: MouseButton::Right,
        down_ms: 30,
        up_ms: 30,
    });
    accept(&mut s, true, 0, false);
    s.drain();
    s.advance(Counter(1_000_000)).unwrap();
    assert_eq!(s.drain()[0].intent, Intent::Cleanup);
    s.advance(Counter(2_000_000)).unwrap();
    assert!(s.drain().is_empty());
}
#[test]
fn diagonal_policy_is_explicit_for_all_hat_directions() {
    assert!(hat_direction(
        Some(1),
        HatDirection::Up,
        HatDiagonal::IncludeAdjacent
    ));
    assert!(hat_direction(
        Some(1),
        HatDirection::Right,
        HatDiagonal::IncludeAdjacent
    ));
    assert!(!hat_direction(
        Some(1),
        HatDirection::Up,
        HatDiagonal::CardinalOnly
    ));
    assert!(!hat_direction(
        None,
        HatDirection::Up,
        HatDiagonal::IncludeAdjacent
    ));
    assert!(!hat_direction(
        Some(8),
        HatDirection::Up,
        HatDiagonal::IncludeAdjacent
    ));
    for (direction, value) in [
        (HatDirection::Up, 0),
        (HatDirection::Right, 2),
        (HatDirection::Down, 4),
        (HatDirection::Left, 6),
    ] {
        assert!(hat_direction(
            Some(value),
            direction,
            HatDiagonal::CardinalOnly
        ));
    }
}
#[test]
fn wheel_occurrences_retain_identity_and_desktop_sequence() {
    let mut s = scheduler(Action::Wheel {
        horizontal: 1,
        vertical: -1,
    });
    for _ in 0..10 {
        accept(&mut s, true, 0, false);
    }
    let output = s.drain();
    assert_eq!(output.len(), 10);
    for (index, event) in output.iter().enumerate() {
        assert_eq!(event.sequence, Counter(index as u64 + 1));
        assert_eq!(event.producer, token(1));
    }
}
#[test]
fn replay_preserves_stop_generation_clock_edges_and_scope_decisions() {
    let mut s = scheduler(Action::KeyTap {
        key: Key(4),
        down_ms: 30,
        up_ms: 30,
    });
    accept(&mut s, true, 0, false);
    let mut expected = s.drain();
    s.advance(Counter(30_000)).unwrap();
    expected.extend(s.drain());
    s.pause("foreground-lost");
    expected.extend(s.drain());
    s.source_lost("source");
    expected.extend(s.drain());
    s.stop();
    expected.extend(s.drain());
    let trace = s.trace();
    let json = serde_json::to_vec(&trace).unwrap();
    let trace: Trace = serde_json::from_slice(&json).unwrap();
    let replay = trace.replay().unwrap();
    assert_eq!(replay.deliveries, expected);
    assert_eq!(replay.decisions, s.decisions());
    assert!(s.advance(Counter(100_000)).is_err());
}
#[test]
fn desktop_only_program_compiles_without_controller_and_captures_source_token() {
    let action = Action::KeyHold { key: Key(4) };
    let endpoint = Endpoint {
        node_id: "source".into(),
        port_id: "button".into(),
    };
    let program = Program {
        sources: vec![SourcePort {
            endpoint: endpoint.clone(),
            contract: action.contract(),
        }],
        nodes: vec![],
        sinks: vec![Sink {
            id: "action".into(),
            endpoint: Endpoint {
                node_id: "desktop".into(),
                port_id: "key-a".into(),
            },
            from: endpoint,
            contract: action.contract(),
            kind: SinkKind::Desktop { action },
        }],
        controllers: vec![],
    };
    program.compile().unwrap();
    let mut phase = crate::engine::typed::Phase::new(program, Counter(1), run().session).unwrap();
    phase.drain();
    for (sequence, reason, value) in [(1, Reason::Baseline, false), (2, Reason::Change, true)] {
        phase
            .accept(crate::engine::typed::Frame {
                source: "source".into(),
                generation: Counter(7),
                sequence: Counter(sequence),
                captured_us: Counter(sequence),
                reason,
                changes: vec![crate::engine::typed::Change {
                    port_id: "button".into(),
                    sample: boolean(value),
                }],
            })
            .unwrap();
    }
    let effects = phase.drain();
    assert!(effects
        .iter()
        .all(|effect| effect.producer.as_ref().unwrap().dependencies[0].generation == Counter(7)));
    assert!(effects.iter().any(|effect| effect.sample == boolean(true)));
}
#[test]
fn multiple_desktop_endpoints_and_mismatched_action_contracts_are_rejected() {
    let action = Action::KeyHold { key: Key(4) };
    let contract = action.contract();
    let source = Endpoint {
        node_id: "source".into(),
        port_id: "button".into(),
    };
    let sink = |id: &str, desktop: &str| Sink {
        id: id.into(),
        endpoint: Endpoint {
            node_id: desktop.into(),
            port_id: id.into(),
        },
        from: source.clone(),
        contract: contract.clone(),
        kind: SinkKind::Desktop {
            action: action.clone(),
        },
    };
    let mut program = Program {
        sources: vec![SourcePort {
            endpoint: source.clone(),
            contract: contract.clone(),
        }],
        nodes: vec![],
        sinks: vec![sink("a", "desktop"), sink("b", "desktop2")],
        controllers: vec![],
    };
    assert!(program
        .compile()
        .unwrap_err()
        .contains("one Keyboard block"));
    program.sinks[1].kind = SinkKind::Desktop { action: Action::MouseHold { button: MouseButton::Left } };
    assert!(program.compile().is_ok(), "Independent Keyboard and Mouse blocks share the runtime");
    program.sinks.pop();
    program.sinks[0].kind = SinkKind::Desktop {
        action: Action::KeyTap {
            key: Key(4),
            down_ms: 30,
            up_ms: 30,
        },
    };
    assert!(program
        .compile()
        .unwrap_err()
        .contains("exact typed action"));
}
#[test]
fn recording_overflow_marks_replay_incomplete_without_stopping_live_actions() {
    let mut s = scheduler(Action::Wheel {
        horizontal: 0,
        vertical: 1,
    });
    for tick in 0..MAX_TRACE + 10 {
        s.advance(Counter(tick as u64)).unwrap();
        s.drain();
    }
    assert!(s.active());
    assert!(s.trace().dropped.0 > 0);
    assert!(s.trace().replay().unwrap_err().contains("incomplete"));
    accept(&mut s, true, MAX_TRACE as u64 + 10, false);
    assert_eq!(s.drain().len(), 1);
}
#[test]
fn bounded_delivery_overflow_cancels_work_instead_of_replaying_unsubmitted_edges() {
    let mut s = scheduler(Action::Wheel {
        horizontal: 0,
        vertical: 1,
    });
    let mut faulted = false;
    for _ in 0..MAX_DELIVERIES {
        if s.accept("action", token(1), boolean(true), Counter(0), false)
            .is_err()
        {
            faulted = true;
            break;
        }
    }
    assert!(faulted);
    assert!(s.fault().unwrap().contains("output queue overflow"));
    let output = s.drain();
    assert!(output.len() <= MAX_DELIVERIES);
    assert!(output
        .iter()
        .all(|delivery| delivery.intent == Intent::Cleanup));
    assert!(output
        .windows(2)
        .all(|pair| pair[0].sequence.0 + 1 == pair[1].sequence.0));
}
#[test]
fn native_liveness_keeps_stationary_pointer_and_transformed_state_without_rearming() {
    let action = Action::Pointer {
        axis: PointerAxis::X,
        speed: 100,
        curve: 1.0,
    };
    let contract = action.contract();
    let source = Endpoint {
        node_id: "source".into(),
        port_id: "axis".into(),
    };
    let transformed = Endpoint {
        node_id: "transform".into(),
        port_id: "value".into(),
    };
    let program = Program {
        sources: vec![SourcePort {
            endpoint: source.clone(),
            contract: contract.clone(),
        }],
        nodes: vec![Node {
            id: "transform".into(),
            inputs: vec![InputBinding {
                port_id: "input".into(),
                from: source,
                contract: contract.clone(),
                edge_id: "input".into(),
            }],
            outputs: vec![PortDescriptor {
                id: "value".into(),
                name: "Value".into(),
                direction: Direction::Output,
                contract: contract.clone(),
            }],
            operation: Operation::Identity {},
        }],
        sinks: vec![Sink {
            id: "action".into(),
            endpoint: Endpoint {
                node_id: "desktop".into(),
                port_id: "pointer".into(),
            },
            from: transformed.clone(),
            contract,
            kind: SinkKind::Desktop {
                action: action.clone(),
            },
        }],
        controllers: vec![],
    };
    let mut phase = crate::engine::typed::Phase::new(program, Counter(1), run().session).unwrap();
    phase.drain();
    let mut scheduler = scheduler(action);
    for (sequence, reason, value) in [(1, Reason::Baseline, 0.0), (2, Reason::Change, 1.0)] {
        phase
            .accept(crate::engine::typed::Frame {
                source: "source".into(),
                generation: Counter(7),
                sequence: Counter(sequence),
                captured_us: Counter(sequence),
                reason,
                changes: vec![crate::engine::typed::Change {
                    port_id: "axis".into(),
                    sample: SignalSample::scalar(PortValue::Number(value)),
                }],
            })
            .unwrap();
        for effect in phase.drain() {
            scheduler.accept_effect(&effect).unwrap();
        }
        scheduler.drain();
    }
    let mut motion = 0;
    for tick in 1..=625 {
        let at = Counter(tick * 8_000 + 2);
        phase.observe_liveness("source", Counter(7), at).unwrap();
        phase.advance(at).unwrap();
        assert!(
            phase.drain().is_empty(),
            "Liveness must not synthesize source changes"
        );
        assert_eq!(
            phase.sample(&transformed),
            Some(&SignalSample::scalar(PortValue::Number(1.0)))
        );
        scheduler.advance(at).unwrap();
        motion += scheduler
            .drain()
            .iter()
            .map(|delivery| {
                if let Intent::Pointer { dx, .. } = delivery.intent {
                    dx
                } else {
                    0
                }
            })
            .sum::<i32>();
    }
    assert_eq!(motion, 500);
    phase
        .accept(crate::engine::typed::Frame {
            source: "source".into(),
            generation: Counter(7),
            sequence: Counter(3),
            captured_us: Counter(5_000_003),
            reason: Reason::Disconnect,
            changes: vec![],
        })
        .unwrap();
    for effect in phase.drain() {
        scheduler.accept_effect(&effect).unwrap();
    }
    assert!(phase
        .observe_liveness("source", Counter(7), Counter(5_008_003))
        .is_err());
    assert!(scheduler
        .drain()
        .iter()
        .all(|delivery| delivery.intent == Intent::Cleanup));
    scheduler.advance(Counter(5_008_003)).unwrap();
    assert!(scheduler.drain().is_empty());
}
