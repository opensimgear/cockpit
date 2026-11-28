use super::*;
use std::{cell::RefCell, ffi::c_void};
use windows_sys::{core::GUID, Win32::Security::SECURITY_ATTRIBUTES};

#[derive(Default)]
struct State {
    mode: &'static str,
    error: u32,
    count: u32,
    opened: usize,
    closed: usize,
    destroyed: usize,
    freed: usize,
    snapshot: usize,
    bad_access: bool,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
fn reset(mode: &'static str, count: u32) {
    STATE.with(|state| {
        *state.borrow_mut() = State {
            mode,
            count,
            ..Default::default()
        }
    });
}
unsafe extern "system" fn last_error() -> u32 {
    STATE.with(|state| state.borrow().error)
}
unsafe extern "system" fn guid(out: *mut GUID) {
    *out = std::mem::zeroed();
}
unsafe extern "system" fn classes(
    _: *const GUID,
    enumerator: *const u16,
    parent: HWND,
    flags: u32,
) -> isize {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.bad_access |= !enumerator.is_null()
            || !parent.is_null()
            || flags != DIGCF_PRESENT | DIGCF_DEVICEINTERFACE;
        if state.mode == "set-failed" {
            state.error = ERROR_ACCESS_DENIED;
            -1
        } else {
            42
        }
    })
}
unsafe extern "system" fn enumerate(
    _: isize,
    _: *const SP_DEVINFO_DATA,
    _: *const GUID,
    index: u32,
    out: *mut SP_DEVICE_INTERFACE_DATA,
) -> i32 {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.bad_access |= (*out).cbSize != size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
        if state.mode == "enum-failed" && index == 1 {
            state.error = ERROR_GEN_FAILURE;
            return 0;
        }
        if index >= state.count {
            state.error = ERROR_NO_MORE_ITEMS;
            return 0;
        }
        (*out).Reserved = index as usize + 1;
        1
    })
}
fn path_for(index: usize, duplicate: bool) -> String {
    if duplicate {
        return "\\\\?\\hid#identical".into();
    }
    match index {
        0 => "\\\\?\\hid#keyboard".into(),
        1 => "\\\\?\\hid#vendor".into(),
        2 => "\\\\?\\hid#restricted".into(),
        _ => format!("\\\\?\\hid#interface-{index}"),
    }
}
unsafe extern "system" fn detail(
    _: isize,
    interface: *const SP_DEVICE_INTERFACE_DATA,
    out: *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    capacity: u32,
    required: *mut u32,
    _: *mut SP_DEVINFO_DATA,
) -> i32 {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let index = (*interface).Reserved - 1;
        let mut units: Vec<_> = path_for(index, state.mode == "duplicate")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let offset = std::mem::offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath);
        *required = (offset + units.len() * 2) as u32;
        if out.is_null() {
            state.error = if state.mode == "detail-query-failed" {
                ERROR_ACCESS_DENIED
            } else {
                ERROR_INSUFFICIENT_BUFFER
            };
            if state.mode == "detail-size-small" {
                *required = 2;
            }
            if state.mode == "detail-size-big" {
                *required = MAX_DETAIL_BYTES + 2;
            }
            if state.mode == "detail-size-odd" {
                *required += 1;
            }
            return 0;
        }
        state.bad_access |= (*out).cbSize != size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        if state.mode == "detail-failed" {
            state.error = ERROR_DEVICE_NOT_CONNECTED;
            return 0;
        }
        if state.mode == "detail-raced" {
            *required = capacity + 2;
            return 1;
        }
        if state.mode == "detail-unterminated" {
            *units.last_mut().unwrap() = 65;
        }
        if state.mode == "detail-empty" {
            units[0] = 0;
        }
        if state.mode == "detail-unicode" {
            units[0] = 0xd800;
        }
        std::ptr::copy_nonoverlapping(
            units.as_ptr(),
            std::ptr::addr_of_mut!((*out).DevicePath).cast::<u16>(),
            units.len(),
        );
        1
    })
}
unsafe extern "system" fn destroy(_: isize) -> i32 {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.destroyed += 1;
        if state.mode == "cleanup-failed" {
            state.error = ERROR_GEN_FAILURE;
            0
        } else {
            1
        }
    })
}
unsafe extern "system" fn open(
    path: *const u16,
    access: u32,
    share: u32,
    security: *const SECURITY_ATTRIBUTES,
    creation: u32,
    flags: u32,
    template: HANDLE,
) -> HANDLE {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.bad_access |= access != 0
            || share != (FILE_SHARE_READ | FILE_SHARE_WRITE)
            || !security.is_null()
            || creation != OPEN_EXISTING
            || flags != 0
            || !template.is_null();
        let mut count = 0;
        while *path.add(count) != 0 {
            count += 1;
        }
        let name = String::from_utf16_lossy(std::slice::from_raw_parts(path, count));
        if name.ends_with("restricted") || state.mode == "open-failed" {
            state.error = ERROR_ACCESS_DENIED;
            return INVALID_HANDLE_VALUE;
        }
        state.opened += 1;
        state.opened as HANDLE
    })
}
unsafe extern "system" fn close(_: HANDLE) -> i32 {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.closed += 1;
        if state.mode == "cleanup-failed" {
            state.error = ERROR_GEN_FAILURE;
            0
        } else {
            1
        }
    })
}
unsafe extern "system" fn attributes(_: HANDLE, out: *mut HIDD_ATTRIBUTES) -> bool {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.bad_access |= (*out).Size != size_of::<HIDD_ATTRIBUTES>() as u32;
        if state.mode == "attrs-failed" {
            state.error = ERROR_GEN_FAILURE;
            return false;
        }
        (*out).VendorID = 0x1111;
        (*out).ProductID = 0x2222;
        (*out).VersionNumber = 0x3333;
        true
    })
}
unsafe extern "system" fn text(_: HANDLE, out: *mut c_void, length: u32) -> bool {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.bad_access |= length != 256;
        if state.mode == "string-failed" {
            state.error = ERROR_NOT_SUPPORTED;
            return false;
        }
        let units = std::slice::from_raw_parts_mut(out.cast::<u16>(), (length / 2) as usize);
        if state.mode == "string-unterminated" {
            units.fill(65);
        } else if state.mode == "string-unicode" {
            units[0] = 0xd800;
        } else if state.mode != "string-empty" {
            units[0] = 65;
        }
        true
    })
}
unsafe extern "system" fn preparsed(_: HANDLE, out: *mut isize) -> bool {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.mode == "null-preparsed" {
            *out = 0;
            true
        } else if matches!(
            state.mode,
            "preparsed-success" | "projection-panic" | "preparsed-records"
        ) {
            *out = 77;
            true
        } else {
            state.error = ERROR_ACCESS_DENIED;
            false
        }
    })
}
unsafe extern "system" fn free_preparsed(_: isize) -> bool {
    STATE.with(|state| state.borrow_mut().freed += 1);
    true
}
unsafe fn snapshot(_: isize) -> windows_caps::Snapshot {
    STATE.with(|state| state.borrow_mut().snapshot += 1);
    if STATE.with(|state| state.borrow().mode == "projection-panic") {
        panic!("projection panic fixture");
    }
    let mut snapshot = windows_caps::Snapshot::unavailable("fake parsed capability availability");
    if STATE.with(|state| state.borrow().mode == "preparsed-records") {
        snapshot.links = (0..3)
            .map(|index| windows_caps::Link {
                index,
                usage_page: 0xff00,
                usage: 1,
                parent: 0,
                number_of_children: 0,
                next_sibling: 0,
                first_child: 0,
                collection_type: 1,
                is_alias: false,
                raw_flags: 0,
            })
            .collect();
    }
    snapshot
}
fn api() -> Api {
    Api {
        guid,
        classes,
        enumerate,
        detail,
        destroy,
        open,
        close,
        attributes,
        manufacturer: text,
        product: text,
        serial: text,
        preparsed,
        free_preparsed,
        snapshot,
        last_error,
    }
}
fn run(mode: &'static str, count: u32) -> Inventory {
    reset(mode, count);
    unsafe { enumerate_with(&api()).unwrap() }
}

mod assertions;
