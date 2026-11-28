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
    paths: Mutex<HashMap<String, String>>,
    enumerate: fn() -> Result<windows_inventory::Inventory, windows_inventory::Failure>,
    inspect: fn(&str) -> Result<Interface, windows_inventory::Failure>,
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
fn summary(row: &Interface) -> Value {
    let caps = row.caps.as_ref();
    let usage = caps.and_then(|c| c.usage_page.zip(c.usage));
    json!({"index":row.index,"interfaceId":row.path.as_deref().map(id),
        "attributes":row.attributes,"product":row.product,"usagePage":usage.map(|u|u.0),
        "usage":usage.map(|u|u.1),"metadataOpened":row.metadata_opened,
        "access":match usage {None=>"unknown", Some((p,u)) if windows_receive::metadata_only(p,u)=>"metadata-only",_=>"unprobed"},
        "complete":caps.is_some_and(|c|c.complete),"failures":row.failures,
        "capabilityFailures":caps.map(|c|&c.failures)})
}

fn selection(row: Interface) -> Result<Selection, String> {
    let attrs = row.attributes.ok_or("HID identity metadata unavailable")?;
    let caps = row
        .caps
        .filter(|c| c.available)
        .ok_or("HID preparsed capabilities unavailable")?;
    let len = |kind| caps.report(kind).map_or(0, |r| usize::from(r.byte_length));
    Ok(Selection {
        path: row.path.ok_or("No available HID path")?,
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
        let path = self
            .paths
            .lock()
            .unwrap()
            .get(target)
            .cloned()
            .ok_or("Unknown HID interface; refresh inventory")?;
        (self.inspect)(&path).map_err(|e| e.reason)
    }
    fn catalogue(&self, inventory: windows_inventory::Inventory) -> Value {
        let mut paths = HashMap::new();
        let rows: Vec<_> = inventory
            .interfaces
            .iter()
            .map(|row| {
                if let Some(path) = &row.path {
                    paths.insert(id(path), path.clone());
                }
                summary(row)
            })
            .collect();
        *self.paths.lock().unwrap() = paths;
        json!({"interfaces":rows,"truncated":inventory.truncated,"failures":inventory.failures})
    }
}
impl Backend for Windows {
    fn inventory(&self, target: Option<&str>) -> Result<Value, String> {
        if let Some(target) = target {
            let row = self.selected(target)?;
            let mut detail = summary(&row);
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
        let selected = selection(self.selected(target)?)?;
        let input = selected.input_report_bytes != 0;
        let receiver =
            windows_receive::open(&selected).map_err(|e| format!("{:?}: {}", e.kind, e.message))?;
        Ok(Box::new(Device { receiver, input }))
    }
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
