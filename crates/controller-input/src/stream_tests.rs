use super::*;
use crate::registry::{Kind, Registry};
use crate::tests::fixture;

#[test]
fn committed_reports_and_losses_wake_routing_but_speculative_clones_do_not() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let mut registry = Registry::default();
    let id = registry.connect(fixture("wake", None)).unwrap();
    let notifications = Arc::new(AtomicUsize::new(0));
    let counted = notifications.clone();
    let _watch = registry
        .watch_reports(
            &[id.clone()],
            Arc::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap();
    registry
        .report(&id, 1, &BTreeMap::from([(0, 1), (1, 10)]))
        .unwrap();
    assert_eq!(
        notifications.load(Ordering::SeqCst),
        0,
        "Partial baselines cannot drive routing."
    );
    registry.report(&id, 2, &BTreeMap::from([(2, 1)])).unwrap();
    assert_eq!(notifications.load(Ordering::SeqCst), 1);
    registry
        .report(&id, 1, &BTreeMap::from([(0, 0), (1, 20)]))
        .unwrap();
    assert_eq!(notifications.load(Ordering::SeqCst), 2);
    let page = registry.stream_page(&registry.session, 0);
    assert_eq!(
        page.events.len(),
        2,
        "The awakened receiver can drain every committed frame."
    );
    let mut staged = registry.clone();
    staged.disconnect(&id);
    assert_eq!(
        notifications.load(Ordering::SeqCst),
        2,
        "Database staging is not source admission."
    );
    registry.disconnect(&id);
    registry.fault(&id, "capture loss");
    assert_eq!(notifications.load(Ordering::SeqCst), 4);
}

#[test]
fn clock_correlation_reads_never_renew_a_report_or_advance_its_cursor() {
    let mut registry = Registry::default();
    let id = registry.connect(fixture("clock", None)).unwrap();
    registry
        .report(&id, 1, &BTreeMap::from([(0, 1), (1, 10)]))
        .unwrap();
    registry.report(&id, 2, &BTreeMap::from([(2, 1)])).unwrap();
    let capture = registry.stream_page(&registry.session, 0);
    let first = registry.list(0)["captureClock"].clone();
    let second = registry.list(0)["captureClock"].clone();
    assert_eq!(first["id"], format!("controllers:{}", registry.session));
    let now = second["nowUs"].as_str().unwrap().parse::<u64>().unwrap();
    assert!(now >= first["nowUs"].as_str().unwrap().parse::<u64>().unwrap());
    assert!(now >= capture.events.last().unwrap().captured_us);
    assert_eq!(
        registry
            .stream_page(&registry.session, capture.cursor)
            .events
            .len(),
        0
    );
    assert_eq!(
        registry
            .stream_page(&registry.session, 0)
            .events
            .last()
            .unwrap()
            .captured_us,
        capture.events.last().unwrap().captured_us
    );
}

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
        native_read_us: None,
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
    let selected = id.clone();
    registry.report(&id, 1, &BTreeMap::from([(1, 0)])).unwrap();
    registry.report(&id, 2, &BTreeMap::from([(2, 0)])).unwrap();
    drop(registry);
    let _watch = service
        .watch_reports(&[selected.clone()], Arc::new(|| {}))
        .unwrap();
    let snapshot = service.stream_snapshot(&[id]).unwrap();
    assert!(!service.stream_page(&snapshot.session, snapshot.cursor).gap);
    service.shutdown();
    assert!(service.stream_snapshot(&["source".into()]).is_err());
    assert!(service.watch_reports(&[selected], Arc::new(|| {})).is_err());
}

fn event(source: &str, reason: Reason) -> Event {
    Event {
        cursor: 0,
        source: source.into(),
        generation: 1,
        sequence: 1,
        captured_us: 0,
        native_read_us: None,
        reason,
        values: BTreeMap::new(),
    }
}
fn counter() -> (
    Arc<std::sync::atomic::AtomicUsize>,
    Arc<dyn Fn() + Send + Sync>,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    (
        count,
        Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }),
    )
}
fn count(value: &std::sync::atomic::AtomicUsize) -> usize {
    value.load(Ordering::SeqCst)
}

#[test]
fn watch_selection_and_overlapping_leases_are_independent_and_drop_revokes() {
    let mut journal = Journal::default();
    let (first, first_wake) = counter();
    let (second, second_wake) = counter();
    let (both, both_wake) = counter();
    let first_lease = journal.watch(&["a".into()], first_wake).unwrap();
    let second_lease = journal.watch(&["a".into()], second_wake).unwrap();
    let both_lease = journal.watch(&["a".into(), "b".into()], both_wake).unwrap();
    journal.publish(event("c", Reason::Baseline));
    assert_eq!((count(&first), count(&second), count(&both)), (0, 0, 0));
    for reason in [
        Reason::Baseline,
        Reason::Change,
        Reason::Gap,
        Reason::Disconnect,
    ] {
        journal.publish(event("a", reason));
    }
    assert_eq!((count(&first), count(&second), count(&both)), (4, 4, 4));
    drop(first_lease);
    journal.publish(event("b", Reason::Change));
    journal.publish(event("a", Reason::Change));
    assert_eq!((count(&first), count(&second), count(&both)), (4, 5, 6));
    drop(second_lease);
    drop(both_lease);
    journal.publish(event("a", Reason::Change));
    assert!(
        journal.watches.is_empty(),
        "Publication cleans expired weak leases."
    );
}

#[test]
fn unrelated_evictions_wake_only_watchers_whose_unread_global_history_was_lost() {
    let mut journal = Journal::default();
    let (unread, unread_wake) = counter();
    let (read, read_wake) = counter();
    let unread_lease = journal.watch(&["selected".into()], unread_wake).unwrap();
    let read_lease = journal.watch(&["selected".into()], read_wake).unwrap();
    for _ in 0..REPORT_LIMIT {
        journal.publish(event("noise", Reason::Change));
    }
    assert_eq!((count(&unread), count(&read)), (0, 0));
    read_lease.advance(journal.cursor());
    read_lease.advance(0); // A delayed older page cannot undo acknowledgement.
    journal.publish(event("noise", Reason::Change));
    assert_eq!((count(&unread), count(&read)), (1, 0));
    assert!(journal.page("session", "session", 0).gap);
    assert!(!journal.page("session", "session", REPORT_LIMIT as u64).gap);
    unread_lease.advance(journal.cursor());
    journal.publish(event("noise", Reason::Change));
    assert_eq!((count(&unread), count(&read)), (1, 0));
    // An acknowledgement never suppresses selected data.
    journal.publish(event("selected", Reason::Gap));
    assert_eq!((count(&unread), count(&read)), (2, 1));
}

#[test]
fn selected_report_and_unread_eviction_notify_once_per_publication() {
    let mut journal = Journal::default();
    let (notifications, wake) = counter();
    let _lease = journal.watch(&["selected".into()], wake).unwrap();
    for _ in 0..REPORT_LIMIT {
        journal.publish(event("noise", Reason::Change));
    }
    journal.publish(event("selected", Reason::Gap));
    assert_eq!(count(&notifications), 1);
    assert!(journal.page("session", "session", 0).gap);
}

#[test]
fn awakened_owner_reads_committed_report_and_lease_drop_never_takes_registry_lock() {
    use std::{
        sync::{mpsc, Mutex},
        time::Duration,
    };
    let mut registry = Registry::default();
    let id = registry.connect(fixture("commit-boundary", None)).unwrap();
    let (notifications, wake) = mpsc::channel();
    let lease = registry
        .watch_reports(
            &[id.clone()],
            Arc::new(move || {
                let _ = notifications.send(());
            }),
        )
        .unwrap();
    let registry = Arc::new(Mutex::new(registry));
    let owner = {
        let registry = registry.clone();
        std::thread::spawn(move || {
            wake.recv_timeout(Duration::from_secs(2)).unwrap();
            let registry = registry.lock().unwrap();
            registry.stream_page(&registry.session, 0)
        })
    };
    {
        let mut registry = registry.lock().unwrap();
        registry
            .report(&id, 1, &BTreeMap::from([(0, 1), (1, 20)]))
            .unwrap();
        registry.report(&id, 2, &BTreeMap::from([(2, 1)])).unwrap();
    }
    let page = owner.join().unwrap();
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].reason, Reason::Baseline);
    assert_eq!(page.events[0].cursor, page.cursor);
    lease.advance(page.cursor);
    let guard = registry.lock().unwrap();
    let (dropped, done) = mpsc::channel();
    let dropper = std::thread::spawn(move || {
        drop(lease);
        dropped.send(()).unwrap();
    });
    let without_lock = done.recv_timeout(Duration::from_secs(2)).is_ok();
    drop(guard);
    dropper.join().unwrap();
    assert!(
        without_lock,
        "Lease disposal waited for the registry mutex."
    );
}

#[test]
fn watch_starts_at_current_boundary_and_exhaustion_wakes_every_selection() {
    let mut journal = Journal::default();
    for _ in 0..REPORT_LIMIT {
        journal.publish(event("old", Reason::Change));
    }
    let (notifications, wake) = counter();
    let lease = journal.watch(&["selected".into()], wake).unwrap();
    journal.publish(event("noise", Reason::Change));
    assert_eq!(
        count(&notifications),
        0,
        "Pre-registration history was never unread by this lease."
    );
    journal.cursor = u64::MAX - 1;
    journal.publish(event("noise", Reason::Change));
    assert_eq!(
        count(&notifications),
        1,
        "Reaching the permanent gap cursor wakes every selection."
    );
    assert!(journal.publish(event("noise", Reason::Change)).is_none());
    assert_eq!(count(&notifications), 2);
    assert!(journal.events.is_empty());
    drop(lease);
}

#[test]
fn watch_registration_checks_known_distinct_bounded_sources_and_live_lease_budget() {
    let mut registry = Registry::default();
    let ids = (0..17)
        .map(|index| {
            registry
                .connect(fixture(&format!("watch{index}"), None))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let no_op: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    for invalid in [
        vec![],
        vec![ids[0].clone(), ids[0].clone()],
        vec!["missing".into()],
        ids.clone(),
    ] {
        assert!(registry.watch_reports(&invalid, no_op.clone()).is_err());
    }
    registry.disconnect(&ids[0]);
    let retained = registry.watch_reports(&ids[..16], no_op.clone()).unwrap();
    let mut leases = Vec::new();
    for _ in 1..REPORT_WATCH_LIMIT {
        leases.push(registry.watch_reports(&ids[..1], no_op.clone()).unwrap());
    }
    assert!(registry.watch_reports(&ids[..1], no_op.clone()).is_err());
    drop(retained);
    let replacement = registry.watch_reports(&ids[..1], no_op.clone()).unwrap();
    drop(replacement);
    drop(leases);
    let _next = (0..REPORT_WATCH_LIMIT)
        .map(|_| registry.watch_reports(&ids[..1], no_op.clone()).unwrap())
        .collect::<Vec<_>>();
}
