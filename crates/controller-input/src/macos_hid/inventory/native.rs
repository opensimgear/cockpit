//! Supported public APIs from IOHIDManager.h, IOHIDDevice.h, IOHIDElement.h
//! and CoreFoundation. Discovery never opens a manager/device or registers input.
//! API declarations: apple-oss-distributions/IOKitUser, hid.subproj/
//! IOHIDElement.h and hidsystem.subproj/IOHIDLib.h (public SDK headers).
use iohidmanager::hid::{HidDevice, HidManagerOptions};
use serde_json::{json, Value};
use std::{ffi::c_void, ptr::null};

type Ref = *const c_void;
struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(value: Ref);
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFSetGetCount(value: Ref) -> isize;
    fn CFSetGetValues(value: Ref, values: *mut Ref);
    fn CFArrayGetCount(value: Ref) -> isize;
    fn CFArrayGetValueAtIndex(value: Ref, index: isize) -> Ref;
    fn CFStringCreateWithBytes(
        allocator: Ref,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        external: u8,
    ) -> Ref;
    fn CFStringGetTypeID() -> usize;
    fn CFStringGetLength(value: Ref) -> isize;
    fn CFStringGetCString(value: Ref, buffer: *mut u8, size: isize, encoding: u32) -> u8;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(value: Ref, kind: isize, result: *mut i64) -> u8;
    fn CFDataGetTypeID() -> usize;
    fn CFDataGetLength(value: Ref) -> isize;
    fn CFDataGetBytePtr(value: Ref) -> *const u8;
}
#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDCheckAccess(request_type: u32) -> u32;
    fn IOHIDManagerCreate(allocator: Ref, options: u32) -> Ref;
    fn IOHIDManagerSetDeviceMatching(manager: Ref, matching: Ref);
    fn IOHIDManagerCopyDevices(manager: Ref) -> Ref;
    fn IOHIDDeviceGetService(device: Ref) -> u32;
    fn IORegistryEntryGetRegistryEntryID(service: u32, id: *mut u64) -> i32;
    fn IOHIDDeviceGetProperty(device: Ref, key: Ref) -> Ref;
    fn IOHIDDeviceCopyMatchingElements(device: Ref, matching: Ref, options: u32) -> Ref;
    fn IOHIDElementGetCookie(element: Ref) -> u32;
    fn IOHIDElementGetParent(element: Ref) -> Ref;
    fn IOHIDElementGetType(element: Ref) -> u32;
    fn IOHIDElementGetCollectionType(element: Ref) -> u32;
    fn IOHIDElementGetUsagePage(element: Ref) -> u32;
    fn IOHIDElementGetUsage(element: Ref) -> u32;
    fn IOHIDElementGetReportID(element: Ref) -> u32;
    fn IOHIDElementGetReportSize(element: Ref) -> u32;
    fn IOHIDElementGetReportCount(element: Ref) -> u32;
    fn IOHIDElementGetLogicalMin(element: Ref) -> isize;
    fn IOHIDElementGetLogicalMax(element: Ref) -> isize;
    fn IOHIDElementGetPhysicalMin(element: Ref) -> isize;
    fn IOHIDElementGetPhysicalMax(element: Ref) -> isize;
    fn IOHIDElementGetUnit(element: Ref) -> u32;
    fn IOHIDElementGetUnitExponent(element: Ref) -> u32;
    fn IOHIDElementIsRelative(element: Ref) -> u8;
    fn IOHIDElementIsArray(element: Ref) -> u8;
    fn IOHIDElementHasNullState(element: Ref) -> u8;
    fn IOHIDElementIsWrapping(element: Ref) -> u8;
    fn IOHIDElementIsNonLinear(element: Ref) -> u8;
    fn IOHIDElementHasPreferredState(element: Ref) -> u8;
    fn IOHIDElementIsVirtual(element: Ref) -> u8;
    fn IOHIDElementGetProperty(element: Ref, key: Ref) -> Ref;
}

pub(super) fn listen_access_status() -> &'static str {
    // IOHIDLib.h: ListenEvent=1; Granted=0, Denied=1, Unknown=2.
    // Passive process-wide TCC check; this never requests authorization.
    match unsafe { IOHIDCheckAccess(1) } {
        0 => "granted",
        1 => "denied",
        _ => "unknown",
    }
}

pub(super) fn ids() -> Result<Vec<u64>, String> {
    unsafe {
        let options =
            HidManagerOptions::DO_NOT_LOAD_PROPERTIES | HidManagerOptions::DO_NOT_SAVE_PROPERTIES;
        let manager = IOHIDManagerCreate(null(), options.bits());
        if manager.is_null() {
            return Err("IOHIDManagerCreate failed".into());
        }
        let manager = Owned(manager);
        // NULL matching means all devices. Do not call IOHIDManagerOpen: it
        // opens every matching device and is unnecessary for metadata discovery.
        IOHIDManagerSetDeviceMatching(manager.0, null());
        let set = IOHIDManagerCopyDevices(manager.0);
        if set.is_null() {
            return Ok(vec![]);
        }
        let set = Owned(set);
        let count = bounded(CFSetGetCount(set.0), 1024, "interface count")?;
        let mut devices = vec![null(); count];
        CFSetGetValues(set.0, devices.as_mut_ptr());
        let mut ids = Vec::with_capacity(count);
        for device in devices {
            if device.is_null() {
                return Err("Null IOHID device in inventory".into());
            }
            let mut id = 0;
            let status = IORegistryEntryGetRegistryEntryID(IOHIDDeviceGetService(device), &mut id);
            if status != 0 || id == 0 {
                return Err(format!(
                    "IORegistryEntryGetRegistryEntryID failed: {status}"
                ));
            }
            ids.push(id);
        }
        ids.sort_unstable();
        if ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("Duplicate IOHID registry identities; inventory incomplete".into());
        }
        Ok(ids)
    }
}

pub(super) fn bounded(value: isize, max: usize, name: &str) -> Result<usize, String> {
    usize::try_from(value)
        .ok()
        .filter(|n| *n <= max)
        .ok_or_else(|| format!("IOHID {name} exceeds metadata budget"))
}
fn property(device: &HidDevice, name: &str) -> Result<Ref, String> {
    unsafe {
        let key =
            CFStringCreateWithBytes(null(), name.as_ptr(), name.len() as isize, 0x08000100, 0);
        if key.is_null() {
            return Err("Cannot create IOHID property key".into());
        }
        let key = Owned(key);
        Ok(IOHIDDeviceGetProperty(device.as_ptr().cast(), key.0))
    }
}

unsafe fn element_number(element: Ref, name: &str) -> Option<u32> {
    let key = CFStringCreateWithBytes(null(), name.as_ptr(), name.len() as isize, 0x08000100, 0);
    if key.is_null() {
        return None;
    }
    let key = Owned(key);
    let value = IOHIDElementGetProperty(element, key.0);
    if value.is_null() || CFGetTypeID(value) != CFNumberGetTypeID() {
        return None;
    }
    let mut result = 0i64;
    if CFNumberGetValue(value, 4, &mut result) == 0 {
        return None;
    }
    u32::try_from(result).ok()
}
pub(super) fn number(device: &HidDevice, name: &str) -> Result<Option<u32>, String> {
    let value = property(device, name)?;
    if value.is_null() {
        return Ok(None);
    }
    unsafe {
        let mut result = 0i64;
        if CFGetTypeID(value) != CFNumberGetTypeID() || CFNumberGetValue(value, 4, &mut result) == 0
        {
            return Err(format!("Invalid numeric IOHID property {name}"));
        }
        u32::try_from(result)
            .map(Some)
            .map_err(|_| format!("Invalid IOHID property {name} range"))
    }
}
pub(super) fn string(device: &HidDevice, name: &str) -> Result<Option<String>, String> {
    let value = property(device, name)?;
    if value.is_null() {
        return Ok(None);
    }
    unsafe {
        if CFGetTypeID(value) != CFStringGetTypeID() {
            return Err(format!("Invalid string IOHID property {name}"));
        }
        let length = bounded(CFStringGetLength(value), 2048, "property string")?;
        let mut bytes = vec![0u8; length * 4 + 1];
        if CFStringGetCString(value, bytes.as_mut_ptr(), bytes.len() as isize, 0x08000100) == 0 {
            return Err(format!("Cannot decode IOHID property {name}"));
        }
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        String::from_utf8(bytes[..end].to_vec())
            .map(Some)
            .map_err(|e| e.to_string())
    }
}
pub(super) fn descriptor(device: &HidDevice) -> Result<Option<Vec<u8>>, String> {
    let value = property(device, "ReportDescriptor")?;
    if value.is_null() {
        return Ok(None);
    }
    unsafe {
        if CFGetTypeID(value) != CFDataGetTypeID() {
            return Err("ReportDescriptor is not CFData".into());
        }
        let size = bounded(CFDataGetLength(value), 65536, "descriptor size")?;
        if size == 0 {
            return Ok(Some(vec![]));
        }
        let bytes = CFDataGetBytePtr(value);
        if bytes.is_null() {
            return Err("Missing IOHID descriptor bytes".into());
        }
        Ok(Some(std::slice::from_raw_parts(bytes, size).to_vec()))
    }
}
pub(super) fn elements(device: &HidDevice) -> Result<Vec<Value>, String> {
    unsafe {
        let array = IOHIDDeviceCopyMatchingElements(device.as_ptr().cast(), null(), 0);
        if array.is_null() {
            return Err("IOHIDDeviceCopyMatchingElements unavailable".into());
        }
        let array = Owned(array);
        let count = bounded(CFArrayGetCount(array.0), 16384, "element count")?;
        let mut result = Vec::with_capacity(count);
        for index in 0..count {
            let e = CFArrayGetValueAtIndex(array.0, index as isize);
            if e.is_null() {
                return Err("Null IOHID element".into());
            }
            let parent = IOHIDElementGetParent(e);
            let kind = IOHIDElementGetType(e);
            result.push(json!({
                "cookie":IOHIDElementGetCookie(e),
                "parentCookie":if parent.is_null() { None } else { Some(IOHIDElementGetCookie(parent)) },
                "type":kind,"collectionType":if kind == 513 { Some(IOHIDElementGetCollectionType(e)) } else { None },
                "usagePage":IOHIDElementGetUsagePage(e),"usage":IOHIDElementGetUsage(e),
                "reportId":IOHIDElementGetReportID(e),"reportSizeBits":IOHIDElementGetReportSize(e),
                "reportCount":IOHIDElementGetReportCount(e),
                "logicalMin":IOHIDElementGetLogicalMin(e).to_string(),"logicalMax":IOHIDElementGetLogicalMax(e).to_string(),
                "physicalMin":IOHIDElementGetPhysicalMin(e).to_string(),"physicalMax":IOHIDElementGetPhysicalMax(e).to_string(),
                "unit":IOHIDElementGetUnit(e),"unitExponent":IOHIDElementGetUnitExponent(e),
                "relative":IOHIDElementIsRelative(e)!=0,"array":IOHIDElementIsArray(e)!=0,"nullState":IOHIDElementHasNullState(e)!=0,
                "wrapping":IOHIDElementIsWrapping(e)!=0,"nonLinear":IOHIDElementIsNonLinear(e)!=0,
                "preferredState":IOHIDElementHasPreferredState(e)!=0,"virtual":IOHIDElementIsVirtual(e)!=0,
                "usageMin":element_number(e,"UsageMin"),"usageMax":element_number(e,"UsageMax")
            }));
        }
        result.sort_by_key(|v| v["cookie"].as_u64());
        Ok(result)
    }
}
