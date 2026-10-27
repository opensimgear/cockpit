use super::*;
use crate::games::discovery_review::Library;
use std::sync::{mpsc, Arc};

fn entry(id: &str) -> Entry {
    Entry {
        id: id.into(),
        action: "add".into(),
        selected: true,
        game_id: "example".into(),
        installation_id: id.into(),
        name: "Example".into(),
        library: "disk".into(),
        install_path: "C:/Games/Example".into(),
        previous_path: None,
        executable_candidates: vec![],
        executable: None,
        update_kind: None,
        integration: None,
        image: None,
        tag_ids: vec![],
        message: None,
        sources: vec![],
        artwork: Some(format!("C:/Art/{id}.jpg").into()),
    }
}
fn state() -> Review {
    Review {
        scan_id: "scan".into(),
        status: "scanning".into(),
        libraries: vec![Library {
            id: "disk".into(),
            name: "Local paths".into(),
            status: "preparing".into(),
            found: 2,
            message: None,
        }],
        ..Default::default()
    }
}

#[test]
fn rows_are_visible_and_status_reads_stay_responsive_while_artwork_is_blocked() {
    let state = Arc::new(Mutex::new(state()));
    assert!(publish_entries(
        &state,
        "scan",
        vec![entry("first"), entry("second")]
    ));
    let (entered, waiting) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let preparing = state.clone();
    let worker = std::thread::spawn(move || {
        prepare_artwork(
            &preparing,
            "scan",
            || false,
            |path| {
                if path.ends_with("second.jpg") {
                    entered.send(()).unwrap();
                    resume.recv().unwrap();
                }
                Some("thumbnail".into())
            },
        )
    });
    waiting
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    {
        let review = state
            .try_lock()
            .expect("Image preparation cannot block status reads");
        assert_eq!(review.entries.len(), 2);
        assert_eq!(review.entries[0].image.as_deref(), Some("thumbnail"));
        assert!(review.entries[1].image.is_none());
        assert_eq!(review.libraries[0].status, "preparing");
        assert_eq!(review.status, "scanning");
        assert!(crate::games::discovery_review::apply(
            &Default::default(),
            &review,
            &serde_json::json!({"scanId":"scan","actions":[]})
        )
        .is_err());
    }
    release.send(()).unwrap();
    assert!(worker.join().unwrap());
    let review = state.lock().unwrap();
    assert_eq!(review.status, "ready");
    assert_eq!(review.libraries[0].status, "complete");
    assert!(review.entries.iter().all(|entry| entry.image.is_some()));
}

#[test]
fn late_artwork_cannot_overwrite_a_different_scan() {
    let state = Mutex::new(state());
    assert!(publish_entries(&state, "scan", vec![entry("first")]));
    assert!(!prepare_artwork(
        &state,
        "scan",
        || false,
        |_| {
            let mut review = state.lock().unwrap();
            review.scan_id = "new-scan".into();
            review.entries = vec![entry("new")];
            Some("old-thumbnail".into())
        }
    ));
    assert!(!publish_entries(&state, "scan", vec![entry("old")]));
    assert!(!prepare_artwork(
        &state,
        "scan",
        || false,
        |_| panic!("Stale work must be skipped")
    ));
    let review = state.lock().unwrap();
    assert_eq!(review.status, "scanning");
    assert_eq!(review.entries[0].id, "new");
    assert!(review.entries[0].image.is_none());
}

#[test]
fn missing_artwork_and_failed_libraries_do_not_prevent_review() {
    let mut review = state();
    review.libraries.push(Library {
        id: "steam".into(),
        name: "Steam".into(),
        status: "failed".into(),
        found: 0,
        message: Some("Unreadable manifest".into()),
    });
    let state = Mutex::new(review);
    assert!(publish_entries(&state, "scan", vec![entry("first")]));
    assert!(prepare_artwork(&state, "scan", || false, |_| None));
    let review = state.lock().unwrap();
    assert_eq!(review.status, "ready");
    assert_eq!(review.libraries[0].status, "complete");
    assert_eq!(review.libraries[1].status, "failed");
    assert!(review.entries[0].image.is_none());
}

#[test]
fn closed_workers_do_not_publish_completion_and_finished_scans_ignore_repeated_work() {
    let state = Mutex::new(state());
    assert!(publish_entries(&state, "scan", vec![entry("first")]));
    assert!(!prepare_artwork(
        &state,
        "scan",
        || true,
        |_| panic!("Closed worker")
    ));
    assert!(prepare_artwork(&state, "scan", || false, |_| None));
    assert!(!publish_entries(&state, "scan", vec![entry("repeat")]));
    assert!(!prepare_artwork(
        &state,
        "scan",
        || false,
        |_| panic!("Finished worker")
    ));
    assert_eq!(state.lock().unwrap().entries[0].id, "first");
}

#[test]
fn artwork_budget_does_not_overflow_or_hide_the_discovered_rows() {
    let state = Mutex::new(state());
    assert!(publish_entries(
        &state,
        "scan",
        vec![entry("first"), entry("second")]
    ));
    assert!(prepare_artwork(
        &state,
        "scan",
        || false,
        |_| Some("x".repeat(ARTWORK_BUDGET))
    ));
    let review = state.lock().unwrap();
    assert_eq!(review.entries.len(), 2);
    assert_eq!(
        review
            .entries
            .iter()
            .filter_map(|entry| entry.image.as_ref())
            .map(String::len)
            .sum::<usize>(),
        ARTWORK_BUDGET
    );
    assert_eq!(review.status, "ready");
}

#[test]
fn unchanged_installation_artwork_is_prepared_once_without_creating_actions() {
    let state = Mutex::new(state());
    let path = PathBuf::from("C:/Art/unchanged.jpg");
    let mut prepared = vec![];
    assert!(prepare_artwork_with_sources(&state, "scan", &[path.clone(), path.clone()], || false, |source| {
        prepared.push(source.to_owned());
        Some("thumbnail".into())
    }));
    assert_eq!(prepared, vec![path]);
    let review = state.lock().unwrap();
    assert!(review.entries.is_empty());
    assert_eq!(review.status, "ready");
}

#[test]
fn extra_source_previews_share_the_artwork_budget() {
    let state = Mutex::new(state());
    let mut calls = 0;
    assert!(prepare_artwork_with_sources(&state, "scan", &["first.jpg".into(), "second.jpg".into()], || false, |_| {
        calls += 1;
        Some("x".repeat(ARTWORK_BUDGET))
    }));
    assert_eq!(calls, 1);
    assert_eq!(state.lock().unwrap().status, "ready");
}
