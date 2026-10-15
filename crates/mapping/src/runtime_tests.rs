use super::*;
use crate::{
    engine::{Reason, ReportReason},
    fixtures::*,
};
#[test]
fn reserved_owner_matches_the_armed_run_without_activating_early() {
    assert!(Runtime::owned_session("../invalid").is_err());
    let owner = uuid::Uuid::new_v4().to_string();
    let mut runtime = Runtime::owned_session(&owner).unwrap();
    let reserved = runtime.next_token().unwrap();
    assert_eq!(reserved.session, owner);
    assert!(runtime.token().is_err());
    assert_eq!(runtime.start(profile(), assignments()).unwrap(), reserved);
    runtime.stop();
    assert_ne!(runtime.next_token().unwrap(), reserved);
}
#[test]
fn saving_and_failed_replacement_do_not_activate_or_disrupt_a_run() {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::store::Store::new(temp.path().into());
    let mut runtime = Runtime::default();
    assert!(runtime.engine().is_none());
    let saved = store.import(&profile().export().unwrap()).unwrap();
    assert!(runtime.engine().is_none());
    assert_eq!(
        runtime.start(saved, assignments()).unwrap().generation,
        Counter(1)
    );
    runtime.accept(baseline("left", 1, false)).unwrap();
    runtime.accept(baseline("right", 1, true)).unwrap();
    let token = runtime.engine().unwrap().token("left-trigger").unwrap();
    let cursor = runtime.recording.cursor();
    let mut invalid = profile();
    invalid.schema = 2;
    assert!(runtime.start(invalid, assignments()).is_err());
    assert_eq!(runtime.recording.cursor(), cursor);
    assert!(runtime.engine().unwrap().ready());
    store.save(&profile()).unwrap();
    assert_eq!(runtime.recording.cursor(), cursor);
    assert!(runtime.engine().unwrap().ready());
    runtime.start(profile(), assignments()).unwrap();
    let page = runtime.recording.page(cursor, 32).unwrap();
    assert_eq!(page.records.len(), 4);
    assert_eq!(page.records[0].report.reason, ReportReason::Stop);
    assert_eq!(page.records[2].report.run_generation, Counter(2));
    assert!(!runtime.engine().unwrap().ready());
    assert!(runtime
        .schedule(token, "stale".into(), Counter(1), Value::Button(true))
        .is_err());
    runtime.accept(baseline("left", 1, true)).unwrap();
    runtime.accept(baseline("right", 1, false)).unwrap();
    assert_eq!(
        runtime.engine().unwrap().contributions()["left-trigger"],
        Value::Button(false)
    );
    runtime
        .accept(frame("left", 1, 2, 10, Reason::Disconnect, vec![]))
        .unwrap();
    runtime.advance(Counter(100)).unwrap();
    runtime.stop();
    assert!(!runtime.engine().unwrap().active());
}
#[test]
fn recording_pages_are_bounded_and_mark_lost_history() {
    let mut sink = RecordingSink::default();
    let mut e = engine();
    let record = e.drain().remove(0);
    sink.record(vec![record; RECORDING_LIMIT + 2]);
    assert!(sink.page(Counter(0), 0).is_err());
    assert!(sink.page(Counter(0), 65).is_err());
    assert!(sink.page(Counter(u64::MAX), 1).is_err());
    let first = sink.page(Counter(0), 32).unwrap();
    assert!(first.gap);
    assert_eq!(first.dropped, Counter(2));
    assert_eq!(first.records.len(), 32);
    assert_eq!(first.next, Counter(34));
    let page = sink.page(first.next, 32).unwrap();
    assert!(!page.gap);
    assert_eq!(page.records[0].cursor, Counter(35));
    assert!(sink.page(sink.cursor(), 1).unwrap().records.is_empty());
    let mut runtime = Runtime::default();
    assert!(runtime.advance(Counter(1)).is_err());
    assert!(runtime.accept(baseline("left", 1, false)).is_err());
    runtime.generation = u64::MAX;
    assert!(runtime.start(profile(), assignments()).is_err());
}
#[test]
fn workers_and_timers_cannot_cross_session_or_run_boundaries() {
    let mut old = Runtime::default();
    let old_run = old.start(profile(), assignments()).unwrap();
    old.accept_for(&old_run, baseline("left", 1, false))
        .unwrap();
    old.accept_for(&old_run, baseline("right", 1, false))
        .unwrap();
    let old_producer = old.engine().unwrap().token("left-trigger").unwrap();
    let mut current = Runtime::default();
    let current_run = current.start(profile(), assignments()).unwrap();
    assert_eq!(old_run.generation, current_run.generation);
    assert_ne!(old_run.session, current_run.session);
    current
        .accept_for(&current_run, baseline("left", 1, false))
        .unwrap();
    current
        .accept_for(&current_run, baseline("right", 1, false))
        .unwrap();
    assert!(current
        .accept_for(&old_run, baseline("left", 2, false))
        .is_err());
    assert!(current.advance_for(&old_run, Counter(10)).is_err());
    assert!(current
        .schedule(
            old_producer,
            "late".into(),
            Counter(10),
            Value::Button(true)
        )
        .is_err());
    let next = current.start(profile(), assignments()).unwrap();
    assert_eq!(next.session, current_run.session);
    assert_ne!(next.generation, current_run.generation);
    assert!(current
        .accept_for(&current_run, baseline("left", 2, false))
        .is_err());
    current.advance_for(&next, Counter(20)).unwrap();
    let mut zero_time = baseline("left", 1, false);
    zero_time.captured_us = Counter(0);
    assert!(current.accept_for(&next, zero_time).is_err());
    current.stop();
    assert!(current.token().is_err());
    assert!(current
        .accept_for(&next, baseline("left", 1, false))
        .is_err());
}
