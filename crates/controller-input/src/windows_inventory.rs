//! Present HID interfaces, independent of mapping admission. Discovery is metadata-only.
//! The exact SetupAPI path identifies an interface; VID/PID, strings and usage never merge rows.
use crate::windows_caps;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    mem::size_of,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Devices::{DeviceAndDriverInstallation::*, HumanInterfaceDevice::*},
    Foundation::*,
    Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
};
mod native;
use native::{Api, OwnedHandle, OwnedPreparsed, OwnedSet};

pub const MAX_INTERFACES: u32 = 1024;
const MAX_DETAIL_BYTES: u32 = 8192;
const MAX_CAP_RECORDS: usize = 32_768;
const MAX_CAP_BYTES: usize = 8 * 1024 * 1024;
struct CapsBudget {
    records: usize,
    bytes: usize,
}
impl Default for CapsBudget {
    fn default() -> Self {
        Self {
            records: MAX_CAP_RECORDS,
            bytes: MAX_CAP_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub stage: String,
    pub native_code: Option<u32>,
    pub reason: String,
}
impl Failure {
    fn native(stage: &str, code: u32) -> Self {
        Self {
            stage: stage.into(),
            native_code: Some(code),
            reason: if code == 0 {
                "Native metadata call failed without a reported OS error".into()
            } else {
                std::io::Error::from_raw_os_error(code as i32).to_string()
            },
        }
    }
    fn invalid(stage: &str, reason: &str) -> Self {
        Self {
            stage: stage.into(),
            native_code: None,
            reason: reason.into(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attributes {
    pub vendor_id: u16,
    pub product_id: u16,
    pub version_number: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Interface {
    pub index: u32,
    /// Exact native interface instance, not a reusable VID/PID or device name.
    /// None means the enumeration failure supplied no trustworthy address.
    pub path: Option<String>,
    pub attributes: Option<Attributes>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub metadata_opened: bool,
    pub caps: Option<windows_caps::Snapshot>,
    pub failures: Vec<Failure>,
}
impl Interface {
    fn new(index: u32) -> Self {
        Self {
            index,
            path: None,
            attributes: None,
            manufacturer: None,
            product: None,
            serial: None,
            metadata_opened: false,
            caps: None,
            failures: vec![],
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub interfaces: Vec<Interface>,
    pub failures: Vec<Failure>,
    /// Bounds, interrupted enumeration and duplicate paths are explicit incomplete results.
    pub truncated: bool,
}

/// Enumerate all present HID interfaces, including keyboards, mice, vendor pages and
/// inaccessible collections. This never registers Raw Input or reads/sends any report.
pub fn enumerate() -> Result<Inventory, Failure> {
    unsafe { enumerate_with(&Api::native()) }
}

/// Inspect one exact path retained from a present-interface inventory. The service
/// must resolve its opaque inventory ID before calling; this is not a file-open API.
pub fn inspect(path: &str) -> Result<Interface, Failure> {
    if path.is_empty() || path.encode_utf16().count() > 4093 || path.contains('\0') {
        return Err(Failure::invalid(
            "interface-identity",
            "Invalid or over-budget retained HID interface path",
        ));
    }
    let mut row = Interface::new(0);
    row.path = Some(path.to_owned());
    unsafe {
        metadata(&Api::native(), &mut row, &mut CapsBudget::default());
    }
    Ok(row)
}

unsafe fn path(
    api: &Api,
    set: isize,
    interface: &SP_DEVICE_INTERFACE_DATA,
) -> Result<String, Failure> {
    let mut required = 0;
    let queried = (api.detail)(set, interface, null_mut(), 0, &mut required, null_mut());
    let code = (api.last_error)();
    if queried == 0 && code != ERROR_INSUFFICIENT_BUFFER {
        return Err(Failure::native("interface-detail-size", code));
    }
    let minimum =
        std::mem::offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath) + size_of::<u16>();
    if required < minimum as u32 || required > MAX_DETAIL_BYTES || required % 2 != 0 {
        return Err(Failure::invalid(
            "interface-detail-size",
            "HID interface detail exceeds bounded UTF-16 buffer or has invalid size",
        ));
    }
    let mut buffer = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
    let detail = buffer
        .as_mut_ptr()
        .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
    (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
    let capacity = required;
    if (api.detail)(set, interface, detail, capacity, &mut required, null_mut()) == 0 {
        return Err(Failure::native("interface-detail", (api.last_error)()));
    }
    if required > capacity || required < minimum as u32 || required % 2 != 0 {
        return Err(Failure::invalid(
            "interface-detail",
            "HID interface detail changed length outside supplied buffer",
        ));
    }
    let units = std::slice::from_raw_parts(
        std::ptr::addr_of!((*detail).DevicePath).cast::<u16>(),
        (required as usize - std::mem::offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath))
            / 2,
    );
    let end = units.iter().position(|unit| *unit == 0).ok_or_else(|| {
        Failure::invalid("interface-detail", "HID interface path is not terminated")
    })?;
    if end == 0 {
        return Err(Failure::invalid(
            "interface-detail",
            "HID interface path is empty",
        ));
    }
    String::from_utf16(&units[..end]).map_err(|_| {
        Failure::invalid(
            "interface-detail",
            "HID interface path contains invalid UTF-16",
        )
    })
}

unsafe fn metadata(api: &Api, row: &mut Interface, budget: &mut CapsBudget) {
    let wide: Vec<_> = row
        .path
        .as_ref()
        .unwrap()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let handle = (api.open)(
        wide.as_ptr(),
        0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        null(),
        OPEN_EXISTING,
        0,
        null_mut(),
    );
    if handle == INVALID_HANDLE_VALUE {
        row.failures
            .push(Failure::native("metadata-open", (api.last_error)()));
        return;
    }
    let mut handle = OwnedHandle::new(api, handle);
    row.metadata_opened = true;
    let mut attributes = HIDD_ATTRIBUTES {
        Size: size_of::<HIDD_ATTRIBUTES>() as u32,
        ..Default::default()
    };
    if (api.attributes)(handle.value, &mut attributes) {
        row.attributes = Some(Attributes {
            vendor_id: attributes.VendorID,
            product_id: attributes.ProductID,
            version_number: attributes.VersionNumber,
        });
    } else {
        row.failures
            .push(Failure::native("attributes", (api.last_error)()));
    }
    for (stage, read, target) in [
        ("manufacturer", api.manufacturer, &mut row.manufacturer),
        ("product", api.product, &mut row.product),
        ("serial", api.serial, &mut row.serial),
    ] {
        let mut text = [0u16; 128];
        if !read(
            handle.value,
            text.as_mut_ptr().cast(),
            (text.len() * 2) as u32,
        ) {
            row.failures
                .push(Failure::native(stage, (api.last_error)()));
        } else if let Some(end) = text.iter().position(|unit| *unit == 0) {
            match String::from_utf16(&text[..end]) {
                Ok(value) if !value.is_empty() => *target = Some(value),
                Ok(_) => {}
                Err(_) => row.failures.push(Failure::invalid(
                    stage,
                    "HID metadata string contains invalid UTF-16",
                )),
            }
        } else {
            row.failures.push(Failure::invalid(
                stage,
                "HID metadata string is not terminated within its bound",
            ));
        }
    }
    let mut preparsed = 0;
    if budget.records == 0 || budget.bytes == 0 {
        row.failures.push(Failure::invalid(
            "caps-capacity",
            "Aggregate parsed HID capability budget exhausted; inspect this interface separately",
        ));
    } else if !(api.preparsed)(handle.value, &mut preparsed) {
        row.failures
            .push(Failure::native("preparsed-data", (api.last_error)()));
    } else if preparsed == 0 {
        row.failures.push(Failure::invalid(
            "preparsed-data",
            "HID API returned a null preparsed-data pointer",
        ));
    } else {
        let mut owned = OwnedPreparsed::new(api, preparsed);
        let snapshot = (api.snapshot)(preparsed);
        let records = snapshot.retained_records();
        let bytes = serde_json::to_vec(&snapshot).map(|value| value.len());
        match bytes {
            Ok(bytes) if records <= budget.records && bytes <= budget.bytes => {
                budget.records -= records;
                budget.bytes -= bytes;
                row.caps = Some(snapshot);
            }
            _ => {
                budget.records = 0;
                budget.bytes = 0;
                row.failures.push(Failure::invalid("caps-capacity","Aggregate parsed HID capability budget exceeded; inspect this interface separately"));
            }
        }
        if !owned.release() {
            row.failures
                .push(Failure::native("preparsed-release", (api.last_error)()));
        }
    }
    if !handle.release() {
        row.failures
            .push(Failure::native("metadata-close", (api.last_error)()));
    }
}

unsafe fn enumerate_with(api: &Api) -> Result<Inventory, Failure> {
    let mut guid = std::mem::zeroed();
    (api.guid)(&mut guid);
    let set = (api.classes)(
        &guid,
        null(),
        null_mut(),
        DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
    );
    if set == -1 {
        return Err(Failure::native("interface-set", (api.last_error)()));
    }
    let mut owned = OwnedSet::new(api, set);
    let mut result = Inventory {
        interfaces: vec![],
        failures: vec![],
        truncated: false,
    };
    let mut seen = BTreeSet::new();
    let mut budget = CapsBudget::default();
    for index in 0..=MAX_INTERFACES {
        let mut interface = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if (api.enumerate)(set, null(), &guid, index, &mut interface) == 0 {
            let code = (api.last_error)();
            if code != ERROR_NO_MORE_ITEMS {
                result
                    .failures
                    .push(Failure::native("interface-enumeration", code));
                result.truncated = true;
            }
            break;
        }
        if index == MAX_INTERFACES {
            result.truncated = true;
            result.failures.push(Failure::invalid(
                "interface-capacity",
                "Present HID interface count exceeds inventory capacity",
            ));
            break;
        }
        let mut row = Interface::new(index);
        match path(api, set, &interface) {
            Ok(path) => {
                if !seen.insert(path.clone()) {
                    row.failures.push(Failure::invalid(
                        "interface-identity",
                        "SetupAPI repeated an exact interface path; inventory is incomplete",
                    ));
                    result.truncated = true;
                }
                row.path = Some(path);
                metadata(api, &mut row, &mut budget);
                if row
                    .failures
                    .iter()
                    .any(|failure| failure.stage == "caps-capacity")
                {
                    result.truncated = true;
                }
            }
            Err(failure) => row.failures.push(failure),
        }
        result.interfaces.push(row);
    }
    if !owned.release() {
        result
            .failures
            .push(Failure::native("interface-set-release", (api.last_error)()));
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
