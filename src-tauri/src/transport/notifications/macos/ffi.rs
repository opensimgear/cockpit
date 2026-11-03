//! System ABI signatures mirrored from the existing serialport IOKit and
//! core-foundation-sys dependencies. No device properties are read here.
use std::ffi::{c_char, c_void};
pub type Pointer = *mut c_void;
#[repr(C)]
pub struct SourceContext {
    pub version: isize, pub info: Pointer,
    pub retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
    pub release: Option<extern "C" fn(*const c_void)>,
    pub description: Option<extern "C" fn(*const c_void) -> *const c_void>,
    pub equal: Option<extern "C" fn(*const c_void, *const c_void) -> u8>,
    pub hash: Option<extern "C" fn(*const c_void) -> usize>,
    pub schedule: Option<extern "C" fn(*const c_void, Pointer, *const c_void)>,
    pub cancel: Option<extern "C" fn(*const c_void, Pointer, *const c_void)>,
    pub perform: extern "C" fn(*const c_void),
}
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    pub fn IONotificationPortCreate(main_port: u32) -> Pointer;
    pub fn IONotificationPortDestroy(port: Pointer);
    pub fn IONotificationPortGetRunLoopSource(port: Pointer) -> Pointer;
    pub fn IOServiceMatching(name: *const c_char) -> Pointer;
    pub fn IOServiceAddMatchingNotification(port: Pointer, notification: *const c_char, matching: Pointer,
        callback: Option<extern "C" fn(Pointer, u32)>, context: Pointer, iterator: *mut u32) -> i32;
    pub fn IOIteratorNext(iterator: u32) -> u32;
    pub fn IOObjectRelease(object: u32) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub static kCFRunLoopDefaultMode: *const c_void;
    pub fn CFRunLoopGetCurrent() -> Pointer;
    pub fn CFRunLoopAddSource(run_loop: Pointer, source: Pointer, mode: *const c_void);
    pub fn CFRunLoopRemoveSource(run_loop: Pointer, source: Pointer, mode: *const c_void);
    pub fn CFRunLoopSourceCreate(allocator: *const c_void, order: isize, context: *mut SourceContext) -> Pointer;
    pub fn CFRunLoopSourceSignal(source: Pointer);
    pub fn CFRunLoopWakeUp(run_loop: Pointer);
    pub fn CFRunLoopRun();
    pub fn CFRunLoopStop(run_loop: Pointer);
    pub fn CFRetain(object: *const c_void) -> *const c_void;
    pub fn CFRelease(object: *const c_void);
}
