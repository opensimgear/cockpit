//! Generic, metadata-only IOHID inventory; independent of controller admission.
//! Device properties and the registry entry identify one native service lifetime.
mod native;
mod policy;
#[cfg(test)]
mod tests;

use iohidmanager::hid::{HidDevice, HidDeviceId};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

// Only IDs actually issued by inventory may reach native receiver selection.
fn issued() -> &'static Mutex<HashMap<String, u64>> {
    static ISSUED: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    ISSUED.get_or_init(Mutex::default)
}
fn hash(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}
use policy::native_restricted;
pub(super) fn listen_access_status() -> &'static str {
    native::listen_access_status()
}
pub(super) fn descriptor(device: &HidDevice) -> Result<Option<Vec<u8>>, String> {
    native::descriptor(device)
}
fn access(restricted: bool) -> Value {
    json!({"sharedOpen":"untested","policy":if restricted {"metadata-only"} else {"explicit-selection-required"},
        "processListenEvent":{"status":listen_access_status(),"scope":"current-process", "provenance":"IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)","perDeviceAccess":false}})
}
fn row(device: &HidDevice) -> Result<Value, String> {
    let id = device.id().map_err(|e| e.to_string())?.0;
    let mut row = json!({
        "identity":{"registryEntryId":id.to_string()},"backend":"macos-iohid",
        "descriptorProvenance":"IOHIDDeviceGetProperty(ReportDescriptor)",
        "elementsProvenance":"IOHIDDeviceCopyMatchingElements", "failures":[]
    });
    let mut failures = Vec::new();
    for (key, property) in [
        ("vendorId", "VendorID"),
        ("productId", "ProductID"),
        ("versionNumber", "VersionNumber"),
        ("locationId", "LocationID"),
        ("usagePage", "PrimaryUsagePage"),
        ("usage", "PrimaryUsage"),
        ("maxInputReportBytes", "MaxInputReportSize"),
        ("maxFeatureReportBytes", "MaxFeatureReportSize"),
    ] {
        match native::number(device, property) {
            Ok(value) => row[key] = json!(value),
            Err(error) => failures.push(error),
        }
    }
    for (key, property) in [
        ("product", "Product"),
        ("manufacturer", "Manufacturer"),
        ("serial", "SerialNumber"),
        ("transport", "Transport"),
    ] {
        match native::string(device, property) {
            Ok(value) => row[key] = json!(value),
            Err(error) => failures.push(error),
        }
    }
    let mut reasons = Vec::new();
    match native::descriptor(device) {
        Ok(Some(raw)) => {
            row["descriptorHex"] =
                json!(raw.iter().map(|b| format!("{b:02x}")).collect::<String>());
            reasons.extend(crate::hid::restrictions::reasons(&raw));
            match osg_hid_report::parse_descriptor(&raw) {
                Ok(parsed) => row["descriptor"] = json!(parsed),
                Err(error) => {
                    row["descriptorError"] = json!({"offset":error.offset,"reason":error.reason})
                }
            }
        }
        Ok(None) => {}
        Err(error) => {
            reasons.push("Report descriptor cannot establish capture policy; metadata only".into());
            failures.push(error);
        }
    }
    match native::elements(device) {
        Ok(elements) => {
            if row["descriptorHex"].is_null() && !policy::element_fallback(&elements) {
                reasons.push("No descriptor and native elements cannot establish a complete controller tree; metadata only".into());
            }
            if policy::elements_restricted(&elements) {
                reasons.push("Native element tree includes keyboard, pointer or system/consumer usages; metadata only".into());
            }
            row["elements"] = json!(elements);
        }
        Err(error) => {
            reasons.push("Native element tree unavailable; metadata only".into());
            failures.push(error);
        }
    }
    if native_restricted(
        row["usagePage"].as_u64().unwrap_or(0),
        row["usage"].as_u64().unwrap_or(0),
    ) {
        reasons.push(
            "Native primary usage is keyboard, pointer or system/consumer; metadata only".into(),
        );
    }
    if !failures.is_empty() {
        reasons.push("Native metadata incomplete; metadata only".into());
    }
    reasons.sort();
    reasons.dedup();
    row["restricted"] = json!(!reasons.is_empty());
    row["restrictionReasons"] = json!(reasons);
    row["restrictionReason"] = json!(reasons.first());
    row["failures"] = json!(failures);
    let evidence = hash(&row);
    row["generationEvidence"] = json!(evidence);
    row["interfaceId"] = json!(hash(&json!([id.to_string(), evidence])));
    // Process authorization is diagnostic, not native device identity evidence.
    row["access"] = access(row["restricted"] == true);
    Ok(row)
}
fn scan() -> Result<Vec<Value>, String> {
    let mut result = Vec::new();
    let mut remaining = 512 * 1024usize;
    for id in native::ids()? {
        match HidDevice::from_id(HidDeviceId(id))
            .map_err(|e| e.to_string())
            .and_then(|device| row(&device))
        {
            Ok(mut value) => {
                value["descriptorAvailable"] = json!(value["descriptorHex"].is_string());
                value["elementCount"] = json!(value["elements"].as_array().map_or(0, Vec::len));
                for key in ["descriptorHex", "descriptor", "elements"] {
                    value.as_object_mut().unwrap().remove(key);
                }
                let bytes = value.to_string().len();
                if bytes > remaining {
                    result.push(json!({"restricted":true,"failures":["Inventory summary byte budget reached; result incomplete"]}));
                    break;
                }
                remaining -= bytes;
                result.push(value);
            }
            Err(error) => result.push(json!({"identity":{"registryEntryId":id.to_string()},
                "restricted":true,"failures":[error]})),
        }
    }
    Ok(result)
}
pub(super) fn inventory(target: Option<&str>) -> Result<Value, String> {
    if let Some(id) = target {
        let (_, row) = resolve(id, false)?;
        return Ok(row);
    }
    let rows = scan()?;
    let mut ids = issued()
        .lock()
        .map_err(|_| "IOHID inventory lock unavailable")?;
    *ids = rows
        .iter()
        .filter_map(|r| {
            Some((
                r["interfaceId"].as_str()?.to_owned(),
                r["identity"]["registryEntryId"].as_str()?.parse().ok()?,
            ))
        })
        .collect();
    let incomplete = rows.iter().any(|r| {
        r["interfaceId"].is_null() || r["failures"].as_array().is_some_and(|f| !f.is_empty())
    });
    Ok(json!({"interfaces":rows,"truncated":incomplete,"failures":[],"access":access(false)}))
}
fn resolve(id: &str, capture: bool) -> Result<(HidDevice, Value), String> {
    let entry = *issued()
        .lock()
        .map_err(|_| "IOHID inventory lock unavailable")?
        .get(id)
        .ok_or("Unknown HID interface generation; refresh inventory")?;
    if !native::ids()?.contains(&entry) {
        return Err("HID interface generation disconnected; refresh inventory".into());
    }
    let device = HidDevice::from_id(HidDeviceId(entry)).map_err(|e| e.to_string())?;
    // Re-read from the handle returned to the receiver, fencing scan->selection.
    let fresh = row(&device)?;
    if fresh["interfaceId"].as_str() != Some(id) {
        return Err("IOHID generation changed during selection".into());
    }
    if capture && fresh["restricted"].as_bool() != Some(false) {
        return Err(format!(
            "Restricted HID interface: {}",
            fresh["restrictionReasons"]
        ));
    }
    Ok((device, fresh))
}
pub(super) fn selected(id: &str) -> Result<(HidDevice, Value), String> {
    resolve(id, true)
}
