use super::*;
use crate::game_telemetry::SourceIdentity;
use std::sync::mpsc;

struct Input(mpsc::Receiver<RawSample>);
impl Source for Input {
    fn poll(&mut self) -> Result<Option<RawSample>, String> {
        Ok(self.0.try_recv().ok())
    }
    fn close(&mut self) {}
}

struct Intermittent(mpsc::Receiver<Option<RawSample>>, mpsc::Sender<()>);
impl Source for Intermittent {
    fn poll(&mut self) -> Result<Option<RawSample>, String> {
        match self.0.try_recv() {
            Ok(Some(sample)) => Ok(Some(sample)),
            Ok(None) => {
                let _ = self.1.send(());
                Ok(None)
            }
            Err(_) => Ok(None),
        }
    }
    fn close(&mut self) {}
}

fn sample(packet: u32) -> RawSample {
    RawSample {
        source: SourceIdentity {
            id: "test".into(),
            epoch: 1,
        },
        physics: packet.to_le_bytes().to_vec(),
        graphics: vec![],
        statics: vec![],
        capture: None,
    }
}

fn wait_for(mut condition: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(3);
    while !condition() {
        assert!(
            Instant::now() < end,
            "Assetto producer did not publish in time"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn process_monitor_proof_exists_before_the_first_capture_and_retires_on_loss() {
    struct Silent(Weak<crate::xplane::process_events::Monitor>);
    impl Source for Silent {
        fn poll(&mut self) -> Result<Option<RawSample>, String> { Ok(None) }
        fn close(&mut self) {}
        fn process_monitor(&self) -> Option<Weak<crate::xplane::process_events::Monitor>> { Some(self.0.clone()) }
        fn terminal_error(&self) -> Option<String> {
            self.0.upgrade().is_none().then(|| "Process monitor stopped.".into())
        }
    }

    let (monitor, _) = crate::xplane::process_events::test_monitor();
    let monitor = Arc::new(monitor);
    let identity = ProcessIdentity { pid: 1, epoch: 1, path: "test.exe".into() };
    let owners = OwnerSlots::default();
    let mut source = SharedSource::attach(&owners, &identity, Game::Competizione, true, || Ok(Silent(Arc::downgrade(&monitor)))).unwrap();
    let wake = Arc::new(osg_streams::NativeWake::default());
    let watch = source.watch(wake.clone()).unwrap().unwrap();
    assert!(source.poll().unwrap().is_none());
    assert_eq!(watch.process_current(), Some(true));
    let observed = wake.observed();
    drop(monitor);
    assert_eq!(watch.process_current(), Some(false));
    wait_for(|| wake.observed() != observed && watch.is_closed());
    assert_eq!(source.poll().unwrap_err(), "Process monitor stopped.");
}

#[test]
fn one_owner_delivers_independent_ordered_and_latest_reads_and_releases_on_drop() {
    let owners = OwnerSlots::default();
    let game = Game::Evo;
    let identity = ProcessIdentity {
        pid: 1,
        epoch: 1,
        path: "test.exe".into(),
    };
    let (send, input) = mpsc::channel();
    let mut ordered = SharedSource::attach(&owners, &identity, game, false, || Ok(Input(input))).unwrap();
    let mut latest =
        SharedSource::attach::<Input>(&owners, &identity, game, true, || panic!("duplicate owner")).unwrap();
    let wake = Arc::new(osg_streams::NativeWake::default());
    let watch = ordered.watch(wake.clone()).unwrap().unwrap();
    assert!(ordered.poll().unwrap().is_none());
    let revision = wake.observed();
    for packet in [1, 1, 2, 3] {
        send.send(sample(packet)).unwrap();
    }
    wait_for(|| wake.observed() >= revision + 3);
    for packet in 1u32..=3 {
        assert_eq!(
            ordered.poll().unwrap().unwrap().physics,
            packet.to_le_bytes()
        );
    }
    assert!(ordered.poll().unwrap().is_none());
    assert_eq!(latest.poll().unwrap().unwrap().physics, 3u32.to_le_bytes());
    let before_context = wake.observed();
    let mut context = sample(3);
    context.graphics = vec![1];
    send.send(context.clone()).unwrap();
    context.statics = vec![2];
    send.send(context).unwrap();
    wait_for(|| wake.observed() >= before_context + 2);
    let graphics = ordered.poll().unwrap().unwrap();
    assert_eq!(graphics.physics, 3u32.to_le_bytes());
    assert_eq!(graphics.graphics, [1]);
    assert!(graphics.statics.is_empty());
    let statics = ordered.poll().unwrap().unwrap();
    assert_eq!(statics.physics, 3u32.to_le_bytes());
    assert_eq!(statics.statics, [2]);
    assert_eq!(latest.poll().unwrap().unwrap().statics, [2]);
    let before_overflow = wake.observed();
    for packet in 4..=4 + LIMITS.max_pending_frames as u32 {
        send.send(sample(packet)).unwrap();
    }
    wait_for(|| wake.observed() > before_overflow + LIMITS.max_pending_frames as u64);
    assert!(ordered.poll().unwrap_err().contains("lost continuity"));
    let baseline = ordered.poll().unwrap().unwrap();
    assert_eq!(
        baseline.physics,
        (4 + LIMITS.max_pending_frames as u32).to_le_bytes()
    );
    let replacement = ProcessIdentity {
        epoch: 2,
        ..identity
    };
    assert!(
        SharedSource::attach::<Input>(&owners, &replacement, game, false, || panic!("live owner")).is_err()
    );
    let done = ordered.owner.done.clone();
    drop(ordered);
    drop(latest);
    wait_for(|| done.load(Ordering::Acquire));
    assert!(watch.is_closed());
    let (_, next) = mpsc::channel();
    assert!(SharedSource::attach(&owners, &replacement, game, false, || Ok(Input(next))).is_ok());
}

#[test]
fn contended_capture_is_skipped_without_error_or_refreshing_the_last_sample() {
    let owners = OwnerSlots::default();
    let identity = ProcessIdentity { pid: 3, epoch: 1, path: "test.exe".into() };
    let (send, input) = mpsc::channel();
    let (observed_tx, observed_rx) = mpsc::channel();
    let mut source = SharedSource::attach(&owners, &identity, Game::Rally, false, || {
        Ok(Intermittent(input, observed_tx))
    }).unwrap();
    let wake = Arc::new(osg_streams::NativeWake::default());
    let watch = source.watch(wake.clone()).unwrap().unwrap();
    send.send(Some(sample(10))).unwrap();
    let mut received = None;
    wait_for(|| {
        received = source.poll().unwrap();
        received.is_some()
    });
    assert_eq!(received.unwrap().physics, 10u32.to_le_bytes());

    send.send(None).unwrap();
    observed_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(source.poll().unwrap().is_none(), "contention must not publish an error frame");
    assert!(source.terminal_error().is_none());
    assert!(!source.owner.done.load(Ordering::Acquire), "transient contention must keep the capture owner alive");
    assert!(!watch.is_closed());

    send.send(Some(sample(11))).unwrap();
    let mut received = None;
    wait_for(|| {
        received = source.poll().unwrap();
        received.is_some()
    });
    assert_eq!(received.unwrap().physics, 11u32.to_le_bytes());
    assert!(source.terminal_error().is_none());
}

#[test]
fn failed_process_lease_releases_the_owner_even_with_an_attached_reader() {
    struct Retired(Arc<AtomicBool>);
    impl Source for Retired {
        fn poll(&mut self) -> Result<Option<RawSample>, String> {
            self.terminal_error().map_or(Ok(None), Err)
        }
        fn close(&mut self) {}
        fn terminal_error(&self) -> Option<String> { self.0.load(Ordering::Acquire).then(|| "Process lease lost.".into()) }
    }

    let owners = OwnerSlots::default();
    let identity = ProcessIdentity { pid: 2, epoch: 1, path: "test.exe".into() };
    let game = Game::Corsa;
    let failed = Arc::new(AtomicBool::new(false));
    let mut retired = SharedSource::attach(&owners, &identity, game, false, || Ok(Retired(failed.clone()))).unwrap();
    failed.store(true, Ordering::Release);
    wait_for(|| retired.owner.done.load(Ordering::Acquire));
    assert_eq!(retired.poll().unwrap_err(), "Process lease lost.");
    assert_eq!(retired.terminal_error().as_deref(), Some("Process lease lost."));

    let (_, input) = mpsc::channel();
    let replacement = SharedSource::attach(&owners, &identity, game, false, || Ok(Input(input))).unwrap();
    assert!(!Arc::ptr_eq(&retired.owner, &replacement.owner));
}

#[test]
fn ordered_reader_rejects_an_invalid_process_identity_before_attachment() {
    let identity = ProcessIdentity { pid: 0, epoch: 1, path: "acr.exe".into() };
    assert!(SharedSource::open(&identity, Game::Rally).is_err());
}

#[test]
fn held_variant_open_does_not_block_another_variant() {
    let owners = OwnerSlots::default();
    let slow = ProcessIdentity { pid: 41, epoch: 1, path: "slow.exe".into() };
    let fast = ProcessIdentity { pid: 42, epoch: 1, path: "fast.exe".into() };
    thread::scope(|scope| {
        let (entered, opening) = mpsc::channel();
        let (release, finish) = mpsc::channel();
        let (fast_result, fast_reader) = mpsc::channel();
        let owners = &owners;
        let slow_identity = &slow;
        let fast_identity = &fast;
        let slow = scope.spawn(move || SharedSource::attach(owners, slow_identity, Game::Corsa, false, || {
            entered.send(()).unwrap();
            finish.recv().map_err(|error| error.to_string())?;
            Err::<Input, _>("held source failed to open".into())
        }));
        opening.recv_timeout(Duration::from_secs(3)).unwrap();
        let other = scope.spawn(move || {
            let (_, input) = mpsc::channel();
            fast_result.send(SharedSource::attach(owners, fast_identity, Game::Evo, true, || Ok(Input(input)))).unwrap();
        });
        let before_release = fast_reader.recv_timeout(Duration::from_secs(3));
        release.send(()).unwrap();
        assert_eq!(slow.join().unwrap().err().as_deref(), Some("held source failed to open"));
        other.join().unwrap();
        let first = before_release.expect("an unrelated game was blocked by a source open").unwrap();
        let second = SharedSource::attach::<Input>(owners, &fast, Game::Evo, false, || panic!("duplicate owner")).unwrap();
        assert!(Arc::ptr_eq(&first.owner, &second.owner));
    });
}
