use super::*;
fn finish(manager: &Manager) -> Status {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let status = manager.status().unwrap();
        if !status.active {
            return status;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn asynchronous_job_cancellation_precedes_signal_registration_and_preserves_exact_identity() {
    let observed = Arc::new(Mutex::new(false));
    let captured = observed.clone();
    let mut manager = Manager {
        runner: Arc::new(move |request, control| {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !control.cancelled() {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(2));
            }
            let signal = Arc::new(Cancellation::create(&request.operation).unwrap());
            control.register(signal.clone()).unwrap();
            control.running();
            *captured.lock().unwrap() = signal.cancelled().unwrap();
            Err("cancelled before mutation".into())
        }),
        current: None,
        worker: None,
    };
    assert!(manager.status().is_none());
    assert!(manager.cancel("missing").is_err());
    let job = manager.start(Action::Create(2), None).unwrap();
    assert!(manager.start(Action::Create(3), None).is_err());
    assert!(manager.cancel("stale").is_err());
    assert_eq!(manager.cancel(&job.job_id).unwrap().state, "cancelling");
    let status = finish(&manager);
    assert_eq!(status.state, "attention");
    assert!(status.message.contains("cancelled"));
    assert!(*observed.lock().unwrap());
    assert!(manager.cancel(&job.job_id).is_err());
}
#[test]
fn completion_failure_panic_and_shutdown_are_reported_without_starting_another_job() {
    let mut manager = Manager {
        runner: Arc::new(|_, control| {
            control.running();
            Ok(Outcome {
                success: true,
                message: "verified".into(),
            })
        }),
        current: None,
        worker: None,
    };
    let first = manager.start(Action::Create(2), None).unwrap();
    assert_eq!(finish(&manager).state, "complete");
    let second = manager
        .start(Action::Recover, Some(first.operation))
        .unwrap();
    assert_ne!(first.job_id, second.job_id);
    assert_eq!(finish(&manager).state, "complete");
    manager.runner = Arc::new(|_, _| {
        Ok(Outcome {
            success: false,
            message: "x".repeat(1000),
        })
    });
    manager.start(Action::Create(2), None).unwrap();
    assert_eq!(finish(&manager).message.len(), 512);
    manager.runner = Arc::new(|_, _| panic!("fixture worker failure"));
    manager.start(Action::Create(2), None).unwrap();
    assert!(finish(&manager).message.contains("worker failed"));
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = cancelled.clone();
    manager.runner = Arc::new(move |_, control| {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !control.cancelled() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
        seen.store(true, std::sync::atomic::Ordering::Release);
        Err("shutdown cancelled".into())
    });
    manager.start(Action::Create(2), None).unwrap();
    drop(manager);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !cancelled.load(std::sync::atomic::Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn default_manager_cannot_bypass_the_native_qualification_gate() {
    let mut manager = Manager::default();
    assert!(manager.start(Action::Create(0), None).is_err());
    assert!(manager.start(Action::Recover, None).is_err());
    assert!(manager.start(Action::Recover, Some("bad".into())).is_err());
    assert!(manager
        .start(Action::Create(2), Some(uuid::Uuid::new_v4().to_string()))
        .is_err());
    manager.start(Action::Create(2), None).unwrap();
    assert!(finish(&manager).message.contains("disabled"));
}
