use super::{
    state::{Observation, Shared, WATCH_LIMIT},
    *,
};
use crate::ProcessIdentity;

fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        created: "123".into(),
        executable: format!("C:\\Target{pid}.exe"),
    }
}
fn observed(pid: u32) -> Observation {
    Observation {
        foreground: Some(identity(pid)),
        error: None,
    }
}
fn application(pid: u32) -> Scope {
    Scope::Application {
        identity: identity(pid),
    }
}
fn subscriber(shared: &Shared, id: u64, scope: Scope) -> Arc<Subscriber> {
    shared.subscribe(id, scope, Arc::new(|_| {})).unwrap()
}

#[test]
fn initial_snapshot_is_required_and_exact_process_identity_is_preserved() {
    let shared = Shared::new();
    assert!(shared
        .subscribe(1, Scope::DesktopWide, Arc::new(|_| {}))
        .is_err());
    shared.observe(observed(42), false);
    let watch = subscriber(&shared, 1, application(42));
    assert_eq!(
        watch.snapshot().unwrap(),
        ScopeSnapshot {
            watch_id: 1,
            sequence: 1,
            loss_epoch: 0,
            eligible: true,
            error: None
        }
    );
    let mut replacement = identity(42);
    replacement.created = "124".into();
    shared.observe(
        Observation {
            foreground: Some(replacement),
            error: None,
        },
        false,
    );
    assert!(!watch.snapshot().unwrap().eligible);
    assert_eq!(watch.snapshot().unwrap().loss_epoch, 1);
}

#[test]
fn away_and_return_between_reads_retains_loss_epoch_and_ordered_callbacks() {
    let shared = Shared::new();
    shared.observe(observed(42), false);
    let callbacks = Arc::new(Mutex::new(Vec::new()));
    let output = callbacks.clone();
    let watch = shared
        .subscribe(
            9,
            application(42),
            Arc::new(move |snapshot| {
                output.lock().unwrap().push(snapshot);
            }),
        )
        .unwrap();
    shared.observe(observed(43), false);
    shared.observe(observed(42), false);
    let final_state = watch.snapshot().unwrap();
    assert!(final_state.eligible);
    assert_eq!((final_state.sequence, final_state.loss_epoch), (3, 1));
    let recorded = callbacks.lock().unwrap();
    assert_eq!(
        recorded
            .iter()
            .map(|state| (state.sequence, state.eligible, state.loss_epoch))
            .collect::<Vec<_>>(),
        vec![(2, false, 1), (3, true, 1)]
    );
}

#[test]
fn same_process_windows_and_desktop_wide_application_switches_keep_held_scope() {
    let shared = Shared::new();
    shared.observe(observed(42), false);
    let application = subscriber(&shared, 1, application(42));
    let global = subscriber(&shared, 2, Scope::DesktopWide);
    shared.observe(observed(42), false);
    assert_eq!(application.snapshot().unwrap().sequence, 1);
    shared.observe(observed(43), false);
    assert_eq!(global.snapshot().unwrap().sequence, 1);
    assert!(global.snapshot().unwrap().eligible);
    assert!(!application.snapshot().unwrap().eligible);
}

#[test]
fn unknown_foreground_and_transient_inspection_error_pause_then_recover() {
    let shared = Shared::new();
    shared.observe(observed(42), false);
    let watch = subscriber(&shared, 1, Scope::DesktopWide);
    shared.observe(Observation::unavailable(), false);
    assert!(!watch.snapshot().unwrap().eligible);
    assert!(watch.snapshot().unwrap().error.is_none());
    shared.observe(Observation::failed("Identity unavailable".into()), false);
    assert_eq!(
        watch.snapshot().unwrap().error.as_deref(),
        Some("Identity unavailable")
    );
    shared.observe(observed(42), false);
    assert!(watch.snapshot().unwrap().eligible);
    assert!(watch.snapshot().unwrap().error.is_none());
    assert_eq!(watch.snapshot().unwrap().loss_epoch, 2);
}

#[test]
fn every_desktop_switch_and_session_lock_retains_a_boundary_even_if_already_away() {
    let shared = Shared::new();
    shared.observe(observed(42), false);
    let watch = subscriber(&shared, 1, application(42));
    shared.observe(observed(42), true); // desktop already returned by inspection
    assert_eq!(watch.snapshot().unwrap().loss_epoch, 1);
    assert!(watch.snapshot().unwrap().eligible);
    shared.observe(Observation::unavailable(), true); // lock
    shared.observe(Observation::unavailable(), true); // disconnect while locked
    shared.observe(observed(42), false); // unlock/desktop ready
    assert_eq!(watch.snapshot().unwrap().loss_epoch, 3);
    assert!(watch.snapshot().unwrap().eligible);
}

#[test]
fn terminal_overflow_and_callback_failure_cannot_recover_from_later_observations() {
    let shared = Shared::new();
    shared.observe(observed(42), false);
    let watch = subscriber(&shared, 1, Scope::DesktopWide);
    shared.fail("Desktop scope notification queue overflow.".into());
    shared.observe(observed(42), false);
    assert!(watch.snapshot().unwrap_err().contains("overflow"));
    assert!(shared
        .subscribe(2, Scope::DesktopWide, Arc::new(|_| {}))
        .is_err());
    let panic_shared = Shared::new();
    panic_shared.observe(observed(42), false);
    let broken = panic_shared
        .subscribe(1, Scope::DesktopWide, Arc::new(|_| panic!("broken wake")))
        .unwrap();
    panic_shared.observe(Observation::unavailable(), false);
    assert!(broken.snapshot().unwrap_err().contains("callback panicked"));
}

#[test]
fn callbacks_run_without_locks_and_retired_subscribers_receive_no_more_updates() {
    let shared = Arc::new(Shared::new());
    shared.observe(observed(42), false);
    let observed_shared = Arc::downgrade(&shared);
    let calls = Arc::new(AtomicU64::new(0));
    let output = calls.clone();
    let watch = shared
        .subscribe(
            1,
            Scope::DesktopWide,
            Arc::new(move |_| {
                assert!(observed_shared.upgrade().unwrap().terminal().is_none());
                output.fetch_add(1, Ordering::AcqRel);
            }),
        )
        .unwrap();
    shared.observe(Observation::unavailable(), false);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    shared.unsubscribe(&watch);
    shared.observe(observed(42), false);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert!(watch.snapshot().unwrap_err().contains("retired"));
}

#[test]
fn scoped_lease_bound_releases_capacity_after_drop() {
    let shared = Shared::new();
    shared.observe(observed(42), false);
    let mut leases = (0..WATCH_LIMIT)
        .map(|id| subscriber(&shared, id as u64, Scope::DesktopWide))
        .collect::<Vec<_>>();
    assert!(shared
        .subscribe(1000, Scope::DesktopWide, Arc::new(|_| {}))
        .err()
        .unwrap()
        .contains("limit"));
    leases.pop();
    assert!(shared
        .subscribe(1001, Scope::DesktopWide, Arc::new(|_| {}))
        .is_ok());
}

#[test]
fn native_no_input_leases_share_owner_and_have_distinct_watch_generations() {
    let first = ScopeWatch::open(Scope::DesktopWide, Arc::new(|_| {})).unwrap();
    let second = ScopeWatch::open(Scope::DesktopWide, Arc::new(|_| {})).unwrap();
    assert!(Arc::ptr_eq(&first.owner, &second.owner));
    assert_ne!(first.id(), second.id());
    assert_eq!(first.snapshot().unwrap().watch_id, first.id());
    assert_eq!(second.snapshot().unwrap().watch_id, second.id());
    drop(first);
    assert!(second.snapshot().is_ok());
}
