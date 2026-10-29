use super::*;
use osg_mapping::{
    engine::{Counter, RunToken},
    profile::{Axis, AxisRange, Layout},
};
use osg_virtual_controller::{
    feeder::{Message, LEASE_US, RENEW_US},
    session::{Channel, Factory, Session},
    vjoy::Binding,
};
use std::sync::{atomic::AtomicUsize, mpsc};

struct CountRenewals {
    child: Box<dyn Channel>,
    renewals: Arc<AtomicUsize>,
}
impl Channel for CountRenewals {
    fn request(&mut self, message: &Message) -> Result<(), String> {
        self.child.request(message)?;
        if matches!(message, Message::Renew { .. }) {
            self.renewals.fetch_add(1, Ordering::AcqRel);
        }
        Ok(())
    }
    fn alive(&mut self) -> bool {
        self.child.alive()
    }
    fn terminate(&mut self) {
        self.child.terminate();
    }
}

#[test]
fn slow_read_only_catalogue_decoration_does_not_block_context_validation_or_output_renewal() {
    let scratch = Scratch::new();
    let (runtime, _, calls) = runtime(&scratch);
    let prepared = runtime
        .prepare_profile_game_context("x-plane-12")
        .unwrap()
        .unwrap();
    runtime
        .set_profile_context_owner(Some(owner(Some("x-plane-12"))))
        .unwrap();
    let admitted = runtime
        .admit_prepared_game_context(&prepared.proof)
        .unwrap();
    let runtime = Arc::new(runtime);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let decorated_runtime = runtime.clone();
    let decorator = thread::spawn(move || {
        let mut games = vec![json!({"id":"dcs","integrationCompatible":true,
            "integrationKey":"dcs","provider":"dcs-bios","integrationSetup":{}})];
        decorated_runtime.decorate_game_contexts_with(&mut games, |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Some("fixture-module".into())
        });
        games
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let renewals = Arc::new(AtomicUsize::new(0));
    let count = renewals.clone();
    let base = crate::profile_session_host::NativeProfileHost::game_context_renewal_test_factory();
    let factory: Factory = Arc::new(move || {
        Ok(Box::new(CountRenewals {
            child: base()?,
            renewals: count.clone(),
        }))
    });
    let mut session = Session::start(
        factory,
        RunToken {
            session: uuid::Uuid::new_v4().to_string(),
            generation: Counter(1),
        },
        vec![Binding {
            target: "fixture".into(),
            slot: 3,
            axes: vec![48, 49, 50],
            layout: Layout {
                buttons: 16,
                hats: 2,
                axes: (0..3)
                    .map(|_| Axis {
                        range: AxisRange::Unipolar,
                        fallback: 0,
                    })
                    .collect(),
            },
        }],
        Counter(0),
    )
    .unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let validating_runtime = runtime.clone();
    let pump = thread::spawn(move || {
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(1) {
            validating_runtime
                .validate_admitted_game_context(&admitted, &prepared.proof)
                .unwrap();
            session.keep_alive().unwrap();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(session.active());
        assert!(!session.recovery_required());
        done_tx.send(()).unwrap();
    });
    // A timeout releases the intentionally blocked provider before failing, so
    // this regression never leaves a blocked worker behind on the old code.
    let completed = done_rx.recv_timeout(Duration::from_secs(3));
    release_tx.send(()).unwrap();
    let games = decorator.join().unwrap();
    pump.join().unwrap();
    completed.unwrap();
    assert!(renewals.load(Ordering::Acquire) > (LEASE_US / RENEW_US) as usize);
    assert_eq!(
        games[0]["integrationSetup"]["activeAircraftModule"],
        "fixture-module"
    );
    assert!(calls.lock().unwrap().is_empty());
    runtime.shutdown();
}
