use super::*;

#[test]
fn blocked_validation_does_not_block_poll_or_refresh_expired_evidence() {
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let mut validation = Validation::default();
    validation.seed(Instant::now() - FRESH);
    validation.start(move || { entered.send(()).unwrap(); blocked.recv().unwrap(); Ok(Instant::now()) });
    assert!(!validation.poll().unwrap());
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    let began = Instant::now();
    for _ in 0..100 { assert!(!validation.poll().unwrap()); }
    assert!(began.elapsed() < FRESH);
    validation.cancel();
    release.send(()).unwrap();
}

#[test]
fn late_success_is_never_redated_to_its_completion() {
    let (requests, _incoming) = mpsc::channel();
    let (completed, results) = mpsc::channel();
    let mut validation = Validation { verified: None, worker: Some(Worker {
        requests, results, cancelled: Arc::new(AtomicBool::new(false)), pending: true,
    }), ..Validation::default() };
    completed.send(Ok(Instant::now() - FRESH)).unwrap();
    assert!(!validation.poll().unwrap());
}

#[test]
fn old_reader_completion_cannot_change_replacement_validation() {
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let mut old = Validation::default();
    old.start(move || { entered.send(()).unwrap(); blocked.recv().unwrap(); Err("old identity failed".into()) });
    assert!(!old.poll().unwrap());
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    old.cancel();
    let mut replacement = Validation::default();
    replacement.seed(Instant::now());
    replacement.start(|| Ok(Instant::now()));
    release.send(()).unwrap();
    assert!(replacement.poll().unwrap());
}

#[test]
fn actual_validation_failure_is_not_hidden_by_a_fresh_seed() {
    let (requests, _incoming) = mpsc::channel();
    let (completed, results) = mpsc::channel();
    let mut validation = Validation { verified: Some(Instant::now()), worker: Some(Worker {
        requests, results, cancelled: Arc::new(AtomicBool::new(false)), pending: true,
    }), ..Validation::default() };
    completed.send(Err("changed identity".into())).unwrap();
    assert_eq!(validation.poll().unwrap_err(), "changed identity");
}

#[test]
fn worker_returns_process_capture_instead_of_queue_or_catalogue_start() {
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let (captured, capture) = mpsc::channel();
    let mut validation = Validation::default();
    validation.start(move || {
        entered.send(()).unwrap();
        // Model arbitrarily slow queued/catalogue work before native capture.
        blocked.recv().unwrap();
        let process_capture = Instant::now();
        captured.send(process_capture).unwrap();
        Ok(process_capture)
    });
    // Advancing from the enqueue time must not pre-age the later native proof.
    assert!(!validation.poll_at(Instant::now() - FRESH).unwrap());
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    release.send(()).unwrap();
    let process_capture = capture.recv_timeout(Duration::from_secs(2)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !validation.poll().unwrap() { assert!(Instant::now() < deadline); }
    assert_eq!(validation.verified, Some(process_capture));
    validation.cancel();
}

#[test]
fn refresh_is_bounded_and_process_capture_still_expires_at_100ms() {
    let (requests, incoming) = mpsc::channel();
    let (_completed, results) = mpsc::channel();
    let began = Instant::now();
    let mut validation = Validation { verified: Some(began), worker: Some(Worker {
        requests, results, cancelled: Arc::new(AtomicBool::new(false)), pending: false,
    }), ..Validation::default() };
    assert!(validation.poll_at(began + REFRESH - Duration::from_nanos(1)).unwrap());
    assert!(incoming.try_recv().is_err());
    assert!(validation.poll_at(began + REFRESH).unwrap());
    incoming.try_recv().unwrap();
    assert!(validation.poll_at(began + FRESH - Duration::from_nanos(1)).unwrap());
    assert!(incoming.try_recv().is_err());
    assert!(!validation.poll_at(began + FRESH).unwrap());
}

#[test]
fn pending_withdraws_once_and_resumes_only_with_a_fresh_baseline() {
    use crate::profile_session_games::{Reader, Reason};
    let (mut source, _, _folder) = crate::profile_session_games::test_baseline_reader();
    let feed = |source: &mut crate::profile_session_games::GameSources, counter, now| {
        let Some(Reader::Dcs { decoder, .. }) = &mut source.reader else { panic!() };
        decoder.feed(&crate::profile_session_games::test_baseline_export(counter, true), now / 1000);
        let snapshot = decoder.snapshot(now / 1000);
        source.frames(snapshot, now)
    };
    feed(&mut source, 1, 10_000);
    assert!(feed(&mut source, 2, 20_000).fresh);
    let pending = source.pending(30_000, "validation pending".into());
    assert!(!pending.context_lost && !pending.fresh && !pending.frames.is_empty());
    assert!(pending.frames.iter().all(|frame| frame.reason == Reason::Gap));
    assert!(!source.can_command());
    assert!(source.reader.is_some());
    assert!(source.pending(40_000, "still pending".into()).frames.is_empty());
    assert!(!feed(&mut source, 3, 50_000).fresh);
    let resumed = feed(&mut source, 4, 60_000);
    assert!(resumed.fresh);
    assert!(resumed.frames.iter().all(|frame| frame.reason == Reason::Baseline));
}

#[test]
fn blocked_catalogue_has_its_own_250ms_horizon_while_process_observations_continue() {
    let began = Instant::now();
    let (release, blocked) = mpsc::channel();
    let (entered, observed) = mpsc::channel();
    let (captures, incoming) = mpsc::channel();
    let mut process = Validation::default();
    let mut catalogue = Validation::catalogue();
    process.seed(began);
    catalogue.seed(began);
    process.start(move || incoming.recv().map_err(|_| "capture closed".into()));
    catalogue.start(move || {
        let capture = Instant::now();
        entered.send(()).unwrap();
        blocked.recv().unwrap();
        Ok(capture)
    });
    // Advance policy time deterministically; the catalogue worker stays blocked
    // throughout independent, successful native process observations.
    for milliseconds in [125, 150, 200, 249] {
        let now = began + Duration::from_millis(milliseconds);
        process.poll_with_at(Some(&mut catalogue), now).unwrap();
        if milliseconds == 125 { observed.recv_timeout(Duration::from_secs(2)).unwrap(); }
        captures.send(now).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let ready = process.poll_with_at(Some(&mut catalogue), now).unwrap();
            if process.verified == Some(now) { assert!(ready); break; }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
    assert!(!process.poll_with_at(Some(&mut catalogue), began + Duration::from_millis(250)).unwrap());
    assert!(process.poll_at(began + Duration::from_millis(250)).unwrap());
    process.cancel();
    catalogue.cancel();
    release.send(()).unwrap();
}

#[test]
fn blocked_process_expires_at_100ms_even_with_fresh_catalogue_evidence() {
    let began = Instant::now();
    let (release, blocked) = mpsc::channel();
    let (entered, observed) = mpsc::channel();
    let mut process = Validation::default();
    let mut catalogue = Validation::catalogue();
    process.seed(began);
    catalogue.seed(began);
    process.start(move || {
        let capture = Instant::now();
        entered.send(()).unwrap();
        blocked.recv().unwrap();
        Ok(capture)
    });
    catalogue.start(|| Ok(Instant::now()));
    assert!(process.poll_with_at(Some(&mut catalogue), began + REFRESH).unwrap());
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(process.poll_with_at(Some(&mut catalogue), began + FRESH - Duration::from_nanos(1)).unwrap());
    assert!(!process.poll_with_at(Some(&mut catalogue), began + FRESH).unwrap());
    assert!(catalogue.poll_at(began + FRESH).unwrap());
    process.cancel();
    catalogue.cancel();
    release.send(()).unwrap();
}

#[test]
fn catalogue_refresh_reads_metadata_and_dates_it_before_file_io() {
    let (mut source, _, folder) = crate::profile_session_games::test_baseline_reader();
    let began = Instant::now();
    source.validate_catalogue(true).unwrap();
    let captured = source.catalogue_validation.verified.unwrap();
    assert!(captured >= began && captured <= Instant::now());
    source.start_catalogue_validation();
    // The guard's internal 250ms skip window is still current. The worker must
    // force metadata inspection instead of redating a skipped successful check.
    let path = folder.path().join("A-10C.json");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.push(b' ');
    std::fs::write(path, bytes).unwrap();
    source.catalogue_validation.seed(Instant::now() - Duration::from_millis(250));
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match source.catalogue_validation.poll() {
            Err(error) => { assert!(error.contains("catalogue")); break; }
            Ok(false) => {},
            Ok(true) => panic!("Changed catalogue was accepted without metadata inspection"),
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

#[test]
fn catalogue_failure_dominates_pending_process_and_both_proofs_are_polled() {
    let (mut source, _, _folder) = crate::profile_session_games::test_baseline_reader();
    let (process_requests, process_incoming) = mpsc::channel();
    let (_process_completed, process_results) = mpsc::channel();
    source.validation.worker = Some(Worker { requests: process_requests, results: process_results,
        cancelled: Arc::new(AtomicBool::new(false)), pending: false });
    let (catalogue_requests, _catalogue_incoming) = mpsc::channel();
    let (catalogue_completed, catalogue_results) = mpsc::channel();
    source.catalogue_validation.worker = Some(Worker { requests: catalogue_requests, results: catalogue_results,
        cancelled: Arc::new(AtomicBool::new(false)), pending: true });
    catalogue_completed.send(Err("catalogue changed".into())).unwrap();
    assert_eq!(source.validation_ready().unwrap_err(), "catalogue changed");
    process_incoming.try_recv().unwrap();
    assert!(!source.catalogue_validation.worker.as_ref().unwrap().pending);
}

#[test]
fn process_failure_still_polls_catalogue_before_retiring_the_reader() {
    let (mut source, _, _folder) = crate::profile_session_games::test_baseline_reader();
    let (process_requests, _process_incoming) = mpsc::channel();
    let (process_completed, process_results) = mpsc::channel();
    source.validation.worker = Some(Worker { requests: process_requests, results: process_results,
        cancelled: Arc::new(AtomicBool::new(false)), pending: true });
    let (catalogue_requests, catalogue_incoming) = mpsc::channel();
    let (_catalogue_completed, catalogue_results) = mpsc::channel();
    source.catalogue_validation.worker = Some(Worker { requests: catalogue_requests, results: catalogue_results,
        cancelled: Arc::new(AtomicBool::new(false)), pending: false });
    process_completed.send(Err("process changed".into())).unwrap();
    assert_eq!(source.validation_ready().unwrap_err(), "process changed");
    catalogue_incoming.try_recv().unwrap();
}

#[test]
fn stopping_reader_cancels_both_workers_and_late_completions_cannot_rearm_replacement() {
    let (mut old, _, _old_folder) = crate::profile_session_games::test_baseline_reader();
    let mut releases = Vec::new();
    let mut completions = Vec::new();
    for validation in [&mut old.validation, &mut old.catalogue_validation] {
        let (release, blocked) = mpsc::channel();
        let (entered, observed) = mpsc::channel();
        let (completed, completion) = mpsc::channel();
        validation.start(move || {
            let capture = Instant::now();
            entered.send(()).unwrap();
            blocked.recv().unwrap();
            completed.send(()).unwrap();
            Ok(capture)
        });
        assert!(!validation.poll().unwrap());
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        releases.push(release);
        completions.push(completion);
    }
    old.stop();
    let (mut replacement, _, _replacement_folder) = crate::profile_session_games::test_baseline_reader();
    for validation in [&mut replacement.validation, &mut replacement.catalogue_validation] {
        validation.seed(Instant::now());
        validation.start(|| Ok(Instant::now()));
    }
    for release in releases { release.send(()).unwrap(); }
    for completion in completions { completion.recv_timeout(Duration::from_secs(2)).unwrap(); }
    assert!(old.reader.is_none() && old.lost);
    assert!(old.validation.verified.is_none() && old.catalogue_validation.verified.is_none());
    assert!(!old.validation.started() && !old.catalogue_validation.started());
    assert!(replacement.validation_ready().unwrap());
}
