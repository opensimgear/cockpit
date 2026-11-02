use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct DelayedRenewal(Arc<std::sync::Mutex<Vec<Instant>>>);
impl Channel for DelayedRenewal {
    fn request(&mut self, message: &Message) -> Result<(), String> {
        if matches!(message, Message::Renew { .. }) {
            self.0.lock().unwrap().push(Instant::now());
            // Exceed the renewal interval so the deadline is due after this ACK.
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Ok(())
    }
    fn alive(&mut self) -> bool { true }
    fn terminate(&mut self) {}
}

#[test]
fn delayed_acknowledgement_does_not_postpone_the_next_lease_renewal() {
    let renewals = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = renewals.clone();
    let factory: Factory = Arc::new(move || Ok(Box::new(DelayedRenewal(observed.clone()))));
    let mut session = Session::start(factory, RunToken {
        session: uuid::Uuid::new_v4().to_string(), generation: Counter(1),
    }, vec![crate::vjoy::tests::binding()], Counter(0)).unwrap();
    assert!(session.renewal_deadline().is_some());
    session.renewed -= std::time::Duration::from_micros(RENEW_US);
    let due = session.renewal_deadline().unwrap();
    assert!(due <= Instant::now());
    assert_eq!(session.renewal_deadline(), Some(due));
    assert!(renewals.lock().unwrap().is_empty(), "queries must not renew");
    session.keep_alive().unwrap();
    let first_received = renewals.lock().unwrap()[0];
    assert!(session.renewed <= first_received);
    let delayed_deadline = session.renewal_deadline().unwrap();
    assert!(delayed_deadline <= Instant::now(), "deadline remains due after delayed ACK");
    assert!(delayed_deadline <= first_received + std::time::Duration::from_micros(RENEW_US));
    assert_eq!(session.renewal_deadline(), Some(delayed_deadline));
    session.keep_alive().unwrap();
    assert_eq!(renewals.lock().unwrap().len(), 2,
        "The ACK delay already consumed the renewal interval");
    session.stop().unwrap();
    assert_eq!(session.renewal_deadline(), None);
}

#[test]
fn initial_lease_deadline_uses_the_start_send_time() {
    struct SlowStart;
    impl Channel for SlowStart {
        fn request(&mut self, message: &Message) -> Result<(), String> {
            if matches!(message, Message::Start { .. }) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Ok(())
        }
        fn alive(&mut self) -> bool { true }
        fn terminate(&mut self) {}
    }
    let factory: Factory = Arc::new(|| Ok(Box::new(SlowStart)));
    let mut session = Session::start(
        factory,
        RunToken {
            session: uuid::Uuid::new_v4().to_string(),
            generation: Counter(1),
        },
        vec![crate::vjoy::tests::binding()],
        Counter(0),
    )
    .unwrap();

    assert!(session.renewal_deadline().unwrap() <= Instant::now());
    session.stop().unwrap();
}

#[test]
fn deadline_query_is_read_only_for_a_live_owned_handoff() {
    struct OwnedChannel(Arc<std::sync::Mutex<Vec<Message>>>);
    impl Channel for OwnedChannel {
        fn request(&mut self, message: &Message) -> Result<(), String> {
            self.0.lock().unwrap().push(message.clone());
            Ok(())
        }
        fn alive(&mut self) -> bool { true }
        fn terminate(&mut self) {}
    }
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let messages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = messages.clone();
    let factory: Factory = Arc::new(move || Ok(Box::new(OwnedChannel(shared.clone()))));
    let mut session = Session::start_with_journal(
        factory,
        RunToken {
            session: uuid::Uuid::new_v4().to_string(),
            generation: Counter(1),
        },
        vec![crate::vjoy::tests::binding()],
        Counter(0),
        Some(dir.path().join("output.json")),
    )
    .unwrap();

    let deadline = session.renewal_deadline().unwrap();
    for _ in 0..4 {
        let _ = session.handoff_snapshot().unwrap();
        assert_eq!(session.renewal_deadline(), Some(deadline));
    }
    assert!(session.active());
    assert!(matches!(messages.lock().unwrap().as_slice(), [Message::Start { .. }]));

    session.stop().unwrap();
    assert_eq!(session.renewal_deadline(), None);
}

struct FailedWorker(Arc<AtomicBool>);
impl Channel for FailedWorker {
    fn request(&mut self, _: &Message) -> Result<(), String> {
        Ok(())
    }
    fn alive(&mut self) -> bool {
        !self.0.load(Ordering::Acquire)
    }
    fn exit_error(&mut self) -> String {
        "Mapping heartbeat expired; fallback submitted and ownership released.".into()
    }
    fn terminate(&mut self) {}
}

#[test]
fn dead_worker_diagnostic_survives_both_session_liveness_checks_and_recovery() {
    for prepared in [false, true] {
        let dead = Arc::new(AtomicBool::new(false));
        let owned = dead.clone();
        let factory: Factory = Arc::new(move || Ok(Box::new(FailedWorker(owned.clone()))));
        let mut session = Session::start(
            factory,
            RunToken {
                session: uuid::Uuid::new_v4().to_string(),
                generation: Counter(1),
            },
            vec![crate::vjoy::tests::binding()],
            Counter(0),
        )
        .unwrap();
        dead.store(true, Ordering::Release);
        let error = if prepared {
            session.keep_alive()
        } else {
            session.poll(&RecordingSink::default())
        }
        .unwrap_err();
        assert_eq!(error, "Mapping heartbeat expired; fallback submitted and ownership released. Fallback recovery completed; mapping remains stopped.");
        assert!(!session.active());
        assert_eq!(session.renewal_deadline(), None);
        assert!(!session.recovery_required());
    }
}
