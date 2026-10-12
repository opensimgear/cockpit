//! Publish Pico module labels on macOS HID registry services. The USB device's
//! shared iProduct stays unchanged; IOHIDDeviceSetProperty would only name a
//! client-local cache, so each write is checked by reading IORegistry again.
//!
//! Invoke this on the naming worker, never the CDC/heartbeat worker. Hardware is
//! accessed only for a validated, nonempty request on macOS. The caller supplies
//! cancellation when its serial connection or inventory changes.

use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const RETRY_DEADLINE: Duration = Duration::from_secs(2);
const RETRY_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
struct ModuleName {
    id: String,
    interface: u8,
    label: String,
}

#[derive(Debug)]
struct Request {
    serial: String,
    modules: Vec<ModuleName>,
}

fn identifier(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn request(payload: &Value) -> Option<Request> {
    if serde_json::to_vec(payload).ok()?.len() > 8192 {
        return None;
    }
    let serial = payload.get("gatewaySerial")?.as_str()?;
    if !identifier(serial.strip_prefix("OSG-PICO-")?, 4, 64) {
        return None;
    }
    let modules = payload.get("modules")?.as_array()?;
    if modules.len() > 8 {
        return None;
    }
    let mut ids = HashSet::new();
    let mut interfaces = HashSet::new();
    let mut names = Vec::with_capacity(modules.len());
    for module in modules {
        let id = module.get("id")?.as_str()?;
        let interface = module.get("hidInterface")?.as_u64()?;
        let label = module.get("label")?.as_str()?;
        if !identifier(id, 1, 40)
            || !(2..=9).contains(&interface)
            || !ids.insert(id)
            || !interfaces.insert(interface)
            || label.trim().is_empty()
            || label.chars().count() > 80
            || label.chars().any(char::is_control)
        {
            return None;
        }
        names.push(ModuleName {
            id: id.to_owned(),
            interface: interface as u8,
            label: label.to_owned(),
        });
    }
    Some(Request {
        serial: serial.to_owned(),
        modules: names,
    })
}

fn failure(message: &str) -> Value {
    json!({"supported": true, "ok": false, "devices": [], "error": message})
}

/// Accepts `{gatewaySerial, modules: [{id, hidInterface, label}]}` and returns
/// `{supported, ok, devices, error?}`, matching the previous helper contract.
#[cfg(test)]
pub fn apply(payload: &Value) -> Value {
    apply_cancellable(payload, &AtomicBool::new(false))
}

/// Stops retries and additional writes once the connection's naming job is stale.
/// The calling worker must also discard results belonging to an earlier epoch.
pub fn apply_cancellable(payload: &Value, cancelled: &AtomicBool) -> Value {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (payload, cancelled);
        json!({"supported": false})
    }
    #[cfg(target_os = "macos")]
    {
        let Some(request) = request(payload) else {
            return failure("Invalid Pico HID naming request.");
        };
        if request.modules.is_empty() {
            return json!({"supported": true, "ok": true, "devices": []});
        }
        let deadline = Instant::now() + RETRY_DEADLINE;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return failure("Pico HID naming was cancelled.");
            }
            let result = name_once(&native::Registry, &request, cancelled);
            if result["ok"] == true || Instant::now() >= deadline {
                return result;
            }
            // Re-enumeration can lag behind the first successful CDC handshake.
            // Short waits keep unplug/inventory cancellation responsive.
            std::thread::sleep(
                RETRY_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
}

trait Registry {
    type Handle;
    fn devices(&self) -> Result<Vec<Self::Handle>, String>;
    fn number(&self, handle: &Self::Handle, key: &str) -> Option<i64>;
    fn string(&self, handle: &Self::Handle, key: &str) -> Option<String>;
    fn interface(&self, handle: &Self::Handle) -> Option<u8>;
    fn registry_id(&self, handle: &Self::Handle) -> String;
    fn set_product(&self, handle: &Self::Handle, label: &str) -> Result<(), i32>;
    fn descendants(&self, handle: &Self::Handle) -> Vec<(Self::Handle, &'static str)>;
}

fn identity<B: Registry>(registry: &B, handle: &B::Handle, serial: &str) -> bool {
    registry.number(handle, "VendorID") == Some(0xCAFE)
        && registry.number(handle, "ProductID") == Some(0x4010)
        && registry.string(handle, "SerialNumber").as_deref() == Some(serial)
}

fn gateway_hid<B: Registry>(registry: &B, handle: &B::Handle, serial: &str) -> bool {
    identity(registry, handle, serial)
        && registry.number(handle, "PrimaryUsagePage") == Some(1)
        && registry.number(handle, "PrimaryUsage") == Some(4)
}

fn set_name<B: Registry>(registry: &B, handle: &B::Handle, label: &str) -> Value {
    let before = registry.string(handle, "Product").unwrap_or_default();
    let written = if before == label {
        Ok(())
    } else {
        registry.set_product(handle, label)
    };
    let actual = registry.string(handle, "Product").unwrap_or_default();
    let mut record = json!({"registryID": registry.registry_id(handle),
        "product": actual, "ok": written.is_ok() && actual == label});
    if let Err(code) = written {
        record["error"] = json!(format!("macOS rejected this HID service name ({code})."));
    } else if actual != label {
        record["error"] = json!("macOS retained the cached HID service name.");
    }
    record
}

fn name_once<B: Registry>(registry: &B, request: &Request, cancelled: &AtomicBool) -> Value {
    let devices = match registry.devices() {
        Ok(devices) => devices,
        Err(error) => return failure(&error),
    };
    let mut records = Vec::with_capacity(request.modules.len());
    let mut failures = Vec::new();
    for module in &request.modules {
        if cancelled.load(Ordering::Acquire) {
            return failure("Pico HID naming was cancelled.");
        }
        let services: Vec<_> = devices
            .iter()
            .filter(|handle| {
                gateway_hid(registry, handle, &request.serial)
                    && registry.interface(handle) == Some(module.interface)
            })
            .collect();
        if services.len() != 1 {
            failures.push(format!(
                "HID interface {} is not uniquely available.",
                module.interface
            ));
            records.push(json!({"id": module.id, "hidInterface": module.interface,
                "label": module.label, "ok": false, "product": "", "descendants": []}));
            continue;
        }
        let service = services[0];
        let mut named = set_name(registry, service, &module.label);
        named["id"] = json!(module.id);
        named["hidInterface"] = json!(module.interface);
        named["label"] = json!(module.label);
        if named["ok"] != true {
            failures.push(format!(
                "macOS could not publish the name for HID interface {}.",
                module.interface
            ));
        }
        let mut descendants = Vec::new();
        for (child, class) in registry.descendants(service).into_iter().take(8) {
            if cancelled.load(Ordering::Acquire) {
                return failure("Pico HID naming was cancelled.");
            }
            // These descendants are reached only from the verified joystick.
            // They must independently belong to this board and USB interface.
            if !identity(registry, &child, &request.serial)
                || registry.interface(&child) != Some(module.interface)
            {
                continue;
            }
            let mut child_name = set_name(registry, &child, &module.label);
            child_name["class"] = json!(class);
            descendants.push(child_name);
        }
        // Descendant caches may reject Product while the actual HID service
        // accepted it. Keep that evidence without hiding main-device success.
        named["descendants"] = json!(descendants);
        records.push(named);
    }
    let mut result = json!({"supported": true, "ok": failures.is_empty(), "devices": records});
    if !failures.is_empty() {
        result["error"] = json!(failures.join(" "));
    }
    result
}

#[cfg(target_os = "macos")]
mod native {
    use super::Registry as RegistryApi;
    use std::ffi::{c_char, c_void};
    use std::ptr;

    type CFRef = *const c_void;
    type IoObject = u32;
    const UTF8: u32 = 0x0800_0100;
    const SERVICE_PLANE: &[u8] = b"IOService\0";

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: CFRef);
        fn CFGetTypeID(value: CFRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFNumberGetValue(number: CFRef, number_type: isize, value: *mut c_void) -> u8;
        fn CFStringCreateWithBytes(
            allocator: CFRef,
            bytes: *const u8,
            length: isize,
            encoding: u32,
            external: u8,
        ) -> CFRef;
        fn CFStringGetLength(string: CFRef) -> isize;
        fn CFStringGetCString(string: CFRef, buffer: *mut c_char, size: isize, encoding: u32)
            -> u8;
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> CFRef;
        fn IOServiceGetMatchingServices(port: u32, matching: CFRef, iterator: *mut IoObject)
            -> i32;
        fn IOIteratorNext(iterator: IoObject) -> IoObject;
        fn IOObjectRetain(object: IoObject) -> i32;
        fn IOObjectRelease(object: IoObject) -> i32;
        fn IOObjectConformsTo(object: IoObject, class: *const c_char) -> u32;
        fn IORegistryEntryGetParentEntry(
            entry: IoObject,
            plane: *const c_char,
            parent: *mut IoObject,
        ) -> i32;
        fn IORegistryEntryGetChildIterator(
            entry: IoObject,
            plane: *const c_char,
            iterator: *mut IoObject,
        ) -> i32;
        fn IORegistryEntryGetRegistryEntryID(entry: IoObject, id: *mut u64) -> i32;
        fn IORegistryEntryCreateCFProperty(
            entry: IoObject,
            key: CFRef,
            allocator: CFRef,
            options: u32,
        ) -> CFRef;
        fn IORegistryEntrySetCFProperty(entry: IoObject, key: CFRef, value: CFRef) -> i32;
    }

    struct OwnedCF(CFRef);
    impl OwnedCF {
        fn string(value: &str) -> Option<Self> {
            let reference = unsafe {
                CFStringCreateWithBytes(ptr::null(), value.as_ptr(), value.len() as isize, UTF8, 0)
            };
            (!reference.is_null()).then(|| Self(reference))
        }
    }
    impl Drop for OwnedCF {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) }
        }
    }

    pub struct Handle(IoObject);
    impl Clone for Handle {
        fn clone(&self) -> Self {
            unsafe { IOObjectRetain(self.0) };
            Self(self.0)
        }
    }
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { IOObjectRelease(self.0) };
        }
    }

    pub struct Registry;
    impl Registry {
        fn property(&self, handle: &Handle, key: &str) -> Option<OwnedCF> {
            let key = OwnedCF::string(key)?;
            let value = unsafe { IORegistryEntryCreateCFProperty(handle.0, key.0, ptr::null(), 0) };
            (!value.is_null()).then(|| OwnedCF(value))
        }

        fn children(
            &self,
            handle: &Handle,
            depth: usize,
            result: &mut Vec<(Handle, &'static str)>,
        ) {
            if depth >= 3 || result.len() >= 8 {
                return;
            }
            let mut iterator = 0;
            if unsafe {
                IORegistryEntryGetChildIterator(
                    handle.0,
                    SERVICE_PLANE.as_ptr().cast(),
                    &mut iterator,
                )
            } != 0
            {
                return;
            }
            let iterator = Handle(iterator);
            for _ in 0..32 {
                let child = unsafe { IOIteratorNext(iterator.0) };
                if child == 0 || result.len() >= 8 {
                    // IOIteratorNext returns a retained object even if the cap
                    // was reached; release it in that case too.
                    if child != 0 {
                        drop(Handle(child));
                    }
                    break;
                }
                let child = Handle(child);
                let class =
                    if unsafe { IOObjectConformsTo(child.0, c"IOHIDInterface".as_ptr()) } != 0 {
                        "IOHIDInterface"
                    } else if unsafe {
                        IOObjectConformsTo(child.0, c"AppleUserHIDEventService".as_ptr())
                    } != 0
                    {
                        "AppleUserHIDEventService"
                    } else {
                        continue;
                    };
                result.push((child.clone(), class));
                self.children(&child, depth + 1, result);
            }
        }
    }

    impl RegistryApi for Registry {
        type Handle = Handle;

        fn devices(&self) -> Result<Vec<Handle>, String> {
            let matching = unsafe { IOServiceMatching(c"IOHIDDevice".as_ptr()) };
            if matching.is_null() {
                return Err("macOS could not create Pico HID service matching.".to_owned());
            }
            let mut iterator = 0;
            // This function consumes the matching dictionary, including when
            // it fails. The iterator and every returned entry are retained.
            let opened = unsafe { IOServiceGetMatchingServices(0, matching, &mut iterator) };
            if opened != 0 {
                return Err(format!(
                    "macOS could not enumerate Pico HID services ({opened})."
                ));
            }
            let iterator = Handle(iterator);
            let mut result = Vec::new();
            for _ in 0..256 {
                let entry = unsafe { IOIteratorNext(iterator.0) };
                if entry == 0 {
                    break;
                }
                result.push(Handle(entry));
            }
            Ok(result)
        }

        fn number(&self, handle: &Handle, key: &str) -> Option<i64> {
            let value = self.property(handle, key)?;
            if unsafe { CFGetTypeID(value.0) != CFNumberGetTypeID() } {
                return None;
            }
            let mut result: i64 = 0;
            // kCFNumberSInt64Type = 4; never assume the registry value's width.
            (unsafe { CFNumberGetValue(value.0, 4, (&mut result as *mut i64).cast()) } != 0)
                .then_some(result)
        }

        fn string(&self, handle: &Handle, key: &str) -> Option<String> {
            let value = self.property(handle, key)?;
            if unsafe { CFGetTypeID(value.0) != CFStringGetTypeID() } {
                return None;
            }
            let length = unsafe { CFStringGetLength(value.0) };
            if !(0..=4096).contains(&length) {
                return None;
            }
            let mut bytes = vec![0u8; length as usize * 4 + 1];
            if unsafe {
                CFStringGetCString(
                    value.0,
                    bytes.as_mut_ptr().cast(),
                    bytes.len() as isize,
                    UTF8,
                )
            } == 0
            {
                return None;
            }
            let end = bytes.iter().position(|byte| *byte == 0)?;
            String::from_utf8(bytes[..end].to_vec()).ok()
        }

        fn interface(&self, handle: &Handle) -> Option<u8> {
            let mut entry = handle.clone();
            for _ in 0..16 {
                if let Some(number) = self.number(&entry, "bInterfaceNumber") {
                    return u8::try_from(number).ok();
                }
                let mut parent = 0;
                if unsafe {
                    IORegistryEntryGetParentEntry(
                        entry.0,
                        SERVICE_PLANE.as_ptr().cast(),
                        &mut parent,
                    )
                } != 0
                {
                    return None;
                }
                entry = Handle(parent);
            }
            None
        }

        fn registry_id(&self, handle: &Handle) -> String {
            let mut id = 0;
            unsafe { IORegistryEntryGetRegistryEntryID(handle.0, &mut id) };
            id.to_string()
        }

        fn set_product(&self, handle: &Handle, label: &str) -> Result<(), i32> {
            let key = OwnedCF::string("Product").ok_or(-1)?;
            let label = OwnedCF::string(label).ok_or(-1)?;
            let result = unsafe { IORegistryEntrySetCFProperty(handle.0, key.0, label.0) };
            if result == 0 {
                Ok(())
            } else {
                Err(result)
            }
        }

        fn descendants(&self, handle: &Handle) -> Vec<(Handle, &'static str)> {
            let mut result = Vec::new();
            self.children(handle, 0, &mut result);
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn payload() -> Value {
        json!({"gatewaySerial": "OSG-PICO-E661640843565431", "modules": [
            {"id": "OSG-SQ-0042", "hidInterface": 2, "label": "OSG Séquentiel 🏁"}]})
    }

    #[test]
    fn request_validation_is_bounded_and_accepts_printable_unicode() {
        assert!(request(&payload()).is_some());
        let mut empty = payload();
        empty["modules"] = json!([]);
        assert!(request(&empty).is_some());
        for key_value in [
            ("id", json!("../device")),
            ("hidInterface", json!(true)),
            ("hidInterface", json!(1)),
            ("hidInterface", json!(10)),
            ("label", json!(" ")),
            ("label", json!("OSG\nDevice")),
            ("label", json!("a".repeat(81))),
        ] {
            let mut bad = payload();
            bad["modules"][0][key_value.0] = key_value.1;
            assert!(request(&bad).is_none(), "{bad}");
        }
        let mut bad = payload();
        bad["gatewaySerial"] = json!("OTHER-BOARD");
        assert!(request(&bad).is_none());
        let mut duplicate = payload();
        let repeated = duplicate["modules"][0].clone();
        duplicate["modules"].as_array_mut().unwrap().push(repeated);
        assert!(request(&duplicate).is_none());
        let mut large = payload();
        large["padding"] = json!("a".repeat(8192));
        assert!(request(&large).is_none());
    }

    struct FakeDevice {
        serial: &'static str,
        interface: u8,
        usage: i64,
        product: String,
        refuse: bool,
        cache_only: bool,
    }

    struct FakeRegistry {
        devices: RefCell<Vec<FakeDevice>>,
        writes: RefCell<Vec<usize>>,
    }

    impl Registry for FakeRegistry {
        type Handle = usize;
        fn devices(&self) -> Result<Vec<usize>, String> {
            Ok((0..self.devices.borrow().len()).collect())
        }
        fn number(&self, handle: &usize, key: &str) -> Option<i64> {
            match key {
                "VendorID" => Some(0xCAFE),
                "ProductID" => Some(0x4010),
                "PrimaryUsagePage" => Some(1),
                "PrimaryUsage" => Some(self.devices.borrow()[*handle].usage),
                _ => None,
            }
        }
        fn string(&self, handle: &usize, key: &str) -> Option<String> {
            let devices = self.devices.borrow();
            match key {
                "SerialNumber" => Some(devices[*handle].serial.to_owned()),
                "Product" => Some(devices[*handle].product.clone()),
                _ => None,
            }
        }
        fn interface(&self, handle: &usize) -> Option<u8> {
            Some(self.devices.borrow()[*handle].interface)
        }
        fn registry_id(&self, handle: &usize) -> String {
            (u64::MAX - *handle as u64).to_string()
        }
        fn set_product(&self, handle: &usize, label: &str) -> Result<(), i32> {
            self.writes.borrow_mut().push(*handle);
            let mut devices = self.devices.borrow_mut();
            if devices[*handle].refuse {
                return Err(-9);
            }
            if !devices[*handle].cache_only {
                devices[*handle].product = label.to_owned();
            }
            Ok(())
        }
        fn descendants(&self, _: &usize) -> Vec<(usize, &'static str)> {
            vec![]
        }
    }

    fn device(serial: &'static str, interface: u8, usage: i64) -> FakeDevice {
        FakeDevice {
            serial,
            interface,
            usage,
            product: "shared USB product".to_owned(),
            refuse: false,
            cache_only: false,
        }
    }

    fn registry(devices: Vec<FakeDevice>) -> FakeRegistry {
        FakeRegistry {
            devices: RefCell::new(devices),
            writes: RefCell::new(vec![]),
        }
    }

    #[test]
    fn only_exact_gateway_joystick_and_interface_are_named() {
        let fake = registry(vec![
            device("OSG-PICO-OTHER1234", 2, 4),
            device("OSG-PICO-E661640843565431", 2, 6),
            device("OSG-PICO-E661640843565431", 3, 4),
            device("OSG-PICO-E661640843565431", 2, 4),
        ]);
        let result = name_once(
            &fake,
            &request(&payload()).unwrap(),
            &AtomicBool::new(false),
        );
        assert_eq!(result["ok"], true);
        assert_eq!(*fake.writes.borrow(), vec![3]);
        assert_eq!(result["devices"][0]["product"], "OSG Séquentiel 🏁");
        assert_eq!(
            result["devices"][0]["registryID"],
            (u64::MAX - 3).to_string()
        );
    }

    #[test]
    fn ambiguous_interface_is_never_written() {
        let fake = registry(vec![
            device("OSG-PICO-E661640843565431", 2, 4),
            device("OSG-PICO-E661640843565431", 2, 4),
        ]);
        let result = name_once(
            &fake,
            &request(&payload()).unwrap(),
            &AtomicBool::new(false),
        );
        assert_eq!(result["ok"], false);
        assert!(fake.writes.borrow().is_empty());
    }

    #[test]
    fn registry_readback_must_confirm_the_actual_product() {
        let mut entry = device("OSG-PICO-E661640843565431", 2, 4);
        entry.cache_only = true;
        let fake = registry(vec![entry]);
        let result = name_once(
            &fake,
            &request(&payload()).unwrap(),
            &AtomicBool::new(false),
        );
        assert_eq!(result["ok"], false);
        assert_eq!(result["devices"][0]["product"], "shared USB product");
        assert_eq!(
            result["devices"][0]["error"],
            "macOS retained the cached HID service name."
        );
    }

    #[test]
    fn cancellation_prevents_all_registry_writes() {
        let fake = registry(vec![device("OSG-PICO-E661640843565431", 2, 4)]);
        let result = name_once(&fake, &request(&payload()).unwrap(), &AtomicBool::new(true));
        assert_eq!(result["ok"], false);
        assert!(fake.writes.borrow().is_empty());
    }

    #[test]
    fn invalid_and_empty_requests_never_access_hardware() {
        #[cfg(target_os = "macos")]
        {
            assert_eq!(apply(&json!({}))["ok"], false);
            let mut empty = payload();
            empty["modules"] = json!([]);
            assert_eq!(
                apply(&empty),
                json!({"supported": true, "ok": true, "devices": []})
            );
        }
        #[cfg(not(target_os = "macos"))]
        assert_eq!(apply(&payload()), json!({"supported": false}));
    }
}
