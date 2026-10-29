use super::*;

#[test]
fn blocked_validation_does_not_block_poll_or_refresh_expired_evidence() {
    let (entered, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let mut validation = Validation::default();
    validation.seed(Instant::now() - FRESH);
    validation.start(move || { entered.send(()).unwrap(); blocked.recv().unwrap(); Ok(()) });
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
    }) };
    completed.send((Instant::now() - FRESH, Ok(()))).unwrap();
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
    replacement.start(|| Ok(()));
    release.send(()).unwrap();
    assert!(replacement.poll().unwrap());
}

#[test]
fn actual_validation_failure_is_not_hidden_by_a_fresh_seed() {
    let (requests, _incoming) = mpsc::channel();
    let (completed, results) = mpsc::channel();
    let mut validation = Validation { verified: Some(Instant::now()), worker: Some(Worker {
        requests, results, cancelled: Arc::new(AtomicBool::new(false)), pending: true,
    }) };
    completed.send((Instant::now(), Err("changed identity".into()))).unwrap();
    assert_eq!(validation.poll().unwrap_err(), "changed identity");
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
