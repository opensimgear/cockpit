use super::*;
use std::collections::VecDeque;
fn axes() -> Vec<Axis> {
    vec![
        Axis {
            code: 0,
            min: -32768,
            max: 32767,
        },
        Axis {
            code: 16,
            min: -1,
            max: 1,
        },
        Axis {
            code: 17,
            min: -1,
            max: 1,
        },
    ]
}
#[test]
fn descriptors_signed_axes_hats_and_diagnostics() {
    let mut axes = axes();
    axes.extend((1..=10).map(|code| Axis {
        code,
        min: 0,
        max: 1023,
    }));
    axes.push(Axis {
        code: 40,
        min: 0,
        max: 0,
    });
    let controls = controls(&[0x20, 0x110, 0x120, 0x2ff, 0x300], &axes);
    assert_eq!(
        controls.iter().filter(|c| c.kind == Kind::Button).count(),
        2
    );
    assert_eq!(controls.iter().filter(|c| c.kind == Kind::Hat).count(), 1);
    assert!(controls.iter().any(|c| c.kind == Kind::Diagnostic));
    assert_eq!(
        controls
            .iter()
            .find(|c| c.data_index == 0x1000)
            .unwrap()
            .decode((-32768i32) as u32)
            .unwrap()
            .normalized,
        Some(-32768)
    );
    for (x, y, expected) in [
        (0, 0, 8),
        (0, -1, 0),
        (1, -1, 1),
        (1, 0, 2),
        (1, 1, 3),
        (0, 1, 4),
        (-1, 1, 5),
        (-1, 0, 6),
        (-1, -1, 7),
    ] {
        assert_eq!(hat(x, y).unwrap(), expected);
    }
    assert!(hat(2, 0).is_err());
    assert_eq!(
        super::controls(
            &[],
            &[Axis {
                code: 16,
                min: 0,
                max: 255
            }]
        )[0]
        .kind,
        Kind::Diagnostic
    );
}
#[test]
fn dropped_events_wait_for_boundary_and_do_not_replay() {
    let controls = controls(&[0x120], &axes());
    let mut f = Frames::default();
    let events = f
        .feed(
            &[(1, 0x120, 1), (3, 16, 1), (3, 17, -1), (0, 0, 0)],
            &controls,
        )
        .unwrap();
    assert!(matches!(&events[0],Event::Values(0,v) if v[&0x120]==1 && v[&0x2008]==1));
    let events = f
        .feed(&[(0, 3, 0), (1, 0x120, 0), (3, 0, 32767)], &controls)
        .unwrap();
    assert!(matches!(&events[0], Event::Gap(_)));
    assert!(!f.recovery_boundary);
    assert!(f.values.is_empty());
    f.feed(&[(0, 0, 0), (1, 0x120, 1), (0, 0, 0)], &controls)
        .unwrap();
    assert!(f.recovery_boundary);
    assert!(f.values.is_empty());
    let mut f = Frames::default();
    assert!(f.feed(&[(1, 0x120, 2)], &controls).is_err());
    let mut f = Frames::default();
    let events = f.feed(&vec![(3, 0, 0); MAX_EVENTS + 1], &controls).unwrap();
    assert!(matches!(&events[0], Event::Gap(_)));
    assert!(f.dropping);
}
struct Fake {
    reads: VecDeque<Result<Vec<(u16, u16, i32)>, String>>,
    fail: bool,
}
impl Kernel for Fake {
    fn suppress(&mut self, _enabled: bool) -> Result<(), String> {
        if self.fail {
            Err("grab failed".into())
        } else {
            Ok(())
        }
    }
    fn read(&mut self) -> Result<Vec<(u16, u16, i32)>, String> {
        self.reads.pop_front().unwrap_or(Ok(vec![]))
    }
    fn snapshot(&self, _: &[u16], _: &[Axis], controls: &[Control]) -> Result<Frames, String> {
        if self.fail {
            return Err("snapshot failed".into());
        }
        let mut f = Frames::default();
        for value in [(1, 0x120, 0), (3, 0, 17), (3, 16, 0), (3, 17, 0)] {
            f.value(value.0, value.1, value.2, controls)?;
        }
        Ok(f)
    }
}
fn input() -> LinuxInput<Fake> {
    let controls = controls(&[0x120], &axes());
    LinuxInput {
        device: Fake {
            reads: VecDeque::new(),
            fail: false,
        },
        definition: Definition {
            identity: Identity {
                backend: "linux-evdev".into(),
                vendor_id: 1,
                product_id: 2,
                serial: None,
                collection: "0".into(),
                fingerprint: String::new(),
                path: "usb:a".into(),
            },
            name: "test".into(),
            controls,
            diagnostics: vec![],
        },
        frames: Frames::default(),
        keys: vec![0x120],
        axes: axes(),
        suppressed: false,
    }
}
#[test]
fn recovery_queries_kernel_and_never_invents_neutral() {
    let mut input = input();
    assert_eq!(input.definition().name, "test");
    assert!(matches!(&input.baseline().unwrap()[0],Event::Values(0,v) if v[&0x1000]==17));
    input
        .device
        .reads
        .push_back(Ok(vec![(0, 3, 0), (1, 0x120, 1)]));
    assert!(matches!(&input.poll().unwrap()[0], Event::Gap(_)));
    assert!(matches!(
        &input.baseline().unwrap()[0],
        Event::PendingBaseline
    ));
    input
        .device
        .reads
        .push_back(Ok(vec![(0, 0, 0), (1, 0x120, 1)]));
    assert!(matches!(&input.baseline().unwrap()[0],Event::Values(0,v) if v[&0x120]==0));
    assert!(!input.frames.dropping);
    input
        .device
        .reads
        .push_back(Ok(vec![(3, 0, 20), (0, 0, 0)]));
    assert!(matches!(&input.poll().unwrap()[0],Event::Values(0,v) if v[&0x1000]==20));
    input
        .device
        .reads
        .push_back(Ok(vec![(3, 0, 20); MAX_EVENTS + 1]));
    assert!(matches!(&input.poll().unwrap()[0], Event::Gap(_)));
    input.frames = Frames::default();
    input.device.reads = (0..16).map(|_| Ok(vec![(3, 0, 1)])).collect();
    assert!(matches!(
        &input.baseline().unwrap()[0],
        Event::PendingBaseline
    ));
    input.device.fail = true;
    assert!(input.baseline().is_err());
    input.device.reads.push_back(Err("removed".into()));
    assert!(input.poll().is_err());
}
#[test]
fn read_only_native_enumeration_and_denial() {
    let mut platform = Linux::new();
    let paths = platform.paths().unwrap();
    assert!(paths.len() <= 1024);
    assert!(platform.open("/nonexistent").is_err());
    let temp = tempfile::tempdir().unwrap();
    let node = temp.path().join("not-input");
    std::fs::write(&node, b"").unwrap();
    platform.paths.insert(
        "bad".into(),
        Metadata {
            node: node.to_string_lossy().into(),
            ..Default::default()
        },
    );
    assert!(platform.open("bad").is_err());
    platform.paths.insert(
        "denied".into(),
        Metadata {
            node: "/no/such/controller".into(),
            ..Default::default()
        },
    );
    assert!(platform
        .open("denied")
        .err()
        .unwrap()
        .contains("Grant read access"));
}

#[test]
fn stable_topology_serials_and_virtual_exclusion() {
    let metadata = Metadata {
        node: "/dev/input/event4".into(),
        path: "pci-usb-2".into(),
        serial: Some("serial1".into()),
        interface: "01".into(),
    };
    let a = definition(
        metadata.clone(),
        "usb/input0",
        "",
        (12, 34),
        "Joystick",
        &[0x120],
        &axes(),
    )
    .unwrap();
    let mut moved = metadata.clone();
    moved.node = "/dev/input/event8".into();
    let b = definition(
        moved,
        "usb/input0",
        "",
        (12, 34),
        "Joystick",
        &[0x120],
        &axes(),
    )
    .unwrap();
    assert_eq!(a.identity, b.identity);
    assert_eq!(a.identity.serial.as_deref(), Some("serial1"));
    assert_eq!(a.identity.collection, "interface:01");
    assert_eq!(
        definition(
            metadata.clone(),
            "usb",
            "unique",
            (12, 34),
            "Joystick",
            &[0x120],
            &[]
        )
        .unwrap()
        .identity
        .serial
        .as_deref(),
        Some("unique")
    );
    assert!(definition(
        metadata.clone(),
        "opensimgear-virtual:a",
        "",
        (12, 34),
        "Joystick",
        &[0x120],
        &[]
    )
    .is_err());
    assert!(definition(
        metadata.clone(),
        "usb",
        "osg-virtual:a",
        (12, 34),
        "Joystick",
        &[0x120],
        &[]
    )
    .is_err());
    assert!(definition(metadata, "usb", "", (12, 34), "Joystick", &[], &[]).is_err());
    let a = definition(
        Metadata::default(),
        "unknown",
        "",
        (12, 34),
        "Joystick",
        &[0x120],
        &[],
    )
    .unwrap();
    let b = definition(
        Metadata::default(),
        "unknown",
        "",
        (12, 34),
        "Joystick",
        &[0x120],
        &[],
    )
    .unwrap();
    assert_ne!(a.identity.path, b.identity.path);
    assert!(a.identity.path.starts_with("session:"));
    let device = udev::Device::from_syspath(std::path::Path::new("/sys/class/net/lo")).unwrap();
    assert_eq!(property(&device, "ID_INPUT_JOYSTICK"), "");
    assert!(!eligible(&device));
}

#[test]
fn kernel_snapshot_keeps_current_held_controls() {
    let axes = axes();
    let controls = controls(&[0x120, 0x121], &axes);
    let snapshot = snapshot_values(
        &[0x120, 0x121],
        &axes,
        &|k| k == 0x120,
        &|axis| if axis == 0 { -100 } else { 0 },
        &controls,
    )
    .unwrap();
    assert_eq!(snapshot.values[&0x120], 1);
    assert_eq!(snapshot.values[&0x121], 0);
    assert_eq!(snapshot.values[&0x1000], (-100i32) as u32);
    assert_eq!(snapshot.values[&0x2008], 8);
    assert!(snapshot_values(&[], &axes, &|_| false, &|_| 5, &controls).is_err());
}

#[test]
fn keyboard_mouse_and_virtual_nodes_are_not_controllers() {
    assert!(controller_node("1", "", "", "/sys/devices/pci/input0"));
    assert!(!controller_node("", "1", "", "/sys/devices/pci/input0"));
    assert!(!controller_node("1", "1", "", "/sys/devices/pci/input0"));
    assert!(!controller_node("1", "", "1", "/sys/devices/pci/input0"));
    assert!(!controller_node("1", "", "", "/sys/devices/virtual/input0"));
}

#[test]
fn owned_grab_releases_and_resets_frames() {
    let mut i = input();
    i.suppress(false).unwrap();
    i.suppress(true).unwrap();
    assert!(i.suppressed);
    i.suppress(false).unwrap();
    assert!(!i.suppressed);
    i.device.fail = true;
    assert!(i.suppress(true).is_err());
    assert!(i.suppressed);
    assert!(i.suppress(false).is_err());
    i.device.fail = false;
    drop(i);
}
