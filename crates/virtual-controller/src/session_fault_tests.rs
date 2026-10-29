use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct DelayedRenewal(Arc<std::sync::Mutex<Vec<Instant>>>);
impl Channel for DelayedRenewal {
    fn request(&mut self, message: &Message) -> Result<(), String> {
        if matches!(message, Message::Renew { .. }) {
            self.0.lock().unwrap().push(Instant::now());
            // Still below the unchanged 100 ms IPC budget, but longer than the
            // renewal interval. The next poll must renew without another wait.
            std::thread::sleep(std::time::Duration::from_millis(60));
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
    session.renewed -= std::time::Duration::from_micros(RENEW_US);
    session.keep_alive().unwrap();
    let first_received = renewals.lock().unwrap()[0];
    assert!(session.renewed <= first_received);
    session.keep_alive().unwrap();
    assert_eq!(renewals.lock().unwrap().len(), 2,
        "The ACK delay already consumed the renewal interval");
    session.stop().unwrap();
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
        assert!(!session.recovery_required());
    }
}
