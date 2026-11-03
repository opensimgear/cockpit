//! IOKit publishes/terminates serial BSD services on a dedicated CFRunLoop.
//! Draining matching iterators arms notifications; no port properties or
//! enumeration are read by callbacks.
use super::*;
use std::{ffi::c_void, sync::{atomic::{AtomicBool, Ordering}, Mutex}, thread::{self, JoinHandle}};
mod ffi;
use ffi::*;
const MAX_MATCHES: usize = 1024;

struct Context { wake: Arc<NativeWake>, closed: Arc<AtomicBool>, lost: AtomicBool, run_loop: Pointer }
impl Context {
    fn drain(&self, iterator: u32, mut next: impl FnMut(u32) -> u32, mut release: impl FnMut(u32)) -> bool {
        let mut changed = false;
        for _ in 0..MAX_MATCHES {
            let service = next(iterator);
            if service == 0 { return changed; }
            release(service); changed = true;
        }
        self.lost.store(true, Ordering::Release);
        changed
    }
}
extern "C" fn matched(context: Pointer, iterator: u32) {
    // IOKit invokes this only on the owning run loop; Native's teardown removes
    // its source/registrations before the stable boxed context is released.
    let context = unsafe { &*context.cast::<Context>() };
    let changed = context.drain(iterator, |iterator| unsafe { IOIteratorNext(iterator) }, |service| { unsafe { IOObjectRelease(service); } });
    if !context.closed.load(Ordering::Acquire) && changed { context.wake.notify(); }
    if context.lost.load(Ordering::Acquire) { unsafe { CFRunLoopStop(context.run_loop); } }
}
extern "C" fn cancel(context: *const c_void) {
    let context = unsafe { &*context.cast::<Context>() };
    if context.closed.load(Ordering::Acquire) { unsafe { CFRunLoopStop(context.run_loop); } }
}

/// Cross-thread signaling handles are independently retained until after join.
#[derive(Clone, Copy)]
struct Control { run_loop: usize, cancel: usize }
impl Control {
    fn signal(self) { unsafe { CFRunLoopSourceSignal(self.cancel as Pointer); CFRunLoopWakeUp(self.run_loop as Pointer); } }
    fn release(self) { unsafe { CFRelease(self.cancel as Pointer); CFRelease(self.run_loop as Pointer); } }
}
struct Native { port: Pointer, source: Pointer, cancel: Pointer, run_loop: Pointer, iterators: Vec<u32>, context: Box<Context> }
impl Native {
    fn open(wake: Arc<NativeWake>, closed: Arc<AtomicBool>) -> Result<Self, String> {
        unsafe {
            let run_loop = CFRunLoopGetCurrent();
            let port = IONotificationPortCreate(0);
            if port.is_null() { return Err("Cannot create the IOKit serial notification port.".into()); }
            let mut native = Self { port, source: std::ptr::null_mut(), cancel: std::ptr::null_mut(), run_loop,
                iterators: vec![], context: Box::new(Context { wake, closed, lost: AtomicBool::new(false), run_loop }) };
            native.source = IONotificationPortGetRunLoopSource(port);
            if native.source.is_null() { return Err("IOKit serial notifications have no run-loop source.".into()); }
            let context = (&mut *native.context as *mut Context).cast();
            let mut source_context = SourceContext { version: 0, info: context, retain: None, release: None,
                description: None, equal: None, hash: None, schedule: None, cancel: None, perform: cancel };
            native.cancel = CFRunLoopSourceCreate(std::ptr::null(), 0, &mut source_context);
            if native.cancel.is_null() { return Err("Cannot create IOKit serial notification cancellation.".into()); }
            CFRunLoopAddSource(run_loop, native.source, kCFRunLoopDefaultMode);
            CFRunLoopAddSource(run_loop, native.cancel, kCFRunLoopDefaultMode);
            for event in [c"IOServiceFirstMatch", c"IOServiceTerminate"] {
                let matching = IOServiceMatching(c"IOSerialBSDClient".as_ptr());
                if matching.is_null() { return Err("Cannot create IOKit serial-device matching.".into()); }
                let mut iterator = 0;
                // IOKit consumes the matching dictionary, including failures.
                let result = IOServiceAddMatchingNotification(port, event.as_ptr(), matching, Some(matched), context, &mut iterator);
                if result != 0 { return Err(format!("Cannot register IOKit serial notifications ({result}).")); }
                native.iterators.push(iterator);
                // Initial iterator consumption is mandatory to arm subsequent
                // events. The root performs the authoritative serial baseline.
                native.context.drain(iterator, |value| IOIteratorNext(value), |value| { IOObjectRelease(value); });
                if native.context.lost.load(Ordering::Acquire) { return Err("IOKit serial notification baseline exceeds its bound.".into()); }
            }
            Ok(native)
        }
    }
    fn control(&self) -> Control { unsafe {
        CFRetain(self.run_loop); CFRetain(self.cancel);
        Control { run_loop: self.run_loop as usize, cancel: self.cancel as usize }
    } }
}
impl Drop for Native {
    fn drop(&mut self) { unsafe {
        if !self.source.is_null() { CFRunLoopRemoveSource(self.run_loop, self.source, kCFRunLoopDefaultMode); }
        if !self.cancel.is_null() { CFRunLoopRemoveSource(self.run_loop, self.cancel, kCFRunLoopDefaultMode); CFRelease(self.cancel); }
        for iterator in self.iterators.drain(..) { IOObjectRelease(iterator); }
        IONotificationPortDestroy(self.port);
    } }
}

pub(super) struct Serial { closed: Arc<AtomicBool>, control: Option<Control>, worker: Option<JoinHandle<()>>, error: Arc<Mutex<Option<String>>> }
impl Serial {
    pub(super) fn start(wake: Arc<NativeWake>) -> Result<Self, String> {
        let closed = Arc::new(AtomicBool::new(false)); let stop = closed.clone();
        let error = Arc::new(Mutex::new(None)); let failure = error.clone();
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new().name("serial-port-events".into()).spawn(move || {
            let native = match Native::open(wake.clone(), stop.clone()) {
                Ok(native) => native,
                Err(error) => { let _ = ready.send(Err(error)); return; },
            };
            let control = native.control();
            if ready.send(Ok(control)).is_err() { control.release(); return; }
            // A signaled source survives cancellation before Run begins. A
            // bare CFRunLoopStop before Run would not provide that fence.
            unsafe { CFRunLoopRun(); }
            if native.context.lost.load(Ordering::Acquire) || !stop.load(Ordering::Acquire) {
                *failure.lock().unwrap_or_else(|p|p.into_inner()) = Some("IOKit serial notification continuity was lost. Restart the workspace observer.".into());
                wake.notify();
            }
        }).map_err(|_| "Cannot start the IOKit serial notification owner.")?;
        match started.recv() {
            Ok(Ok(control)) => Ok(Self { closed, control: Some(control), worker: Some(worker), error }),
            Ok(Err(error)) => { let _ = worker.join(); Err(error) },
            Err(_) => { let _ = worker.join(); Err("IOKit serial notifications stopped during startup.".into()) },
        }
    }
    pub(super) fn error(&self) -> Option<String> { self.error.lock().unwrap_or_else(|p|p.into_inner()).clone() }
    pub(super) fn shutdown(&mut self) -> Result<(), String> {
        self.closed.store(true, Ordering::Release);
        if let Some(control) = self.control { control.signal(); }
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
        if let Some(control) = self.control.take() { control.release(); }
        Ok(())
    }
}
impl Drop for Serial { fn drop(&mut self) { let _ = self.shutdown(); } }

#[cfg(test)]
mod tests;
