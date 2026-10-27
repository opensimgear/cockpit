use super::*;
use serde_json::json;
use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

fn fixture() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn explicit_refresh_bypasses_only_requested_steam_genre_cache() {
    let root=fixture();
    load_with(root.path(),&[42],&||false,|_,_|Ok(response(42,42,true,json!([{"description":"Racing"}]))));
    let calls=AtomicUsize::new(0);
    let result=load_using(root.path(),&[42],&||false,|id,_|{
        assert_eq!(id,42);calls.fetch_add(1,Ordering::Relaxed);Ok(response(42,42,true,json!([{"description":"Simulation"}])))
    },true);
    assert_eq!(calls.load(Ordering::Relaxed),1);assert_eq!(result[&42],["Simulation"]);
}

fn response(requested_id: u32, identity: u32, success: bool, genres: Value) -> Vec<u8> {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        requested_id.to_string(),
        json!({"success":success,"data":{"steam_appid":identity,"genres":genres}}),
    );
    serde_json::to_vec(&Value::Object(metadata)).unwrap()
}

#[test]
fn appdetails_url_requests_english_genres_and_categories_by_exact_id() {
    assert_eq!(
        metadata_url(42),
        "https://store.steampowered.com/api/appdetails?appids=42&filters=basic,genres,categories&l=english"
    );
}

#[test]
fn parser_requires_success_exact_app_identity_and_bounded_genres() {
    let bytes = response(
        42,
        42,
        true,
        json!([{"id":"1","description":"Action"},{"id":"2","description":"Racing"}]),
    );
    assert_eq!(
        parse_response(42, &bytes).unwrap(),
        vec!["Action".to_owned(), "Racing".to_owned()]
    );
    assert!(parse_response(42, &response(42, 43, true, json!([]))).is_none());
    assert!(parse_response(42, &response(42, 42, false, json!([]))).is_none());
    assert!(parse_response(
        42,
        &serde_json::to_vec(&json!({"43":{"success":true,"data":{"steam_appid":42,"genres":[]}}}))
            .unwrap()
    )
    .is_none());
    assert!(parse_response(42, &response(42, 42, true, json!({}))).is_none());
    let too_many = Value::Array(
        (0..MAX_LABELS + 1)
            .map(|_| json!({"description":"x"}))
            .collect(),
    );
    assert!(parse_response(42, &response(42, 42, true, too_many)).is_none());
    assert!(parse_response(42, &response(42, 42, true, json!([{"description":"x\nY"}]))).is_none());
    assert!(parse_response(42, &vec![b' '; MAX_RESPONSE_BYTES + 1]).is_none());
}

#[test]
fn successful_empty_results_are_cached_and_skip_network_next_time() {
    let root = fixture();
    fs::create_dir(root.path().join("platform-tags")).unwrap();
    assert!(write_cache(&root.path().join("platform-tags"), 42, &[]));
    let calls = AtomicUsize::new(0);
    let result = load_with(root.path(), &[42], &|| false, |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        Err("must be served from cache".into())
    });
    assert_eq!(result.get(&42), Some(&Vec::new()));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn cache_stops_at_entry_cap_but_can_refresh_an_existing_app_id() {
    let root = fixture();
    let directory = root.path().join("platform-tags");
    fs::create_dir(&directory).unwrap();
    assert!(write_cache(&directory, 1, &["Old".into()]));
    for index in 0..MAX_CACHE_ENTRIES - 1 {
        fs::write(directory.join(format!("reserved-{index}.tmp")), b"reserved").unwrap();
    }
    assert!(!write_cache(&directory, 2, &["New".into()]));
    assert!(!directory.join("2.json").exists());
    assert!(write_cache(&directory, 1, &["Updated".into()]));
    assert_eq!(
        read_cache(&directory, 1).unwrap(),
        vec!["Updated".to_owned()]
    );
}

#[test]
fn cache_identity_and_label_bounds_are_checked_before_use() {
    let root = fixture();
    let directory = root.path().join("platform-tags");
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("42.json"),
        serde_json::to_vec(&CachedGenres {
            app_id: 43,
            genres: vec!["Wrong app".into()],
        })
        .unwrap(),
    )
    .unwrap();
    fs::write(
        directory.join("44.json"),
        serde_json::to_vec(&CachedGenres {
            app_id: 44,
            genres: vec!["x".repeat(MAX_LABEL_CHARS + 1)],
        })
        .unwrap(),
    )
    .unwrap();
    let calls = Mutex::new(Vec::new());
    let result = load_with(root.path(), &[42, 44], &|| false, |app_id, _| {
        calls.lock().unwrap().push(app_id);
        Ok(response(
            app_id,
            app_id,
            true,
            json!([{"description":"Fresh"}]),
        ))
    });
    assert_eq!(result[&42], vec!["Fresh".to_owned()]);
    assert_eq!(result[&44], vec!["Fresh".to_owned()]);
    let mut calls = calls.into_inner().unwrap();
    calls.sort_unstable();
    assert_eq!(calls, [42, 44]);
}

#[test]
fn cancelled_load_stops_before_reading_cache_or_starting_requests() {
    let root = fixture();
    let directory = root.path().join("platform-tags");
    fs::create_dir(&directory).unwrap();
    assert!(write_cache(&directory, 42, &["Action".into()]));
    let calls = AtomicUsize::new(0);
    let result = load_with(root.path(), &[42, 43], &|| true, |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        Err("cancelled".into())
    });
    assert!(result.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn metadata_reader_rejects_oversized_responses() {
    assert_eq!(read_bounded(Cursor::new(b"abc"), 3).unwrap(), b"abc");
    assert!(read_bounded(Cursor::new(b"abcd"), 3).is_err());
}

#[test]
fn unavailable_cache_does_not_block_metadata_enrichment() {
    let root = fixture();
    let missing = root.path().join("not-created");
    let calls = AtomicUsize::new(0);
    let result = load_with(&missing, &[42], &|| false, |app_id, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(response(
            app_id,
            app_id,
            true,
            json!([{"description":"Simulation"}]),
        ))
    });
    assert_eq!(result[&42], vec!["Simulation".to_owned()]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!missing.exists());
}

#[test]
fn redirected_cache_directory_is_never_used() {
    let root = fixture();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.path(), root.path().join("platform-tags")).unwrap();
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(root.path().join("platform-tags"))
        .arg(root.path())
        .output()
        .unwrap()
        .status
        .success());
    let calls = AtomicUsize::new(0);
    let result = load_with(root.path(), &[42], &|| false, |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(response(42, 42, true, json!([{"description":"Action"}])))
    });
    assert_eq!(result[&42], vec!["Action".to_owned()]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!root.path().join("42.json").exists());
}
