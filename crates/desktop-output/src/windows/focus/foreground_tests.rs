use super::*;
use crate::windows::focus::state::{Observation, Shared};
use std::sync::{Arc, Mutex};

fn identity(pid: u32, executable: &str) -> ProcessIdentity {
    ProcessIdentity {
        executable: executable.into(),
        pid,
        created: format!("created-{pid}"),
    }
}

#[test]
fn foreground_watch_starts_from_cached_observation_and_orders_identity_updates() {
    let shared = Shared::new();
    let first = identity(10, "C:\\Games\\first.exe");
    shared.observe(
        Observation {
            foreground: Some(first.clone()),
            error: None,
        },
        false,
    );
    let seen: Arc<Mutex<Vec<ForegroundSnapshot>>> = Arc::new(Mutex::new(Vec::new()));
    let captured = seen.clone();
    let watch = shared
        .subscribe_foreground(
            1,
            Arc::new(move |snapshot| captured.lock().unwrap().push(snapshot)),
        )
        .unwrap();
    assert_eq!(watch.snapshot().unwrap().sequence, 1);
    assert_eq!(watch.snapshot().unwrap().identity, Some(first));
    assert!(!watch.snapshot().unwrap().terminal);
    let second = identity(11, "C:\\Games\\second.exe");
    shared.observe(
        Observation {
            foreground: Some(second.clone()),
            error: None,
        },
        false,
    );
    shared.observe(Observation::unavailable(), false);
    let events = seen.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(events[0].identity, Some(second));
    assert_eq!(events[1].identity, None);
}

#[test]
fn foreground_watch_carries_inspection_errors_and_terminal_failure() {
    let shared = Shared::new();
    shared.observe(Observation::unavailable(), false);
    let seen: Arc<Mutex<Vec<ForegroundSnapshot>>> = Arc::new(Mutex::new(Vec::new()));
    let captured = seen.clone();
    let watch = shared
        .subscribe_foreground(
            2,
            Arc::new(move |snapshot| captured.lock().unwrap().push(snapshot)),
        )
        .unwrap();
    shared.observe(Observation::failed("inspection failed".into()), false);
    assert_eq!(watch.snapshot().unwrap().sequence, 2);
    assert_eq!(
        watch.snapshot().unwrap().error.as_deref(),
        Some("inspection failed")
    );
    shared.fail("native observer failed".into());
    let terminal = watch.snapshot().unwrap();
    assert_eq!(terminal.sequence, 3);
    assert_eq!(terminal.identity, None);
    assert_eq!(terminal.error.as_deref(), Some("native observer failed"));
    assert!(terminal.terminal);
    assert_eq!(
        seen.lock()
            .unwrap()
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
}

#[test]
fn foreground_subscription_is_bounded_and_retires() {
    let shared = Shared::new();
    shared.observe(Observation::unavailable(), false);
    let watches = (0..super::super::state::WATCH_LIMIT)
        .map(|id| {
            shared
                .subscribe_foreground(id as u64 + 1, Arc::new(|_| {}))
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(shared
        .subscribe_foreground(1000, Arc::new(|_| {}))
        .err()
        .unwrap()
        .contains("limit"));
    shared.unsubscribe_foreground(&watches[0]);
    assert!(watches[0].snapshot().unwrap_err().contains("retired"));
    assert!(shared.subscribe_foreground(1000, Arc::new(|_| {})).is_ok());
}

#[test]
fn scope_revocation_dispatch_precedes_foreground_callback_and_panic_is_isolated() {
    use crate::Scope;
    let shared = Shared::new();
    shared.observe(Observation::unavailable(), false);
    let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let scope_order = order.clone();
    let scope = shared
        .subscribe(
            10,
            Scope::DesktopWide,
            Arc::new(move |_| scope_order.lock().unwrap().push("scope")),
        )
        .unwrap();
    let foreground_order = order.clone();
    let foreground = shared
        .subscribe_foreground(
            11,
            Arc::new(move |_| foreground_order.lock().unwrap().push("foreground")),
        )
        .unwrap();
    let app = identity(25, "C:\\Games\\game.exe");
    shared.observe(
        Observation {
            foreground: Some(app.clone()),
            error: None,
        },
        false,
    );
    assert_eq!(*order.lock().unwrap(), vec!["scope", "foreground"]);

    shared.unsubscribe_foreground(&foreground);
    shared.unsubscribe(&scope);
    let scope = shared
        .subscribe(12, Scope::DesktopWide, Arc::new(|_| {}))
        .unwrap();
    let panic_watch = shared
        .subscribe_foreground(13, Arc::new(|_| panic!("optional chooser callback")))
        .unwrap();
    shared.observe(Observation::unavailable(), false);
    assert!(
        scope.snapshot().is_ok(),
        "foreground callback panic must not poison scope observation"
    );
    shared.unsubscribe_foreground(&panic_watch);
}

#[test]
fn foreground_and_scope_watches_share_the_native_owner_lifecycle() {
    use crate::Scope;
    let scope = super::super::ScopeWatch::open(Scope::DesktopWide, Arc::new(|_| {})).unwrap();
    let foreground = super::super::ForegroundWatch::open(Arc::new(|_| {})).unwrap();
    assert!(scope.snapshot().is_ok());
    assert!(foreground.snapshot().is_ok());
    drop(foreground);
    assert!(
        scope.snapshot().is_ok(),
        "retiring the chooser must leave scope revocation active"
    );
    drop(scope);
}

#[test]
fn foreground_sequence_exhaustion_is_diagnostic_without_failing_scope_watchers() {
    use crate::Scope;
    let shared = Shared::new();
    shared.observe(Observation::unavailable(), false);
    let scope = shared
        .subscribe(31, Scope::DesktopWide, Arc::new(|_| {}))
        .unwrap();
    let foreground = shared.subscribe_foreground(32, Arc::new(|_| {})).unwrap();
    foreground.lock().snapshot.sequence = u64::MAX;
    shared.observe(
        Observation {
            foreground: Some(identity(32, "C:\\Games\\game.exe")),
            error: None,
        },
        false,
    );
    assert!(shared.terminal().is_none());
    assert!(scope.snapshot().is_ok());
    let snapshot = foreground.snapshot().unwrap();
    assert_eq!(snapshot.sequence, u64::MAX);
    assert!(snapshot
        .error
        .as_deref()
        .unwrap()
        .contains("sequence exhausted"));
}
