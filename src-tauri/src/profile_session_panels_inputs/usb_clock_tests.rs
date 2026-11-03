use super::*;
use crate::module_console::push;
use osg_streams::{NativeLimits, NativeStream};

#[test]
fn fractional_rx_witness_survives_inactive_update_then_activation_snapshot() {
    let route = UsbRouteToken { module_id: "4f53474400112233445566778899aabb".into(),
        serial: "00112233445566778899aabb".into(), connection_generation: 1,
        boot: 2, configuration_generation: 3 };
    let capture = Capture::new(route.clone()).unwrap();
    let zero = Instant::now();
    let initial_at = zero + Duration::from_nanos(100_900);
    let updated_at = zero + Duration::from_nanos(200_100);
    let clock = Clock { started: zero + Duration::from_nanos(150_500), origin: 1_000_000 };
    let metadata = json!({"ok":true,"schemaVersion":1,"stream":3,"head":0,"oldest":1,"overflow":0,
        "buttonCount":1,"held":[false],"contactButtonCount":1,"maintainedButtons":[0],
        "encoderButtons":[],"selectorButtons":[],"axisCount":1,"axes":[65535],
        "axisNeutral":[32767],"axisUsage":[48],"axisAvailable":[true],"sampledAtMs":10});
    let ports = crate::profile_resource_ports::events(&metadata).unwrap().into_iter().map(|port| Port {
        logical: port.id.clone(), observed: port.id, contract: port.contract,
        resource_id: "fractional".into(), contract_hash: "fractional".into(),
    }).collect();
    let snapshot = |state, at| Snapshot { route: route.clone(), stream: 3, state: Some(Arc::new(state)),
        received_at: Some(at), ready: true, error: None };
    let native = NativeStream::new(1, snapshot(metadata.clone(), initial_at), 100, "usb:fractional",
        NativeLimits { max_subscribers: 4, max_snapshot_bytes: 32*1024, max_frame_bytes: 32*1024,
            max_pending_frames: 64, max_pending_bytes: 512*1024 }).unwrap();
    let mut native_reader = native.subscribe().unwrap();
    let mut worker = Worker { source: Source { spec: Spec { logical: "fractional".into(),
            target: Target::Usb { key: "fractional".into(), route: route.clone() }, ports },
        cursor: None, held: vec![], sequence: 0, at: 0, retry: zero, generation: 1, sampled_at: None },
        reader: capture.subscribe().unwrap(), capture, running: false, epoch: 0, failed: false, clock };
    let mut mailbox = Mailbox::default();
    worker.delivery(native_reader.recv().unwrap(), &mut mailbox, false, false, updated_at).unwrap();

    let mut current = metadata;
    current["sampledAtMs"] = json!(11);
    let batch = Batch { events: vec![], witnessed_at: updated_at,
        latest: push::Frame { boot: 2, sequence: 2, kind: push::Kind::Update,
            state: Arc::new(current.clone()), reason: Some("health".into()), exhausted: false,
            received_at: updated_at, sampled_at_ms: 11, stream: 3, head: 0, cursor: 0 } };
    native.publish(1, batch, snapshot(current, updated_at), 200, "usb:fractional").unwrap();
    let update = native_reader.recv().unwrap();
    assert!(matches!(&update, Delivery::Event(event) if event.captured_us == 200 && event.value.witnessed_at == updated_at));
    worker.delivery(update, &mut mailbox, false, false, updated_at).unwrap();
    assert_eq!(worker.source.at, 1_000_049);

    // The former exact-Instant/floored-microsecond reconstruction produced a
    // later time, making the real activation snapshot appear to go backwards.
    let rounded = initial_at + Duration::from_micros(200 - 100);
    assert_eq!(clock.at(rounded).unwrap(), 1_000_050);
    worker.reset().unwrap();
    let mut activation = native.subscribe().unwrap();
    let baseline = activation.recv().unwrap();
    assert!(matches!(&baseline, Delivery::Snapshot(value) if value.captured_us == 200
        && value.value.received_at == Some(updated_at)));
    worker.delivery(baseline, &mut mailbox, true, false, updated_at).unwrap();
    let frame = &mailbox.baselines["fractional"];
    assert_eq!(frame.captured_us, Counter(1_000_049));
    assert!(frame.changes.iter().any(|change| change.port_id == "axis:0"
        && change.sample == SignalSample::scalar(PortValue::Number(65535.0))));
    assert!(frame.changes.iter().any(|change| change.port_id == "button-event:0"
        && change.sample == SignalSample::Unknown));
    assert!(mailbox.diagnostics.is_empty());
}
