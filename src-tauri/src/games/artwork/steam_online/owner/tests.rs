use super::*;
use serde_json::json;

fn snapshot(ids: impl IntoIterator<Item = u32>, scan: &str) -> Value {
    json!({"catalogue":ids.into_iter().map(|id| json!({"id":format!("game-{id}"),
        "installations":[{"sources":[{"client":"steam","appId":id.to_string()}]}]})).collect::<Vec<_>>(),
        "discovery":{"scanId":scan,"status":"ready","entries":[]}})
}
fn root() -> &'static Path { Path::new("owner-test-root") }
fn key(app_id: u32) -> Key { Key { root: root().into(), app_id } }
fn success() -> Result<(), String> { Ok(()) }

#[test]
fn failures_wait_for_actual_deadline_and_retry_without_a_new_snapshot() {
    let pool = Mutex::new(Service::default());
    let (id, _inner) = register(&pool, root()).unwrap();
    replace(&pool, id, &snapshot([42], "scan")).unwrap();
    assert!(step(&pool, id, Instant::now(), success).is_none());
    run_worker(&pool, |_| Err("offline".into()));
    let deadline = step(&pool, id, Instant::now(), || panic!("Backoff must park")).unwrap();
    assert!(deadline > Instant::now());
    assert!(pool.lock().unwrap().jobs.is_empty());
    assert!(step(&pool, id, deadline, success).is_none());
    assert_eq!(pool.lock().unwrap().jobs.len(), 1);
    run_worker(&pool, |_| Ok("ready".into()));
    assert!(step(&pool, id, deadline, || panic!("Ready must park indefinitely")).is_none());
}

#[test]
fn changed_scan_retries_failure_immediately_but_scanning_cancels_targets() {
    let pool = Mutex::new(Service::default());
    let (id, _inner) = register(&pool, root()).unwrap();
    replace(&pool, id, &snapshot([42], "old")).unwrap();
    step(&pool, id, Instant::now(), success);
    run_worker(&pool, |_| Err("offline".into()));
    replace(&pool, id, &snapshot([42], "new")).unwrap();
    assert!(step(&pool, id, Instant::now(), success).is_none());
    assert_eq!(pool.lock().unwrap().jobs.len(), 1);
    let mut scanning = snapshot([42], "scanning");
    scanning["discovery"]["status"] = json!("scanning");
    replace(&pool, id, &scanning).unwrap();
    assert!(pool.lock().unwrap().jobs.is_empty());
    run_worker(&pool, |_| panic!("Retired queue must not download"));
}

#[test]
fn retired_inflight_completion_cannot_overwrite_readded_target_ticket() {
    let pool = Mutex::new(Service::default());
    let (id, _inner) = register(&pool, root()).unwrap();
    replace(&pool, id, &snapshot([42], "scan")).unwrap();
    step(&pool, id, Instant::now(), success);
    let calls = std::sync::atomic::AtomicUsize::new(0);
    run_worker(&pool, |_| {
        if calls.fetch_add(1, Ordering::Relaxed) == 0 {
            replace(&pool, id, &snapshot([], "replacement")).unwrap();
            replace(&pool, id, &snapshot([42], "replacement")).unwrap();
            step(&pool, id, Instant::now(), success);
            Ok("obsolete".into())
        } else {
            assert!(matches!(pool.lock().unwrap().states.get(&key(42)), Some(State::Pending)));
            Ok("current".into())
        }
    });
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(matches!(pool.lock().unwrap().states.get(&key(42)), Some(State::Ready(image)) if image.as_ref() == "current"));
}

#[test]
fn last_root_registration_cancels_queued_and_fences_inflight_completion() {
    let pool = Mutex::new(Service::default());
    let (first, _first_inner) = register(&pool, root()).unwrap();
    let (second, _second_inner) = register(&pool, root()).unwrap();
    for id in [first, second] { replace(&pool, id, &snapshot([42, 43], "scan")).unwrap(); }
    step(&pool, first, Instant::now(), success);
    unregister(&pool, first);
    assert_eq!(pool.lock().unwrap().jobs.len(), 2, "Another root lease retains its work");
    run_worker(&pool, |_| {
        unregister(&pool, second);
        assert!(pool.lock().unwrap().jobs.is_empty());
        Ok("late".into())
    });
    let service = pool.lock().unwrap();
    assert!(service.states.is_empty());
    assert!(service.tickets.is_empty());
    assert!(service.owners.is_empty());
}

#[test]
fn larger_than_service_working_set_is_processed_once_without_eviction_reload_loop() {
    let pool = Mutex::new(Service::default());
    let (id, _inner) = register(&pool, root()).unwrap();
    replace(&pool, id, &snapshot(1..=1024, "scan")).unwrap();
    step(&pool, id, Instant::now(), success);
    assert_eq!(pool.lock().unwrap().states.len(), MAX_ENTRIES);
    run_worker(&pool, |_| Ok("cached".into()));
    step(&pool, id, Instant::now(), success);
    run_worker(&pool, |_| Ok("cached".into()));
    let mut service = pool.lock().unwrap();
    assert_eq!(service.states.len(), MAX_ENTRIES);
    assert_eq!(service.owners[&id].attempts.len(), 1024);
    assert!(service.owners[&id].attempts.values().all(|attempt| matches!(attempt, Attempt::Ready)));
    service.states.clear();
    drop(service);
    assert!(step(&pool, id, Instant::now(), || panic!("Eviction cannot reload Ready targets")).is_none());
    assert!(pool.lock().unwrap().jobs.is_empty());
}

#[test]
fn registrations_targets_and_scan_tokens_are_bounded_without_changing_valid_targets() {
    let pool = Mutex::new(Service::default());
    let mut leases = Vec::new();
    for _ in 0..MAX_OWNERS { leases.push(register(&pool, root()).unwrap()); }
    assert!(register(&pool, root()).is_err());
    let id = leases[0].0;
    replace(&pool, id, &snapshot(1..=MAX_TARGETS as u32, "scan")).unwrap();
    assert!(replace(&pool, id, &snapshot(1..=MAX_TARGETS as u32 + 1, "scan")).is_err());
    assert!(replace(&pool, id, &snapshot([], &"x".repeat(MAX_SCAN_BYTES + 1))).is_err());
    assert_eq!(pool.lock().unwrap().owners[&id].targets.len(), MAX_TARGETS);
}

#[test]
fn spawn_failure_has_deadline_and_completion_wakes_are_weak() {
    let pool = Mutex::new(Service::default());
    let (id, inner) = register(&pool, root()).unwrap();
    replace(&pool, id, &snapshot([42], "scan")).unwrap();
    let before = inner.wake.observed();
    assert!(step(&pool, id, Instant::now(), || Err("cannot spawn".into())).is_some());
    assert!(inner.wake.observed() > before);
    drop(inner);
    assert!(wakes(&pool.lock().unwrap()).is_empty());
}

#[test]
fn owners_with_different_scans_share_one_failure_without_retry_ping_pong() {
    let pool = Mutex::new(Service::default());
    let (first, _first_inner) = register(&pool, root()).unwrap();
    let (second, _second_inner) = register(&pool, root()).unwrap();
    replace(&pool, first, &snapshot([42], "first")).unwrap();
    replace(&pool, second, &snapshot([42], "second")).unwrap();
    step(&pool, first, Instant::now(), success);
    step(&pool, second, Instant::now(), || panic!("Single flight"));
    run_worker(&pool, |_| Err("offline".into()));
    for id in [first, second] {
        assert!(step(&pool, id, Instant::now(), || panic!("Both attempts must back off")).is_some());
    }
    assert!(pool.lock().unwrap().jobs.is_empty());
}

#[test]
fn shutdown_is_idempotent_and_prevents_replacement_without_http_work() {
    let directory = crate::test_directory();
    let owner = Owner::new(directory.path()).unwrap();
    owner.replace(&snapshot([], "scan")).unwrap();
    owner.shutdown();
    owner.shutdown();
    assert!(owner.replace(&snapshot([42], "scan")).is_err());
}

#[test]
fn accepted_owner_intent_is_pending_before_scheduling_but_unknown_and_evicted_ready_are_not() {
    let pool = Mutex::new(Service::default());
    let (id, _inner) = register(&pool, root()).unwrap();
    replace(&pool, id, &snapshot([42], "scan")).unwrap();
    let mut accepted = snapshot([42], "scan");
    decorate_with(&pool, root(), &mut accepted);
    assert_eq!(accepted["artworkPending"], true);
    assert_eq!(accepted["discovery"]["artworkPending"], true);
    assert!(pool.lock().unwrap().jobs.is_empty(), "Receipt reads cannot schedule work");
    assert!(pool.lock().unwrap().states.is_empty());
    let mut unknown = snapshot([43], "scan");
    decorate_with(&pool, root(), &mut unknown);
    assert_eq!(unknown["artworkPending"], false);
    pool.lock().unwrap().owners.get_mut(&id).unwrap().attempts.insert(42, Attempt::Ready);
    let mut evicted = snapshot([42], "scan");
    decorate_with(&pool, root(), &mut evicted);
    assert_eq!(evicted["artworkPending"], false);
    assert!(pool.lock().unwrap().jobs.is_empty());
}

#[test]
fn explicit_refresh_keeps_full_pending_cache_bounded_and_only_records_backed_ready_results() {
    let pool = Mutex::new(Service::default());
    let (id, _inner) = register(&pool, root()).unwrap();
    replace(&pool, id, &snapshot([1, 1000], "scan")).unwrap();
    let mut service = pool.lock().unwrap();
    for app_id in 1..=MAX_ENTRIES as u32 { service.request(key(app_id), "scan", Instant::now()); }
    assert!(!service.cache_ready(key(1000), "disk-only".into()));
    assert_eq!(service.states.len(), MAX_ENTRIES);
    assert!(!service.states.contains_key(&key(1000)));
    assert!(!service.owners[&id].attempts.contains_key(&1000));
    assert!(pending(&service, &key(1000)), "Unscheduled accepted intent must survive a disk-only refresh");
    assert!(service.cache_ready(key(1), "updated".into()));
    assert_eq!(service.states.len(), MAX_ENTRIES);
    assert!(!service.tickets.contains_key(&key(1)), "Refresh fences the previous pending job");
    assert!(matches!(service.owners[&id].attempts.get(&1), Some(Attempt::Ready)));
}

#[test]
fn explicit_refresh_evicts_terminal_state_and_ticket_and_updates_existing_key_at_capacity() {
    let mut service = Service::default();
    for app_id in 1..=MAX_ENTRIES as u32 {
        service.states.insert(key(app_id), State::Ready("old".into()));
        service.tickets.insert(key(app_id), app_id as u64);
    }
    assert!(service.cache_ready(key(1000), "new".into()));
    assert_eq!(service.states.len(), MAX_ENTRIES);
    assert!(!service.states.contains_key(&key(1)));
    assert!(!service.tickets.contains_key(&key(1)));
    assert!(service.cache_ready(key(1000), "updated".into()));
    assert_eq!(service.states.len(), MAX_ENTRIES);
    assert!(matches!(service.states.get(&key(1000)), Some(State::Ready(image)) if image.as_ref() == "updated"));
}
