use super::*;
use crate::fixtures::*;
#[test]
fn counters_are_lossless_canonical_strings() {
    assert_eq!(
        serde_json::to_string(&Counter(u64::MAX)).unwrap(),
        "\"18446744073709551615\""
    );
    assert_eq!(
        serde_json::from_str::<Counter>("\"18446744073709551615\"")
            .unwrap()
            .0,
        u64::MAX
    );
    for invalid in [
        "1",
        "true",
        "\"01\"",
        "\"-1\"",
        "\"\"",
        "\"1.0\"",
        "\"١\"",
        "\"18446744073709551616\"",
    ] {
        assert!(serde_json::from_str::<Counter>(invalid).is_err());
    }
}
#[test]
fn held_start_level_sync_overlapping_holds_and_stop() {
    let mut e = engine();
    assert!(e.active());
    assert!(!e.ready());
    assert_eq!(e.generation(), Counter(1));
    assert_eq!(e.drain().len(), 2);
    assert_eq!(e.profile().name, "Two source preview");
    assert!(e.token("left-trigger").is_err());
    e.accept(baseline("left", 1, true)).unwrap();
    assert!(!e.last["main"].buttons[0]);
    assert!(e.last["aux"].buttons[0]);
    assert_eq!(e.last["main"].axes[0], 10);
    assert!(!e.ready());
    e.accept(baseline("right", 1, false)).unwrap();
    assert!(e.ready());
    e.accept(frame(
        "left",
        1,
        2,
        1,
        Reason::Change,
        vec![("trigger", Value::Button(false))],
    ))
    .unwrap();
    e.accept(frame(
        "left",
        1,
        3,
        2,
        Reason::Change,
        vec![("trigger", Value::Button(true))],
    ))
    .unwrap();
    e.accept(frame(
        "right",
        1,
        2,
        3,
        Reason::Change,
        vec![("trigger", Value::Button(true))],
    ))
    .unwrap();
    e.accept(frame(
        "left",
        1,
        4,
        4,
        Reason::Change,
        vec![("trigger", Value::Button(false))],
    ))
    .unwrap();
    assert!(e.last["main"].buttons[0]);
    e.accept(frame(
        "right",
        1,
        3,
        5,
        Reason::Change,
        vec![("trigger", Value::Button(false))],
    ))
    .unwrap();
    assert!(!e.last["main"].buttons[0]);
    let reports = e.drain();
    assert!(reports
        .iter()
        .any(|r| r.target == "main" && r.state.buttons[0]));
    e.advance(Counter(1_000_000)).unwrap();
    assert!(e.ready());
    assert_eq!(e.last["main"].axes[0], 10);
    assert!(e.drain().is_empty());
    e.stop();
    assert!(!e.active());
    assert!(!e.ready());
    assert_eq!(e.last["main"].axes[0], -123);
    assert_eq!(e.drain().len(), 2);
    e.stop();
    assert!(e.drain().is_empty());
    assert!(e.advance(Counter(2_000_000)).is_err());
    assert!(e.accept(baseline("left", 2, false)).is_err());
}
#[test]
fn atomic_changes_axes_hats_stale_and_missing_baselines() {
    let mut e = engine();
    assert!(e
        .accept(frame(
            "left",
            1,
            1,
            0,
            Reason::Change,
            vec![("trigger", Value::Button(true))]
        ))
        .is_err());
    e.accept(baseline("left", 1, false)).unwrap();
    let before = e.last.clone();
    let seq = e.sources[&crate::trace::synthetic("left")].sequence;
    for changes in [
        vec![("x", Value::Axis(40000))],
        vec![("hat", Value::Hat(Some(8)))],
        vec![("trigger", Value::Axis(0))],
        vec![("unknown", Value::Button(true))],
        vec![("x", Value::Axis(1)), ("x", Value::Axis(2))],
        vec![],
    ] {
        assert!(e
            .accept(frame("left", 1, 2, 1, Reason::Change, changes))
            .is_err());
        assert_eq!(e.last, before);
        assert_eq!(e.sources[&crate::trace::synthetic("left")].sequence, seq);
    }
    for (seq, hat) in [(2, Some(1)), (3, Some(7)), (4, None)] {
        e.accept(frame(
            "left",
            1,
            seq,
            seq,
            Reason::Change,
            vec![
                ("hat", Value::Hat(hat)),
                ("x", Value::Axis(if seq == 2 { -32768 } else { 32767 })),
            ],
        ))
        .unwrap();
        assert_eq!(e.last["main"].hats[0], hat);
    }
    assert!(e
        .accept(frame(
            "left",
            1,
            4,
            5,
            Reason::Change,
            vec![("x", Value::Axis(0))]
        ))
        .is_err());
    assert!(e
        .accept(frame(
            "left",
            0,
            5,
            5,
            Reason::Change,
            vec![("x", Value::Axis(0))]
        ))
        .is_err());
    assert!(e
        .accept(frame(
            "left",
            1,
            5,
            1,
            Reason::Change,
            vec![("x", Value::Axis(0))]
        ))
        .is_err());
    assert!(e
        .accept(frame(
            "left",
            1,
            0,
            5,
            Reason::Change,
            vec![("x", Value::Axis(0))]
        ))
        .is_err());
    assert!(e
        .accept(frame("other", 1, 1, 0, Reason::Baseline, vec![]))
        .is_err());
    let mut incomplete = baseline("right", 1, false);
    incomplete.changes.clear();
    assert!(e.accept(incomplete).is_err());
    let mut duplicate = baseline("left", 1, false);
    duplicate.captured_us = Counter(5);
    assert!(e.accept(duplicate).is_err());
    assert!(e
        .accept(frame(
            "left",
            1,
            5,
            5,
            Reason::Disconnect,
            vec![("x", Value::Axis(0))]
        ))
        .is_err());
}
#[test]
fn gaps_disconnect_generations_and_pending_timers_release_only_affected_owners() {
    let mut e = engine();
    e.accept(baseline("left", 1, false)).unwrap();
    e.accept(baseline("right", 1, true)).unwrap();
    e.drain();
    let token = e.token("left-trigger").unwrap();
    e.schedule(
        token.clone(),
        "press".into(),
        Counter(10),
        Value::Button(true),
    )
    .unwrap();
    assert!(e
        .accept(frame(
            "left",
            1,
            3,
            10,
            Reason::Change,
            vec![("trigger", Value::Button(true))]
        ))
        .is_err());
    assert!(e.last["main"].buttons[0]);
    assert_eq!(e.last["main"].axes[0], -123);
    assert!(e.timers.is_empty());
    assert!(!e.drain().iter().any(|r| r.reason == ReportReason::Timer));
    assert!(e
        .schedule(
            token.clone(),
            "old".into(),
            Counter(20),
            Value::Button(true)
        )
        .is_err());
    let mut next = baseline("left", 2, true);
    next.captured_us = Counter(11);
    e.accept(next).unwrap();
    assert!(e
        .schedule(token, "old".into(), Counter(20), Value::Button(true))
        .is_err());
    e.accept(frame("right", 1, 2, 12, Reason::Disconnect, vec![]))
        .unwrap();
    assert!(!e.last["main"].buttons[0]);
    assert!(e
        .accept(frame(
            "left",
            1,
            5,
            13,
            Reason::Change,
            vec![("trigger", Value::Button(true))]
        ))
        .is_err());
    e.accept(frame("left", 2, 2, 14, Reason::Gap, vec![]))
        .unwrap();
    assert_eq!(e.last["main"].hats[0], None);
}
#[test]
fn timers_preserve_pulse_edges_and_obey_owner_limits() {
    let mut e = engine();
    e.accept(baseline("left", 1, false)).unwrap();
    e.accept(baseline("right", 1, false)).unwrap();
    e.drain();
    let token = e.token("left-trigger").unwrap();
    e.schedule(
        token.clone(),
        "press".into(),
        Counter(30_000),
        Value::Button(true),
    )
    .unwrap();
    e.schedule(
        token.clone(),
        "release".into(),
        Counter(60_000),
        Value::Button(false),
    )
    .unwrap();
    e.advance(Counter(100_000)).unwrap();
    let output = e.drain();
    assert_eq!(output.len(), 2);
    assert!(output[0].state.buttons[0]);
    assert!(!output[1].state.buttons[0]);
    assert_eq!(output[0].at_us, Counter(30_000));
    assert_eq!(output[1].at_us, Counter(60_000));
    assert!(e.advance(Counter(0)).is_err());
    assert!(e
        .schedule(
            token.clone(),
            "past".into(),
            Counter(0),
            Value::Button(true)
        )
        .is_err());
    assert!(e
        .schedule(
            token.clone(),
            "future".into(),
            Counter(u64::MAX),
            Value::Button(true)
        )
        .is_err());
    assert!(e
        .schedule(
            token.clone(),
            "bad/id".into(),
            Counter(100_000),
            Value::Button(true)
        )
        .is_err());
    assert!(e
        .schedule(
            token.clone(),
            "wrong".into(),
            Counter(100_000),
            Value::Axis(3)
        )
        .is_err());
    assert!(e.token("level-copy").is_err());
    assert!(e.token("unknown").is_err());
    for n in 0..16 {
        e.schedule(
            token.clone(),
            format!("timer{n}"),
            Counter(200_000),
            Value::Button(true),
        )
        .unwrap();
    }
    assert!(e
        .schedule(
            token.clone(),
            "extra".into(),
            Counter(200_000),
            Value::Button(true)
        )
        .is_err());
    e.schedule(
        token.clone(),
        "timer0".into(),
        Counter(210_000),
        Value::Button(false),
    )
    .unwrap();
    e.cancel_timer(&token, "timer0").unwrap();
    assert_eq!(e.timers.len(), 15);
    let mut stale = token;
    stale.run_generation = Counter(2);
    assert!(e.cancel_timer(&stale, "timer1").is_err());
    e.stop();
    assert!(e.timers.is_empty());
}
#[test]
fn explicit_priority_and_report_backpressure() {
    let mut p = profile();
    let mut second = p.nodes[2].clone();
    second.id = "second-axis".into();
    p.nodes.push(second);
    p.arbitration.push(profile::Arbitration {
        output: p.nodes[2].output.clone().unwrap(),
        priority: vec!["second-axis".into(), "left-x".into()],
    });
    let mut e = Engine::new(p, assignments(), Counter(1)).unwrap();
    e.accept(baseline("left", 1, false)).unwrap();
    let token = e.token("second-axis").unwrap();
    e.schedule(token, "override".into(), Counter(1), Value::Axis(300))
        .unwrap();
    e.advance(Counter(1)).unwrap();
    assert_eq!(e.last["main"].axes[0], 300);
    for seq in 2..5000 {
        if !e.active() {
            break;
        }
        let _ = e.accept(frame(
            "left",
            1,
            seq,
            seq,
            Reason::Change,
            vec![("trigger", Value::Button(seq % 2 == 0))],
        ));
    }
    assert!(!e.active());
    assert!(e.fault().unwrap().contains("fell behind"));
    let reports = e.drain();
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| r.reason == ReportReason::Overflow));
    assert!(!reports[0].state.buttons[0]);
    assert!(e.contributions().is_empty());
}
#[test]
fn assignments_are_explicit_and_cannot_select_virtual_sources() {
    assert!(Engine::new(profile(), assignments(), Counter(0)).is_err());
    assert!(Engine::new(profile(), vec![], Counter(1)).is_err());
    let valid = assignments();
    let mut bad = valid.clone();
    bad[0].role = "unknown".into();
    assert!(Engine::new(profile(), bad, Counter(1)).is_err());
    let mut bad = valid.clone();
    bad[0].source.id = "opensimgear-virtual:a".into();
    assert!(Engine::new(profile(), bad, Counter(1)).is_err());
    let mut bad = valid.clone();
    bad[0].source.origin = Origin::HostController;
    assert!(Engine::new(profile(), bad, Counter(1)).is_err());
    let mut bad = valid.clone();
    bad[0].controls[0].control = "unknown".into();
    assert!(Engine::new(profile(), bad, Counter(1)).is_err());
    let mut bad = valid.clone();
    bad[0].controls[1] = bad[0].controls[0].clone();
    assert!(Engine::new(profile(), bad, Counter(1)).is_err());
    let mut bad = valid.clone();
    bad[0].controls[1].source_control = bad[0].controls[0].source_control.clone();
    assert!(Engine::new(profile(), bad, Counter(1)).is_err());
    let mut p = profile();
    p.roles[1].controls[0].kind = Kind::Axis {
        range: profile::AxisRange::Bipolar,
    };
    p.nodes[1].output = p.nodes[2].output.clone();
    p.arbitration.push(profile::Arbitration {
        output: p.nodes[2].output.clone().unwrap(),
        priority: vec!["left-x".into(), "right-trigger".into()],
    });
    let mut bad = valid;
    bad[1].source = bad[0].source.clone();
    assert!(Engine::new(p, bad, Counter(1)).is_err());
}

#[test]
fn timer_limit_stale_tokens_and_hat_priority_are_bounded() {
    let mut p = profile();
    let mut duplicate = p.nodes[3].clone();
    duplicate.id = "priority-hat".into();
    p.nodes.push(duplicate);
    p.arbitration.push(profile::Arbitration {
        output: p.nodes[3].output.clone().unwrap(),
        priority: vec!["priority-hat".into(), "left-hat".into()],
    });
    let mut e = Engine::new(p, assignments(), Counter(1)).unwrap();
    e.accept(baseline("left", 1, false)).unwrap();
    e.drain();
    let token = e.token("priority-hat").unwrap();
    e.schedule(
        token.clone(),
        "diagonal".into(),
        Counter(1),
        Value::Hat(Some(3)),
    )
    .unwrap();
    e.advance(Counter(1)).unwrap();
    assert_eq!(e.last["main"].hats[0], Some(3));
    e.schedule(token, "neutral".into(), Counter(2), Value::Hat(None))
        .unwrap();
    e.advance(Counter(2)).unwrap();
    assert_eq!(e.last["main"].hats[0], None);
    let token = e.token("left-trigger").unwrap();
    for n in 0..MAX_TIMERS {
        e.timers.insert(
            (100, format!("fixture{n}")),
            Timer {
                token: token.clone(),
                value: Value::Button(true),
            },
        );
    }
    assert!(e
        .schedule(
            token.clone(),
            "overflow".into(),
            Counter(101),
            Value::Button(true)
        )
        .is_err());
    e.timers.clear();
    let mut stale = token.clone();
    stale.run_generation = Counter(99);
    e.timers.insert(
        (3, "obsolete".into()),
        Timer {
            token: stale,
            value: Value::Button(true),
        },
    );
    e.advance(Counter(3)).unwrap();
    assert!(!e.last["main"].buttons[0]);
    e.reports = vec![
        Report {
            run_generation: Counter(1),
            sequence: Counter(1),
            at_us: Counter(3),
            target: "main".into(),
            reason: ReportReason::Change,
            state: e.last["main"].clone()
        };
        MAX_REPORTS
    ]
    .into();
    e.schedule(token, "queued".into(), Counter(4), Value::Button(true))
        .unwrap();
    assert!(e.advance(Counter(4)).is_err());
    assert!(!e.active());
    assert_eq!(e.drain().len(), 2);
}
#[test]
fn a_native_source_cannot_exceed_its_control_bound_across_roles() {
    let mut p = profile();
    let mut bindings = assignments();
    for (role_index, role) in p.roles.iter_mut().enumerate() {
        for n in 0..140 {
            let id = format!("extra{n}");
            role.controls.push(profile::Control {
                id: id.clone(),
                kind: Kind::Button,
            });
            bindings[role_index].controls.push(ControlAssignment {
                control: id,
                source_control: format!("role{role_index}-control{n}"),
            });
        }
    }
    bindings[1].source = bindings[0].source.clone();
    assert!(Engine::new(p, bindings, Counter(1))
        .err()
        .unwrap()
        .contains("256 controls"));
    assert!(!native_id("cockpit-virtual:one"));
}
