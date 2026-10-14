use super::*;
use crate::{
    engine::{Reason, ReportReason},
    fixtures::*,
};
#[test]
fn saving_and_failed_replacement_do_not_activate_or_disrupt_a_run() {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::store::Store::new(temp.path().into());
    let mut runtime = Runtime::default();
    assert!(runtime.engine().is_none());
    let saved = store.import(&profile().export().unwrap()).unwrap();
    assert!(runtime.engine().is_none());
    assert_eq!(runtime.start(saved, assignments()).unwrap(), Counter(1));
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
    let page = runtime.recording.page(cursor, 64).unwrap();
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
    let page = sink.page(first.next, 64).unwrap();
    assert!(!page.gap);
    assert_eq!(page.records[0].cursor, Counter(35));
    assert!(sink.page(sink.cursor(), 1).unwrap().records.is_empty());
    let mut runtime = Runtime::default();
    assert!(runtime.advance(Counter(1)).is_err());
    assert!(runtime.accept(baseline("left", 1, false)).is_err());
    runtime.generation = u64::MAX;
    assert!(runtime.start(profile(), assignments()).is_err());
}
