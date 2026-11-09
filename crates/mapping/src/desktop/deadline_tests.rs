use super::*;
use crate::{engine::typed::Dependency, program::Endpoint};

fn run() -> RunToken {
    RunToken {
        session: "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12".into(),
        generation: Counter(1),
    }
}

fn producer() -> Token {
    Token {
        run: run(),
        node_id: "source".into(),
        dependencies: vec![Dependency {
            endpoint: Endpoint {
                node_id: "source".into(),
                port_id: "value".into(),
            },
            generation: Counter(1),
            epoch: Counter(1),
        }],
    }
}

fn scheduler(actions: Vec<(&str, Action)>) -> Scheduler {
    Scheduler::new(
        run(),
        actions
            .into_iter()
            .map(|(id, action)| ActionDefinition {
                id: id.into(),
                action,
            })
            .collect(),
    )
    .unwrap()
}

fn accept(s: &mut Scheduler, id: &str, value: PortValue, at: u64, baseline: bool) {
    s.accept(
        id,
        producer(),
        SignalSample::scalar(value),
        Counter(at),
        baseline,
    )
    .unwrap();
}

fn tap() -> Action {
    Action::KeyTap {
        key: Key(4),
        down_ms: 1,
        up_ms: 1,
    }
}

fn repeat() -> Action {
    Action::Repeat {
        key: Key(5),
        delay_ms: 1,
        interval_ms: 8,
    }
}

fn pointer() -> Action {
    Action::Pointer {
        axis: PointerAxis::X,
        speed: 1000,
        curve: 1.0,
    }
}

#[test]
fn one_millisecond_taps_keep_both_dwells_and_final_queue_retirement() {
    let mut s = scheduler(vec![("tap", tap())]);
    assert_eq!(s.next_deadline_us(), None);
    for _ in 0..2 {
        accept(&mut s, "tap", PortValue::Boolean(true), 1, false);
    }
    assert_eq!(s.drain().len(), 1);
    assert_eq!(s.next_deadline_us(), Some(Counter(1001)));
    s.advance(Counter(1000)).unwrap();
    assert!(s.drain().is_empty());
    assert_eq!(s.next_deadline_us(), Some(Counter(1001)));
    for (at, released) in [(1001, true), (2001, false), (3001, true)] {
        s.advance(Counter(at)).unwrap();
        let output = s.drain();
        assert_eq!(output.len(), 1);
        let Intent::Held { controls } = &output[0].intent else {
            panic!("Expected tap edge")
        };
        assert_eq!(controls.is_empty(), released);
        assert_eq!(s.next_deadline_us(), Some(Counter(at + 1000)));
    }
    // The final up dwell still owns a queued tap until its retirement deadline.
    s.advance(Counter(4001)).unwrap();
    assert!(s.drain().is_empty());
    assert_eq!(s.next_deadline_us(), None);
}

#[test]
fn queued_native_taps_time_each_dwell_from_its_exact_receipt() {
    let mut s = scheduler(vec![("tap", tap()), ("other", Action::KeyHold { key: Key(5) })]);
    s.use_native_tap_timing().unwrap();
    for _ in 0..2 {
        accept(&mut s, "tap", PortValue::Boolean(true), 1, false);
    }
    let down = s.drain().pop().unwrap();
    s.advance(Counter(4_000)).unwrap();
    assert!(s.drain().is_empty());
    assert_eq!(s.next_deadline_us(), None);

    accept(&mut s, "other", PortValue::Boolean(false), 4_000, true);
    accept(&mut s, "other", PortValue::Boolean(true), 4_001, false);
    assert_eq!(s.drain().pop().unwrap().action_id, "other");

    s.finish_native_tap("tap", down.operation_id, Some(Counter(5_000))).unwrap();
    assert_eq!(s.next_deadline_us(), Some(Counter(6_001)));
    s.advance(Counter(6_000)).unwrap();
    assert!(s.drain().is_empty());
    s.advance(Counter(6_001)).unwrap();
    let up = s.drain().pop().unwrap();
    assert!(matches!(&up.intent, Intent::Held { controls } if controls.is_empty()));

    s.advance(Counter(8_000)).unwrap();
    assert!(s.drain().is_empty());
    s.finish_native_tap("tap", down.operation_id, Some(Counter(9_000))).unwrap();
    assert_eq!(s.next_deadline_us(), None);
    s.finish_native_tap("tap", up.operation_id, Some(Counter(9_000))).unwrap();
    s.advance(Counter(10_000)).unwrap();
    assert!(s.drain().is_empty());
    s.advance(Counter(10_001)).unwrap();
    let next = s.drain().pop().unwrap();
    assert!(matches!(&next.intent, Intent::Held { controls } if !controls.is_empty()));
    assert_eq!(s.trace().replay().unwrap().deliveries.len(), 4);
}

#[test]
fn rejected_native_tap_and_scope_pause_discard_pending_receipts() {
    for scope_pause in [false, true] {
        let mut s = scheduler(vec![("tap", tap())]);
        s.use_native_tap_timing().unwrap();
        for _ in 0..2 {
            accept(&mut s, "tap", PortValue::Boolean(true), 1, false);
        }
        let down = s.drain().pop().unwrap();
        if scope_pause {
            s.pause("Scope lost");
        } else {
            s.finish_native_tap("tap", down.operation_id, None).unwrap();
        }
        assert!(matches!(s.drain().pop().unwrap().intent, Intent::Cleanup));
        s.finish_native_tap("tap", down.operation_id, Some(Counter(9_000))).unwrap();
        s.advance(Counter(15_000)).unwrap();
        assert!(s.drain().is_empty());
        assert_eq!(s.next_deadline_us(), None);
        assert_eq!(s.trace().replay().unwrap().deliveries.len(), 2);
    }
}

#[test]
fn repeat_exposes_exact_delay_interval_and_late_reschedule() {
    let mut s = scheduler(vec![("repeat", repeat())]);
    accept(&mut s, "repeat", PortValue::Boolean(false), 0, true);
    s.drain();
    accept(&mut s, "repeat", PortValue::Boolean(true), 1, false);
    s.drain();
    assert_eq!(s.next_deadline_us(), Some(Counter(1001)));
    s.advance(Counter(1001)).unwrap();
    assert_eq!(s.drain()[0].intent, Intent::Repeat { key: Key(5) });
    assert_eq!(s.next_deadline_us(), Some(Counter(9001)));
    s.advance(Counter(200_001)).unwrap();
    assert!(s.drain().is_empty());
    assert_eq!(s.next_deadline_us(), Some(Counter(208_001)));
    accept(&mut s, "repeat", PortValue::Boolean(false), 200_002, false);
    assert_eq!(s.drain()[0].intent, Intent::Held { controls: vec![] });
    assert_eq!(s.next_deadline_us(), None);
}

#[test]
fn pointer_uses_exact_shared_cadence_and_idle_does_not_spin() {
    let mut s = scheduler(vec![("pointer", pointer())]);
    accept(&mut s, "pointer", PortValue::Number(0.0), 0, true);
    s.drain();
    accept(&mut s, "pointer", PortValue::Number(1.0), 1, false);
    assert_eq!(s.next_deadline_us(), Some(Counter(8001)));
    s.advance(Counter(8000)).unwrap();
    assert!(s.drain().is_empty());
    assert_eq!(s.next_deadline_us(), Some(Counter(8001)));
    s.advance(Counter(8001)).unwrap();
    assert_eq!(s.drain()[0].intent, Intent::Pointer { dx: 8, dy: 0 });
    assert_eq!(s.next_deadline_us(), Some(Counter(16001)));
    accept(&mut s, "pointer", PortValue::Number(0.0), 9001, false);
    assert_eq!(s.next_deadline_us(), None);
    s.advance(Counter(16001)).unwrap();
    assert!(s.drain().is_empty());
    assert_eq!(s.next_deadline_us(), None);
    accept(&mut s, "pointer", PortValue::Number(1.0), 17001, false);
    assert_eq!(s.next_deadline_us(), Some(Counter(25001)));
}

fn busy() -> Scheduler {
    let mut s = scheduler(vec![
        ("tap", tap()),
        ("repeat", repeat()),
        ("pointer", pointer()),
    ]);
    accept(&mut s, "repeat", PortValue::Boolean(false), 0, true);
    accept(&mut s, "pointer", PortValue::Number(0.0), 0, true);
    s.drain();
    accept(&mut s, "pointer", PortValue::Number(1.0), 1, false);
    accept(&mut s, "repeat", PortValue::Boolean(true), 2, false);
    accept(&mut s, "tap", PortValue::Boolean(true), 3, false);
    s.drain();
    s
}

#[test]
fn query_selects_minimum_without_advancing_draining_or_recording() {
    let mut s = busy();
    let clock = s.at_us();
    let trace = serde_json::to_value(s.trace()).unwrap();
    let decisions = serde_json::to_value(s.decisions()).unwrap();
    for _ in 0..3 {
        assert_eq!(s.next_deadline_us(), Some(Counter(1002)));
    }
    assert_eq!(s.at_us(), clock);
    assert_eq!(serde_json::to_value(s.trace()).unwrap(), trace);
    assert_eq!(serde_json::to_value(s.decisions()).unwrap(), decisions);
    s.advance(Counter(1002)).unwrap();
    assert_eq!(s.next_deadline_us(), Some(Counter(1003)));
    assert_eq!(s.pending_deliveries().count(), 1);
    assert_eq!(s.next_deadline_us(), Some(Counter(1003)));
    assert_eq!(s.drain()[0].intent, Intent::Repeat { key: Key(5) });
}

#[test]
fn pause_source_loss_and_stop_withdraw_all_deadlines_but_keep_cleanup() {
    for boundary in ["pause", "source-loss", "stop"] {
        let mut s = busy();
        match boundary {
            "pause" => s.pause("Focus lost"),
            "source-loss" => s.source_lost("source"),
            _ => s.stop(),
        }
        assert_eq!(s.next_deadline_us(), None, "{boundary}");
        let cleanup = s.drain();
        assert_eq!(cleanup.len(), 3, "{boundary}");
        assert!(cleanup
            .iter()
            .all(|delivery| matches!(delivery.intent, Intent::Cleanup)));
        assert_eq!(s.next_deadline_us(), None);
        if s.active() {
            s.advance(Counter(20_000)).unwrap();
            assert!(s.drain().is_empty());
            assert_eq!(s.next_deadline_us(), None);
        }
    }
}

#[test]
fn tap_queue_fault_withdraws_timers_and_preserves_cleanup() {
    let mut s = busy();
    for _ in 1..MAX_TAPS {
        accept(&mut s, "tap", PortValue::Boolean(true), 3, false);
    }
    assert!(s
        .accept(
            "tap",
            producer(),
            SignalSample::scalar(PortValue::Boolean(true)),
            Counter(3),
            false
        )
        .is_err());
    assert!(s.fault().is_some());
    assert_eq!(s.next_deadline_us(), None);
    assert!(s
        .drain()
        .iter()
        .all(|delivery| matches!(delivery.intent, Intent::Cleanup)));
    assert_eq!(s.next_deadline_us(), None);
}
