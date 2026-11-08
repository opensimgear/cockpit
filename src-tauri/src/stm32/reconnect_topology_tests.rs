use super::*;
use crate::transport::notifications::recovery::PortRecovery;
use std::sync::atomic::AtomicUsize;

fn bind_recovery(gateway: &mut Gateway, rows: Arc<Mutex<Vec<PortInfo>>>) -> Arc<PortRecovery> {
    let events = Arc::new(PortRecovery::default());
    let source_rows = rows;
    gateway.recovery = Some(crate::transport::notifications::recovery::RecoverySource {
        ports: Arc::new(move || Ok(source_rows.lock().unwrap().clone())),
        events: events.clone(),
    });
    gateway.recovery_wake = Arc::new(osg_streams::NativeWake::default());
    gateway
        .recovery_wake
        .bind_thread(thread::current())
        .unwrap();
    events
}

struct CountingFactory {
    inner: MockFactory,
    enumerations: Arc<AtomicUsize>,
}

impl Factory for CountingFactory {
    fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
        let ports = self.inner.ports();
        self.enumerations.fetch_add(1, Ordering::SeqCst);
        ports
    }

    fn open(&mut self, port: &PortInfo, timeout: Duration) -> Result<Box<dyn Connection>, String> {
        self.inner.open(port, timeout)
    }
}

#[test]
fn automatic_recovery_stays_quiet_until_raw_topology_invalidation_and_reopens_exact_gateway() {
    let (mut gateway, context) = automatic_harness();
    assert_eq!(connect(&mut gateway)["ok"], true);

    context.lock().unwrap().ports.clear();
    let rows = Arc::new(Mutex::new(Vec::<PortInfo>::new()));
    let event_rows = rows.clone();
    let cached_reads = Arc::new(AtomicUsize::new(0));
    let cache_count = cached_reads.clone();
    let events = Arc::new(PortRecovery::default());
    events.report(true, false);
    gateway.recovery = Some(crate::transport::notifications::recovery::RecoverySource {
        ports: Arc::new(move || {
            cache_count.fetch_add(1, Ordering::SeqCst);
            Ok(event_rows.lock().unwrap().clone())
        }),
        events: events.clone(),
    });
    gateway.recovery_wake = Arc::new(osg_streams::NativeWake::default());
    let enumerations = Arc::new(AtomicUsize::new(0));
    gateway.factory = Box::new(CountingFactory {
        inner: MockFactory(context.clone()),
        enumerations: enumerations.clone(),
    });

    let (ready_tx, ready_rx) = mpsc::channel();
    let recover = thread::spawn(move || {
        gateway
            .recovery_wake
            .bind_thread(thread::current())
            .unwrap();
        ready_tx.send(()).unwrap();
        let result = gateway.recover_automatic(Instant::now() + Duration::from_secs(2));
        (gateway, result)
    });
    ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();

    let started = Instant::now();
    while enumerations.load(Ordering::SeqCst) == 0 && started.elapsed() < Duration::from_secs(1) {
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(enumerations.load(Ordering::SeqCst), 1, "recovery must take one fresh baseline");
    thread::sleep(Duration::from_millis(80));
    assert_eq!(
        enumerations.load(Ordering::SeqCst),
        1,
        "an absent gateway must not cause periodic port enumeration"
    );
    assert_eq!(context.lock().unwrap().opened, vec![PATH]);

    *rows.lock().unwrap() = vec![info(PATH)];
    context.lock().unwrap().ports = vec![info(PATH)];
    events.report(true, true);
    let (gateway, result) = recover.join().unwrap();
    assert_eq!(result, Ok(()));
    assert_eq!(gateway.state["connected"], true);
    assert_eq!(gateway.state["gatewaySerial"], UID);
    assert_eq!(cached_reads.load(Ordering::SeqCst), 1);
    assert_eq!(enumerations.load(Ordering::SeqCst), 2);
    assert_eq!(context.lock().unwrap().opened, vec![PATH, PATH]);
}

struct TransientOpenFactory {
    inner: MockFactory,
    failures_remaining: Arc<AtomicUsize>,
    attempts: Arc<AtomicUsize>,
}

impl Factory for TransientOpenFactory {
    fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
        self.inner.ports()
    }

    fn open(&mut self, port: &PortInfo, timeout: Duration) -> Result<Box<dyn Connection>, String> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        if self
            .failures_remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err("transient open failure".into());
        }
        self.inner.open(port, timeout)
    }
}

#[test]
fn automatic_recovery_retries_matched_transient_open_failure_without_second_event() {
    let (mut gateway, context) = automatic_harness();
    assert_eq!(connect(&mut gateway)["ok"], true);

    let rows = Arc::new(Mutex::new(vec![info(PATH)]));
    let events = bind_recovery(&mut gateway, rows);
    events.report(true, false);

    let attempts = Arc::new(AtomicUsize::new(0));
    let failures_remaining = Arc::new(AtomicUsize::new(1));
    gateway.factory = Box::new(TransientOpenFactory {
        inner: MockFactory(context.clone()),
        failures_remaining,
        attempts: attempts.clone(),
    });
    let result = gateway.recover_automatic(Instant::now() + Duration::from_secs(1));

    assert_eq!(result, Ok(()));
    assert_eq!(attempts.load(Ordering::SeqCst), 2, "the matching row is retried after one transient failure");
    assert_eq!(events.covered(), true);
    assert_eq!(gateway.state["gatewaySerial"], UID);
    assert_eq!(context.lock().unwrap().opened, vec![PATH, PATH]);
}

#[test]
fn manual_apply_waits_for_topology_event_and_is_not_replayed() {
    const NEW_PATH: &str = "/dev/cu.usbmodemGW2";

    let (mut gateway, context) = harness();
    assert_eq!(connect(&mut gateway)["ok"], true);
    stage_remove(&mut gateway);
    context.lock().unwrap().ports.clear();

    let rows = Arc::new(Mutex::new(Vec::<PortInfo>::new()));
    let event_rows = rows.clone();
    let events = Arc::new(PortRecovery::default());
    events.report(true, false);
    gateway.recovery = Some(crate::transport::notifications::recovery::RecoverySource {
        ports: Arc::new(move || Ok(event_rows.lock().unwrap().clone())),
        events: events.clone(),
    });
    gateway.recovery_wake = Arc::new(osg_streams::NativeWake::default());
    let enumerations = Arc::new(AtomicUsize::new(0));
    gateway.factory = Box::new(CountingFactory {
        inner: MockFactory(context.clone()),
        enumerations: enumerations.clone(),
    });

    let (ready_tx, ready_rx) = mpsc::channel();
    let applying = thread::spawn(move || {
        gateway
            .recovery_wake
            .bind_thread(thread::current())
            .unwrap();
        ready_tx.send(()).unwrap();
        let result = gateway.apply();
        (gateway, result)
    });
    ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();

    let started = Instant::now();
    loop {
        let sent = context
            .lock()
            .unwrap()
            .writes
            .iter()
            .any(|line| line.trim_end().ends_with(" apply"));
        if sent {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(2), "Apply mutation was not sent");
        thread::sleep(Duration::from_millis(2));
    }

    let wait_started = Instant::now();
    while enumerations.load(Ordering::SeqCst) == 0
        && wait_started.elapsed() < Duration::from_secs(2)
    {
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(enumerations.load(Ordering::SeqCst), 1);
    assert_eq!(
        context
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|line| line.trim_end().ends_with(" apply"))
            .count(),
        1
    );

    *rows.lock().unwrap() = vec![info(NEW_PATH)];
    context.lock().unwrap().ports = vec![info(NEW_PATH)];
    events.report(true, true);
    let (gateway, result) = applying.join().unwrap();

    assert_eq!(result, Ok(()));
    assert_eq!(gateway.state["connected"], true);
    assert_eq!(gateway.state["port"], NEW_PATH);
    assert_eq!(gateway.state["needsApply"], false);
    assert_eq!(gateway.state["modules"][0]["activeSlot"], -1);
    assert_eq!(enumerations.load(Ordering::SeqCst), 2);
    assert_eq!(
        context
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|line| line.trim_end().ends_with(" apply"))
            .count(),
        1,
        "recovery must reconcile the applied layout without replaying Apply"
    );
    assert_eq!(context.lock().unwrap().opened, vec![PATH, NEW_PATH]);
}
