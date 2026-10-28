use super::*;
use crate::{engine::typed::Dependency, program::Endpoint};

fn run() -> RunToken {
    RunToken { session: "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12".into(), generation: Counter(1) }
}
fn producer(id: &str) -> Token {
    Token { run: run(), node_id: id.into(), dependencies: vec![Dependency {
        endpoint: Endpoint { node_id: id.into(), port_id: "axis".into() },
        generation: Counter(1), epoch: Counter(1),
    }] }
}
fn scheduler(speed: u32) -> Scheduler {
    let mut s = Scheduler::new(run(), [
        ("x", PointerAxis::X), ("x2", PointerAxis::X), ("y", PointerAxis::Y),
    ].into_iter().map(|(id, axis)| ActionDefinition {
        id: id.into(), action: Action::Pointer { axis, speed, curve: 1.0 },
    }).collect()).unwrap();
    for id in ["x", "x2", "y"] { accept(&mut s, id, 0.0, 0, true); }
    s.drain();
    s
}
fn accept(s: &mut Scheduler, id: &str, value: f64, at: u64, baseline: bool) {
    s.accept(id, producer(id), SignalSample::scalar(PortValue::Number(value)), Counter(at), baseline).unwrap();
}
fn movement(s: &mut Scheduler) -> Vec<(String, u64, i32, i32)> {
    s.drain().into_iter().filter_map(|d| match d.intent {
        Intent::Pointer { dx, dy } => {
            assert_eq!(d.producer, producer(&d.action_id));
            Some((d.action_id, d.at_us.0, dx, dy))
        }, _ => None,
    }).collect()
}

#[test]
fn axes_started_at_different_times_emit_on_one_cadence() {
    let mut s = scheduler(1000);
    accept(&mut s, "x", 1.0, 1, false);
    accept(&mut s, "y", 1.0, 4001, false);
    s.advance(Counter(8001)).unwrap();
    assert_eq!(movement(&mut s), vec![("x".into(), 8001, 8, 0), ("y".into(), 8001, 0, 4)]);
    s.advance(Counter(12001)).unwrap();
    assert!(movement(&mut s).is_empty());
    s.advance(Counter(16001)).unwrap();
    assert_eq!(movement(&mut s), vec![("x".into(), 16001, 8, 0), ("y".into(), 16001, 0, 8)]);
}

#[test]
fn restarting_axis_does_not_shift_the_other_axis_cadence() {
    let mut s = scheduler(1000);
    accept(&mut s, "x", 1.0, 1, false);
    accept(&mut s, "y", 1.0, 1, false);
    s.advance(Counter(8001)).unwrap();
    movement(&mut s);
    accept(&mut s, "y", 0.0, 10001, false);
    accept(&mut s, "y", 1.0, 12001, false);
    s.advance(Counter(16001)).unwrap();
    assert_eq!(movement(&mut s), vec![("x".into(), 16001, 8, 0), ("y".into(), 16001, 0, 4)]);
}

#[test]
fn reversal_integrates_old_velocity_only_until_its_change_timestamp() {
    let mut s = scheduler(1000);
    accept(&mut s, "x", 1.0, 1, false);
    accept(&mut s, "x", -1.0, 3001, false);
    s.advance(Counter(8001)).unwrap();
    assert_eq!(movement(&mut s), vec![("x".into(), 8001, -2, 0)]);
    // Changing speed below a tick cannot apply the new speed backwards.
    accept(&mut s, "x", 0.5, 10001, false);
    s.advance(Counter(16001)).unwrap();
    assert_eq!(movement(&mut s), vec![("x".into(), 16001, 1, 0)]);
}

#[test]
fn idle_rebases_tick_and_neutral_discards_fractional_work() {
    let mut s = scheduler(1000);
    accept(&mut s, "x", 0.1, 1, false);
    accept(&mut s, "x", 0.0, 4001, false);
    accept(&mut s, "y", 1.0, 6001, false);
    s.advance(Counter(8001)).unwrap();
    assert!(movement(&mut s).is_empty());
    s.advance(Counter(14001)).unwrap();
    assert_eq!(movement(&mut s), vec![("y".into(), 14001, 0, 8)]);
}

#[test]
fn fractional_multiple_actions_preserve_displacement_provenance_and_replay() {
    let mut s = scheduler(1000);
    accept(&mut s, "x", 0.125, 1, false);
    accept(&mut s, "x2", 0.0625, 1, false);
    accept(&mut s, "y", -0.125, 1, false);
    let mut totals = BTreeMap::<String, (i32, i32)>::new();
    for t in 1..=32 {
        s.advance(Counter(1 + t * 1000)).unwrap();
        for (id, at, dx, dy) in movement(&mut s) {
            assert_eq!((at - 1) % POINTER_TICK_US, 0);
            let total = totals.entry(id).or_default(); total.0 += dx; total.1 += dy;
        }
    }
    assert_eq!(totals, BTreeMap::from([
        ("x".into(), (4, 0)), ("x2".into(), (2, 0)), ("y".into(), (0, -4)),
    ]));
    let replay = s.trace().replay().unwrap();
    assert!(replay.fault.is_none());
    assert_eq!(replay.deliveries.iter().filter(|d| matches!(d.intent, Intent::Pointer { .. })).count(), 10);
}
