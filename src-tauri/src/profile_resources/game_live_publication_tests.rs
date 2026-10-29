use super::*;
use std::{sync::{mpsc, atomic::AtomicBool}, time::{Duration, Instant}};

#[test]
fn live_reader_validation_never_waits_for_inventory_adoption() {
    let runtime = runtime();
    runtime.publish_profile_game_delivery(delivery("dcs", "reader", 1)).unwrap();
    let guard = runtime.state.lock().unwrap();
    let began = Instant::now();
    assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "reader").unwrap(), None);
    assert!(began.elapsed() < Duration::from_millis(100));
    drop(guard);
    assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "reader").unwrap(), Some(()));
}

#[test]
fn live_updates_cannot_claim_replace_or_resurrect_a_reader() {
    let runtime = runtime();
    assert!(!runtime.publish_current_profile_game_delivery(delivery("dcs", "old", 1), || true).unwrap());
    runtime.publish_profile_game_delivery(delivery("dcs", "old", 1)).unwrap();
    assert!(runtime.publish_current_profile_game_delivery(delivery("dcs", "old", 2), || true).unwrap());
    assert!(!runtime.publish_current_profile_game_delivery(delivery("dcs", "old", 3), || false).unwrap());
    assert_eq!(runtime.state.lock().unwrap().game_deliveries["dcs"].revision, 2);
    runtime.publish_profile_game_delivery(delivery("dcs", "incoming", 1)).unwrap();
    assert!(!runtime.publish_current_profile_game_delivery(delivery("dcs", "old", 4), || true).unwrap());
    assert_eq!(runtime.state.lock().unwrap().game_deliveries["dcs"].session, "incoming");
    runtime.withdraw_profile_game_delivery("dcs", "incoming").unwrap();
    assert!(!runtime.publish_current_profile_game_delivery(delivery("dcs", "incoming", 2), || true).unwrap());
    assert!(runtime.state.lock().unwrap().game_deliveries.is_empty());
}

#[test]
fn cancellation_is_checked_after_a_blocked_capture_gate_before_publication() {
    let runtime = Arc::new(runtime());
    runtime.publish_profile_game_delivery(delivery("dcs", "reader", 1)).unwrap();
    let capture = runtime.capture_gate.lock().unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let current = cancelled.clone();
    let worker_runtime = runtime.clone();
    let (entered, waiting) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        entered.send(()).unwrap();
        worker_runtime.publish_current_profile_game_delivery(delivery("dcs", "reader", 2),
            || !current.load(Ordering::Acquire))
    });
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    cancelled.store(true, Ordering::Release);
    drop(capture);
    assert!(!worker.join().unwrap().unwrap());
    assert_eq!(runtime.state.lock().unwrap().game_deliveries["dcs"].revision, 1);
}

#[test]
fn queued_outgoing_worker_cannot_overwrite_a_new_reader() {
    let runtime = Arc::new(runtime());
    runtime.publish_profile_game_delivery(delivery("dcs", "old", 1)).unwrap();
    let old_runtime = runtime.clone();
    let (release, held) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        held.recv_timeout(Duration::from_secs(1)).unwrap();
        old_runtime.publish_current_profile_game_delivery(delivery("dcs", "old", 2), || true)
    });
    runtime.publish_profile_game_delivery(delivery("dcs", "incoming", 1)).unwrap();
    release.send(()).unwrap();
    assert!(!worker.join().unwrap().unwrap());
    assert_eq!(runtime.state.lock().unwrap().game_deliveries["dcs"].session, "incoming");
}

#[test]
fn returning_reader_cas_uses_the_last_owner_after_withdrawal() {
    let runtime = runtime();
    runtime.publish_profile_game_delivery(delivery("dcs", "old", 1)).unwrap();
    runtime.withdraw_profile_game_delivery("dcs", "old").unwrap();
    assert!(!runtime.replace_profile_game_delivery(delivery("dcs", "wrong", 1), None, || true).unwrap());
    assert!(!runtime.replace_profile_game_delivery(delivery("dcs", "cancelled", 1), Some("old"), || false).unwrap());
    assert!(runtime.replace_profile_game_delivery(delivery("dcs", "returning", 1), Some("old"), || true).unwrap());
    assert!(!runtime.replace_profile_game_delivery(delivery("dcs", "late", 1), Some("old"), || true).unwrap());
    assert_eq!(runtime.state.lock().unwrap().game_delivery_readers["dcs"], "returning");
    assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "returning").unwrap(), Some(()));
    runtime.withdraw_profile_game_delivery("dcs", "old").unwrap();
    assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "returning").unwrap(), Some(()));
}

#[test]
fn first_reader_claim_and_volatile_history_obey_the_game_budget() {
    let runtime = runtime();
    assert!(runtime.replace_profile_game_delivery(delivery("dcs", "first", 1), None, || true).unwrap());
    runtime.withdraw_profile_game_delivery("dcs", "first").unwrap();
    assert!(!runtime.replace_profile_game_delivery(delivery("dcs", "unowned", 1), None, || true).unwrap());
    let full = super::runtime();
    for index in 0..64 { full.state.lock().unwrap().game_delivery_readers.insert(format!("game-{index}"), "reader".into()); }
    assert!(full.publish_profile_game_delivery(delivery("dcs", "over-budget", 1)).unwrap_err().contains("64-game"));
    assert!(full.state.lock().unwrap().game_deliveries.is_empty());
}

#[test]
fn dormant_profile_captures_the_previous_stopped_readers_cas_predecessor() {
    let runtime = runtime();
    runtime.publish_profile_game_delivery(delivery("dcs", "profile-a", 1)).unwrap();
    runtime.withdraw_profile_game_delivery("dcs", "profile-a").unwrap();
    assert!(runtime.state.lock().unwrap().game_deliveries.is_empty());
    // Profile B is admitted while no process/reader exists. Capture at Acquire,
    // rather than assuming an empty active overlay has no prior reader.
    let predecessor = runtime.profile_game_reader("dcs").unwrap();
    assert_eq!(predecessor.as_deref(), Some("profile-a"));
    assert!(runtime.replace_profile_game_delivery(delivery("dcs", "profile-b-returned", 1),
        predecessor.as_deref(), || true).unwrap());
    assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "profile-b-returned").unwrap(), Some(()));
    // Another dormant capture cannot authorize a later overwrite after a
    // newer owner has installed its independently admitted reader.
    let stale = runtime.profile_game_reader("dcs").unwrap();
    runtime.publish_profile_game_delivery(delivery("dcs", "profile-c", 1)).unwrap();
    assert!(!runtime.replace_profile_game_delivery(delivery("dcs", "late-profile-b", 1),
        stale.as_deref(), || true).unwrap());
    assert_eq!(runtime.try_validate_profile_game_delivery("dcs", "profile-c").unwrap(), Some(()));
}
