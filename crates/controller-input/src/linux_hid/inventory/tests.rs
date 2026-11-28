use super::*;
use std::{
    fs::{self, File},
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

const GAME: &[u8] = &[
    0x05, 1, 0x09, 4, 0xa1, 1, 0x15, 0, 0x25, 0xff, 0x75, 8, 0x95, 1, 0x09, 0x30, 0x81, 2, 0xc0,
];
struct Fixture {
    _temp: tempfile::TempDir,
    roots: Roots,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let roots = Roots {
            sys: temp.path().join("sys"),
            dev: temp.path().join("dev"),
            udev_data: temp.path().join("udev"),
            native_udev: false,
        };
        for path in [
            roots.sys.join("class/hidraw"),
            roots.sys.join("class/input"),
            roots.sys.join("bus/hid"),
            roots.dev.join("input"),
            roots.udev_data.clone(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        Self { _temp: temp, roots }
    }
    fn hid(&self, name: &str, index: u32, bus: &str, descriptor: &[u8]) -> PathBuf {
        let parent = self.roots.sys.join("devices").join(name);
        fs::create_dir_all(&parent).unwrap();
        symlink(self.roots.sys.join("bus/hid"), parent.join("subsystem")).unwrap();
        fs::write(parent.join("uevent"),format!("HID_ID={bus}:0000231D:00000200\nHID_NAME=Fixture controller\nHID_PHYS=fixture/input0\nHID_UNIQ=\n")).unwrap();
        fs::write(parent.join("report_descriptor"), descriptor).unwrap();
        let entry = self
            .roots
            .sys
            .join("class/hidraw")
            .join(format!("hidraw{index}"));
        fs::create_dir(&entry).unwrap();
        symlink(&parent, entry.join("device")).unwrap();
        fs::write(entry.join("dev"), "0:0\n").unwrap();
        fs::write(self.roots.dev.join(format!("hidraw{index}")), []).unwrap();
        parent
    }
    fn event(&self, parent: &Path, index: u32, properties: &str) {
        let input = parent.join(format!("input/input{index}"));
        fs::create_dir_all(&input).unwrap();
        let event = self
            .roots
            .sys
            .join("class/input")
            .join(format!("event{index}"));
        fs::create_dir(&event).unwrap();
        symlink(input, event.join("device")).unwrap();
        fs::write(event.join("dev"), format!("13:{}\n", 64 + index)).unwrap();
        fs::write(
            self.roots.dev.join("input").join(format!("event{index}")),
            [],
        )
        .unwrap();
        fs::write(
            self.roots.udev_data.join(format!("c13:{}", 64 + index)),
            properties,
        )
        .unwrap();
    }
}

/// Synthetic metadata for the service's injected I/O boundary. These fixture
/// nodes are regular files, never live hidraw devices, and are deleted on return.
pub(in crate::hid::linux) fn fixture_inventory() -> Inventory {
    let fixture = Fixture::new();
    fixture.hid("usb/service-fixture", 0, "0003", GAME);
    let mut inventory = enumerate_at(&fixture.roots).unwrap();
    let row = &mut inventory.interfaces[0];
    row.restriction_reasons
        .retain(|reason| !reason.contains("character node"));
    row.failures
        .retain(|failure| failure.stage != "device-node");
    row.restricted = !row.restriction_reasons.is_empty();
    if let Some(evidence) = &mut row.evidence {
        evidence.node.character_device = true;
    }
    inventory
}

#[test]
fn exact_hid_parent_correlates_usb_composites_bluetooth_and_duplicate_units() {
    let fixture = Fixture::new();
    let first = fixture.hid("usb/1-1/1-1:1.0/0003:231D:0200.0001", 0, "0003", GAME);
    let second = fixture.hid("usb/1-1/1-1:1.1/0003:231D:0200.0002", 1, "0003", GAME);
    let bluetooth = fixture.hid("bluetooth/hci0/0005:231D:0200.0003", 2, "0005", GAME);
    fixture.event(&first, 0, "E:ID_INPUT_JOYSTICK=1\n");
    fixture.event(&first, 3, "E:ID_INPUT_JOYSTICK=1\n");
    fixture.event(&second, 1, "E:ID_INPUT_KEYBOARD=1\n");
    fixture.event(&bluetooth, 2, "E:ID_INPUT_JOYSTICK=1\n");
    let inventory = enumerate_at(&fixture.roots).unwrap();
    assert_eq!(inventory.interfaces.len(), 3);
    let row = &inventory.interfaces[0];
    assert_eq!(row.hid_parent, first.canonicalize().unwrap());
    assert_eq!(
        row.evdev_paths,
        vec![
            fixture.roots.dev.join("input/event0"),
            fixture.roots.dev.join("input/event3")
        ]
    );
    assert_eq!(
        inventory.interfaces[1].evdev_paths,
        vec![fixture.roots.dev.join("input/event1")]
    );
    assert!(inventory.interfaces[1]
        .restriction_reasons
        .iter()
        .any(|r| r.contains("keyboard/mouse/system")));
    assert!(!row
        .restriction_reasons
        .iter()
        .any(|r| r.contains("keyboard/mouse/system")));
    assert_eq!(inventory.interfaces[2].transport, "bluetooth");
    assert_ne!(row.interface_id, inventory.interfaces[1].interface_id);
    assert_eq!(row.vendor_id, Some(0x231d));
    assert_eq!(row.product_id, Some(0x200));
    assert_eq!(row.descriptor.as_deref(), Some(GAME));
    let json = serde_json::to_value(row).unwrap();
    assert!(json.get("descriptor").is_none());
    assert!(json.get("evidence").is_none());
}

#[test]
fn keyboard_mouse_system_and_mixed_descriptors_are_metadata_only() {
    assert!(restrictions::reasons(GAME).is_empty());
    for (page, usage) in [
        (1, 1),
        (1, 2),
        (1, 6),
        (1, 7),
        (1, 0x80),
        (7, 1),
        (0x0c, 1),
        (0x84, 1),
        (0x85, 1),
    ] {
        let mut descriptor = GAME.to_vec();
        descriptor[1] = page;
        descriptor[3] = usage;
        assert!(
            !restrictions::reasons(&descriptor).is_empty(),
            "{page}:{usage}"
        );
        let mut mixed = GAME.to_vec();
        mixed.extend(descriptor);
        assert!(!restrictions::reasons(&mixed).is_empty());
    }
    assert!(!restrictions::reasons(&[0xfe]).is_empty());
    assert!(!restrictions::reasons(&[0x75, 8, 0x95, 1, 0x81, 2]).is_empty());
}

#[test]
fn unknown_ancestry_descriptor_failures_and_permission_errors_stay_visible() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let parent = fixture.hid("usb/hid", 0, "0003", GAME);
    fs::set_permissions(
        parent.join("report_descriptor"),
        fs::Permissions::from_mode(0),
    )
    .unwrap();
    if unsafe { libc::geteuid() } != 0 {
        assert!(enumerate_at(&fixture.roots).unwrap().interfaces[0]
            .failures
            .iter()
            .any(|failure| failure.kind == "permission-denied" && failure.stage == "descriptor"));
    }
    fs::set_permissions(
        parent.join("report_descriptor"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    fs::remove_file(parent.join("report_descriptor")).unwrap();
    let inventory = enumerate_at(&fixture.roots).unwrap();
    assert_eq!(inventory.interfaces.len(), 1);
    let row = &inventory.interfaces[0];
    assert!(row.restricted && row.descriptor.is_none());
    assert!(row
        .failures
        .iter()
        .any(|f| f.kind == "unavailable" && f.stage == "descriptor"));
    fs::write(
        parent.join("report_descriptor"),
        vec![0; MAX_DESCRIPTOR + 1],
    )
    .unwrap();
    assert!(enumerate_at(&fixture.roots).unwrap().interfaces[0]
        .failures
        .iter()
        .any(|f| f.kind == "capacity"));
    fs::remove_file(parent.join("subsystem")).unwrap();
    assert_eq!(
        evidence::hid_parent(
            &fixture.roots.sys.join("class/hidraw/hidraw0/device"),
            &fixture.roots,
            true
        )
        .unwrap_err()
        .kind,
        "stale-generation"
    );
    assert!(enumerate_at(&fixture.roots).unwrap().interfaces[0]
        .failures
        .iter()
        .any(|f| f.stage == "hid-parent"));
    assert_eq!(
        Failure::io(
            "read",
            std::io::Error::from_raw_os_error(libc::EACCES),
            true
        )
        .kind,
        "permission-denied"
    );
    assert_eq!(
        Failure::io(
            "read",
            std::io::Error::from_raw_os_error(libc::ENOENT),
            true
        )
        .kind,
        "stale-generation"
    );
}

#[test]
fn symlink_escape_and_major_minor_mismatch_do_not_gain_open_authority() {
    let fixture = Fixture::new();
    fixture.hid("usb/hid", 0, "0003", GAME);
    let entry = fixture.roots.sys.join("class/hidraw/hidraw0");
    fs::write(entry.join("dev"), "1:2").unwrap();
    let inventory = enumerate_at(&fixture.roots).unwrap();
    let row = &inventory.interfaces[0];
    assert!(row.evidence.is_none() && row.restricted);
    assert!(row.failures.iter().any(|f| f.stage == "sysfs-dev"));
    fs::remove_file(entry.join("device")).unwrap();
    symlink(fixture._temp.path(), entry.join("device")).unwrap();
    assert!(enumerate_at(&fixture.roots).unwrap().interfaces[0]
        .failures
        .iter()
        .any(|f| f.kind == "malformed" && f.stage == "hid-parent"));
}

#[test]
fn generation_fences_fd_inode_parent_identity_and_descriptor_mutations() {
    let fixture = Fixture::new();
    fixture.hid("usb/hid", 0, "0003", GAME);
    let row = enumerate_at(&fixture.roots).unwrap().interfaces.remove(0);
    let mut expected = row.evidence.clone().unwrap();
    expected.node.character_device = true;
    assert!(evidence::compare(&expected, &expected, &expected.node, GAME).is_ok());
    let mut changed = expected.clone();
    changed.node.inode += 1;
    assert_eq!(
        evidence::compare(&expected, &changed, &expected.node, GAME)
            .unwrap_err()
            .kind,
        "stale-generation"
    );
    let mut changed = expected.clone();
    changed.parent.inode += 1;
    assert!(evidence::compare(&expected, &changed, &expected.node, GAME).is_err());
    let mut changed = expected.clone();
    changed.node.mode ^= 0o400;
    assert!(evidence::compare(&expected, &changed, &expected.node, GAME).is_err());
    assert_eq!(
        evidence::verify_access(Ok(false)).unwrap_err().kind,
        "permission-denied"
    );
    assert!(evidence::compare(&expected, &expected, &expected.node, &[1, 2, 3]).is_err());
    let mut changed = expected.clone();
    changed.uevent_sha256 = "changed".into();
    assert!(evidence::compare(&expected, &changed, &expected.node, GAME).is_err());
    let file = File::open(&row.path).unwrap();
    assert_eq!(
        verify_opened(&row, &file, GAME).unwrap_err().kind,
        "stale-generation",
        "Regular fixture files are never admitted as live hidraw fds"
    );
    let before = evidence::stamp(&file.metadata().unwrap());
    fs::remove_file(&row.path).unwrap();
    fs::write(&row.path, []).unwrap();
    assert_ne!(
        before.inode,
        evidence::metadata(&row.path, "test", true).unwrap().inode
    );
    assert_eq!(
        verify_opened(&row, &file, GAME).unwrap_err().kind,
        "stale-generation"
    );
}

#[test]
fn enumeration_limits_and_missing_subsystems_have_explicit_outcomes() {
    let fixture = Fixture::new();
    for index in 0..MAX_INTERFACES + 1 {
        fs::create_dir(
            fixture
                .roots
                .sys
                .join("class/hidraw")
                .join(format!("hidraw{index}")),
        )
        .unwrap();
    }
    let inventory = enumerate_at(&fixture.roots).unwrap();
    assert!(inventory.truncated);
    assert_eq!(inventory.interfaces.len(), MAX_INTERFACES);
    let mut roots = fixture.roots.clone();
    roots.sys = fixture._temp.path().join("missing");
    let missing = enumerate_at(&roots).unwrap();
    assert!(missing.interfaces.is_empty() && !missing.truncated);
    assert_eq!(missing.failures[0].kind, "unavailable");
}

#[test]
fn vendor_roots_cannot_hide_keyboard_fields_without_an_evdev_sibling() {
    let fixture = Fixture::new();
    let descriptor = [
        0x06, 0, 0xff, 0x09, 1, 0xa1, 1, 0x05, 7, 0x19, 4, 0x29, 0x1d, 0x15, 0, 0x25, 1, 0x75, 1,
        0x95, 0x1a, 0x81, 2, 0xc0,
    ];
    fixture.hid("usb/vendor-keyboard", 0, "0003", &descriptor);
    let inventory = enumerate_at(&fixture.roots).unwrap();
    let row = &inventory.interfaces[0];
    assert!(row.evdev_paths.is_empty() && row.restricted);
    assert!(row
        .restriction_reasons
        .iter()
        .any(|reason| reason.contains("Explicit keyboard")));
    let vendor = [
        0x06, 0, 0xff, 0x09, 7, 0xa1, 1, 0x09, 6, 0x15, 0, 0x25, 0xff, 0x75, 8, 0x95, 1, 0x81, 2,
        0xc0,
    ];
    assert!(
        restrictions::reasons(&vendor).is_empty(),
        "Vendor page IDs resembling keyboard numbers are not keyboard evidence"
    );
    let feature_keyboard = [
        0x06, 0, 0xff, 0x09, 1, 0xa1, 1, 0x05, 7, 0x09, 4, 0x15, 0, 0x25, 1, 0x75, 1, 0x95, 1,
        0xb1, 2, 0xc0,
    ];
    assert!(
        restrictions::reasons(&feature_keyboard).is_empty(),
        "Feature-only page metadata is not a captured input stream"
    );
}

#[test]
fn permission_loss_does_not_reuse_an_existing_readable_fd() {
    use std::{io::Read, os::unix::fs::PermissionsExt};
    let fixture = Fixture::new();
    fixture.hid("usb/permission", 0, "0003", GAME);
    let row = enumerate_at(&fixture.roots).unwrap().interfaces.remove(0);
    fs::write(&row.path, b"fixture").unwrap();
    let mut file = File::open(&row.path).unwrap();
    fs::set_permissions(&row.path, fs::Permissions::from_mode(0)).unwrap();
    let mut retained = Vec::new();
    file.read_to_end(&mut retained).unwrap();
    assert_eq!(retained, b"fixture");
    if unsafe { libc::geteuid() } != 0 {
        assert_eq!(scan::access(&row.path).unwrap(), false);
        assert_eq!(
            verify_opened(&row, &file, GAME).unwrap_err().kind,
            "permission-denied"
        );
    }
    assert_eq!(
        evidence::verify_access(Ok(false)).unwrap_err().kind,
        "permission-denied"
    );
}
