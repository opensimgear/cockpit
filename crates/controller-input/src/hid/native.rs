use super::{
    session::{hex, Get, Reader},
    Backend,
};
use crate::{
    windows_caps::ReportKind,
    windows_inventory::{self, Interface},
    windows_receive::{self, Receive, Receiver, Selection},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Mutex, time::Duration};

pub(super) struct Windows {
    paths: Mutex<HashMap<String, Issued>>,
    enumerate: fn() -> Result<windows_inventory::Inventory, windows_inventory::Failure>,
    inspect: fn(&str) -> Result<Interface, windows_inventory::Failure>,
}
#[derive(Clone)]
struct Issued {
    path: String,
    known: Evidence,
    contract: Option<String>,
}
#[derive(Clone)]
struct Evidence {
    attributes: Option<windows_inventory::Attributes>,
    serial: Option<String>,
    caps: Option<String>,
}
impl Evidence {
    fn from(row: &Interface) -> Self {
        Self {
            attributes: row.attributes.clone(),
            serial: row.serial.clone(),
            // Diagnostic parser failures cannot fence a complete inspection.
            caps: row
                .caps
                .as_ref()
                .filter(|caps| caps.available && caps.complete)
                .map(|caps| id(&json!(caps).to_string())),
        }
    }
    fn compatible(&self, current: &Self) -> bool {
        fn agrees<T: PartialEq>(old: &Option<T>, new: &Option<T>) -> bool {
            old.as_ref()
                .zip(new.as_ref())
                .is_none_or(|(old, new)| old == new)
        }
        agrees(&self.attributes, &current.attributes)
            && agrees(&self.serial, &current.serial)
            && agrees(&self.caps, &current.caps)
    }
    fn matches(&self, current: &Self) -> bool {
        self.compatible(current)
            && (self.attributes.is_none() || current.attributes.is_some())
            && (self.serial.is_none() || current.serial.is_some())
            && (self.caps.is_none() || current.caps.is_some())
    }
    fn enrich(&mut self, current: Self) {
        self.attributes = current.attributes.or(self.attributes.take());
        self.serial = current.serial.or(self.serial.take());
        self.caps = current.caps.or(self.caps.take());
    }
}
impl Default for Windows {
    fn default() -> Self {
        Self {
            paths: Mutex::default(),
            enumerate: windows_inventory::enumerate,
            inspect: windows_inventory::inspect,
        }
    }
}

fn id(path: &str) -> String {
    format!("{:x}", Sha256::digest(path.as_bytes()))
}
fn handle(row: &Interface) -> Option<String> {
    let path = row.path.as_deref()?;
    let evidence = json!([path, row.attributes, row.serial, row.caps]);
    Some(format!(
        "{:x}",
        Sha256::digest(evidence.to_string().as_bytes())
    ))
}
fn summary(row: &Interface) -> Value {
    let caps = row.caps.as_ref();
    let usage = caps.and_then(|c| c.usage_page.zip(c.usage));
    json!({"index":row.index,"interfaceId":handle(row),"pathFingerprint":row.path.as_deref().map(id),
        "attributes":row.attributes,"product":row.product,"usagePage":usage.map(|u|u.0),
        "usage":usage.map(|u|u.1),"metadataOpened":row.metadata_opened,
        "access":match usage {None=>"unknown", Some((p,u)) if windows_receive::metadata_only(p,u)=>"metadata-only",_=>"unprobed"},
        "complete":caps.is_some_and(|c|c.complete),"failures":row.failures,
        "capabilityFailures":caps.map(|c|&c.failures)})
}

fn selection(row: &Interface) -> Result<Selection, String> {
    let attrs = row
        .attributes
        .as_ref()
        .ok_or("HID identity metadata unavailable")?;
    let caps = row
        .caps
        .as_ref()
        .filter(|c| c.available)
        .ok_or("HID preparsed capabilities unavailable")?;
    let len = |kind| caps.report(kind).map_or(0, |r| usize::from(r.byte_length));
    Ok(Selection {
        path: row.path.clone().ok_or("No available HID path")?,
        vendor_id: attrs.vendor_id,
        product_id: attrs.product_id,
        version_number: attrs.version_number,
        usage_page: caps.usage_page.ok_or("Missing HID usage page")?,
        usage: caps.usage.ok_or("Missing HID usage")?,
        input_report_bytes: len(ReportKind::Input),
        feature_report_bytes: len(ReportKind::Feature),
    })
}

impl Windows {
    fn selected(&self, target: &str) -> Result<Interface, String> {
        let issued = self
            .paths
            .lock()
            .unwrap()
            .get(target)
            .cloned()
            .ok_or("Unknown HID interface; refresh inventory")?;
        let inspected = (self.inspect)(&issued.path).map_err(|e| e.reason);
        let mut paths = self.paths.lock().unwrap();
        let current = paths
            .get_mut(target)
            .ok_or("Stale HID interface; refresh inventory")?;
        let row = match inspected {
            Ok(row) => row,
            Err(error) => {
                paths.remove(target);
                return Err(error);
            }
        };
        let evidence = Evidence::from(&row);
        let contract = handle(&row);
        if row.path.as_deref() != Some(&issued.path)
            || !current.known.matches(&evidence)
            || current
                .contract
                .as_ref()
                .is_some_and(|expected| Some(expected) != contract.as_ref())
        {
            paths.remove(target);
            return Err("HID capabilities or identity changed; refresh inventory".into());
        }
        current.known.enrich(evidence);
        current.contract = contract;
        Ok(row)
    }
    fn catalogue(&self, inventory: windows_inventory::Inventory) -> Value {
        let mut issued = self.paths.lock().unwrap();
        let mut previous = std::mem::take(&mut *issued);
        let mut paths = HashMap::new();
        let rows: Vec<_> = inventory
            .interfaces
            .iter()
            .take(windows_inventory::MAX_INTERFACES as usize)
            .map(|row| {
                let mut summary = summary(row);
                if let Some(path) = &row.path {
                    // An incomplete SetupAPI result can repeat an exact path.
                    // It must not grant two independently selectable tokens.
                    if paths.values().any(|entry: &Issued| entry.path == *path) {
                        summary["interfaceId"] = Value::Null;
                        return summary;
                    }
                    let evidence = Evidence::from(row);
                    let reusable = previous.iter().find_map(|(token, old)| {
                        let authoritative =
                            evidence.attributes.is_some() && evidence.caps.is_some();
                        (old.path == *path
                            && old.known.compatible(&evidence)
                            && (!authoritative
                                || old
                                    .contract
                                    .as_ref()
                                    .is_none_or(|bound| Some(bound) == handle(row).as_ref())))
                        .then(|| token.clone())
                    });
                    let (token, mut entry) = reusable
                        .and_then(|token| previous.remove(&token).map(|entry| (token, entry)))
                        .unwrap_or_else(|| {
                            (
                                id(&uuid::Uuid::new_v4().to_string()),
                                Issued {
                                    path: path.clone(),
                                    known: evidence.clone(),
                                    contract: None,
                                },
                            )
                        });
                    entry.known.enrich(evidence);
                    summary["interfaceId"] = json!(token);
                    paths.insert(token, entry);
                }
                summary
            })
            .collect();
        *issued = paths;
        json!({"interfaces":rows,"truncated":inventory.truncated || inventory.interfaces.len() > windows_inventory::MAX_INTERFACES as usize,"failures":inventory.failures})
    }
}
impl Backend for Windows {
    fn inventory(&self, target: Option<&str>) -> Result<Value, String> {
        if let Some(target) = target {
            let row = self.selected(target)?;
            let mut detail = summary(&row);
            // Preserve the token initially issued to a budget-deferred row.
            detail["interfaceId"] = json!(target);
            // A single-interface inspection has no SetupAPI enumeration ordinal.
            detail.as_object_mut().unwrap().remove("index");
            detail["capabilities"] = json!(row.caps);
            detail["manufacturer"] = json!(row.manufacturer);
            detail["serial"] = json!(row.serial);
            return Ok(detail);
        }
        Ok(self.catalogue((self.enumerate)().map_err(|e| e.reason)?))
    }
    fn open(&self, target: &str) -> Result<Box<dyn Reader>, String> {
        let row = self.selected(target)?;
        // Check the actual opened handle, including field layout changes that
        // preserve VID/PID, top-level usage, and report byte lengths.
        let selected = selection(&row)?;
        let input = selected.input_report_bytes != 0;
        let receiver =
            windows_receive::open(&selected).map_err(|e| format!("{:?}: {}", e.kind, e.message))?;
        if let Err(error) = verify_held(&row, receiver.capabilities()) {
            self.paths.lock().unwrap().remove(target);
            return Err(error);
        }
        Ok(Box::new(Device { receiver, input }))
    }
}

fn verify_held(row: &Interface, caps: &crate::windows_caps::Snapshot) -> Result<(), String> {
    if Some(caps) != row.caps.as_ref() {
        return Err("Opened HID capabilities changed; refresh inventory".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
struct Device {
    receiver: Receiver,
    input: bool,
}
fn delivery(received: Receive) -> Result<Option<Vec<u8>>, Value> {
    match received {
        Receive::Report(bytes) => Ok(Some(bytes)),
        Receive::Timeout => Ok(None),
        Receive::Removed => Err(json!({"kind":"removed"})),
        Receive::Error(e) => Err(json!(e)),
    }
}
impl Reader for Device {
    fn poll(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, Value> {
        if !self.input {
            std::thread::sleep(timeout);
            return Ok(None);
        }
        delivery(self.receiver.poll(timeout))
    }
    fn project(&self, bytes: &[u8]) -> Value {
        json!(self.receiver.samples(ReportKind::Input, bytes))
    }
    fn get(&mut self, request: &Get, remaining: Duration) -> Result<Value, Value> {
        let kind = if request.feature {
            ReportKind::Feature
        } else {
            ReportKind::Input
        };
        let packet = self
            .receiver
            .get(
                &request.id,
                kind,
                request.report_id,
                request.max_bytes,
                remaining,
            )
            .map_err(|e| json!(e))?;
        Ok(
            json!({"kind":kind,"wireHex":hex(&packet.bytes),"samples":self.receiver.samples(kind,&packet.bytes)}),
        )
    }
}
