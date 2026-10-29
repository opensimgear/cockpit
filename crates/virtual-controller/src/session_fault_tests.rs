use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

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
