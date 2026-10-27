use super::*;
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc,
};

fn image() -> String {
    let image = image::RgbImage::from_fn(640, 360, |x, y| {
        image::Rgb([(x / 3) as u8, (y / 2) as u8, 120])
    });
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    http::encode(bytes.get_ref()).unwrap()
}

#[test]
fn explicit_refresh_fetches_again_even_when_selected_app_artwork_is_cached() {
    let root=crate::test_directory();let original=image();
    cached_or_fetch(root.path(),42,|_|Ok(original.clone())).unwrap();
    let calls=AtomicUsize::new(0);
    cached_or_fetch_using(root.path(),42,|id|{assert_eq!(id,42);calls.fetch_add(1,Ordering::Relaxed);Ok(original.clone())},true).unwrap();
    assert_eq!(calls.load(Ordering::Relaxed),1);
    assert_eq!(cached_or_fetch_using(root.path(),42,|_|Err("offline".into()),true).unwrap(),original);
}
fn key(root: &Path, id: u32) -> Key {
    Key {
        root: root.into(),
        app_id: id,
    }
}
fn state() -> Value {
    json!({"catalogue":[{"id":"known","name":"Known","installations":[{"installPath":"C:/Games/Old","sources":[{"client":"steam","appId":"42"}]}]}],
        "discovery":{"scanId":"scan","status":"ready","libraries":[{"id":"steam","status":"complete"}],"observations":[{"client":"steam","appId":"42","installPath":"C:/Games/New"}],
        "entries":[{"id":"new","gameId":"known","installPath":"C:/Games/New","image":null}]}})
}

#[test]
fn disk_cache_survives_restart_and_offline_fetch_and_repairs_invalid_bytes() {
    let root = crate::test_directory();
    let image = image();
    let calls = AtomicUsize::new(0);
    let first = cached_or_fetch(root.path(), 42, |id| {
        assert_eq!(id, 42);
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(image.clone())
    })
    .unwrap();
    assert_eq!(first, image);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        cached_or_fetch(root.path(), 42, |_| panic!(
            "Cached artwork must work offline"
        ))
        .unwrap(),
        image
    );
    let file = root.path().join("steam-artwork/42.jpg");
    fs::write(&file, b"corrupt").unwrap();
    assert_eq!(
        cached_or_fetch(root.path(), 42, |_| Ok(image.clone())).unwrap(),
        image
    );
    fs::write(&file, vec![0; DISCOVERY_PREVIEW_LIMIT + 1]).unwrap();
    assert_eq!(
        cached_or_fetch(root.path(), 42, |_| Ok(image.clone())).unwrap(),
        image
    );
    assert!(cached_or_fetch(root.path(), 0, |_| panic!("No unidentified downloads")).is_err());
    assert!(cached_or_fetch(Path::new("relative"), 42, |_| panic!(
        "No invalid-root downloads"
    ))
    .is_err());
    assert!(cached_or_fetch(root.path(), 43, |_| Err("offline".into())).is_err());
    assert!(!root.path().join("steam-artwork/43.jpg").exists());
    assert!(cached_or_fetch(
        root.path(),
        44,
        |_| Ok("data:image/jpeg;base64,AAAA".into())
    )
    .is_err());
}

#[test]
fn low_resolution_cached_preview_upgrades_online_and_survives_offline() {
    let root = crate::test_directory();
    let mut source = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(384, 180, image::Rgb([60, 90, 120]))
        .write_to(&mut source, image::ImageFormat::Png)
        .unwrap();
    let small = http::encode(source.get_ref()).unwrap();
    let path = root.path().join("steam-artwork/42.jpg");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, discovery_preview_bytes(&small).unwrap()).unwrap();
    assert_eq!(
        cached_or_fetch(root.path(), 42, |_| Err("offline".into())).unwrap(),
        small
    );
    assert_eq!(
        cached_or_fetch(root.path(), 42, |_| Ok("invalid".into())).unwrap(),
        small
    );
    let large = image();
    assert_eq!(
        cached_or_fetch(root.path(), 42, |_| Ok(large.clone())).unwrap(),
        large
    );
    assert_eq!(
        cached_or_fetch(root.path(), 42, |_| panic!("Upgraded cache works offline")).unwrap(),
        large
    );
    let decoded = decoded(&fs::read(path).unwrap()).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (640, 360));
}

#[test]
fn cache_rejects_non_file_destinations_and_limits_disk_usage() {
    let root = crate::test_directory();
    let directory = directory(root.path()).unwrap();
    fs::create_dir_all(directory.join("42.jpg")).unwrap();
    assert!(cached_or_fetch(root.path(), 42, |_| panic!("Cannot replace directories")).is_err());
    for index in 0..MAX_ENTRIES {
        fs::write(directory.join(format!("other-{index}")), b"x").unwrap();
    }
    assert!(cached_or_fetch(root.path(), 43, |_| Ok(image()))
        .unwrap_err()
        .contains("limit"));
    let bad = crate::test_directory();
    fs::write(bad.path().join("steam-artwork"), b"not a directory").unwrap();
    assert!(cached_or_fetch(bad.path(), 42, |_| panic!("Invalid cache directory")).is_err());
}

#[test]
fn requests_are_single_flight_and_failures_retry_on_rescan_or_after_backoff() {
    let mut service = Service::default();
    let key = key(Path::new("root"), 42);
    let now = Instant::now();
    assert!(matches!(
        service.request(key.clone(), "scan", now),
        State::Pending
    ));
    service.request(key.clone(), "scan", now);
    assert_eq!(service.jobs.len(), 1);
    service.states.insert(
        key.clone(),
        State::Failed {
            scan: "scan".into(),
            retry: now + Duration::from_secs(300),
        },
    );
    assert!(matches!(
        service.request(key.clone(), "scan", now),
        State::Failed { .. }
    ));
    assert!(matches!(
        service.request(key.clone(), "new-scan", now),
        State::Pending
    ));
    service.states.insert(
        key.clone(),
        State::Failed {
            scan: "new-scan".into(),
            retry: now,
        },
    );
    assert!(matches!(
        service.request(key.clone(), "new-scan", now),
        State::Pending
    ));
    service
        .states
        .insert(key.clone(), State::Ready("image".into()));
    assert!(matches!(service.request(key, "any", now), State::Ready(_)));
    let mut full = Service::default();
    for id in 1..=MAX_ENTRIES as u32 {
        full.request(key_for(id), "scan", now);
    }
    assert!(matches!(
        full.request(key_for(1000), "scan", now),
        State::Failed { .. }
    ));
    full.states.insert(key_for(1), State::Ready("image".into()));
    assert!(matches!(
        full.request(key_for(1000), "scan", now),
        State::Pending
    ));
    assert_eq!(full.states.len(), MAX_ENTRIES);
}
fn key_for(id: u32) -> Key {
    key(Path::new("bounded-root"), id)
}

#[test]
fn slow_network_keeps_status_responsive_and_does_not_reopen_scan_completion() {
    let root = crate::test_directory();
    let key = key(root.path(), 42);
    let pool = Arc::new(Mutex::new(Service::default()));
    pool.lock()
        .unwrap()
        .request(key.clone(), "scan", Instant::now());
    pool.lock().unwrap().workers = 1;
    let (entered, waiting) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let worker_pool = pool.clone();
    let worker = std::thread::spawn(move || {
        run_worker(&worker_pool, |_| {
            entered.send(()).unwrap();
            resume.recv().unwrap();
            Ok("downloaded".into())
        })
    });
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut status = pool
        .try_lock()
        .expect("Network must not hold the status mutex");
    assert!(matches!(
        status.request(key.clone(), "scan", Instant::now()),
        State::Pending
    ));
    assert!(status.jobs.is_empty());
    drop(status);
    release.send(()).unwrap();
    worker.join().unwrap();
    assert!(
        matches!(pool.lock().unwrap().states.get(&key),Some(State::Ready(image)) if image=="downloaded")
    );
    assert_eq!(pool.lock().unwrap().workers, 0);
    let mut review = state();
    let image = image();
    service()
        .lock()
        .unwrap()
        .states
        .insert(key, State::Ready(image.clone()));
    decorate(root.path(), &mut review);
    assert_eq!(review["discovery"]["status"], "ready");
    assert_eq!(review["discovery"]["libraries"][0]["status"], "complete");
    assert_eq!(review["discovery"]["entries"][0]["image"], image);
    assert_eq!(review["catalogue"][0]["defaultBackground"]["image"], image);
    assert_eq!(review["artworkPending"], false);
}

#[test]
fn offline_completion_stops_artwork_refresh_without_changing_scan_results() {
    let root = crate::test_directory();
    let key = key(root.path(), 42);
    let pool = Mutex::new(Service::default());
    pool.lock()
        .unwrap()
        .request(key.clone(), "scan", Instant::now());
    pool.lock().unwrap().workers = 1;
    run_worker(&pool, |_| Err("offline".into()));
    let result = pool.lock().unwrap().states[&key].clone();
    assert!(matches!(result, State::Failed { .. }));
    assert_eq!(pool.lock().unwrap().workers, 0);
    service().lock().unwrap().states.insert(key.clone(), result);
    let mut review = state();
    decorate(root.path(), &mut review);
    assert_eq!(review["artworkPending"], false);
    assert_eq!(review["discovery"]["status"], "ready");
    assert!(review["discovery"]["entries"][0]["image"].is_null());
    service().lock().unwrap().states.insert(key, State::Pending);
    decorate(root.path(), &mut review);
    assert_eq!(review["artworkPending"], true);
    assert_eq!(review["discovery"]["status"], "ready");
}

#[test]
fn exact_store_identity_is_required_and_local_or_user_artwork_is_retained() {
    let mut state = state();
    let targets = targets::collect(&state);
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().all(|target| target.app_id == 42));
    state["catalogue"][0]["background"] = json!({"image":"user-image"});
    state["discovery"]["entries"][0]["image"] = json!("local-image");
    assert!(targets::collect(&state).is_empty());
    state["discovery"]["entries"][0]["image"] = Value::Null;
    assert!(
        targets::collect(&state).is_empty(),
        "A saved background also wins for a newly found copy"
    );
    state["catalogue"][0]["background"] = Value::Null;
    state["discovery"]["entries"][0]["installPath"] = json!("C:/Games/Different");
    assert_eq!(
        targets::collect(&state)
            .iter()
            .filter(|target| target.entry.is_some())
            .count(),
        1,
        "A second copy can use its confirmed title identity"
    );
    state["discovery"]["entries"][0]["gameId"] = json!("unknown-game");
    assert!(
        targets::collect(&state)
            .iter()
            .all(|target| target.entry.is_none()),
        "Names alone cannot choose online artwork"
    );
    let root = crate::test_directory();
    state["discovery"]["status"] = json!("scanning");
    decorate(root.path(), &mut state);
    assert!(state.get("artworkPending").is_none());
}

#[test]
#[ignore = "Opt-in read-only request to Steam; no game is launched."]
fn live_steam_artwork_download_is_cached_and_works_offline() {
    let root = crate::test_directory();
    let mut review = state();
    review["catalogue"][0]["installations"][0]["sources"][0]["appId"] = json!("1808500");
    review["discovery"]["observations"][0]["appId"] = json!("1808500");
    decorate(root.path(), &mut review);
    assert_eq!(review["discovery"]["status"], "ready");
    assert_eq!(review["artworkPending"], true);
    let deadline = Instant::now() + Duration::from_secs(20);
    while review["artworkPending"] == true {
        assert!(Instant::now() < deadline, "Artwork download must finish");
        std::thread::sleep(Duration::from_millis(100));
        decorate(root.path(), &mut review);
    }
    let first = review["discovery"]["entries"][0]["image"]
        .as_str()
        .expect("Live Steam header image")
        .to_owned();
    assert!(discovery_preview_bytes(&first).unwrap().len() > 1000);
    assert_eq!(
        cached_or_fetch(root.path(), 1808500, |_| panic!(
            "Second request must be offline"
        ))
        .unwrap(),
        first
    );
}
