use super::*;

#[test]
fn native_helper_requires_an_installed_wake_without_creating_a_subscription() {
    let mut scopes = Scopes::default();
    assert!(scopes
        .bind(1, &Scope::DesktopWide)
        .unwrap_err()
        .contains("wake"));
    assert!(scopes.watches.is_empty());
    assert!(scopes.evidence(1).unwrap_err().contains("not bound"));
    scopes.retire(1);
}

#[test]
fn registration_validation_is_nonzero_unique_and_bounded_before_native_work() {
    assert!(validate_registration(0, false, 0)
        .unwrap_err()
        .contains("nonzero"));
    assert!(validate_registration(1, true, 1)
        .unwrap_err()
        .contains("already bound"));
    assert!(validate_registration(65, false, 64)
        .unwrap_err()
        .contains("limit"));
    assert!(validate_registration(1, false, 63).is_ok());
    let mut scopes = Scopes::default();
    assert!(scopes
        .bind(0, &Scope::DesktopWide)
        .unwrap_err()
        .contains("nonzero"));
    assert!(scopes.watches.is_empty());
}

#[test]
fn transient_snapshot_errors_are_ineligible_and_retain_the_loss_epoch() {
    let snapshot = ScopeSnapshot {
        watch_id: 7,
        sequence: 9,
        loss_epoch: 4,
        eligible: true,
        error: Some("Foreground identity unavailable.".into()),
    };
    let unavailable = evidence(snapshot.clone());
    assert_eq!(unavailable.loss_epoch, 4);
    assert!(!unavailable.eligible);
    let available = evidence(ScopeSnapshot {
        error: None,
        ..snapshot
    });
    assert!(available.eligible);
    assert_eq!(available.loss_epoch, 4);
}

#[test]
fn real_native_watches_bind_once_and_retire_without_native_input() {
    let mut scopes = Scopes::default();
    scopes.set_wake(Arc::new(|| {}));
    scopes.bind(11, &Scope::DesktopWide).unwrap();
    scopes.bind(12, &Scope::DesktopWide).unwrap();
    let first = scopes.watches[&11].id();
    assert_ne!(first, scopes.watches[&12].id());
    assert!(scopes.evidence(11).is_ok());
    assert!(scopes.evidence(12).is_ok());
    assert!(scopes
        .bind(11, &Scope::DesktopWide)
        .unwrap_err()
        .contains("already bound"));
    assert_eq!(scopes.watches.len(), 2);
    scopes.retire(11);
    assert!(scopes.evidence(11).unwrap_err().contains("not bound"));
    assert!(scopes.evidence(12).is_ok());
    // Reuse cannot inherit the retired watch's notification generation. The
    // still-live second lease also avoids restarting the shared native owner.
    scopes.bind(11, &Scope::DesktopWide).unwrap();
    assert_ne!(first, scopes.watches[&11].id());
    scopes.retire(11);
    scopes.retire(12);
    scopes.retire(12);
    assert!(scopes.watches.is_empty());
}
