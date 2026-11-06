use super::*;
use std::time::{Duration, Instant};

fn batch(revision: u64) -> ProviderBatch {
    ProviderBatch { scope: "profile-game:dcs".into(), session: "reader".into(), revision,
        complete: true, observations: vec![] }
}
fn completion(publication: &mut Publication, identity: &Identity) -> Option<Result<bool, String>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let result = publication.poll(Some(identity));
        if result.is_some() || publication.pending.is_none() { return result; }
        assert!(Instant::now() < deadline, "Game publication worker did not finish");
        std::thread::yield_now();
    }
}

#[test]
fn publication_wakes_host_after_queueing_completion() {
    let wake = Arc::new(NativeWake::default());
    wake.bind_thread(std::thread::current()).unwrap();
    let mut publication = Publication::with_wake(Some(&wake));
    publication.publish(batch(1));
    let observed = wake.observed();
    publication.start(|_, _| Ok(true)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while wake.observed() == observed {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "Publication did not wake the host");
        wake.wait(observed, remaining);
    }
    assert!(publication.poll(Some(&("dcs".into(), "reader".into()))).unwrap().unwrap());
}

#[test]
fn blocked_publication_never_blocks_poll_or_spawns_more_than_one_worker() {
    let identity = ("dcs".into(), "reader".into());
    let mut publication = Publication::default();
    publication.publish(batch(1));
    let (entered, waiting) = mpsc::channel();
    let (release, held) = mpsc::channel();
    publication.start(move |_, cancelled| {
        entered.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(2)).unwrap();
        Ok(!cancelled.load(Ordering::Acquire))
    }).unwrap();
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    let began = Instant::now();
    assert!(publication.poll(Some(&identity)).is_none());
    publication.publish(batch(2));
    publication.publish(batch(3));
    publication.start(|_, _| panic!("Blocked publication spawned another worker")).unwrap();
    assert!(began.elapsed() < Duration::from_millis(100));
    assert!(publication.waiting(&identity));
    release.send(()).unwrap();
    assert!(completion(&mut publication, &identity).unwrap().unwrap());
    publication.start(|change, _| {
        let Change::Publish(batch) = change else { panic!() };
        assert_eq!(batch.revision, 3, "Only the newest coalesced update may run next");
        Ok(true)
    }).unwrap();
    assert!(completion(&mut publication, &identity).unwrap().unwrap());
    assert!(!publication.waiting(&identity));
}

#[test]
fn revocation_retains_blocked_worker_and_queues_exact_reader_withdrawal() {
    let identity = ("dcs".into(), "reader".into());
    let mut publication = Publication::default();
    let (entered, waiting) = mpsc::channel();
    let (release, held) = mpsc::channel();
    publication.publish(batch(1));
    publication.start(move |_, cancelled| {
        entered.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(cancelled.load(Ordering::Acquire));
        Ok(false)
    }).unwrap();
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    publication.withdraw(identity.clone());
    publication.withdraw(identity.clone());
    assert!(!publication.waiting(&identity));
    publication.start(|_, _| panic!("Revocation freed a still-blocked worker slot")).unwrap();
    release.send(()).unwrap();
    assert!(completion(&mut publication, &identity).is_none(), "Cancelled completion retained authority");
    publication.start(move |change, _| {
        assert!(matches!(change, Change::Withdraw(_)));
        Ok(true)
    }).unwrap();
    assert!(completion(&mut publication, &identity).unwrap().unwrap());
    publication.withdraw(identity.clone());
    publication.start(|_, _| panic!("Completed reader withdrawal was repeated")).unwrap();
    assert!(publication.pending.is_none());
}

#[test]
fn old_reader_completion_and_dropped_owner_have_no_authority() {
    let mut publication = Publication::default();
    let (entered, waiting) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let (result, observed) = mpsc::channel();
    publication.publish(batch(1));
    publication.start(move |_, cancelled| {
        entered.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(2)).unwrap();
        result.send(cancelled.load(Ordering::Acquire)).unwrap();
        Ok(true)
    }).unwrap();
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(publication);
    release.send(()).unwrap();
    assert!(observed.recv_timeout(Duration::from_secs(1)).unwrap());
    let mut publication = Publication::default();
    publication.publish(batch(1));
    publication.start(|_, _| Ok(true)).unwrap();
    assert!(completion(&mut publication, &("dcs".into(), "incoming-reader".into())).is_none());
}

#[test]
fn returning_reader_releases_its_temporary_watch_after_a_fresh_source_reply() {
    let (mut reader, address, _catalogue) = crate::profile_session_games::test_baseline_reader();
    let directory = crate::test_directory();
    let games = crate::games::GamesRuntime::test_runtime(
        Arc::new(|_| serde_json::json!({})),
        crate::games::Paths::in_directory(directory.path()),
        Arc::new(crate::game_discovery::NativePlatform),
    );
    let old = Arc::new(osg_streams::NativeWake::default());
    reader.set_wake(Some(old.clone()));
    let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let finished = Arc::new(AtomicBool::new(false));
    let producer_finished = finished.clone();
    let producer = std::thread::spawn(move || {
        for counter in 1..=50 {
            if producer_finished.load(Ordering::Acquire) { break; }
            sender.send_to(&crate::profile_session_games::test_baseline_export(counter, true), address).unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let result = resume_batch(&mut reader, &games, 10_000);
    finished.store(true, Ordering::Release);
    let error = result.unwrap_err();
    producer.join().unwrap();
    // The authored decoder has no selected simulator proof, so it cannot
    // produce a real reader publication even after receiving a fresh baseline.
    assert_eq!(error, "The returning game has no complete reader publication.");
    assert!(Arc::ptr_eq(&reader.notification_wake().unwrap(), &old));
}
