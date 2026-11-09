//! iOS discovery uses the system Bonjour service instead of raw multicast.
use super::{bonjour_record, discovery_result, MAX_HINTS};
use serde_json::Value;
use std::{
    collections::{btree_map::Entry, BTreeMap},
    ffi::{c_char, c_void, CStr, CString},
    io::ErrorKind,
    ptr::{self, NonNull},
    time::{Duration, Instant},
};

type RawRef = *mut c_void;
type BrowseReply = unsafe extern "C" fn(
    RawRef,
    u32,
    u32,
    i32,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_void,
);
type ResolveReply = unsafe extern "C" fn(
    RawRef,
    u32,
    u32,
    i32,
    *const c_char,
    *const c_char,
    u16,
    u16,
    *const u8,
    *mut c_void,
);

// The Apple SDK exposes DNS-SD through libSystem.
#[link(name = "System")]
unsafe extern "C" {
    fn DNSServiceCreateConnection(reference: *mut RawRef) -> i32;
    fn DNSServiceBrowse(
        reference: *mut RawRef,
        flags: u32,
        interface: u32,
        service_type: *const c_char,
        domain: *const c_char,
        callback: BrowseReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceResolve(
        reference: *mut RawRef,
        flags: u32,
        interface: u32,
        name: *const c_char,
        service_type: *const c_char,
        domain: *const c_char,
        callback: ResolveReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceRefSockFD(reference: RawRef) -> libc::c_int;
    fn DNSServiceProcessResult(reference: RawRef) -> i32;
    fn DNSServiceRefDeallocate(reference: RawRef);
}

const SHARE_CONNECTION: u32 = 0x4000;
const ADD: u32 = 0x2;
const POLICY_DENIED: i32 = -65570;
const BAD_REFERENCE: i32 = -65541;

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct Service {
    interface: u32,
    name: CString,
    service_type: CString,
    domain: CString,
}

#[derive(Default)]
struct Callbacks {
    generation: u64,
    visible: BTreeMap<Service, u64>,
    resolved: BTreeMap<RawRef, Option<(String, Value)>>,
    error: Option<i32>,
}

struct Reference(NonNull<c_void>);

impl Drop for Reference {
    fn drop(&mut self) {
        // The local owner drops subordinate operations before the shared reference.
        unsafe { DNSServiceRefDeallocate(self.0.as_ptr()) };
    }
}

impl Reference {
    fn from_raw(reference: RawRef) -> Result<Self, String> {
        NonNull::new(reference)
            .map(Self)
            .ok_or_else(|| "Missing Bonjour reference".into())
    }

    fn operation(&self, start: impl FnOnce(*mut RawRef) -> i32) -> Result<Self, i32> {
        let parent = self.0.as_ptr();
        let mut reference = parent;
        match start(&mut reference) {
            0 => NonNull::new(reference)
                .filter(|reference| reference.as_ptr() != parent)
                .map(Self)
                .ok_or(BAD_REFERENCE),
            error => Err(error),
        }
    }
}

enum Lookup {
    Resolving(Reference),
    Complete(Option<(String, Value)>),
}

pub(super) fn browse(duration: Duration) -> Result<Value, String> {
    let deadline = Instant::now() + duration;
    let mut callbacks = Box::<Callbacks>::default();
    let context = ptr::from_mut(callbacks.as_mut()).cast::<c_void>();
    let mut raw = ptr::null_mut();
    check(unsafe { DNSServiceCreateConnection(&mut raw) })?;
    let connection = Reference::from_raw(raw)?;
    let _browser = connection
        .operation(|reference| unsafe {
            DNSServiceBrowse(
                reference,
                SHARE_CONNECTION,
                0,
                c"_osg-companion._tcp".as_ptr(),
                c"local.".as_ptr(),
                browse_reply,
                context,
            )
        })
        .map_err(bonjour_error)?;
    let socket = unsafe { DNSServiceRefSockFD(raw) };
    if socket < 0 {
        return Err("Bonjour discovery socket is unavailable".into());
    }
    let mut lookups = BTreeMap::<Service, (u64, Lookup)>::new();
    while readable(socket, deadline)? {
        // Bonjour invokes all callbacks on this thread while processing the socket.
        check(unsafe { DNSServiceProcessResult(raw) })?;
        if let Some(error) = callbacks.error {
            return Err(bonjour_error(error));
        }
        lookups
            .retain(|service, (generation, _)| callbacks.visible.get(service) == Some(generation));
        for (_, lookup) in lookups.values_mut() {
            if let Lookup::Resolving(reference) = lookup {
                if let Some(result) = callbacks.resolved.remove(&reference.0.as_ptr()) {
                    *lookup = Lookup::Complete(result);
                }
            }
        }
        // Any replies for removed services belong to their retired references.
        callbacks.resolved.clear();
        for (service, generation) in &callbacks.visible {
            let Entry::Vacant(entry) = lookups.entry(service.clone()) else {
                continue;
            };
            let reference = connection.operation(|reference| unsafe {
                DNSServiceResolve(
                    reference,
                    SHARE_CONNECTION,
                    service.interface,
                    service.name.as_ptr(),
                    service.service_type.as_ptr(),
                    service.domain.as_ptr(),
                    resolve_reply,
                    context,
                )
            });
            let lookup = match reference {
                Ok(reference) => Lookup::Resolving(reference),
                Err(POLICY_DENIED) => return Err(bonjour_error(POLICY_DENIED)),
                Err(_) => Lookup::Complete(None),
            };
            entry.insert((*generation, lookup));
        }
    }
    let hints = lookups
        .into_values()
        .filter_map(|(_, lookup)| match lookup {
            Lookup::Complete(hint) => hint,
            Lookup::Resolving(_) => None,
        })
        .collect();
    Ok(discovery_result(hints))
}

fn readable(socket: libc::c_int, deadline: Instant) -> Result<bool, String> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        let mut descriptor = libc::pollfd {
            fd: socket,
            events: libc::POLLIN,
            revents: 0,
        };
        let timeout = i32::try_from(remaining.as_millis().saturating_add(1)).unwrap_or(i32::MAX);
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("Bonjour discovery wait failed: {error}"));
        }
        if result == 0 {
            return Ok(false);
        }
        if descriptor.revents & libc::POLLIN != 0 {
            return Ok(true);
        }
        return Err("Bonjour discovery socket closed".into());
    }
}

fn check(code: i32) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(bonjour_error(code))
    }
}

fn bonjour_error(code: i32) -> String {
    if code == POLICY_DENIED {
        "Allow Local Network access for OpenSimGear Companion in iOS Settings, then try discovery again.".into()
    } else {
        format!("Bonjour discovery failed ({code})")
    }
}

unsafe extern "C" fn browse_reply(
    _: RawRef,
    flags: u32,
    interface: u32,
    error: i32,
    name: *const c_char,
    service_type: *const c_char,
    domain: *const c_char,
    context: *mut c_void,
) {
    // The context box outlives every operation and is accessed only by this callback thread.
    let callbacks = unsafe { &mut *context.cast::<Callbacks>() };
    if error != 0 {
        callbacks.error = Some(error);
        return;
    }
    if name.is_null() || service_type.is_null() || domain.is_null() {
        return;
    }
    let service = Service {
        interface,
        name: unsafe { CStr::from_ptr(name) }.to_owned(),
        service_type: unsafe { CStr::from_ptr(service_type) }.to_owned(),
        domain: unsafe { CStr::from_ptr(domain) }.to_owned(),
    };
    if flags & ADD == 0 {
        callbacks.visible.remove(&service);
    } else if callbacks.visible.len() < MAX_HINTS {
        if let Entry::Vacant(entry) = callbacks.visible.entry(service) {
            callbacks.generation += 1;
            entry.insert(callbacks.generation);
        }
    }
}

unsafe extern "C" fn resolve_reply(
    reference: RawRef,
    _: u32,
    _: u32,
    error: i32,
    name: *const c_char,
    hostname: *const c_char,
    port: u16,
    txt_length: u16,
    txt: *const u8,
    context: *mut c_void,
) {
    let callbacks = unsafe { &mut *context.cast::<Callbacks>() };
    if error == POLICY_DENIED {
        callbacks.error = Some(error);
        return;
    }
    let hint = if error == 0 && !name.is_null() && !hostname.is_null() && !txt.is_null() {
        // These buffers are valid only during the callback. The hint owns its strings.
        let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
        let hostname = unsafe { CStr::from_ptr(hostname) }.to_string_lossy();
        let txt = unsafe { std::slice::from_raw_parts(txt, usize::from(txt_length)) };
        bonjour_record::hint(&name, &hostname, port, txt).map(|hint| (name.into_owned(), hint))
    } else {
        None
    };
    callbacks.resolved.entry(reference).or_insert(hint);
}
