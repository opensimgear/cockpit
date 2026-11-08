use super::*;
use crate::{
    registry::{Control as InputControl, Identity},
    visibility::{
        session::Source,
        suppression::{Command, Control},
    },
};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
};

#[test]
fn idle_descriptor_wait_wakes_only_for_the_ready_report_or_command() {
    let wake = Wake::new().unwrap();
    let (first, mut first_writer) = UnixStream::pair().unwrap();
    let (second, _second_writer) = UnixStream::pair().unwrap();
    let mut descriptors = [
        descriptor(wake.0.as_raw_fd()),
        descriptor(first.as_raw_fd()),
        descriptor(second.as_raw_fd()),
    ];
    wait(&mut descriptors, Some(Instant::now())).unwrap();
    assert!(descriptors.iter().all(|fd| fd.revents == 0));

    first_writer.write_all(&[1]).unwrap();
    wait(
        &mut descriptors,
        Some(Instant::now() + Duration::from_secs(1)),
    )
    .unwrap();
    assert_eq!(descriptors.map(|fd| fd.revents), [0, libc::POLLIN, 0]);

    drop(first_writer);
    wake.signal().unwrap();
    wait(
        &mut descriptors,
        Some(Instant::now() + Duration::from_secs(1)),
    )
    .unwrap();
    assert_eq!(descriptors[0].revents, libc::POLLIN);
    assert_ne!(descriptors[1].revents & FAILED, 0);
    assert_eq!(descriptors[2].revents, 0);
}

#[test]
fn queued_apply_and_stop_signal_the_capture_owner_without_replaying_a_wake() {
    let wake = Arc::new(Wake::new().unwrap());
    let control = Control::default();
    control.watch_capture(&wake);
    let source = Source {
        id: uuid::Uuid::new_v4().to_string(),
        session: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        sequence: 1,
        interface: "physical".into(),
    };
    let id = control.apply(vec![source]).unwrap();
    let mut descriptors = [descriptor(wake.0.as_raw_fd())];
    wait(&mut descriptors, Some(Instant::now())).unwrap();
    assert_eq!(descriptors[0].revents, libc::POLLIN);
    wake.clear().unwrap();
    assert!(matches!(control.take(), Some(Command::Apply { .. })));

    control.stop(Some(&id)).unwrap();
    wait(&mut descriptors, Some(Instant::now())).unwrap();
    assert_eq!(descriptors[0].revents, libc::POLLIN);
    wake.clear().unwrap();
    wake.clear().unwrap();
    assert!(matches!(control.take(), Some(Command::Stop)));
    wait(&mut descriptors, Some(Instant::now())).unwrap();
    assert_eq!(descriptors[0].revents, 0);
}

struct PipePlatform(Arc<AtomicBool>, Option<UnixStream>);

impl Platform for PipePlatform {
    fn paths(&mut self) -> Result<Vec<String>, String> {
        Ok(self
            .0
            .load(Ordering::Acquire)
            .then(|| "fixture".into())
            .into_iter()
            .collect())
    }

    fn open(&mut self, _: &str) -> Result<Box<dyn Input>, String> {
        Ok(Box::new(PipeInput(
            self.1.take().ok_or("Fixture already opened")?,
        )))
    }
}

struct PipeInput(UnixStream);

impl Input for PipeInput {
    fn descriptor(&self) -> Option<RawFd> {
        Some(self.0.as_raw_fd())
    }

    fn definition(&self) -> Definition {
        Definition {
            identity: Identity {
                backend: "linux-fixture".into(),
                vendor_id: 1,
                product_id: 2,
                serial: None,
                collection: "0".into(),
                fingerprint: String::new(),
                path: "fixture".into(),
            },
            name: "Fixture".into(),
            controls: vec![InputControl::new(9, 1, 0, 0, 1)],
            diagnostics: vec![],
        }
    }

    fn baseline(&mut self) -> Result<Vec<Event>, String> {
        Ok(vec![Event::Values(0, BTreeMap::from([(1, 0)]))])
    }

    fn poll(&mut self) -> Result<Vec<Event>, String> {
        let mut report = [0];
        self.0
            .read_exact(&mut report)
            .map_err(|error| error.to_string())?;
        Ok(vec![if report[0] == 2 {
            Event::Gap("Fixture overflow".into())
        } else {
            Event::Values(0, BTreeMap::from([(1, u32::from(report[0]))]))
        }])
    }
}

fn await_detail(shared: &Shared, check: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let detail = {
            let registry = shared.lock().unwrap();
            registry.list(0)["sources"][0]["id"]
                .as_str()
                .and_then(|id| registry.inspect(id, 0).ok())
        };
        if let Some(detail) = detail.filter(|value| check(value)) {
            return detail;
        }
        assert!(
            Instant::now() < deadline,
            "Capture did not publish the expected detail"
        );
        std::thread::yield_now();
    }
}

#[test]
fn dispatcher_reads_reports_recovers_overflow_and_retires_on_topology_wake() {
    let present = Arc::new(AtomicBool::new(true));
    let (report, mut report_writer) = UnixStream::pair().unwrap();
    let (mut topology, mut topology_writer) = UnixStream::pair().unwrap();
    let shared = Shared::default();
    let platform = PipePlatform(present.clone(), Some(report));
    let stop = Arc::new(AtomicBool::new(false));
    let wake = Arc::new(Wake::new().unwrap());
    let worker = {
        let stop = stop.clone();
        let wake = wake.clone();
        let shared = shared.clone();
        std::thread::spawn(move || {
            run_ready(
                Capture::new(platform, shared),
                &stop,
                Control::default(),
                &wake,
                topology.as_raw_fd(),
                || {
                    topology
                        .read_exact(&mut [0])
                        .map_err(|error| error.to_string())?;
                    Ok((true, BTreeSet::from(["fixture".into()])))
                },
            )
        })
    };
    let baseline = await_detail(&shared, |detail| detail["source"]["baselineReady"] == true);
    report_writer.write_all(&[1]).unwrap();
    await_detail(&shared, |detail| {
        detail["controls"][0]["value"]["normalized"] == 1
    });
    report_writer.write_all(&[2]).unwrap();
    await_detail(&shared, |detail| {
        detail["source"]["baselineReady"] == true
            && detail["source"]["generation"] != baseline["source"]["generation"]
    });

    present.store(false, Ordering::Release);
    topology_writer.write_all(&[1]).unwrap();
    await_detail(&shared, |detail| detail["source"]["online"] == false);
    stop.store(true, Ordering::Release);
    wake.signal().unwrap();
    worker.join().unwrap().unwrap();
}

#[test]
fn topology_descriptor_loss_stops_capture_and_withdraws_its_inventory() {
    let present = Arc::new(AtomicBool::new(true));
    let (report, _report_writer) = UnixStream::pair().unwrap();
    let (topology, topology_writer) = UnixStream::pair().unwrap();
    drop(topology_writer);
    let shared = Shared::default();
    let capture = Capture::new(PipePlatform(present, Some(report)), shared.clone());
    let error = run_ready(
        capture,
        &AtomicBool::new(false),
        Control::default(),
        &Arc::new(Wake::new().unwrap()),
        topology.as_raw_fd(),
        || panic!("A failed descriptor must stop before reading topology"),
    )
    .unwrap_err();
    assert!(error.contains("continuity was lost"));
    await_detail(&shared, |detail| detail["source"]["online"] == false);
}

#[test]
fn native_probe_starts_and_stops_when_the_udev_service_is_present() {
    if !Path::new("/run/udev/control").exists() {
        assert!(monitor()
            .err()
            .unwrap()
            .contains("requires a running udev service"));
        return;
    }
    let monitor = monitor().unwrap();
    // The real nonblocking drain is safe without devices and may also consume
    // a bounded hotplug batch if devices change during the test.
    topology(&monitor).unwrap();
    let probe = crate::ControllerService::qualification_probe();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let status = probe.status();
        if status["backendStatus"] == "qualification-probe" {
            break;
        }
        assert_ne!(status["backendStatus"], "unavailable", "{status}");
        assert!(Instant::now() < deadline, "Controller probe did not start");
        std::thread::yield_now();
    }
    probe.shutdown();
    assert_eq!(probe.status()["backendStatus"], "stopped");
}
