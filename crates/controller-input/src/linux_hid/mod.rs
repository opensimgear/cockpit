//! Linux hidraw and evdev refer to different kernel interfaces. Only hidraw
//! descriptors and deliveries enter the portable decoder here.
mod inventory;
mod io;
mod receiver;
#[cfg(test)]
mod tests;

use super::{
    session::{hex, Get, Reader},
    Backend,
};
use inventory::{Interface, Inventory};
use osg_hid_report::{parse_descriptor, Descriptor};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Mutex};

pub(super) struct Linux {
    scan: fn() -> Result<Inventory, inventory::Failure>,
    handles: Mutex<HashMap<String, Interface>>,
}
impl Default for Linux {
    fn default() -> Self {
        Self {
            scan: inventory::enumerate,
            handles: Mutex::default(),
        }
    }
}

fn parse(bytes: &[u8]) -> Result<Descriptor, String> {
    parse_descriptor(bytes).map_err(|d| format!("Descriptor at byte {}: {}", d.offset, d.reason))
}

fn key(row: &Interface) -> String {
    let evidence = json!([row.path, row.hid_parent, row.evidence]);
    format!("{:x}", Sha256::digest(evidence.to_string().as_bytes()))
}
impl Linux {
    fn selected(&self, id: &str) -> Result<Interface, String> {
        self.handles
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| "Unknown HID interface generation; refresh inventory".into())
    }
}
impl Backend for Linux {
    fn inventory(&self, target: Option<&str>) -> Result<Value, String> {
        let scan = (self.scan)().map_err(|e| format!("{}: {}", e.stage, e.message))?;
        if let Some(id) = target {
            self.selected(id)?;
            let row = scan
                .interfaces
                .iter()
                .find(|row| key(row) == id)
                .ok_or("HID interface generation changed; refresh inventory")?;
            let mut detail = json!(row);
            detail["interfaceId"] = json!(id);
            if let Some(raw) = &row.descriptor {
                detail["descriptorHex"] = json!(hex(raw));
                match parse(raw) {
                    Ok(d) => detail["descriptor"] = json!(d),
                    Err(e) => detail["descriptorError"] = json!(e),
                }
            }
            return Ok(detail);
        }
        let mut handles = HashMap::new();
        let mut rows = Vec::new();
        for row in scan.interfaces {
            let id = key(&row);
            let mut summary = json!(row);
            summary["interfaceId"] = json!(id);
            rows.push(summary);
            handles.insert(id, row);
        }
        *self.handles.lock().unwrap() = handles;
        Ok(json!({"interfaces":rows,"truncated":scan.truncated,"failures":scan.failures}))
    }
    fn open(&self, target: &str) -> Result<Box<dyn Reader>, String> {
        receiver::open(self.selected(target)?)
    }
}
