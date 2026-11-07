use super::*;
use crate::native_duplex::{BlockingReceiver, CancelIo, DeadlineWriter, DuplexParts};
mod wire;
struct ReceiverHalf(mpsc::Receiver<Vec<u8>>);
impl BlockingReceiver for ReceiverHalf {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
        let bytes = self.0.recv().map_err(|_| "Disconnected fixture")?;
        buffer[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

struct WriterHalf(mpsc::Sender<Value>);
impl DeadlineWriter for WriterHalf {
    fn write_all(&mut self, bytes: &[u8], _: Instant) -> Result<(), String> {
        let frame = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        self.0
            .send(frame)
            .map_err(|_| "Disconnected fixture".into())
    }

    fn control_lines(&mut self, _: bool, _: bool) -> Result<(), String> {
        Ok(())
    }
}

struct Cancel(mpsc::Sender<Vec<u8>>);
impl CancelIo for Cancel {
    fn cancel(&self) {
        let _ = self.0.send(Vec::new());
    }
}

fn assign_screen(hub: &Hub) {
    let offer = hub
        .command(&json!({"command":"companion_pair_offer"}))
        .unwrap();
    let mut wire = Wire::new(Arc::default());
    wire.reset(hub, "fixture-link");
    let pending = wire
        .handle(
            hub,
            &json!({"type":"serial-request","linkId":"fixture-link",
        "requestId":"1","frame":{"type":"pair","secret":offer["secret"],
        "companionId":"screen","name":"Test screen"}}),
        )
        .unwrap();
    assert!(approve(hub, "screen").unwrap());
    let paired = wire
        .handle(
            hub,
            &json!({"type":"serial-request","linkId":"fixture-link",
        "requestId":"2","frame":{"type":"pair-status","requestId":pending[0]["frame"]["requestId"],
        "secret":offer["secret"]}}),
        )
        .unwrap();
    assert_eq!(paired[0]["frame"]["type"], "paired");

    let fixture: Value = serde_json::from_str(include_str!(
        "../../../apps/companion-esp32/test/live-publication.json"
    ))
    .unwrap();
    let hash = &fixture["publicationHash"];
    hub.retain_legacy_test_fixture(
        serde_json::from_value(json!({
            "canonicalDocument":fixture["canonicalDocument"],"publicationHash":hash,
            "capabilityFingerprint":fixture["capabilityFingerprint"],"assets":[]
        }))
        .unwrap(),
    )
    .unwrap();
    hub.command(
        &json!({"command":"companion_assign","instanceId":"usb-panel",
        "companionId":"screen","publicationHash":hash}),
    )
    .unwrap();
}

struct WorkerFixture {
    device: mpsc::Sender<Vec<u8>>,
    outgoing: mpsc::Receiver<Value>,
    committed: mpsc::Receiver<Value>,
    control: mpsc::Sender<Action>,
    wake: Arc<NativeWake>,
    public: Arc<Mutex<Value>>,
    handle: thread::JoinHandle<Result<(), String>>,
}

fn start_worker(expected: Option<&'static str>, paired: bool) -> WorkerFixture {
    let (device_tx, host_rx) = mpsc::channel();
    let (host_tx, device_rx) = mpsc::channel();
    let (mutations, committed) = mpsc::channel();
    let (control, commands) = mpsc::channel();
    let wake = Arc::new(NativeWake::default());
    let worker_wake = wake.clone();
    let mut duplex = Some(DuplexParts {
        receiver: Box::new(ReceiverHalf(host_rx)),
        writer: Box::new(WriterHalf(host_tx)),
        cancel: Arc::new(Cancel(device_tx.clone())),
    });
    let public = Arc::new(Mutex::new(json!({"connected":true,"ready":false})));
    let observed = public.clone();
    let observer = StatusObserver::new(move || {
        let state = observed.lock().unwrap();
        mutations.send(state["device"]["status"].clone()).unwrap();
    });
    let worker_public = public.clone();
    let handle = thread::spawn(move || {
        let hub = Hub::new(
            "test-host".into(),
            Arc::new(osg_companion_service::NullSourceSink),
        );
        if paired {
            assign_screen(&hub);
        }
        worker(
            &hub,
            &commands,
            &worker_public,
            expected,
            &observer,
            worker_wake,
            |baud, wake| ReadPump::start(duplex.take().ok_or("Unexpected reopen")?, baud, wake),
        )
    });
    WorkerFixture {
        device: device_tx,
        outgoing: device_rx,
        committed,
        control,
        wake,
        public,
        handle,
    }
}

fn send_frame(device: &mpsc::Sender<Vec<u8>>, frame: Value) {
    let mut bytes = serde_json::to_vec(&frame).unwrap();
    bytes.push(b'\n');
    device.send(bytes).unwrap();
}

impl WorkerFixture {
    fn receive(&self) -> Value {
        self.outgoing.recv_timeout(Duration::from_secs(2)).unwrap()
    }

    fn send(&self, frame: Value) {
        send_frame(&self.device, frame);
    }

    fn command(&self, action: Action) {
        self.control.send(action).unwrap();
        self.wake.notify();
    }

    fn wait_field(&self, field: &str, expected: Value) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.public.lock().unwrap()[field] != expected {
            self.committed
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| panic!("Waiting for {field}={expected}: {error}"));
        }
    }
}

#[test]
fn usb_watch_tracks_provisioning_identity_and_arm_challenge() {
    let fixture = start_worker(None, false);
    let probe = fixture.receive();
    let mut status = json!({"protocolVersion":2,"deviceType":"esp32","statusEvents":true,
        "requestId":probe["requestId"],"companionId":"","status":"offline","setupSaved":false});
    fixture.send(status.clone());
    let watch = fixture.receive();
    assert_eq!(watch["command"], "status-watch");
    status["type"] = json!("status-event");
    status["requestId"] = watch["requestId"].clone();
    fixture.send(status.clone());

    fixture.command(Action::Arm);
    fixture.wait_field(
        "stage",
        json!("Release touch and wait for a healthy panel before arming"),
    );
    fixture.command(Action::Provision(
        json!({"command":"provision","_assignment":{}}),
        "new-screen".into(),
    ));
    assert_eq!(fixture.receive(), json!({"command":"provision"}));
    fixture.command(Action::Provision(
        json!({"command":"provision"}),
        "other-screen".into(),
    ));
    fixture.wait_field("stage", json!("Provisioning is already running"));

    status["setupSaved"] = json!(true);
    status["status"] = json!("awaiting-assignment");
    status["companionId"] = json!("other-screen");
    fixture.send(status.clone());
    assert!(fixture
        .outgoing
        .recv_timeout(Duration::from_millis(100))
        .is_err());
    assert_eq!(fixture.public.lock().unwrap()["device"]["companionId"], "");
    status["companionId"] = json!("new-screen");
    fixture.send(status.clone());
    fixture.wait_field("provisioning", json!(false));
    assert_eq!(
        fixture.public.lock().unwrap()["device"]["companionId"],
        "new-screen"
    );
    assert_eq!(fixture.receive()["command"], "status-ping");

    status["readyForArm"] = json!(true);
    status["status"] = json!("live-disarmed");
    status["sessionId"] = json!("session");
    status["generation"] = json!("2");
    status["publicationHash"] = json!("hash");
    status["armEpoch"] = json!("3");
    fixture.send(status);
    fixture.wait_field("stage", json!("Waiting for the display and released touch"));
    fixture.command(Action::Arm);
    assert_eq!(
        fixture.receive(),
        json!({"command":"arm","sessionId":"session",
        "generation":"2","publicationHash":"hash","armEpoch":"3"})
    );
    fixture.command(Action::Stop);
    assert_eq!(fixture.handle.join().unwrap(), Ok(()));
}

#[test]
fn usb_scan_reports_results_and_failure_without_restarting_the_watch() {
    let fixture = start_worker(None, false);
    let probe = fixture.receive();
    let mut status = json!({"protocolVersion":2,"deviceType":"esp32","statusEvents":true,
        "requestId":probe["requestId"],"companionId":"","status":"offline","setupSaved":false});
    fixture.send(status.clone());
    let watch = fixture.receive();
    assert_eq!(watch["command"], "status-watch");
    status["type"] = json!("status-event");
    status["requestId"] = watch["requestId"].clone();
    fixture.send(status);
    for reply in [
        json!({"status":"wifi-networks","networks":[{"ssid":"test"}]}),
        json!({"status":"wifi-scan-failed"}),
    ] {
        fixture.command(Action::Record(json!({"command":"scan-wifi"})));
        assert_eq!(fixture.receive()["command"], "scan-wifi");
        fixture.wait_field("scanning", json!(true));
        fixture.send(reply.clone());
        fixture.wait_field("scanning", json!(false));
        if reply["status"] == "wifi-networks" {
            assert_eq!(
                fixture.public.lock().unwrap()["networks"],
                reply["networks"]
            );
        } else {
            assert!(fixture.public.lock().unwrap()["scanError"].is_string());
        }
    }
    assert_eq!(fixture.receive()["watchId"], watch["requestId"]);
    fixture.command(Action::Stop);
    assert_eq!(fixture.handle.join().unwrap(), Ok(()));
}

#[test]
fn usb_worker_subscribes_uses_liveness_and_rejects_retired_status() {
    let WorkerFixture {
        device: device_tx,
        outgoing: device_rx,
        committed,
        control,
        wake,
        public,
        handle,
    } = start_worker(None, true);
    let receive = || device_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let send = |frame: Value| {
        send_frame(&device_tx, frame);
    };
    let probe = receive();
    assert_eq!(probe["command"], "status");
    let base = json!({"protocolVersion":2,"deviceType":"esp32","statusEvents":true,
        "requestId":probe["requestId"],"companionId":"screen","status":"offline","setupSaved":false});
    send(base.clone());
    assert_eq!(
        committed.recv_timeout(Duration::from_secs(2)).unwrap(),
        "offline"
    );
    let watch = receive();
    assert_eq!(watch["command"], "status-watch");
    let mut event = base;
    event["type"] = json!("status-event");
    event["requestId"] = watch["requestId"].clone();
    let ping = receive();
    assert_eq!(ping["command"], "status-ping");
    assert_eq!(ping["watchId"], watch["requestId"]);
    let before = public.lock().unwrap()["_statusObservedMs"]
        .as_u64()
        .unwrap();
    send(event.clone());
    send(json!({"type":"status-alive","requestId":ping["requestId"],"watchId":watch["requestId"]}));
    let next = receive();
    assert_eq!(next["command"], "status-ping");
    assert!(
        public.lock().unwrap()["_statusObservedMs"]
            .as_u64()
            .unwrap()
            > before
    );
    assert!(
        committed.try_recv().is_err(),
        "Liveness must not publish a status change"
    );
    let mut retired = event.clone();
    retired["requestId"] = json!("retired-watch");
    retired["status"] = json!("error");
    send(retired);
    event["status"] = json!("awaiting-assignment");
    send(event.clone());
    assert_eq!(
        committed.recv_timeout(Duration::from_secs(2)).unwrap(),
        "awaiting-assignment"
    );
    assert!(committed.try_recv().is_err());

    let mut failed = event.clone();
    failed["status"] = json!("offline");
    failed["setupSaved"] = json!(true);
    failed["connectionMode"] = json!("serial");
    let mut snapshot = failed.clone();
    snapshot.as_object_mut().unwrap().remove("type");
    snapshot["requestId"] = next["requestId"].clone();
    send(snapshot.clone());
    assert!(committed.recv_timeout(Duration::from_millis(100)).is_err());
    send(failed.clone());
    assert_eq!(
        committed.recv_timeout(Duration::from_secs(2)).unwrap(),
        "offline"
    );
    let recovery_probe = loop {
        let frame = receive();
        if frame["command"] == "status" {
            break frame;
        }
        assert_eq!(frame["command"], "status-ping");
        send(
            json!({"type":"status-alive","requestId":frame["requestId"],"watchId":watch["requestId"]}),
        );
    };
    send(failed);
    assert!(device_rx.recv_timeout(Duration::from_millis(100)).is_err());
    snapshot["requestId"] = recovery_probe["requestId"].clone();
    let mut foreign = snapshot.clone();
    foreign["companionId"] = json!("foreign-screen");
    send(foreign);
    assert!(device_rx.recv_timeout(Duration::from_millis(100)).is_err());
    assert_eq!(public.lock().unwrap()["device"]["companionId"], "screen");
    send(snapshot);
    assert_eq!(receive()["command"], "connect");
    event["setupSaved"] = json!(true);
    event["connectionMode"] = json!("serial");
    send(event);
    assert_eq!(receive()["command"], "status-ping");
    while committed.recv_timeout(Duration::from_secs(2)).unwrap() != "awaiting-assignment" {}
    assert_eq!(public.lock().unwrap()["recovery"]["phase"], "recovered");

    control
        .send(Action::Record(json!({"command":"forget"})))
        .unwrap();
    wake.notify();
    assert_eq!(receive()["command"], "forget");
    let identify = receive();
    assert_eq!(identify["command"], "status");
    send(
        json!({"protocolVersion":2,"deviceType":"esp32","statusEvents":true,
        "requestId":identify["requestId"],"companionId":"","status":"offline","setupSaved":false}),
    );
    assert_eq!(
        committed.recv_timeout(Duration::from_secs(2)).unwrap(),
        "offline"
    );
    let replacement = receive();
    assert_eq!(replacement["command"], "status-watch");
    assert_ne!(replacement["requestId"], watch["requestId"]);

    device_tx.send(Vec::new()).unwrap();
    assert_eq!(handle.join().unwrap().unwrap_err(), "USB disconnected.");
}

#[test]
fn usb_unconfirmed_watch_expires_even_when_board_replies_to_pings() {
    let fixture = start_worker(None, false);
    let receive = || {
        fixture
            .outgoing
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
    };
    let probe = receive();
    send_frame(
        &fixture.device,
        json!({"protocolVersion":2,"deviceType":"esp32",
        "statusEvents":true,"requestId":probe["requestId"],"companionId":"screen",
        "status":"offline","setupSaved":false}),
    );
    let watch = receive();
    assert_eq!(watch["command"], "status-watch");
    let before = fixture.public.lock().unwrap()["_statusObservedMs"].clone();
    let mut pings = 0;
    loop {
        match fixture.outgoing.recv_timeout(Duration::from_secs(2)) {
            Ok(ping) => {
                assert_eq!(ping["command"], "status-ping");
                pings += 1;
                assert!(pings <= 6, "A pong cannot renew an unconfirmed watch");
                let _ = fixture.device.send(format!("{}\n", json!({
                    "type":"status-alive","requestId":ping["requestId"],"watchId":watch["requestId"]
                })).into_bytes());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(error) => panic!("Expected another ping or liveness expiration: {error}"),
        }
    }
    assert!(pings > 0);
    assert_eq!(
        fixture.handle.join().unwrap().unwrap_err(),
        "USB companion stopped responding."
    );
    assert_eq!(fixture.public.lock().unwrap()["_statusObservedMs"], before);
}

#[test]
fn usb_reconnect_rejects_replacement_before_starting_a_session() {
    let fixture = start_worker(Some("screen"), true);
    let probe = fixture
        .outgoing
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    send_frame(
        &fixture.device,
        json!({"protocolVersion":2,"deviceType":"esp32",
        "statusEvents":true,"requestId":probe["requestId"],"companionId":"replacement",
        "status":"offline","setupSaved":true,"connectionMode":"serial"}),
    );
    assert_eq!(
        fixture.handle.join().unwrap().unwrap_err(),
        "Detected USB companion identity changed before reconnect."
    );
    assert!(fixture.outgoing.try_recv().is_err());
    assert!(fixture.committed.try_recv().is_err());
    assert_eq!(fixture.public.lock().unwrap()["ready"], false);
}

#[test]
fn usb_legacy_worker_keeps_correlated_status_and_stops_cleanly() {
    let fixture = start_worker(Some("screen"), true);
    let receive = || {
        fixture
            .outgoing
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
    };
    let probe = receive();
    let mut status = json!({"protocolVersion":2,"deviceType":"esp32",
        "requestId":probe["requestId"],"companionId":"screen","status":"offline",
        "setupSaved":true,"connectionMode":"serial"});
    send_frame(&fixture.device, status.clone());
    assert_eq!(receive()["command"], "connect");
    let next = receive();
    assert_eq!(next["command"], "status");
    status["status"] = json!("awaiting-assignment");
    send_frame(&fixture.device, status.clone());
    assert_eq!(
        fixture
            .committed
            .recv_timeout(Duration::from_secs(2))
            .unwrap(),
        "offline"
    );
    assert!(fixture
        .committed
        .recv_timeout(Duration::from_millis(100))
        .is_err());
    status["requestId"] = next["requestId"].clone();
    send_frame(&fixture.device, status);
    assert_eq!(
        fixture
            .committed
            .recv_timeout(Duration::from_secs(2))
            .unwrap(),
        "awaiting-assignment"
    );
    fixture.control.send(Action::Stop).unwrap();
    fixture.wake.notify();
    assert_eq!(receive()["command"], "stop");
    assert_eq!(fixture.handle.join().unwrap(), Ok(()));
}
