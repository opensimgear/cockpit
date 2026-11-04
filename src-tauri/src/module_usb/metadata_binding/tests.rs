use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

#[derive(Default)]
struct Cancel(AtomicUsize);
impl CancelIo for Cancel {
    fn cancel(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn capture() -> (Capture, Arc<NativeWake>, Arc<Cancel>) {
    let wake = Arc::new(NativeWake::default());
    let cancel = Arc::new(Cancel::default());
    let capture = Capture::new(7, wake.clone(), cancel.clone());
    (capture, wake, cancel)
}

fn line(sequence: u32, kind: &str, revision: u32, generation: u32) -> String {
    format!(
        "@meta {}",
        json!({
            "boot":7,"sequence":sequence,"kind":kind,"revision":revision,
            "exhausted":false,"state":{"generation":generation,"savedGeneration":generation,
            "owner":0,"claimExpiresMs":0,"dirty":false,"pendingApply":false}
        })
    )
}

fn send(capture: &Capture, line: String, at: Instant) {
    capture.accept(Input::Line {
        line,
        received_at: at,
    });
}

#[test]
fn bounded_latest_coalesces_frames_and_preserves_the_exact_receive_time() {
    let (capture, wake, _) = capture();
    let first_at = Instant::now();
    send(&capture, line(1, "baseline", 1, 4), first_at);
    let second_at = Instant::now();
    send(&capture, line(2, "change", 2, 5), second_at);

    assert_eq!(wake.observed(), 2);
    assert_eq!(capture.current_revision(), 2);
    assert!(capture.take_changed(2).is_none());
    let snapshot = capture.take_changed(0).unwrap();
    let latest = snapshot.latest.unwrap();
    assert_eq!(snapshot.revision, 2);
    assert_eq!(latest.sequence, 2);
    assert_eq!(latest.state.generation, 5);
    assert_eq!(latest.received_at, second_at);
}

#[test]
fn strict_decode_failure_is_terminal_sticky_and_cancels_once_outside_the_lock() {
    let (capture, wake, cancel) = capture();
    send(&capture, line(1, "baseline", 1, 4), Instant::now());
    // A sequence gap is a strict decoder failure.
    send(&capture, line(3, "change", 2, 5), Instant::now());
    let failed = capture.snapshot();
    assert!(failed.terminal);
    assert!(failed.error.is_some());
    assert_eq!(failed.revision, 2);
    assert_eq!(wake.observed(), 2);
    assert_eq!(cancel.0.load(Ordering::SeqCst), 1);

    capture.accept(Input::Lost("later transport loss".into()));
    send(&capture, line(2, "change", 2, 5), Instant::now());
    assert_eq!(cancel.0.load(Ordering::SeqCst), 1);
    assert_eq!(capture.snapshot().error, failed.error);
    assert_eq!(capture.snapshot().revision, failed.revision);
}

#[test]
fn terminal_exhaustion_keeps_the_last_frame_but_withdraws_authority() {
    let (capture, wake, cancel) = capture();
    let at = Instant::now();
    let terminal = line(1, "baseline", 1, 4).replace("\"exhausted\":false", "\"exhausted\":true");
    send(&capture, terminal, at);

    let snapshot = capture.snapshot();
    assert!(snapshot.terminal);
    assert_eq!(
        snapshot.error.as_deref(),
        Some("USB metadata sequence is exhausted.")
    );
    let latest = snapshot
        .latest
        .expect("terminal frame remains available for diagnostics");
    assert!(latest.exhausted);
    assert_eq!(latest.received_at, at);
    assert_eq!(snapshot.revision, 1);
    assert_eq!(wake.observed(), 1);
    assert_eq!(cancel.0.load(Ordering::SeqCst), 1);
}

#[test]
fn local_revision_exhaustion_is_terminal_instead_of_hiding_a_new_latest_frame() {
    let (capture, wake, cancel) = capture();
    capture.lock().snapshot.revision = u64::MAX - 1;
    let at = Instant::now();
    send(&capture, line(1, "baseline", 1, 4), at);

    let snapshot = capture.snapshot();
    assert_eq!(snapshot.revision, u64::MAX);
    assert!(snapshot.terminal);
    assert_eq!(
        snapshot.error.as_deref(),
        Some("USB metadata capture revision is exhausted.")
    );
    assert_eq!(snapshot.latest.unwrap().received_at, at);
    assert_eq!(wake.observed(), 1);
    assert_eq!(cancel.0.load(Ordering::SeqCst), 1);

    send(&capture, line(2, "change", 2, 5), Instant::now());
    assert_eq!(capture.current_revision(), u64::MAX);
    assert_eq!(wake.observed(), 1);
    assert_eq!(cancel.0.load(Ordering::SeqCst), 1);
}

#[test]
fn unrelated_console_lines_and_events_are_ignored() {
    let (capture, wake, cancel) = capture();
    let at = Instant::now();
    send(&capture, "@events {\"some\":\"event\"}".into(), at);
    send(&capture, "ordinary reply".into(), at);
    send(&capture, "@metaX {}".into(), at);
    assert_eq!(capture.current_revision(), 0);
    assert_eq!(wake.observed(), 0);
    assert_eq!(cancel.0.load(Ordering::SeqCst), 0);
    assert!(capture.snapshot().latest.is_none());
}

struct ReentrantCancel {
    capture: Mutex<Option<std::sync::Weak<Capture>>>,
    count: AtomicUsize,
    lock_was_free: AtomicUsize,
}
impl CancelIo for ReentrantCancel {
    fn cancel(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
        if let Some(capture) = self
            .capture
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|c| c.upgrade())
        {
            if capture.state.try_lock().is_ok() {
                self.lock_was_free.fetch_add(1, Ordering::SeqCst);
            }
            let _ = capture.snapshot();
        }
    }
}

#[test]
fn raw_cancel_callback_runs_after_releasing_the_capture_lock() {
    let cancel = Arc::new(ReentrantCancel {
        capture: Mutex::new(None),
        count: AtomicUsize::new(0),
        lock_was_free: AtomicUsize::new(0),
    });
    let capture = Arc::new(Capture::new(
        7,
        Arc::new(NativeWake::default()),
        cancel.clone(),
    ));
    *cancel.capture.lock().unwrap() = Some(Arc::downgrade(&capture));
    send(&capture, "@meta {}".into(), Instant::now());
    assert_eq!(cancel.count.load(Ordering::SeqCst), 1);
    assert_eq!(cancel.lock_was_free.load(Ordering::SeqCst), 1);
}

#[test]
fn input_loss_marks_the_capture_terminal_and_wakes_the_worker() {
    let (capture, wake, cancel) = capture();
    capture.accept(Input::Lost("disconnected".into()));
    let snapshot = capture.snapshot();
    assert!(snapshot.terminal);
    assert_eq!(snapshot.error.as_deref(), Some("disconnected"));
    assert_eq!(snapshot.revision, 1);
    assert_eq!(wake.observed(), 1);
    assert_eq!(cancel.0.load(Ordering::SeqCst), 0);
}
