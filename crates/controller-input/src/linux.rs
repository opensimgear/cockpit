//! evdev SYN_REPORT assembly. No grabs, writes, uinput access or permission mutations.
use crate::{
    capture::{self, Event, Input, Platform, Values, MAX_EVENTS},
    registry::{Control, Definition, Identity, Kind},
};
use evdev::{raw_stream::RawDevice, EventType, KeyCode};
use std::{collections::BTreeMap, fs::OpenOptions, os::unix::fs::OpenOptionsExt};
#[derive(Clone, Default)]
struct Metadata {
    node: String,
    path: String,
    serial: Option<String>,
    interface: String,
}
#[derive(Clone, Copy, Debug)]
struct Axis {
    code: u16,
    min: i32,
    max: i32,
}
fn controls(keys: &[u16], axes: &[Axis]) -> Vec<Control> {
    let mut result = Vec::new();
    for &key in keys.iter().filter(|k| **k >= 0x120 && **k < 0x300) {
        let mut c = Control::new(9, key, 0, 0, key);
        c.id = format!("evdev:key:{key:04x}");
        result.push(c);
    }
    for axis in axes {
        if (16..24).contains(&axis.code)
            && axes.iter().any(|a| a.code == (axis.code ^ 1))
            && axis.min == -1
            && axis.max == 1
            && axes
                .iter()
                .any(|a| a.code == (axis.code ^ 1) && a.min == -1 && a.max == 1)
        {
            if axis.code % 2 == 0 {
                let mut c = Control::new(1, 0x39, 0, 0, 0x2000 + axis.code / 2);
                c.id = format!("evdev:hat:{}", (axis.code - 16) / 2);
                c.bits = 8;
                c.max = 7;
                c.has_null = true;
                result.push(c);
            }
        } else {
            let usage = match axis.code {
                0..=5 => 0x30 + axis.code,
                6 => 0x36,
                7 => 0x37,
                8 => 0x38,
                9 => 0xbb,
                10 => 0xba,
                _ => 0,
            };
            let mut c = Control::new(
                if axis.code == 9 || axis.code == 10 {
                    2
                } else {
                    1
                },
                usage,
                0,
                0,
                0x1000 + axis.code,
            );
            c.id = format!("evdev:abs:{:04x}", axis.code);
            c.bits = 32;
            c.min = axis.min.into();
            c.max = axis.max.into();
            if usage == 0 || axis.min >= axis.max {
                c.kind = Kind::Diagnostic;
            }
            result.push(c);
        }
    }
    result
}
fn hat(x: i32, y: i32) -> Result<u32, String> {
    match (x, y) {
        (0, 0) => Ok(8),
        (0, -1) => Ok(0),
        (1, -1) => Ok(1),
        (1, 0) => Ok(2),
        (1, 1) => Ok(3),
        (0, 1) => Ok(4),
        (-1, 1) => Ok(5),
        (-1, 0) => Ok(6),
        (-1, -1) => Ok(7),
        _ => Err("Invalid evdev hat state.".into()),
    }
}
#[derive(Default)]
struct Frames {
    values: Values,
    hats: BTreeMap<u16, i32>,
    dropping: bool,
    recovery_boundary: bool,
    pending: usize,
}
impl Frames {
    fn value(
        &mut self,
        kind: u16,
        code: u16,
        value: i32,
        controls: &[Control],
    ) -> Result<(), String> {
        if kind == 1 && controls.iter().any(|c| c.data_index == code) {
            if !(0..=1).contains(&value) {
                return Err("Unexpected controller button value.".into());
            }
            self.values.insert(code, value as u32);
        } else if kind == 3 {
            let hat_index = 0x2000 + code / 2;
            if (16..24).contains(&code) && controls.iter().any(|c| c.data_index == hat_index) {
                self.hats.insert(code, value);
                if let (Some(x), Some(y)) =
                    (self.hats.get(&(code & !1)), self.hats.get(&(code | 1)))
                {
                    self.values.insert(hat_index, hat(*x, *y)?);
                }
            } else if controls.iter().any(|c| c.data_index == 0x1000 + code) {
                self.values.insert(0x1000 + code, value as u32);
            }
        }
        Ok(())
    }
    fn feed(
        &mut self,
        events: &[(u16, u16, i32)],
        controls: &[Control],
    ) -> Result<Vec<Event>, String> {
        let mut result = Vec::new();
        for &(kind, code, value) in events {
            if kind == 0 && code == 3 {
                self.values.clear();
                self.hats.clear();
                self.dropping = true;
                self.recovery_boundary = false;
                self.pending = 0;
                result.push(Event::Gap(
                    "Linux SYN_DROPPED: waiting for SYN_REPORT and a kernel snapshot.".into(),
                ));
            } else if self.dropping {
                if kind == 0 && code == 0 {
                    self.recovery_boundary = true;
                }
            } else if kind == 0 && code == 0 {
                result.push(Event::Values(0, self.values.clone()));
                self.pending = 0;
            } else {
                self.pending += 1;
                if self.pending > MAX_EVENTS {
                    self.values.clear();
                    self.hats.clear();
                    self.dropping = true;
                    self.recovery_boundary = false;
                    result.push(Event::Gap("Linux frame exceeded its event bound.".into()));
                } else {
                    self.value(kind, code, value, controls)?;
                }
            }
        }
        Ok(result)
    }
}
pub struct Linux {
    paths: BTreeMap<String, Metadata>,
}
impl Linux {
    pub fn new() -> Self {
        Self {
            paths: BTreeMap::new(),
        }
    }
}
fn property(device: &udev::Device, key: &str) -> String {
    device
        .property_value(key)
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .into()
}
fn controller_node(joystick: &str, keyboard: &str, mouse: &str, path: &str) -> bool {
    joystick == "1" && keyboard != "1" && mouse != "1" && !path.contains("/devices/virtual/")
}
fn eligible(device: &udev::Device) -> bool {
    controller_node(
        &property(device, "ID_INPUT_JOYSTICK"),
        &property(device, "ID_INPUT_KEYBOARD"),
        &property(device, "ID_INPUT_MOUSE"),
        &device.syspath().to_string_lossy(),
    )
}
impl Platform for Linux {
    fn paths(&mut self) -> Result<Vec<String>, String> {
        let mut enumeration = udev::Enumerator::new().map_err(|e| e.to_string())?;
        enumeration
            .match_subsystem("input")
            .map_err(|e| e.to_string())?;
        self.paths.clear();
        for device in enumeration.scan_devices().map_err(|e| e.to_string())? {
            if self.paths.len() >= 1024 {
                return Err("Too many input interfaces.".into());
            }
            if !eligible(&device) {
                continue;
            }
            let Some(node) = device.devnode().filter(|p| {
                p.file_name()
                    .is_some_and(|s| s.to_string_lossy().starts_with("event"))
            }) else {
                continue;
            };
            let node = node.to_string_lossy().into_owned();
            let serial = property(&device, "ID_SERIAL_SHORT");
            self.paths.insert(
                node.clone(),
                Metadata {
                    node,
                    path: property(&device, "ID_PATH"),
                    serial: (!serial.is_empty()).then_some(serial),
                    interface: property(&device, "ID_USB_INTERFACE_NUM"),
                },
            );
        }
        Ok(self.paths.keys().cloned().collect())
    }
    fn open(&mut self, path: &str) -> Result<Box<dyn Input>, String> {
        let metadata = self
            .paths
            .get(path)
            .ok_or("Controller disappeared.")?
            .clone();
        // Explicit read-only open: RawDevice::open otherwise tries read/write first.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(&metadata.node)
            .map_err(|e| capture::permission_error("linux", e))?;
        let device = RawDevice::from_fd(file.into()).map_err(|e| e.to_string())?;
        let physical = device.physical_path().unwrap_or("");
        let unique = device.unique_name().unwrap_or("");
        if capture::owned_virtual(physical)
            || capture::owned_virtual(unique)
            || device.input_id().bus_type().0 == 6
        {
            return Err("Virtual evdev controller excluded from capture.".into());
        }
        let axes: Vec<_> = device
            .get_absinfo()
            .map_err(|e| e.to_string())?
            .map(|(code, info)| Axis {
                code: code.0,
                min: info.minimum(),
                max: info.maximum(),
            })
            .collect();
        let keys: Vec<_> = device
            .supported_keys()
            .map(|s| s.iter().map(|k| k.0).collect())
            .unwrap_or_default();
        let id = device.input_id();
        let definition = definition(
            metadata,
            physical,
            unique,
            (id.vendor(), id.product()),
            device.name().unwrap_or("Linux controller"),
            &keys,
            &axes,
        )?;
        Ok(Box::new(LinuxInput {
            device,
            definition,
            frames: Frames::default(),
            axes,
            keys,
        }))
    }
}
fn definition(
    metadata: Metadata,
    physical: &str,
    unique: &str,
    id: (u16, u16),
    name: &str,
    keys: &[u16],
    axes: &[Axis],
) -> Result<Definition, String> {
    if capture::owned_virtual(physical) || capture::owned_virtual(unique) {
        return Err("Cockpit virtual output excluded.".into());
    }
    let controls = controls(keys, axes);
    if controls.is_empty() {
        return Err("No supported evdev controller controls.".into());
    }
    // Never persist the recycled eventN node as identity. Missing topology requires explicit reassignment.
    let observation = if metadata.path.is_empty() {
        format!("session:{}:{}", uuid::Uuid::new_v4(), physical)
    } else {
        format!("{}:{}", metadata.path, physical)
    };

    Ok(Definition {
        identity: Identity {
            backend: "linux-evdev".into(),
            vendor_id: id.0,
            product_id: id.1,
            serial: if unique.is_empty() {
                metadata.serial
            } else {
                Some(unique.into())
            },
            collection: format!("interface:{}", metadata.interface),
            fingerprint: String::new(),
            path: observation,
        },
        name: name.into(),
        controls,
        diagnostics: vec!["evdev controls only; vendor hidraw features are not captured.".into()],
    })
}
struct LinuxInput<D: Kernel> {
    device: D,
    definition: Definition,
    frames: Frames,
    axes: Vec<Axis>,
    keys: Vec<u16>,
}
trait Kernel {
    fn read(&mut self) -> Result<Vec<(u16, u16, i32)>, String>;
    fn snapshot(&self, keys: &[u16], axes: &[Axis], controls: &[Control])
        -> Result<Frames, String>;
}
impl Kernel for RawDevice {
    fn read(&mut self) -> Result<Vec<(u16, u16, i32)>, String> {
        match self.fetch_events() {
            Ok(events) => Ok(events
                .take(MAX_EVENTS + 1)
                .map(|e| (e.event_type().0, e.code(), e.value()))
                .collect()),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(vec![]),
            Err(e) => Err(format!("Linux controller disconnected or read failed: {e}")),
        }
    }
    fn snapshot(
        &self,
        keys: &[u16],
        axes: &[Axis],
        controls: &[Control],
    ) -> Result<Frames, String> {
        let key_state = if self.supported_events().contains(EventType::KEY) {
            Some(self.get_key_state().map_err(|e| e.to_string())?)
        } else {
            None
        };
        let abs_state = self.get_abs_state().map_err(|e| e.to_string())?;
        snapshot_values(
            keys,
            axes,
            &|key| key_state.as_ref().is_some_and(|s| s.contains(KeyCode(key))),
            &|axis| abs_state[axis as usize].value,
            controls,
        )
    }
}
fn snapshot_values(
    keys: &[u16],
    axes: &[Axis],
    key_down: &dyn Fn(u16) -> bool,
    axis_value: &dyn Fn(u16) -> i32,
    controls: &[Control],
) -> Result<Frames, String> {
    let mut frames = Frames::default();
    for key in keys {
        frames.value(1, *key, i32::from(key_down(*key)), controls)?;
    }
    for axis in axes {
        frames.value(3, axis.code, axis_value(axis.code), controls)?;
    }
    Ok(frames)
}
impl<D: Kernel> Input for LinuxInput<D> {
    fn definition(&self) -> Definition {
        self.definition.clone()
    }
    fn baseline(&mut self) -> Result<Vec<Event>, String> {
        if self.frames.dropping && !self.frames.recovery_boundary {
            let events = self.device.read()?;
            self.frames.feed(&events, &self.definition.controls)?;
            if !self.frames.recovery_boundary {
                return Ok(vec![Event::PendingBaseline]);
            }
        }
        // Discard pending pre-snapshot events so an old button press cannot overwrite the snapshot.
        let mut quiet = false;
        for _ in 0..16 {
            if self.device.read()?.is_empty() {
                quiet = true;
                break;
            }
        }
        if !quiet {
            return Ok(vec![Event::PendingBaseline]);
        }
        let frames = self
            .device
            .snapshot(&self.keys, &self.axes, &self.definition.controls)?;
        self.frames = frames;
        Ok(vec![Event::Values(0, self.frames.values.clone())])
    }
    fn poll(&mut self) -> Result<Vec<Event>, String> {
        let events = self.device.read()?;
        if events.len() > MAX_EVENTS {
            self.frames.dropping = true;
            self.frames.recovery_boundary = false;
            return Ok(vec![Event::Gap("Linux input batch overflow.".into())]);
        }
        self.frames.feed(&events, &self.definition.controls)
    }
}
#[cfg(test)]
#[path = "linux_tests.rs"]
mod tests;
