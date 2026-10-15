//! IOHID controller callbacks. All access is shared; no seizure or helper is required.
use crate::{
    capture::{self, Event, Input, Platform, MAX_EVENTS},
    registry::{Control, Definition, Identity, Kind, MAX_CONTROLS},
};
use iohidmanager::hid::{
    DeviceMatch, HidDevice, HidDeviceInfo, HidElement, HidElementType, HidManager,
    HidManagerOptions, HidUsage,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TrySendError},
        Arc,
    },
};
#[derive(Clone)]
struct Element {
    cookie: u32,
    page: u32,
    usage: u32,
    report: u32,
    parent: u32,
    bits: u32,
    min: i64,
    max: i64,
    relative: bool,
    array: bool,
    has_null: bool,
}
fn controls(elements: &[Element]) -> Result<Vec<Control>, String> {
    if elements.len() > MAX_CONTROLS {
        return Err("IOHID descriptor exceeds the controller control limit.".into());
    }
    let mut result = Vec::new();
    for (index, e) in elements.iter().enumerate() {
        let page = u16::try_from(e.page).map_err(|_| "Invalid IOHID usage page.")?;
        let usage = u16::try_from(e.usage).map_err(|_| "Invalid IOHID usage.")?;
        let report = u8::try_from(e.report).map_err(|_| "Invalid IOHID report ID.")?;
        let mut c = Control::new(page, usage, report, 0, index as u16);
        c.id = format!(
            "iohid:{page:04x}:{usage:04x}:{report}:{}:{}",
            e.parent, e.cookie
        );
        c.bits = u16::try_from(e.bits).unwrap_or(0);
        c.min = e.min;
        c.max = e.max;
        c.has_null = e.has_null;
        if e.relative
            || e.array
            || c.bits == 0
            || c.bits > 32
            || e.min < i32::MIN as i64
            || e.max > u32::MAX as i64
            || e.min >= e.max
        {
            c.kind = Kind::Diagnostic;
        }
        result.push(c);
    }
    Ok(result)
}
fn allowed_transport(transport: &str, serial: &str) -> bool {
    // Fail closed for software/unknown transports, including Cockpit's helper marker.
    ["USB", "Bluetooth", "Bluetooth Low Energy"].contains(&transport)
        && !capture::owned_virtual(serial)
}
type NativeValue = (u32, i64, u64);
struct Queue {
    receive: Receiver<NativeValue>,
    timestamps: BTreeMap<u32, u64>,
    lost: Arc<AtomicBool>,
}
impl Queue {
    fn drain(&mut self, elements: &[Element]) -> Vec<Event> {
        if self.lost.swap(false, Ordering::AcqRel) {
            for _ in 0..MAX_EVENTS {
                if self.receive.try_recv().is_err() {
                    break;
                }
            }
            return vec![Event::Gap(
                "IOHID callback queue overflow; a new device snapshot is required.".into(),
            )];
        }
        let mut events = Vec::new();
        for _ in 0..MAX_EVENTS {
            let Ok((cookie, value, timestamp)) = self.receive.try_recv() else {
                break;
            };
            if timestamp == 0
                || self
                    .timestamps
                    .get(&cookie)
                    .is_some_and(|last| timestamp <= *last)
            {
                continue;
            }
            if let Some((index, element)) = elements
                .iter()
                .enumerate()
                .find(|(_, e)| e.cookie == cookie)
            {
                if value < i32::MIN as i64 || value > u32::MAX as i64 {
                    return vec![Event::Gap(
                        "IOHID value exceeds the supported 32-bit range.".into(),
                    )];
                }
                self.timestamps.insert(cookie, timestamp);
                events.push(Event::Values(
                    element.report as u8,
                    BTreeMap::from([(index as u16, value as u32)]),
                ));
            }
        }
        events
    }
}
trait Device: Snapshot {
    fn info(&self) -> HidDeviceInfo;
    fn elements(&self) -> Vec<Element>;
    fn subscribe(
        &mut self,
        send: mpsc::SyncSender<NativeValue>,
        lost: Arc<AtomicBool>,
    ) -> Result<Box<dyn std::any::Any>, String>;
}
trait Provider {
    fn scan(&mut self) -> Result<Vec<(String, Box<dyn Device>)>, String>;
}
struct NativeProvider;
impl Provider for NativeProvider {
    fn scan(&mut self) -> Result<Vec<(String, Box<dyn Device>)>, String> {
        // A fresh manager refreshes topology independently of the UI run loop.
        let manager = HidManager::with_options(
            HidManagerOptions::DO_NOT_LOAD_PROPERTIES | HidManagerOptions::DO_NOT_SAVE_PROPERTIES,
        )
        .map_err(|e| capture::permission_error("macos", e))?;
        manager
            .set_device_matching_multiple(&[
                DeviceMatch::usage(HidUsage::Joystick),
                DeviceMatch::usage(HidUsage::GamePad),
                DeviceMatch::usage(HidUsage::Custom(1, 8)),
            ])
            .map_err(|e| e.to_string())?;
        let mut devices: Vec<(String, Box<dyn Device>)> = Vec::new();
        for device in manager.live_devices() {
            if devices.len() >= 1024 {
                return Err("Too many IOHID interfaces.".into());
            }
            let id = device.id().map_err(|e| e.to_string())?;
            // Own a separate IOHID object so dropping the enumeration manager cannot close active input.
            let device = HidDevice::from_id(id).map_err(|e| e.to_string())?;
            let native = device
                .elements()
                .into_iter()
                .filter(|e| {
                    matches!(
                        e.element_kind(),
                        HidElementType::InputButton
                            | HidElementType::InputAxis
                            | HidElementType::InputMisc
                    )
                })
                .collect();
            devices.push((
                id.0.to_string(),
                Box::new(NativeDevice {
                    device,
                    native,
                    opened: false,
                }),
            ));
        }
        Ok(devices)
    }
}
pub struct Mac {
    provider: Box<dyn Provider>,
    devices: BTreeMap<String, Box<dyn Device>>,
}
impl Mac {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            provider: Box::new(NativeProvider),
            devices: BTreeMap::new(),
        })
    }
}
impl Platform for Mac {
    fn paths(&mut self) -> Result<Vec<String>, String> {
        let devices = self.provider.scan()?;
        if devices.len() > 1024 {
            return Err("Too many IOHID interfaces.".into());
        }
        self.devices.clear();
        for (id, device) in devices {
            let info = device.info();
            if allowed_transport(
                info.transport.as_deref().unwrap_or(""),
                info.serial_number.as_deref().unwrap_or(""),
            ) {
                self.devices.insert(id, device);
            }
        }
        Ok(self.devices.keys().cloned().collect())
    }
    fn open(&mut self, path: &str) -> Result<Box<dyn Input>, String> {
        let mut device = self
            .devices
            .remove(path)
            .ok_or("IOHID controller disappeared.")?;
        let elements = device.elements();
        let definition = definition(device.info(), path, &elements)?;
        let (send, receive) = mpsc::sync_channel(MAX_EVENTS);
        let lost = Arc::new(AtomicBool::new(false));
        let subscription = device.subscribe(send, lost.clone())?;
        Ok(Box::new(MacInput {
            device,
            elements,
            definition,
            queue: Queue {
                receive,
                lost,
                timestamps: BTreeMap::new(),
            },
            _subscription: Some(subscription),
            suppressed: false,
        }))
    }
}
fn publish(send: &mpsc::SyncSender<NativeValue>, lost: &AtomicBool, value: NativeValue) {
    if let Err(TrySendError::Full(_)) = send.try_send(value) {
        lost.store(true, Ordering::Release);
    }
}
struct NativeDevice {
    device: HidDevice,
    native: Vec<HidElement>,
    opened: bool,
}
impl Drop for NativeDevice {
    fn drop(&mut self) {
        if self.opened {
            let _ = self.device.close_with_options(0);
        }
    }
}
impl Device for NativeDevice {
    fn info(&self) -> HidDeviceInfo {
        self.device.info()
    }
    fn elements(&self) -> Vec<Element> {
        self.native
            .iter()
            .map(|e| Element {
                cookie: e.cookie(),
                page: e.usage_page(),
                usage: e.usage(),
                report: e.report_id(),
                parent: e.parent().map_or(0, |p| p.cookie()),
                bits: e.report_size_bits(),
                min: e.logical_min(),
                max: e.logical_max(),
                relative: e.is_relative(),
                array: e.is_array() || e.report_count() != 1,
                has_null: e.has_null_state(),
            })
            .collect()
    }
    fn subscribe(
        &mut self,
        send: mpsc::SyncSender<NativeValue>,
        lost: Arc<AtomicBool>,
    ) -> Result<Box<dyn std::any::Any>, String> {
        if !self.opened {
            self.device
                .open_with_options(0)
                .map_err(|e| capture::permission_error("macos", e))?;
            self.opened = true;
        }
        let subscription = self
            .device
            .on_input_value(&[], move |v| {
                publish(
                    &send,
                    &lost,
                    (v.element().cookie(), v.integer_value(), v.timestamp()),
                )
            })
            .map_err(|e| capture::permission_error("macos", e))?;
        Ok(Box::new(subscription))
    }
}
// The permission gate is evaluated before closing the current capture handle.
fn reopen_owned(
    opened: &mut bool,
    enabled: bool,
    qualified: bool,
    mut close: impl FnMut() -> Result<(), String>,
    mut open: impl FnMut(u32) -> Result<(), String>,
) -> Result<(), String> {
    if enabled && !qualified {
        return Err("IOHID seizure is not qualified; no device was reopened.".into());
    }
    if *opened {
        close()?;
        *opened = false;
    }
    open(if enabled { 1 } else { 0 })?;
    *opened = true;
    Ok(())
}
impl Snapshot for NativeDevice {
    fn reopen(
        &mut self,
        enabled: bool,
        send: mpsc::SyncSender<NativeValue>,
        lost: Arc<AtomicBool>,
    ) -> Result<Box<dyn std::any::Any>, String> {
        reopen_owned(
            &mut self.opened,
            enabled,
            false,
            || self.device.close_with_options(0).map_err(|e| e.to_string()),
            |options| {
                self.device
                    .open_with_options(options)
                    .map_err(|e| capture::permission_error("macos", e))
            },
        )?;
        self.subscribe(send, lost)
    }
    fn values(&self) -> Result<Vec<NativeValue>, String> {
        let descriptors = controls(&self.elements())?;
        self.native
            .iter()
            .zip(descriptors)
            .filter(|(_, control)| control.kind != Kind::Diagnostic)
            .map(|(element, _)| {
                self.device
                    .get_value(element)
                    .map(|v| (element.cookie(), v.integer_value(), v.timestamp()))
                    .map_err(|e| capture::permission_error("macos", e))
            })
            .collect()
    }
}
impl Snapshot for Box<dyn Device> {
    fn reopen(
        &mut self,
        enabled: bool,
        send: mpsc::SyncSender<NativeValue>,
        lost: Arc<AtomicBool>,
    ) -> Result<Box<dyn std::any::Any>, String> {
        (**self).reopen(enabled, send, lost)
    }
    fn values(&self) -> Result<Vec<NativeValue>, String> {
        (**self).values()
    }
}
fn definition(info: HidDeviceInfo, path: &str, elements: &[Element]) -> Result<Definition, String> {
    let controls = controls(elements)?;
    if controls.is_empty() {
        return Err("No supported IOHID input elements.".into());
    }
    let serial = info.serial_number.filter(|s| !s.is_empty());
    let collection = format!(
        "usage:{}:{}",
        info.usage_page.unwrap_or(0),
        info.usage.unwrap_or(0)
    );
    // A missing serial/location never turns a recycled registry ID into permanent identity.
    let observation = match info.location_id.filter(|id| *id != 0) {
        Some(location) => format!("location:{location}:{collection}:registry:{path}"),
        None => format!("session:{}:registry:{path}", uuid::Uuid::new_v4()),
    };
    Ok(Definition {
        identity: Identity {
            backend: "macos-iohid".into(),
            vendor_id: u16::try_from(info.vendor_id.unwrap_or(0))
                .map_err(|_| "Invalid IOHID vendor ID.")?,
            product_id: u16::try_from(info.product_id.unwrap_or(0))
                .map_err(|_| "Invalid IOHID product ID.")?,
            serial,
            collection,
            fingerprint: String::new(),
            path: observation,
        },
        name: info.product.unwrap_or_else(|| "macOS controller".into()),
        controls,
        diagnostics: vec![
            "IOHID value callbacks; native report-group qualification remains open.".into(),
        ],
    })
}
trait Snapshot {
    fn values(&self) -> Result<Vec<(u32, i64, u64)>, String>;
    fn reopen(
        &mut self,
        _enabled: bool,
        _send: mpsc::SyncSender<NativeValue>,
        _lost: Arc<AtomicBool>,
    ) -> Result<Box<dyn std::any::Any>, String> {
        Err("IOHID seizure is unsupported by this capture adapter.".into())
    }
}
struct MacInput<D: Snapshot> {
    device: D,
    elements: Vec<Element>,
    definition: Definition,
    queue: Queue,
    _subscription: Option<Box<dyn std::any::Any>>,
    suppressed: bool,
}
impl<D: Snapshot> Drop for MacInput<D> {
    fn drop(&mut self) {
        self._subscription.take();
    }
}
impl<D: Snapshot> Input for MacInput<D> {
    fn suppress(&mut self, enabled: bool) -> Result<(), String> {
        if !enabled && !self.suppressed {
            return Ok(());
        }
        self.suppressed = true;
        self._subscription.take();
        let (send, receive) = mpsc::sync_channel(MAX_EVENTS);
        let lost = Arc::new(AtomicBool::new(false));
        self._subscription = Some(self.device.reopen(enabled, send, lost.clone())?);
        self.queue = Queue {
            receive,
            lost,
            timestamps: BTreeMap::new(),
        };
        self.suppressed = enabled;
        Ok(())
    }
    fn definition(&self) -> Definition {
        self.definition.clone()
    }
    fn baseline(&mut self) -> Result<Vec<Event>, String> {
        self.queue.lost.store(false, Ordering::Release);
        for _ in 0..MAX_EVENTS {
            if self.queue.receive.try_recv().is_err() {
                break;
            }
        }
        let values = self.device.values()?;
        let mut reports: BTreeMap<u8, BTreeMap<u16, u32>> = BTreeMap::new();
        self.queue.timestamps.clear();
        for (index, e) in self.elements.iter().enumerate() {
            if self.definition.controls[index].kind == Kind::Diagnostic {
                continue;
            }
            let Some((_, value, timestamp)) = values.iter().find(|v| v.0 == e.cookie) else {
                return Ok(vec![Event::PendingBaseline]);
            };
            if *timestamp == 0 {
                return Ok(vec![Event::PendingBaseline]);
            }
            if *value < i32::MIN as i64 || *value > u32::MAX as i64 {
                return Err("IOHID baseline exceeds supported range.".into());
            }
            self.queue.timestamps.insert(e.cookie, *timestamp);
            reports
                .entry(e.report as u8)
                .or_default()
                .insert(index as u16, *value as u32);
        }
        if self.queue.lost.load(Ordering::Acquire) {
            return Ok(vec![Event::Gap(
                "IOHID overflow while reading the baseline.".into(),
            )]);
        }
        Ok(reports
            .into_iter()
            .map(|(id, values)| Event::Values(id, values))
            .collect())
    }
    fn poll(&mut self) -> Result<Vec<Event>, String> {
        Ok(self.queue.drain(&self.elements))
    }
}
#[cfg(test)]
#[path = "macos_tests.rs"]
mod tests;
