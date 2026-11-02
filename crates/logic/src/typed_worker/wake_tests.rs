use super::*;
use crate::{typed_test_support::*, typed_worker::*};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

fn completed() -> Reply {
    Reply::Completed {
        ticket: request(true).ticket,
        result: Ok(Default::default()),
    }
}

// A real framed Read transport that holds its second read open until the test
// closes it. This isolates completion from the later EOF deterministically.
struct HeldReader {
    frame: io::Cursor<Vec<u8>>,
    close: mpsc::Receiver<()>,
}
impl Read for HeldReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.frame.position() < self.frame.get_ref().len() as u64 {
            self.frame.read(bytes)
        } else {
            let _ = self.close.recv();
            Ok(0)
        }
    }
}

#[test]
fn pipe_completion_and_eof_are_committed_before_their_wakes() {
    let (wake, woken) = mpsc::channel();
    let events = Arc::new(Events::new(Arc::new(move || wake.send(()).unwrap())));
    let (replies, incoming) = mpsc::sync_channel(1);
    let (close, closed) = mpsc::channel();
    let mut frame = Vec::new();
    wire::write(&completed(), &mut frame).unwrap();
    let reader_events = events.clone();
    let reader = thread::spawn(move || {
        read_replies(
            HeldReader {
                frame: io::Cursor::new(frame),
                close: closed,
            },
            replies,
            reader_events,
        )
    });
    woken.recv_timeout(Duration::from_secs(2)).unwrap();
    let start = Instant::now();
    assert!(
        response(&incoming, &request(true), &definition(), start, || start)
            .unwrap()
            .is_ok()
    );
    assert!(events.failure().is_none());
    close.send(()).unwrap();
    woken.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(events.failure().is_some());
    assert!(incoming.try_recv().unwrap().is_err());
    reader.join().unwrap();
}

struct BrokenWriter;
impl Write for BrokenWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "closed write pipe",
        ))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn write_failure_wakes_and_survives_an_already_full_reply_mailbox() {
    for full in [false, true] {
        let (wake, woken) = mpsc::channel();
        let events = Arc::new(Events::new(Arc::new(move || wake.send(()).unwrap())));
        let (replies, incoming) = mpsc::sync_channel(1);
        if full {
            replies.send(Ok(completed())).unwrap();
        }
        let (requests, outgoing) = mpsc::sync_channel(1);
        requests
            .send(Request::Invoke {
                request: request(true),
            })
            .unwrap();
        let writer_events = events.clone();
        let writer =
            thread::spawn(move || write_requests(BrokenWriter, outgoing, replies, writer_events));
        woken.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(events.failure().as_deref(), Some("closed write pipe"));
        assert_eq!(incoming.try_recv().unwrap().is_ok(), full);
        writer.join().unwrap();
    }
}

#[test]
fn excessive_replies_fail_closed_without_blocking_or_growing_the_mailbox() {
    let (wake, woken) = mpsc::channel();
    let events = Events::new(Arc::new(move || wake.send(()).unwrap()));
    let (replies, incoming) = mpsc::sync_channel(1);
    assert!(events.publish(&replies, completed()));
    assert!(!events.publish(&replies, completed()));
    assert!(events.failure().unwrap().contains("capacity"));
    assert_eq!(woken.try_iter().count(), 2);
    assert!(incoming.try_recv().unwrap().is_ok());
    assert!(incoming.try_recv().is_err());
    drop(incoming);
    assert!(!events.publish(&replies, completed()));
}

#[test]
fn stop_releases_callback_while_pipe_thread_is_still_blocked() {
    let wake: Wake = Arc::new(|| panic!("stopped worker must not signal"));
    let weak = Arc::downgrade(&wake);
    let events = Arc::new(Events::new(wake));
    let retained_by_thread = events.clone();
    events.stop();
    assert!(weak.upgrade().is_none());
    let (replies, incoming) = mpsc::sync_channel(1);
    retained_by_thread.fail(&replies, "closed".into());
    retained_by_thread.fail(&replies, "second failure".into());
    assert_eq!(events.failure().as_deref(), Some("closed"));
    assert!(incoming.try_recv().unwrap().is_err());
}

#[test]
fn wake_before_park_leaves_a_permit_for_the_owner() {
    let (owner, thread_ready) = mpsc::channel();
    let begin_park = Arc::new(AtomicBool::new(false));
    let begin = begin_park.clone();
    let (done, finished) = mpsc::channel();
    let reader = thread::spawn(move || {
        owner.send(thread::current()).unwrap();
        // A blocking channel receive also uses park/unpark internally and
        // could consume the permit under test before reaching park().
        while !begin.load(Ordering::Acquire) {
            thread::yield_now();
        }
        thread::park();
        done.send(42).unwrap();
    });
    let owner = thread_ready.recv().unwrap();
    let events = Events::new(Arc::new(move || owner.unpark()));
    let (replies, incoming) = mpsc::sync_channel(1);
    assert!(events.publish(&replies, completed()));
    begin_park.store(true, Ordering::Release);
    assert_eq!(finished.recv_timeout(Duration::from_secs(2)).unwrap(), 42);
    reader.join().unwrap();
    assert!(incoming.try_recv().unwrap().is_ok());
}

#[test]
fn idle_pending_response_expires_at_original_deadline_without_input() {
    let (_replies, incoming) = mpsc::sync_channel(1);
    let start = Instant::now();
    for offset in [0, 20, 99] {
        assert!(
            response(&incoming, &request(true), &definition(), start, || {
                start + Duration::from_millis(offset)
            })
            .is_none()
        );
    }
    assert_eq!(
        response(&incoming, &request(true), &definition(), start, || {
            start + RESPONSE_DEADLINE
        })
        .unwrap()
        .unwrap_err(),
        RESPONSE_TIMEOUT
    );
}

#[test]
fn idle_pipe_failure_revokes_a_live_child_and_rejects_new_submissions() {
    const CHILD: &str = "OSG_TYPED_IDLE_PIPE_TEST_CHILD";
    if std::env::var_os(CHILD).is_some() {
        // Close the real stdout pipe but keep this process alive, blocked on
        // stdin. No process-death or pending-invocation check can detect it.
        #[cfg(windows)]
        {
            use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
            drop(unsafe { OwnedHandle::from_raw_handle(io::stdout().as_raw_handle()) });
        }
        #[cfg(unix)]
        unsafe {
            libc::close(1);
        }
        let mut byte = [0];
        let _ = io::stdin().read(&mut byte);
        return;
    }
    for check_submit in [false, true] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "typed_worker::transport::tests::idle_pipe_failure_revokes_a_live_child_and_rejects_new_submissions", "--nocapture"])
            .env(CHILD, "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().unwrap();
        let resources = crate::resources::Guard::attach(&child).unwrap();
        let mut output = child.stdout.take().unwrap();
        // Consume libtest's banner, then exercise the actual native pipe EOF.
        output.read_to_end(&mut Vec::new()).unwrap();
        assert!(child.try_wait().unwrap().is_none());
        let (wake, woken) = mpsc::channel();
        let events = Arc::new(Events::new(Arc::new(move || wake.send(()).unwrap())));
        let (replies, incoming) = mpsc::sync_channel(1);
        let (outgoing, requests) = mpsc::sync_channel(1);
        let mut worker = TypedWorker {
            definition: definition(),
            source_hash: String::new(),
            child: Some(child),
            _resources: resources,
            outgoing,
            incoming,
            events: events.clone(),
            pending: None,
        };
        assert!(worker.alive());
        read_replies(output, replies, events);
        woken.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(worker.child.as_mut().unwrap().try_wait().unwrap().is_none());
        assert!(worker.poll().is_none());
        if check_submit {
            assert!(worker.submit(invocation(request(true))).is_err());
            assert!(requests.try_recv().is_err());
        } else {
            assert!(!worker.alive());
        }
        assert!(worker.id().is_none());
        assert!(worker.response_deadline().is_none());
    }
}
