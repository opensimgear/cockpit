use super::*;
use serde_json::json;

fn snapshot() -> Value {
    json!({"catalogue":[{"id":"known","installations":[{"sources":[{"client":"steam","appId":"42"}]}]}],
        "discovery":{"scanId":"scan","status":"ready","entries":[]}})
}

fn key() -> Key { Key { root: PathBuf::from("unused-root"), app_id: 42 } }

#[test]
fn passive_unknown_target_never_creates_pending_state_or_jobs() {
    let pool = Mutex::new(Service::default());
    let mut state = snapshot();
    decorate_with(&pool, &key().root, &mut state);
    assert_eq!(state["artworkPending"], false);
    assert_eq!(state["discovery"]["artworkPending"], false);
    let service = pool.lock().unwrap();
    assert!(service.states.is_empty());
    assert!(service.jobs.is_empty());
    assert_eq!(service.workers, 0);
}

#[test]
fn preparation_starts_single_flight_work_and_decoration_projects_completion() {
    let pool = Mutex::new(Service::default());
    let source = snapshot();
    let before = source.clone();
    let mut spawned = 0;
    prepare_with(&pool, &key().root, &source, || { spawned += 1; Ok(()) });
    prepare_with(&pool, &key().root, &source, || panic!("Already scheduled"));
    assert_eq!(source, before, "Preparation cannot mutate its source snapshot");
    assert_eq!(spawned, 1);
    assert_eq!(pool.lock().unwrap().jobs.len(), 1);
    let mut pending = source.clone();
    decorate_with(&pool, &key().root, &mut pending);
    assert_eq!(pending["artworkPending"], true);
    run_worker(&pool, |target| { assert_eq!(target.app_id, 42); Ok("cached-image".into()) });
    let mut ready = source;
    decorate_with(&pool, &key().root, &mut ready);
    assert_eq!(ready["catalogue"][0]["defaultBackground"]["image"], "cached-image");
    assert_eq!(ready["artworkPending"], false);
    assert_eq!(pool.lock().unwrap().workers, 0);
}

#[test]
fn scanning_snapshot_does_not_prepare_or_spawn_work() {
    let pool = Mutex::new(Service::default());
    let mut source = snapshot();
    source["discovery"]["status"] = json!("scanning");
    prepare_with(&pool, &key().root, &source, || panic!("Scanning cannot schedule artwork"));
    let service = pool.lock().unwrap();
    assert!(service.states.is_empty());
    assert!(service.jobs.is_empty());
    assert_eq!(service.workers, 0);
}

#[test]
fn passive_decoration_does_not_retry_expired_or_different_scan_failures() {
    let pool = Mutex::new(Service::default());
    pool.lock().unwrap().states.insert(key(), State::Failed {
        scan: "old-scan".into(), retry: Instant::now() - Duration::from_secs(1),
    });
    let mut state = snapshot();
    decorate_with(&pool, &key().root, &mut state);
    assert_eq!(state["artworkPending"], false);
    let service = pool.lock().unwrap();
    assert!(matches!(service.states.get(&key()), Some(State::Failed { scan, .. }) if scan == "old-scan"));
    assert!(service.jobs.is_empty());
    assert_eq!(service.workers, 0);
}

#[test]
fn preparation_spawn_failure_finishes_pending_targets_without_read_retry() {
    let pool = Mutex::new(Service::default());
    prepare_with(&pool, &key().root, &snapshot(), || Err("spawn failed".into()));
    let mut state = snapshot();
    decorate_with(&pool, &key().root, &mut state);
    assert_eq!(state["artworkPending"], false);
    let service = pool.lock().unwrap();
    assert!(matches!(service.states.get(&key()), Some(State::Failed { .. })));
    assert!(service.jobs.is_empty());
    assert_eq!(service.workers, 0);
}

#[test]
fn ready_requests_share_large_artwork_and_projection_keeps_the_image_bytes() {
    let image: Arc<str> = "x".repeat(128 * 1024).into();
    let pool = Mutex::new(Service::default());
    pool.lock().unwrap().cache_ready(key(), image.clone());
    let ready = pool.lock().unwrap().request(key(), "scan", Instant::now());
    let State::Ready(shared) = ready else { panic!("Expected cached artwork"); };
    assert!(Arc::ptr_eq(&image, &shared), "Reading a ready job must not copy its image buffer");
    let mut state = snapshot();
    decorate_with(&pool, &key().root, &mut state);
    assert_eq!(state["catalogue"][0]["defaultBackground"]["image"].as_str(), Some(image.as_ref()));
    assert_eq!(state["artworkPending"], false);
}
