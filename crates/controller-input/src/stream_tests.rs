use super::*;
use crate::registry::{Kind, Registry};
use crate::tests::fixture;
#[test]
fn journal_preserves_atomic_edges_and_independent_cursors() {
    let mut registry = Registry::default();
    let id = registry.connect(fixture("source", None)).unwrap();
    assert!(registry.stream_snapshot(&[id.clone()]).is_err());
    registry.report(&id, 1, &BTreeMap::from([(1, 0)])).unwrap();
    registry.report(&id, 2, &BTreeMap::from([(2, 0)])).unwrap();
    let snapshot = registry.stream_snapshot(&[id.clone()]).unwrap();
    assert_eq!(snapshot.sources[0].baseline.reason, Reason::Baseline);
    assert_eq!(snapshot.sources[0].controls[0].kind, Kind::Button);
    assert!(snapshot.sources[0].baseline.captured_us > 0);
    registry
        .report(&id, 1, &BTreeMap::from([(0, 1), (1, 0)]))
        .unwrap();
    registry.report(&id, 1, &BTreeMap::from([(1, 0)])).unwrap();
    let page = registry.stream_page(&snapshot.session, snapshot.cursor);
    assert!(!page.gap);
    assert_eq!(page.events.len(), 2);
    let control = &snapshot.sources[0].controls[0].id;
    assert_eq!(page.events[0].values[control].normalized, Some(1));
    assert_eq!(page.events[1].values[control].normalized, Some(0));
    assert_eq!(page.events[1].sequence, page.events[0].sequence + 1);
    assert_eq!(
        registry
            .stream_page(&snapshot.session, snapshot.cursor)
            .events
            .len(),
        2
    );
    assert!(registry
        .stream_page(&snapshot.session, page.cursor)
        .events
        .is_empty());
    registry.fault(&id, "lost report");
    assert!(registry.stream_snapshot(&[id.clone()]).is_err());
    registry.disconnect(&id);
    let loss = registry.stream_page(&snapshot.session, page.cursor);
    assert_eq!(loss.events[0].reason, Reason::Gap);
    assert_eq!(loss.events[1].reason, Reason::Disconnect);
    assert!(loss.events.iter().all(|e| e.values.is_empty()));
    assert!(registry.stream_page("old-process", page.cursor).gap);
    assert!(registry.stream_snapshot(&[]).is_err());
    assert!(registry.stream_snapshot(&[id.clone(), id]).is_err());
}
#[test]
fn overflow_reports_a_gap_instead_of_skipping_a_release() {
    let mut journal = Journal::default();
    let event = Event {
        cursor: 0,
        source: "source".into(),
        generation: 1,
        sequence: 1,
        captured_us: 0,
        reason: Reason::Change,
        values: BTreeMap::new(),
    };
    for _ in 0..REPORT_LIMIT + 1 {
        journal.publish(event.clone());
    }
    assert_eq!(journal.events.len(), REPORT_LIMIT);
    let gap = journal.page("current", "current", 0);
    assert!(gap.gap);
    assert!(gap.events.is_empty());
    let page = journal.page("current", "current", 1);
    assert!(!page.gap);
    assert_eq!(page.events.len(), REPORT_LIMIT);
    assert!(journal.page("current", "current", journal.cursor + 1).gap);
    journal.cursor = u64::MAX;
    journal.publish(event);
    assert!(journal.page("current", "current", u64::MAX).gap);
    assert!(journal.events.is_empty());
}
#[test]
fn service_stream_does_not_require_ui_polling_and_shutdown_blocks_new_baselines() {
    let service = crate::ControllerService::new(None, false);
    assert!(service.stream_snapshot(&["missing".into()]).is_err());
    let mut registry = service.registry.lock().unwrap();
    let id = registry.connect(fixture("source", None)).unwrap();
    registry.report(&id, 1, &BTreeMap::from([(1, 0)])).unwrap();
    registry.report(&id, 2, &BTreeMap::from([(2, 0)])).unwrap();
    drop(registry);
    let snapshot = service.stream_snapshot(&[id]).unwrap();
    assert!(!service.stream_page(&snapshot.session, snapshot.cursor).gap);
    service.shutdown();
    assert!(service.stream_snapshot(&["source".into()]).is_err());
}
