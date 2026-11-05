use super::*;
use crate::desktop::tests::{fixture, frame};
use osg_mapping::{desktop::{Action, Key}, engine::Reason};

#[test]
fn short_tap_wakes_at_its_exact_release_and_queries_do_not_dispatch() {
    let (mut desktop, mut phase, _, events) = fixture(Action::KeyTap {key:Key(4),down_ms:1,up_ms:1}, true);
    let frames = frame(&mut phase, 2, 2, Reason::Change, true);
    desktop.poll_focused(&frames, &phase, true).unwrap();
    let submitted = events.lock().unwrap().len();
    let outcomes = desktop.outcomes.len();
    assert_eq!(desktop.next_deadline_us(), Some(Counter(1002)));
    assert_eq!(desktop.next_deadline_us(), Some(Counter(1002)));
    assert_eq!(events.lock().unwrap().len(), submitted);
    assert_eq!(desktop.outcomes.len(), outcomes);
    phase.advance(Counter(1001)).unwrap();
    desktop.poll_focused(&[], &phase, true).unwrap();
    assert_eq!(events.lock().unwrap().len(), submitted);
    phase.advance(Counter(1002)).unwrap();
    desktop.poll_focused(&[], &phase, true).unwrap();
    assert!(events.lock().unwrap().len() > submitted, "The due release must reach the worker");
    assert_eq!(desktop.next_deadline_us(), Some(Counter(2002)), "The final up dwell must also retire on time");
    desktop.poll_focused(&[], &phase, false).unwrap();
    assert_eq!(desktop.next_deadline_us(), None, "Focus loss withdraws action and shadow deadlines");
    desktop.stop().unwrap();
    assert_eq!(desktop.next_deadline_us(), None);
}

#[test]
fn isolated_shadow_deadline_survives_a_later_canonical_liveness_update() {
    let (mut desktop, mut phase, _, _) = fixture(Action::KeyHold {key:Key(4)}, false);
    let isolated = desktop.shadow.as_ref().unwrap().next_deadline_us().unwrap();
    phase.observe_liveness("source", Counter(7), Counter(20_000)).unwrap();
    assert!(phase.next_deadline_us().unwrap().0 > isolated.0);
    assert_eq!(desktop.next_deadline_us(), Some(isolated));
    desktop.stop().unwrap();
    assert_eq!(desktop.next_deadline_us(), None);
}
