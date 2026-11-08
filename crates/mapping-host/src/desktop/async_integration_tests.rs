use super::*;
use std::{sync::mpsc, time::{Duration, Instant}};

struct BlockedArm {
    worker: Worker<Recording>,
    entered: Option<mpsc::SyncSender<()>>,
    resume: mpsc::Receiver<()>,
}
impl Channel for BlockedArm {
    fn request(&mut self, request: &Request) -> Result<Ack, String> {
        if matches!(request.command, osg_desktop_output::protocol::Command::Arm { .. }) {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
                self.resume.recv_timeout(Duration::from_secs(3)).map_err(|error| error.to_string())?;
            }
        }
        Ok(self.worker.receive(request.clone(), 10))
    }
}

#[test]
fn actual_desktop_poll_keeps_reducing_inputs_and_revokes_a_blocked_destination() {
    let (mut desktop, mut phase, focused, events) = prepared_fixture(Action::KeyHold { key: Key(4) }, false);
    let (entered, arrival) = mpsc::sync_channel(1);
    let (resume, release) = mpsc::sync_channel(1);
    let (wake, notifications) = mpsc::sync_channel(32);
    desktop.output = Output::threaded(Coordinator::with_channel(Box::new(BlockedArm {
        worker: Worker::new(Recording { focused, events: events.clone() }), entered: Some(entered), resume: release,
    }), clock), Arc::new(move || { let _ = wake.try_send(()); }), BTreeMap::new()).unwrap();
    desktop.start_focused(&phase, true).unwrap();
    arrival.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(events.lock().unwrap().is_empty(), "Neutral preparation submitted native input");
    let frames = frame(&mut phase, 2, 2, Reason::Change, true);
    let before = Instant::now();
    desktop.poll_focused(&frames, &phase, true).unwrap();
    assert!(before.elapsed() < Duration::from_millis(100), "Profile input waited for destination IPC");
    // The same canonical reducer and desktop poll continue before releasing IPC.
    let frames = frame(&mut phase, 3, 3, Reason::Change, false);
    desktop.poll_focused(&frames, &phase, true).unwrap();
    assert_eq!(phase.sample(&endpoint("source", "button")), Some(&boolean(false)));
    let frames = frame(&mut phase, 4, 4, Reason::Disconnect, false);
    desktop.poll_focused(&frames, &phase, true).unwrap();
    assert!(events.lock().unwrap().is_empty());
    // Discard setup notifications, then require an unsolicited completion wake.
    while notifications.try_recv().is_ok() {}
    resume.send(()).unwrap();
    notifications.recv_timeout(Duration::from_secs(1)).unwrap();
    desktop.stop().unwrap();
    assert!(events.lock().unwrap().is_empty(), "Revoked press reached native input after blocked Arm");
    assert!(desktop.output.snapshot().cleanup_complete);
    assert!(!desktop.output.pending());
}
