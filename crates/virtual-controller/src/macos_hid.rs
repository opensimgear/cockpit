//! IOHIDUserDevice backend for the separate helper. No live path is qualified.
use crate::{
    apple_helper,
    hid::Joystick,
    uinput::{Backend, Device, Event, Template},
};
use std::{
    ffi::{c_void, CString},
    fs::{File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    ptr,
};
type Ref = *const c_void;
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(value: Ref);
    #[cfg(test)]
    fn CFRetain(value: Ref) -> Ref;
    fn CFStringCreateWithCString(allocator: Ref, value: *const i8, encoding: u32) -> Ref;
    fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
    fn CFNumberCreate(allocator: Ref, kind: isize, value: *const c_void) -> Ref;
    fn CFDictionaryCreate(
        allocator: Ref,
        keys: *const Ref,
        values: *const Ref,
        count: isize,
        key_callbacks: Ref,
        value_callbacks: Ref,
    ) -> Ref;
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(value: Ref) -> bool;
}
#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDUserDeviceCreate(allocator: Ref, properties: Ref) -> Ref;
    fn IOHIDUserDeviceHandleReport(device: Ref, report: *const u8, length: isize) -> i32;
}
#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecTaskCreateFromSelf(allocator: Ref) -> Ref;
    fn SecTaskCopyValueForEntitlement(task: Ref, key: Ref, error: *mut Ref) -> Ref;
}
struct Object(Ref);
impl Object {
    fn new(value: Ref) -> Result<Self, String> {
        if value.is_null() {
            Err("Apple HID object creation or authorization failed.".into())
        } else {
            Ok(Self(value))
        }
    }
    fn string(value: &str) -> Result<Self, String> {
        let value = CString::new(value).map_err(|_| "Invalid HID string.")?;
        Self::new(unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), 0x08000100) })
    }
    fn number(value: i32) -> Result<Self, String> {
        Self::new(unsafe { CFNumberCreate(ptr::null(), 3, (&value as *const i32).cast()) })
    }
    fn data(bytes: &[u8]) -> Result<Self, String> {
        Self::new(unsafe { CFDataCreate(ptr::null(), bytes.as_ptr(), bytes.len() as isize) })
    }
}
impl Drop for Object {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}
fn properties(j: &Joystick) -> Result<Object, String> {
    let entries = [
        ("ReportDescriptor", Object::data(&j.descriptor)?),
        ("VendorID", Object::number(0)?),
        (
            "ProductID",
            Object::number(0xc000 + i32::from(j.template.binding.slot))?,
        ),
        ("VersionNumber", Object::number(1)?),
        ("Manufacturer", Object::string("OpenSimGear")?),
        ("Product", Object::string(&j.template.name)?),
        ("SerialNumber", Object::string(&j.serial())?),
        ("Transport", Object::string("Virtual")?),
        ("PhysicalDeviceUniqueID", Object::string(&j.serial())?),
    ];
    let keys = entries
        .iter()
        .map(|(key, _)| Object::string(key))
        .collect::<Result<Vec<_>, _>>()?;
    let key_refs = keys.iter().map(|k| k.0).collect::<Vec<_>>();
    let values = entries.iter().map(|(_, v)| v.0).collect::<Vec<_>>();
    Object::new(unsafe {
        CFDictionaryCreate(
            ptr::null(),
            key_refs.as_ptr(),
            values.as_ptr(),
            values.len() as isize,
            ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
            ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
        )
    })
}
fn authorized() -> Result<(), String> {
    let task = Object::new(unsafe { SecTaskCreateFromSelf(ptr::null()) })?;
    let key = Object::string(apple_helper::ENTITLEMENT)?;
    let value =
        Object::new(unsafe { SecTaskCopyValueForEntitlement(task.0, key.0, ptr::null_mut()) })?;
    if unsafe { CFGetTypeID(value.0) != CFBooleanGetTypeID() || !CFBooleanGetValue(value.0) } {
        return Err("The Apple virtual-HID entitlement is not authorized for this helper.".into());
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct Api {
    create: unsafe extern "C" fn(Ref, Ref) -> Ref,
    report: unsafe extern "C" fn(Ref, *const u8, isize) -> i32,
}
const API: Api = Api {
    create: IOHIDUserDeviceCreate,
    report: IOHIDUserDeviceHandleReport,
};
pub struct Native {
    _lease: File,
    api: Api,
}
fn ownership() -> Result<File, String> {
    let uid = unsafe { libc::geteuid() };
    let path = std::env::temp_dir().join(format!("opensimgear-cockpit-apple-hid-{uid}.lock"));
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let m = f.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.uid() != uid || m.mode() & 0o077 != 0 {
        return Err("Unsafe Apple HID ownership lock; preserved without modification.".into());
    }
    f.try_lock()
        .map_err(|_| "A previous Apple HID owner is still alive.")?;
    Ok(f)
}
fn supported_os() -> bool {
    let mut bytes = [0u8; 64];
    let mut len = bytes.len();
    let key = b"kern.osrelease\0";
    let result = unsafe {
        libc::sysctlbyname(
            key.as_ptr().cast(),
            bytes.as_mut_ptr().cast(),
            &mut len,
            ptr::null_mut(),
            0,
        )
    };
    result == 0
        && std::str::from_utf8(&bytes[..len.min(bytes.len())])
            .ok()
            .and_then(|s| s.split('.').next())
            .and_then(|s| s.parse::<u32>().ok())
            .is_some_and(|major| major >= 24)
        && cfg!(any(target_arch = "x86_64", target_arch = "aarch64"))
}
impl Native {
    pub fn open() -> Result<Self, String> {
        if !apple_helper::QUALIFIED {
            return Err(apple_helper::REASON.into());
        }
        if !supported_os() {
            return Err(
                "The Apple HID helper requires an approved macOS 15+ architecture/version.".into(),
            );
        }
        authorized()?;
        Ok(Self {
            _lease: ownership()?,
            api: API,
        })
    }
}
pub struct NativeDevice {
    object: Object,
    joystick: Joystick,
    api: Api,
}
impl Device for NativeDevice {
    fn emit(&mut self, events: &[Event]) -> Result<(), String> {
        let bytes = self.joystick.report(events)?;
        let status =
            unsafe { (self.api.report)(self.object.0, bytes.as_ptr(), bytes.len() as isize) };
        if status != 0 {
            Err(format!(
                "Apple HID input report failed ({status}); stop and release the device."
            ))
        } else {
            Ok(())
        }
    }
}
impl Backend for Native {
    type Device = NativeDevice;
    fn ready(&mut self) -> Result<(), String> {
        authorized()
    }
    fn create(&mut self, t: &Template) -> Result<NativeDevice, String> {
        let joystick = Joystick::new(t.binding.clone())?;
        let props = properties(&joystick)?;
        let object = Object::new(unsafe { (self.api.create)(ptr::null(), props.0) })?;
        Ok(NativeDevice {
            object,
            joystick,
            api: self.api,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::uinput::tests::binding;
    unsafe extern "C" fn create(_: Ref, properties: Ref) -> Ref {
        CFRetain(properties)
    }
    unsafe extern "C" fn missing(_: Ref, _: Ref) -> Ref {
        ptr::null()
    }
    unsafe extern "C" fn report(_: Ref, bytes: *const u8, len: isize) -> i32 {
        assert!(!bytes.is_null());
        assert!(len > 0 && len <= 22);
        0
    }
    unsafe extern "C" fn fail_report(_: Ref, _: *const u8, _: isize) -> i32 {
        -1
    }
    #[test]
    fn cf_properties_and_injected_device_calls_cover_the_native_boundary_without_creating_hid() {
        assert!(Native::open().is_err());
        assert!(Object::new(ptr::null()).is_err());
        assert!(Object::string("bad\0value").is_err());
        let _ = supported_os();
        assert!(authorized().is_err());
        let mut n = Native {
            _lease: ownership().unwrap(),
            api: Api { create, report },
        };
        assert!(ownership().is_err());
        assert!(n.ready().is_err());
        let t = Template::new(binding(1)).unwrap();
        let mut d = n.create(&t).unwrap();
        let events = t
            .events(&t.binding.report(&t.binding.fallback()).unwrap())
            .unwrap();
        d.emit(&events).unwrap();
        assert!(d.emit(&[]).is_err());
        d.api.report = fail_report;
        assert!(d.emit(&events).is_err());
        drop(d);
        n.api.create = missing;
        assert!(n.create(&t).is_err());
        drop(n);
        drop(ownership().unwrap());
    }
}
