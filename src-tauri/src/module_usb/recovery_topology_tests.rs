use super::*;
use crate::transport::notifications::recovery::PortRecovery;
use std::sync::atomic::AtomicUsize;

struct ObservedFactory {
    device: FakeFactory,
    scans: mpsc::Sender<()>,
    opens: Arc<AtomicUsize>,
    fail_first_open: bool,
}

impl Factory for ObservedFactory {
    fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
        let rows = self.device.ports()?;
        let _ = self.scans.send(());
        Ok(rows)
    }

    fn open(&mut self, port: &PortInfo, read: Duration) -> Result<Box<dyn crate::transport::Connection>, String> {
        self.device.open(port, read)
    }

    fn open_duplex(&mut self, port: &PortInfo) -> Result<crate::transport::duplex::DuplexParts, String> {
        self.opens.fetch_add(1, Ordering::Relaxed);
        if std::mem::take(&mut self.fail_first_open) {
            return Err("fixture port is not ready to open".into());
        }
        self.device.open_duplex(port)
    }
}

struct Fixture {
    worker: Worker,
    device: Arc<Mutex<Device>>,
    scans: mpsc::Receiver<()>,
    opens: Arc<AtomicUsize>,
    cached_reads: Arc<AtomicUsize>,
    events: Arc<PortRecovery>,
}

fn fixture(present: bool, fail_first_open: bool) -> Fixture {
    let device = Arc::new(Mutex::new(Device { ports: if present { vec![port()] } else { vec![] }, ..Device::default() }));
    let events = Arc::new(PortRecovery::default());
    events.report(true, false);
    let (notify, scans) = mpsc::channel();
    let opens = Arc::new(AtomicUsize::new(0));
    let factory = Box::new(ObservedFactory { device: FakeFactory(device.clone()), scans: notify, opens: opens.clone(), fail_first_open });
    let mut worker = Worker::new(factory, Timing { reconnect: Duration::from_secs(3), ..timing() }, Arc::default());
    let source = device.clone();
    let cached_reads = Arc::new(AtomicUsize::new(0));
    let reads = cached_reads.clone();
    worker.recovery = Some(RecoverySource {
        ports: Arc::new(move || {
            reads.fetch_add(1, Ordering::Relaxed);
            Ok(source.lock().unwrap().ports.clone())
        }),
        events: events.clone(),
    });
    Fixture { worker, device, scans, opens, cached_reads, events }
}

fn reconcile(mut worker: Worker) -> Result<(), String> {
    worker.wake.bind_thread(thread::current()).unwrap();
    let prior = UsbRouteToken {
        module_id: MODULE.into(), serial: SERIAL.into(), boot: 100,
        connection_generation: 1, configuration_generation: 1,
    };
    worker.reconcile(&prior, &definition(), false, false)
}

#[test]
fn absent_port_waits_for_shared_topology_and_revalidates_the_returned_port() {
    let Fixture { worker, device, scans, opens, cached_reads, events } = fixture(false, false);
    let recovery = thread::spawn(move || reconcile(worker));
    scans.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(scans.recv_timeout(Duration::from_millis(140)), Err(mpsc::RecvTimeoutError::Timeout));

    device.lock().unwrap().ports.push(port());
    events.report(true, true);
    assert_eq!(recovery.join().unwrap(), Ok(()));
    assert_eq!(cached_reads.load(Ordering::Relaxed), 1);
    assert_eq!(opens.load(Ordering::Relaxed), 1);
    assert_eq!(scans.try_iter().count(), 1, "connect must revalidate the exact reported port");
}

#[test]
fn coverage_loss_wakes_the_missing_port_wait_and_allows_the_bounded_fallback() {
    let Fixture { worker, device, scans, opens, cached_reads, events } = fixture(false, false);
    let recovery = thread::spawn(move || reconcile(worker));
    scans.recv_timeout(Duration::from_secs(1)).unwrap();
    events.report(false, false);
    scans.recv_timeout(Duration::from_secs(1)).unwrap();
    device.lock().unwrap().ports.push(port());
    assert_eq!(recovery.join().unwrap(), Ok(()));
    assert_eq!(cached_reads.load(Ordering::Relaxed), 0);
    assert_eq!(opens.load(Ordering::Relaxed), 1);
}

#[test]
fn matched_port_open_retries_without_another_topology_notification() {
    let Fixture { worker, opens, .. } = fixture(true, true);
    assert_eq!(reconcile(worker), Ok(()));
    assert_eq!(opens.load(Ordering::Relaxed), 2);
}

#[test]
fn shutdown_wakes_a_worker_waiting_for_a_missing_port() {
    let Fixture { worker, scans, .. } = fixture(false, false);
    let wake = worker.wake.clone();
    let cancel = worker.cancel.clone();
    let recovery = thread::spawn(move || reconcile(worker));
    scans.recv_timeout(Duration::from_secs(1)).unwrap();
    cancel.store(true, Ordering::Release);
    wake.notify();
    assert_eq!(recovery.join().unwrap(), Err(CLOSED.into()));
}
