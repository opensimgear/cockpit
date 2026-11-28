use super::*;
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

const PROPERTIES: [&str; 16] = [
    "ID_PATH",
    "ID_PATH_TAG",
    "ID_USB_INTERFACE_NUM",
    "ID_SERIAL_SHORT",
    "ID_SERIAL",
    "ID_BUS",
    "ID_VENDOR_ID",
    "ID_MODEL_ID",
    "ID_INPUT",
    "ID_INPUT_JOYSTICK",
    "ID_INPUT_KEYBOARD",
    "ID_INPUT_MOUSE",
    "ID_INPUT_KEY",
    "ID_INPUT_TOUCHPAD",
    "ID_INPUT_TABLET",
    "ID_INPUT_POINTINGSTICK",
];
fn names(path: &Path, prefix: &str, maximum: usize) -> Result<(Vec<String>, bool), Failure> {
    let entries = fs::read_dir(path).map_err(|e| Failure::io("enumeration", e, false))?;
    let mut result = Vec::new();
    let mut truncated = false;
    for entry in entries {
        let entry = entry.map_err(|e| Failure::io("enumeration-entry", e, false))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(number) = name.strip_prefix(prefix) else {
            continue;
        };
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if result.len() == maximum {
            truncated = true;
            break;
        }
        result.push(name.to_owned());
    }
    result.sort();
    Ok((result, truncated))
}
fn pairs(bytes: &[u8]) -> Result<BTreeMap<String, String>, Failure> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Failure::new("malformed", "metadata", "Metadata text is not UTF-8"))?;
    let mut result = BTreeMap::new();
    for line in text.lines().take(128) {
        let line = line.strip_prefix("E:").unwrap_or(line);
        if let Some((key, value)) = line.split_once('=') {
            if key.len() > 64 || value.len() > 1024 {
                return Err(Failure::new(
                    "capacity",
                    "metadata",
                    "Metadata property exceeds its bound",
                ));
            }
            result.insert(key.into(), value.into());
        }
    }
    Ok(result)
}
fn properties(path: &Path, roots: &Roots) -> Result<BTreeMap<String, String>, Failure> {
    let all = if roots.native_udev {
        let device =
            udev::Device::from_syspath(path).map_err(|e| Failure::io("udev-identity", e, false))?;
        let mut properties = BTreeMap::new();
        for key in PROPERTIES {
            if let Some(value) = device.property_value(key) {
                let value = value.to_str().ok_or_else(|| {
                    Failure::new("malformed", "udev-identity", "udev identity is not UTF-8")
                })?;
                if value.len() > 1024 {
                    return Err(Failure::new(
                        "capacity",
                        "udev-identity",
                        "udev identity exceeds its bound",
                    ));
                }
                properties.insert(key.into(), value.into());
            }
        }
        properties
    } else {
        let dev = evidence::read(&path.join("dev"), 64, "sysfs-dev", false)?;
        let dev = std::str::from_utf8(&dev)
            .map_err(|_| Failure::new("malformed", "sysfs-dev", "Device number is not UTF-8"))?
            .trim();
        if !dev.bytes().all(|b| b.is_ascii_digit() || b == b':') {
            return Err(Failure::new(
                "malformed",
                "sysfs-dev",
                "Invalid device number",
            ));
        }
        let data = roots.udev_data.join(format!("c{dev}"));
        match evidence::read(&data, 16_384, "udev-properties", false) {
            Ok(bytes) => pairs(&bytes)?,
            Err(e) if e.kind == "unavailable" => BTreeMap::new(),
            Err(e) => return Err(e),
        }
    };
    Ok(all
        .into_iter()
        .filter(|(key, _)| PROPERTIES.contains(&key.as_str()))
        .collect())
}
fn input_flags(p: &BTreeMap<String, String>) -> (bool, bool, bool, bool) {
    let yes = |key| p.get(key).is_some_and(|value| value == "1");
    let joystick = yes("ID_INPUT_JOYSTICK");
    (
        joystick,
        yes("ID_INPUT_KEYBOARD"),
        yes("ID_INPUT_MOUSE") || yes("ID_INPUT_TOUCHPAD") || yes("ID_INPUT_POINTINGSTICK"),
        yes("ID_INPUT_KEY") && !joystick,
    )
}
fn evdev(roots: &Roots, inventory: &mut Inventory) -> BTreeMap<PathBuf, Vec<Evdev>> {
    let mut grouped = BTreeMap::<PathBuf, Vec<Evdev>>::new();
    let class = roots.sys.join("class/input");
    let (entries, truncated) = match names(&class, "event", MAX_EVDEV) {
        Ok(entries) => entries,
        Err(error) => {
            inventory.failures.push(error);
            return grouped;
        }
    };
    inventory.truncated |= truncated;
    for name in entries {
        let path = class.join(&name);
        let parent = match evidence::hid_parent(&path.join("device"), roots, false) {
            Ok(parent) => parent,
            Err(error) => {
                if error.kind != "unavailable" && inventory.failures.len() < MAX_FAILURES {
                    inventory.failures.push(error);
                }
                continue;
            }
        };
        let props = match properties(&path, roots) {
            Ok(p) => p,
            Err(error) => {
                if inventory.failures.len() < MAX_FAILURES {
                    inventory.failures.push(error);
                }
                BTreeMap::new()
            }
        };
        let (joystick, keyboard, mouse, system) = input_flags(&props);
        let sysfs_path = fs::canonicalize(&path).unwrap_or(path);
        grouped.entry(parent.clone()).or_default().push(Evdev {
            path: roots.dev.join("input").join(name),
            sysfs_path,
            hid_parent: parent,
            joystick,
            keyboard,
            mouse,
            system,
        });
    }
    grouped
}
pub(super) fn access(path: &Path) -> Result<bool, Failure> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Failure::new("malformed", "permission", "Device path contains NUL"))?;
    let status =
        // Kernel effective-credential/ACL evaluation, never mode-bit emulation.
        unsafe { libc::syscall(libc::SYS_faccessat2,libc::AT_FDCWD, path.as_ptr(), libc::R_OK, libc::AT_EACCESS) };
    if status == 0 {
        Ok(true)
    } else {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            Ok(false)
        } else {
            Err(Failure::io("permission", error, false))
        }
    }
}
fn identity(interface: &mut Interface, raw: &[u8]) {
    let fields = match pairs(raw) {
        Ok(fields) => fields,
        Err(error) => {
            interface.fail(error);
            return;
        }
    };
    interface.name = fields.get("HID_NAME").filter(|v| !v.is_empty()).cloned();
    interface.physical = fields.get("HID_PHYS").filter(|v| !v.is_empty()).cloned();
    interface.unique = fields.get("HID_UNIQ").filter(|v| !v.is_empty()).cloned();
    let id = fields.get("HID_ID").and_then(|id| {
        let mut pieces = id.split(':');
        let bus = u16::from_str_radix(pieces.next()?, 16).ok()?;
        let vendor = u32::from_str_radix(pieces.next()?, 16).ok()?;
        let product = u32::from_str_radix(pieces.next()?, 16).ok()?;
        pieces.next().is_none().then_some((bus, vendor, product))
    });
    if let Some((bus, vendor, product)) = id {
        interface.vendor_id = Some(vendor);
        interface.product_id = Some(product);
        interface.transport = match bus {
            3 => "usb".into(),
            5 => "bluetooth".into(),
            _ => format!("hid-bus-{bus:04x}"),
        };
    } else {
        interface.fail(Failure::new(
            "malformed",
            "hid-id",
            "Kernel HID_ID is unavailable or malformed",
        ));
    }
}
fn interface(roots: &Roots, name: &str, evdev: &BTreeMap<PathBuf, Vec<Evdev>>) -> Interface {
    let path = roots.dev.join(name);
    let sysfs_path = roots.sys.join("class/hidraw").join(name);
    let mut i = Interface {
        interface_id: path.to_string_lossy().into_owned(),
        path,
        sysfs_path,
        hid_parent: PathBuf::new(),
        hid_ancestry: Vec::new(),
        transport: "unknown".into(),
        vendor_id: None,
        product_id: None,
        name: None,
        physical: None,
        unique: None,
        udev_properties: BTreeMap::new(),
        evdev: Vec::new(),
        evdev_paths: Vec::new(),
        restricted: true,
        restriction_reasons: Vec::new(),
        read_access: None,
        descriptor_sha256: None,
        failures: Vec::new(),
        descriptor: None,
        evidence: None,
        roots: roots.clone(),
    };
    let parent = match evidence::hid_parent(&i.sysfs_path.join("device"), roots, false) {
        Ok(parent) => parent,
        Err(error) => {
            i.fail(error);
            i.restriction_reasons
                .push("HID ancestry is unproven; metadata only".into());
            return i;
        }
    };
    i.hid_parent = parent.clone();
    i.hid_ancestry = parent
        .ancestors()
        .take_while(|p| p.starts_with(&roots.sys))
        .take(128)
        .map(Path::to_path_buf)
        .collect();
    match evidence::read(&parent.join("uevent"), 16_384, "hid-uevent", false) {
        Ok(raw) => identity(&mut i, &raw),
        Err(e) => i.fail(e),
    }
    match properties(&i.sysfs_path, roots) {
        Ok(props) => i.udev_properties = props,
        Err(error) => i.fail(error),
    }
    i.evdev = evdev.get(&parent).cloned().unwrap_or_default();
    i.evdev_paths = i.evdev.iter().map(|e| e.path.clone()).collect();
    let (_, keyboard, mouse, system) = input_flags(&i.udev_properties);
    if keyboard || mouse || system || i.evdev.iter().any(|e| e.keyboard || e.mouse || e.system) {
        i.restriction_reasons
            .push("Correlated udev input policy is keyboard/mouse/system; metadata only".into());
    }
    match evidence::read(
        &parent.join("report_descriptor"),
        MAX_DESCRIPTOR,
        "descriptor",
        false,
    ) {
        Ok(raw) if !raw.is_empty() => {
            i.descriptor_sha256 = Some(evidence::digest(&raw));
            i.restriction_reasons.extend(restrictions::reasons(&raw));
            match evidence::current(&i, &raw, false) {
                Ok(evidence) => i.evidence = Some(evidence),
                Err(error) => i.fail(error),
            }
            i.descriptor = Some(raw);
        }
        Ok(_) => i.fail(Failure::new(
            "unavailable",
            "descriptor",
            "Report descriptor is empty",
        )),
        Err(error) => i.fail(error),
    }
    if i.descriptor.is_none() {
        i.restriction_reasons
            .push("Descriptor policy is unavailable; metadata only".into());
    }
    if let Some(evidence) = &i.evidence {
        if !evidence.node.character_device {
            i.fail(Failure::new(
                "malformed",
                "device-node",
                "Enumerated node is not a character device",
            ));
            i.restriction_reasons
                .push("Device namespace is not an admissible character node; metadata only".into());
        }
    } else {
        i.restriction_reasons
            .push("Generation identity could not be proved; metadata only".into());
    }
    match access(&i.path) {
        Ok(value) => {
            i.read_access = Some(value);
            if !value {
                i.fail(Failure::new(
                    "permission-denied",
                    "permission",
                    "Read access check denied; no device open was attempted",
                ));
            }
        }
        Err(error) => i.fail(error),
    }
    i.restricted = !i.restriction_reasons.is_empty();
    i
}
pub(super) fn enumerate(roots: &Roots) -> Result<Inventory, Failure> {
    let class = roots.sys.join("class/hidraw");
    let (entries, truncated) = match names(&class, "hidraw", MAX_INTERFACES) {
        Ok(entries) => entries,
        Err(failure) if failure.kind == "unavailable" => {
            return Ok(Inventory {
                interfaces: Vec::new(),
                failures: vec![failure],
                truncated: false,
            })
        }
        Err(failure) => return Err(failure),
    };
    let mut inventory = Inventory {
        interfaces: Vec::new(),
        failures: Vec::new(),
        truncated,
    };
    let correlated = evdev(roots, &mut inventory);
    let mut bytes = 0usize;
    for name in entries {
        let mut row = interface(roots, &name, &correlated);
        let size = row.descriptor.as_ref().map_or(0, Vec::len);
        if bytes
            .checked_add(size)
            .is_none_or(|n| n > MAX_DESCRIPTOR_TOTAL)
        {
            row.descriptor = None;
            row.evidence = None;
            row.restricted = true;
            row.restriction_reasons
                .push("Aggregate descriptor budget exceeded; metadata only".into());
            row.fail(Failure::new(
                "capacity",
                "descriptor",
                "Aggregate inventory descriptor byte budget exceeded",
            ));
            inventory.truncated = true;
        } else {
            bytes += size;
        }
        inventory.interfaces.push(row);
    }
    Ok(inventory)
}
