use super::*;
fn element(cookie: u32) -> Element {
    Element {
        cookie,
        page: 9,
        usage: 1,
        report: 0,
        parent: 0,
        bits: 1,
        min: 0,
        max: 1,
        relative: false,
        array: false,
        has_null: false,
    }
}
#[test]
fn descriptor_normalization_and_unknown_fields() {
    let mut axis = element(2);
    axis.page = 1;
    axis.usage = 0x30;
    axis.bits = 16;
    axis.min = -32768;
    axis.max = 32767;
    let mut hat = element(3);
    hat.page = 1;
    hat.usage = 0x39;
    hat.bits = 4;
    hat.min = 1;
    hat.max = 8;
    hat.has_null = true;
    let elements = vec![element(1), axis, hat];
    let c = controls(&elements).unwrap();
    assert_eq!(c[1].decode(0xffff).unwrap().native, -1);
    assert_eq!(c[2].decode(0).unwrap().normalized, None);
    let mut unknown = element(4);
    unknown.relative = true;
    assert_eq!(
        controls(&[unknown.clone()]).unwrap()[0].kind,
        Kind::Diagnostic
    );
    unknown.relative = false;
    unknown.array = true;
    assert_eq!(
        controls(&[unknown.clone()]).unwrap()[0].kind,
        Kind::Diagnostic
    );
    unknown.array = false;
    unknown.bits = 65536;
    assert_eq!(
        controls(&[unknown.clone()]).unwrap()[0].kind,
        Kind::Diagnostic
    );
    unknown.bits = 1;
    unknown.min = 2;
    assert_eq!(
        controls(&[unknown.clone()]).unwrap()[0].kind,
        Kind::Diagnostic
    );
    unknown.min = 0;
    unknown.page = 65536;
    assert!(controls(&[unknown.clone()]).is_err());
    unknown.page = 9;
    unknown.usage = 65536;
    assert!(controls(&[unknown.clone()]).is_err());
    unknown.usage = 1;
    unknown.report = 256;
    assert!(controls(&[unknown]).is_err());
    assert!(controls(&vec![element(1); MAX_CONTROLS + 1]).is_err());
}
#[test]
fn bounded_callbacks_drop_stale_values_after_loss() {
    let (tx, receive) = mpsc::sync_channel(MAX_EVENTS);
    let mut queue = Queue {
        receive,
        timestamps: BTreeMap::new(),
        lost: Arc::new(AtomicBool::new(false)),
    };
    tx.try_send((1, 1, 10)).unwrap();
    tx.try_send((99, 1, 10)).unwrap();
    assert!(matches!(&queue.drain(&[element(1)])[0],Event::Values(0,v) if v[&0]==1));
    assert!(queue.drain(&[element(1)]).is_empty());
    tx.try_send((1, i64::MAX, 11)).unwrap();
    assert!(matches!(&queue.drain(&[element(1)])[0], Event::Gap(_)));
    tx.try_send((1, 1, 10)).unwrap();
    queue.lost.store(true, Ordering::Release);
    assert!(matches!(&queue.drain(&[element(1)])[0], Event::Gap(_)));
    assert!(queue.drain(&[element(1)]).is_empty());
    assert!(allowed_transport("USB", "hardware"));
    assert!(allowed_transport("Bluetooth", ""));
    assert!(!allowed_transport("USB", "opensimgear-virtual:test"));
    assert!(!allowed_transport("Virtual", ""));
    assert!(!allowed_transport("", ""));
}
#[test]
fn native_manager_is_read_only_and_can_have_no_devices() {
    // No subscriptions or permissions are requested by enumeration.
    if let Ok(mut platform) = Mac::new() {
        assert!(platform.paths().unwrap().len() <= 1024);
        assert!(platform.open("not-an-observation").is_err());
    }
}

fn info() -> HidDeviceInfo {
    HidDeviceInfo {
        vendor_id: Some(0x123),
        product_id: Some(0x456),
        product: Some("Joystick".into()),
        manufacturer: None,
        serial_number: Some("Serial".into()),
        transport: Some("USB".into()),
        usage_page: Some(1),
        usage: Some(4),
        location_id: Some(12),
    }
}
#[test]
fn identity_and_unsupported_descriptors() {
    let a = definition(info(), "17", &[element(1)]).unwrap();
    assert_eq!(a.identity.serial.as_deref(), Some("Serial"));
    assert!(a.identity.path.contains("location:12"));
    assert_eq!(a.identity.collection, "usage:1:4");
    assert!(definition(info(), "17", &[]).is_err());
    let mut metadata = info();
    metadata.vendor_id = Some(65536);
    assert!(definition(metadata, "17", &[element(1)]).is_err());
    let mut metadata = info();
    metadata.product_id = Some(65536);
    assert!(definition(metadata, "17", &[element(1)]).is_err());
    let mut metadata = info();
    metadata.serial_number = Some("".into());
    metadata.product = None;
    metadata.location_id = None;
    let a = definition(metadata.clone(), "17", &[element(1)]).unwrap();
    let b = definition(metadata, "17", &[element(1)]).unwrap();
    assert_ne!(a.identity.path, b.identity.path);
    assert!(a.identity.serial.is_none());
    assert_eq!(a.name, "macOS controller");
}
struct FakeSnapshot {
    values: Vec<NativeValue>,
    fail: bool,
    lost: Arc<AtomicBool>,
    overflow: bool,
}
impl Snapshot for FakeSnapshot {
    fn values(&self) -> Result<Vec<NativeValue>, String> {
        if self.fail {
            return Err(capture::permission_error("macos", "not permitted"));
        }
        if self.overflow {
            self.lost.store(true, Ordering::Release);
        }
        Ok(self.values.clone())
    }
}
#[test]
fn baseline_requires_fresh_values_and_filters_older_callbacks() {
    let (send, receive) = mpsc::sync_channel(MAX_EVENTS);
    let lost = Arc::new(AtomicBool::new(false));
    let mut input = MacInput {
        device: FakeSnapshot {
            values: vec![(1, 1, 10)],
            fail: false,
            lost: lost.clone(),
            overflow: false,
        },
        elements: vec![element(1)],
        definition: definition(info(), "17", &[element(1)]).unwrap(),
        queue: Queue {
            receive,
            lost,
            timestamps: BTreeMap::new(),
        },
        _subscription: None,
    };
    assert_eq!(input.definition().name, "Joystick");
    assert!(matches!(&input.baseline().unwrap()[0],Event::Values(0,v) if v[&0]==1));
    send.try_send((1, 0, 9)).unwrap();
    send.try_send((1, 0, 10)).unwrap();
    send.try_send((1, 0, 0)).unwrap();
    assert!(input.poll().unwrap().is_empty());
    send.try_send((1, 0, 11)).unwrap();
    assert!(matches!(&input.poll().unwrap()[0],Event::Values(0,v) if v[&0]==0));
    input.device.values.clear();
    assert!(matches!(
        &input.baseline().unwrap()[0],
        Event::PendingBaseline
    ));
    input.device.values = vec![(1, 0, 0)];
    assert!(matches!(
        &input.baseline().unwrap()[0],
        Event::PendingBaseline
    ));
    input.device.values = vec![(1, i64::MAX, 10)];
    assert!(input.baseline().is_err());
    input.device.values = vec![(1, 0, 12)];
    input.device.overflow = true;
    assert!(matches!(&input.baseline().unwrap()[0], Event::Gap(_)));
    input.device.fail = true;
    assert!(input
        .baseline()
        .err()
        .unwrap()
        .contains("Privacy & Security"));
    input.device.fail = false;
    input.device.overflow = false;
    input.definition.controls[0].kind = Kind::Diagnostic;
    assert!(input.baseline().unwrap().is_empty());
}

struct FakeDevice {
    info: HidDeviceInfo,
    fail: bool,
    elements: Vec<Element>,
    dropped: Arc<std::sync::atomic::AtomicUsize>,
}
impl Drop for FakeDevice {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }
}
impl Snapshot for FakeDevice {
    fn values(&self) -> Result<Vec<NativeValue>, String> {
        Ok(vec![(1, 1, 5)])
    }
}
impl Device for FakeDevice {
    fn info(&self) -> HidDeviceInfo {
        self.info.clone()
    }
    fn elements(&self) -> Vec<Element> {
        self.elements.clone()
    }
    fn subscribe(
        &mut self,
        send: mpsc::SyncSender<NativeValue>,
        lost: Arc<AtomicBool>,
    ) -> Result<Box<dyn std::any::Any>, String> {
        if self.fail {
            return Err(capture::permission_error("macos", "denied"));
        }
        publish(&send, &lost, (1, 0, 6));
        Ok(Box::new(send))
    }
}
struct FakeProvider {
    devices: Vec<(String, Box<dyn Device>)>,
    fail: bool,
}
impl Provider for FakeProvider {
    fn scan(&mut self) -> Result<Vec<(String, Box<dyn Device>)>, String> {
        if self.fail {
            Err("enumeration failed".into())
        } else {
            Ok(std::mem::take(&mut self.devices))
        }
    }
}
#[test]
fn shared_iohid_lifecycle_filters_virtual_and_releases_owners() {
    let dropped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let make = |metadata: HidDeviceInfo, fail: bool| -> Box<dyn Device> {
        Box::new(FakeDevice {
            info: metadata,
            fail,
            elements: vec![element(1)],
            dropped: dropped.clone(),
        })
    };
    let mut virtual_info = info();
    virtual_info.serial_number = Some("opensimgear-virtual:owned".into());
    let mut platform = Mac {
        provider: Box::new(FakeProvider {
            devices: vec![
                ("a".into(), make(info(), false)),
                ("b".into(), make(virtual_info, false)),
                ("denied".into(), make(info(), true)),
            ],
            fail: false,
        }),
        devices: BTreeMap::new(),
    };
    assert_eq!(platform.paths().unwrap(), vec!["a", "denied"]);
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
    assert!(platform.open("missing").is_err());
    assert!(platform
        .open("denied")
        .err()
        .unwrap()
        .contains("Privacy & Security"));
    let mut input = platform.open("a").unwrap();
    assert_eq!(input.definition().identity.backend, "macos-iohid");
    assert!(matches!(&input.baseline().unwrap()[0],Event::Values(0,v) if v[&0]==1));
    assert!(input.poll().unwrap().is_empty());
    drop(input);
    assert_eq!(dropped.load(Ordering::Relaxed), 3);
    platform.provider = Box::new(FakeProvider {
        devices: vec![],
        fail: true,
    });
    assert!(platform.paths().is_err());
    platform.provider = Box::new(FakeProvider {
        devices: (0..1025)
            .map(|n| (n.to_string(), make(info(), false)))
            .collect(),
        fail: false,
    });
    assert!(platform.paths().is_err());
    let (send, receive) = mpsc::sync_channel(1);
    let lost = AtomicBool::new(false);
    publish(&send, &lost, (1, 1, 1));
    publish(&send, &lost, (1, 0, 2));
    assert!(lost.load(Ordering::Acquire));
    drop(receive);
    publish(&send, &lost, (1, 0, 3));
}
