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
fn one_owner_delivers_independent_ordered_and_latest_reads_and_releases_on_drop() {
    let game = Game::Evo;
    let identity = ProcessIdentity {
        pid: 1,
        epoch: 1,
        path: "test.exe".into(),
    };
    let (send, input) = mpsc::channel();
    let mut ordered = SharedSource::attach(&identity, game, false, || Ok(Input(input))).unwrap();
    let mut latest =
        SharedSource::attach::<Input>(&identity, game, true, || panic!("duplicate owner")).unwrap();
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
        SharedSource::attach::<Input>(&replacement, game, false, || panic!("live owner")).is_err()
    );
    let done = ordered.owner.done.clone();
    drop(ordered);
    drop(latest);
    wait_for(|| done.load(Ordering::Acquire));
    assert!(watch.is_closed());
    let (_, next) = mpsc::channel();
    assert!(SharedSource::attach(&replacement, game, false, || Ok(Input(next))).is_ok());
}
