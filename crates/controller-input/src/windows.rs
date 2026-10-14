//! Dedicated background Raw Input owner. Discovery does not acquire controllers or modify HID policy.
use crate::registry::{Control, Definition, Identity, Kind, Registry, MAX_CONTROLS};
use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use windows_sys::Win32::{
    Devices::HumanInterfaceDevice::*,
    Foundation::*,
    Storage::FileSystem::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::*, WindowsAndMessaging::*},
};

// Keep Windows calls at an explicit boundary so authored descriptors, reports,
// enumeration races and API failures exercise the production parser in native tests.
struct Api {
    info: unsafe extern "system" fn(HANDLE, u32, *mut core::ffi::c_void, *mut u32) -> u32,
    list: unsafe extern "system" fn(*mut RAWINPUTDEVICELIST, *mut u32, u32) -> u32,
    raw: unsafe extern "system" fn(HRAWINPUT, u32, *mut core::ffi::c_void, *mut u32, u32) -> u32,
    caps: unsafe extern "system" fn(isize, *mut HIDP_CAPS) -> i32,
    buttons: unsafe extern "system" fn(i32, *mut HIDP_BUTTON_CAPS, *mut u16, isize) -> i32,
    values: unsafe extern "system" fn(i32, *mut HIDP_VALUE_CAPS, *mut u16, isize) -> i32,
    data: unsafe extern "system" fn(i32, *mut HIDP_DATA, *mut u32, isize, *mut u8, u32) -> i32,
    open: unsafe extern "system" fn(
        *const u16,
        u32,
        u32,
        *const windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
        u32,
        u32,
        HANDLE,
    ) -> HANDLE,
    product: unsafe extern "system" fn(HANDLE, *mut core::ffi::c_void, u32) -> bool,
    serial: unsafe extern "system" fn(HANDLE, *mut core::ffi::c_void, u32) -> bool,
    close: unsafe extern "system" fn(HANDLE) -> i32,
}
impl Api {
    fn native() -> Self {
        Self {
            info: GetRawInputDeviceInfoW,
            list: GetRawInputDeviceList,
            raw: GetRawInputData,
            caps: HidP_GetCaps,
            buttons: HidP_GetButtonCaps,
            values: HidP_GetValueCaps,
            data: HidP_GetData,
            open: CreateFileW,
            product: HidD_GetProductString,
            serial: HidD_GetSerialNumberString,
            close: CloseHandle,
        }
    }
}
type Shared = Arc<Mutex<Registry>>;
struct Device {
    path: String,
    preparsed: Vec<usize>,
    report_len: usize,
    controls: Vec<Control>,
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn win_error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, message, wparam, lparam)
}
/// Only these top-level application collections are registered; keyboards/mice are never read.
fn eligible(page: u16, usage: u16, vid: u16, pid: u16) -> bool {
    page == 1 && [4, 5, 8].contains(&usage) && !(vid == 0x1234 && pid == 0xbead)
}
fn words(bytes: usize) -> Vec<usize> {
    vec![0; bytes.div_ceil(size_of::<usize>())]
}
unsafe fn device(api: &Api, handle: HANDLE) -> Result<Option<(Device, Definition)>, String> {
    let mut info = RID_DEVICE_INFO::default();
    info.cbSize = size_of::<RID_DEVICE_INFO>() as u32;
    let mut length = info.cbSize;
    if (api.info)(
        handle,
        RIDI_DEVICEINFO,
        (&mut info as *mut RID_DEVICE_INFO).cast(),
        &mut length,
    ) == u32::MAX
    {
        return Err(win_error("Controller information"));
    }
    if info.dwType != RIM_TYPEHID {
        return Ok(None);
    }
    let hid = info.Anonymous.hid;
    if !eligible(
        hid.usUsagePage,
        hid.usUsage,
        hid.dwVendorId as u16,
        hid.dwProductId as u16,
    ) {
        return Ok(None);
    }
    length = 0;
    if (api.info)(handle, RIDI_DEVICENAME, null_mut(), &mut length) == u32::MAX
        || length == 0
        || length > 2048
    {
        return Err("Invalid HID path length.".into());
    }
    let mut path = vec![0u16; length as usize + 1];
    if (api.info)(
        handle,
        RIDI_DEVICENAME,
        path.as_mut_ptr().cast(),
        &mut length,
    ) == u32::MAX
    {
        return Err(win_error("HID path"));
    }
    let path_text =
        String::from_utf16_lossy(&path[..path.iter().position(|c| *c == 0).unwrap_or(path.len())]);
    length = 0;
    if (api.info)(handle, RIDI_PREPARSEDDATA, null_mut(), &mut length) == u32::MAX
        || length == 0
        || length > 262144
    {
        return Err("Invalid preparsed HID size.".into());
    }
    let mut preparsed = words(length as usize);
    if (api.info)(
        handle,
        RIDI_PREPARSEDDATA,
        preparsed.as_mut_ptr().cast(),
        &mut length,
    ) == u32::MAX
    {
        return Err(win_error("HID descriptor"));
    }
    let pp = preparsed.as_ptr() as isize;
    let mut caps = HIDP_CAPS::default();
    if (api.caps)(pp, &mut caps) != HIDP_STATUS_SUCCESS {
        return Err("HidP_GetCaps rejected the descriptor.".into());
    }
    let controls = parse_caps(api, pp, &caps)?;
    let mut name = format!("Controller {:04X}:{:04X}", hid.dwVendorId, hid.dwProductId);
    let mut serial = None;
    let mut diagnostics = Vec::new();
    // Zero desired access requests descriptor metadata only, not exclusive input access.
    let file = (api.open)(
        path.as_ptr(),
        0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        null(),
        OPEN_EXISTING,
        0,
        null_mut(),
    );
    if file != INVALID_HANDLE_VALUE {
        let mut buffer = [0u16; 128];
        if (api.product)(file, buffer.as_mut_ptr().cast(), 256) {
            let s = String::from_utf16_lossy(
                &buffer[..buffer.iter().position(|c| *c == 0).unwrap_or(128)],
            );
            if !s.trim().is_empty() {
                name = s;
            }
        }
        buffer.fill(0);
        if (api.serial)(file, buffer.as_mut_ptr().cast(), 256) {
            let s = String::from_utf16_lossy(
                &buffer[..buffer.iter().position(|c| *c == 0).unwrap_or(128)],
            );
            if !s.trim().is_empty() {
                serial = Some(s);
            }
        }
        (api.close)(file);
    }
    if serial.is_none() {
        diagnostics.push("No reliable serial is exposed. After a USB port change, explicitly reassign the remembered controller.".into());
    }
    let unknown = controls
        .iter()
        .filter(|c| c.kind == Kind::Diagnostic)
        .count();
    if unknown > 0 {
        diagnostics.push(format!(
            "{unknown} unknown, relative, or array fields are diagnostic only."
        ));
    }
    // The hardware component contains MI_/COL identifiers but no location/serial suffix.
    let collection = format!(
        "{:04x}:{:04x}:{}",
        caps.UsagePage,
        caps.Usage,
        path_text
            .split('#')
            .nth(1)
            .unwrap_or("")
            .to_ascii_lowercase()
    );
    let definition = Definition {
        identity: Identity {
            backend: "windows-raw-input".into(),
            vendor_id: hid.dwVendorId as u16,
            product_id: hid.dwProductId as u16,
            serial,
            collection,
            fingerprint: String::new(),
            path: path_text.clone(),
        },
        name,
        controls: controls.clone(),
        diagnostics,
    };
    Ok(Some((
        Device {
            path: path_text,
            preparsed,
            report_len: caps.InputReportByteLength as usize,
            controls,
        },
        definition,
    )))
}
unsafe fn parse_caps(api: &Api, pp: isize, caps: &HIDP_CAPS) -> Result<Vec<Control>, String> {
    if caps.NumberInputButtonCaps as usize > MAX_CONTROLS
        || caps.NumberInputValueCaps as usize > MAX_CONTROLS
        || caps.InputReportByteLength == 0
        || caps.InputReportByteLength > 4096
    {
        return Err("Controller descriptor exceeds capture bounds.".into());
    }
    let mut controls = Vec::new();
    let mut count = caps.NumberInputButtonCaps;
    let mut buttons = vec![HIDP_BUTTON_CAPS::default(); count as usize];
    if count > 0
        && (api.buttons)(HidP_Input, buttons.as_mut_ptr(), &mut count, pp) != HIDP_STATUS_SUCCESS
    {
        return Err("Cannot read HID button capabilities.".into());
    }
    for c in buttons.iter().take(count as usize).filter(|c| !c.IsAlias) {
        let (usage, index, end) = if c.IsRange {
            let r = c.Anonymous.Range;
            (r.UsageMin, r.DataIndexMin, r.DataIndexMax)
        } else {
            let r = c.Anonymous.NotRange;
            (r.Usage, r.DataIndex, r.DataIndex)
        };
        if end < index || usize::from(end - index) + controls.len() + 1 > MAX_CONTROLS {
            return Err("Too many HID button fields.".into());
        }
        for field in index..=end {
            let mut control = Control::new(
                c.UsagePage,
                usage
                    .checked_add(field - index)
                    .ok_or("Invalid usage range.")?,
                c.ReportID,
                c.LinkCollection,
                field,
            );
            if c.UsagePage != 9 {
                control.kind = Kind::Diagnostic;
            }
            controls.push(control);
        }
    }
    count = caps.NumberInputValueCaps;
    let mut values = vec![HIDP_VALUE_CAPS::default(); count as usize];
    if count > 0
        && (api.values)(HidP_Input, values.as_mut_ptr(), &mut count, pp) != HIDP_STATUS_SUCCESS
    {
        return Err("Cannot read HID value capabilities.".into());
    }
    for c in values.iter().take(count as usize).filter(|c| !c.IsAlias) {
        let (usage, index, end) = if c.IsRange {
            let r = c.Anonymous.Range;
            (r.UsageMin, r.DataIndexMin, r.DataIndexMax)
        } else {
            let r = c.Anonymous.NotRange;
            (r.Usage, r.DataIndex, r.DataIndex)
        };
        if end < index || usize::from(end - index) + controls.len() + 1 > MAX_CONTROLS {
            return Err("Too many HID value fields.".into());
        }
        for field in index..=end {
            let mut control = Control::new(
                c.UsagePage,
                usage
                    .checked_add(field - index)
                    .ok_or("Invalid usage range.")?,
                c.ReportID,
                c.LinkCollection,
                field,
            );
            control.bits = c.BitSize;
            control.min = i64::from(c.LogicalMin);
            control.max = if c.LogicalMin < 0 {
                i64::from(c.LogicalMax)
            } else {
                i64::from(c.LogicalMax as u32)
            };
            control.has_null = c.HasNull;
            if !c.IsAbsolute || (!c.IsRange && c.ReportCount > 1) {
                control.kind = Kind::Diagnostic;
            }
            controls.push(control);
        }
    }
    Ok(controls)
}
unsafe fn enumerate(
    api: &Api,
    devices: &mut BTreeMap<usize, Device>,
    shared: &Shared,
) -> Result<(), String> {
    let mut count = 0;
    if (api.list)(
        null_mut(),
        &mut count,
        size_of::<RAWINPUTDEVICELIST>() as u32,
    ) == u32::MAX
        || count > 1024
    {
        return Err(win_error("Raw Input enumeration"));
    }
    let mut list = vec![RAWINPUTDEVICELIST::default(); count as usize];
    let n = (api.list)(
        list.as_mut_ptr(),
        &mut count,
        size_of::<RAWINPUTDEVICELIST>() as u32,
    );
    if n == u32::MAX {
        return Err(win_error("Raw Input device list changed; retrying"));
    }
    let handles: BTreeSet<_> = list
        .iter()
        .take(n as usize)
        .map(|d| d.hDevice as usize)
        .collect();
    devices.retain(|handle, device| {
        if handles.contains(handle) {
            true
        } else {
            let mut registry = shared.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(id) = registry.id_for_path(&device.path) {
                registry.disconnect(&id);
            }
            false
        }
    });
    for item in list
        .iter()
        .take(n as usize)
        .filter(|d| d.dwType == RIM_TYPEHID)
    {
        if devices.contains_key(&(item.hDevice as usize)) {
            continue;
        }
        match device(api, item.hDevice) {
            Ok(Some((device, definition))) => {
                let mut r = shared.lock().unwrap_or_else(|p| p.into_inner());
                match r.connect(definition) {
                    Ok(_) => {
                        devices.insert(item.hDevice as usize, device);
                    }
                    Err(e) => r.diagnostic = e,
                }
            }
            Ok(None) => (),
            Err(e) => shared.lock().unwrap_or_else(|p| p.into_inner()).diagnostic = e,
        }
    }
    Ok(())
}
unsafe fn input(
    api: &Api,
    raw: HRAWINPUT,
    devices: &BTreeMap<usize, Device>,
    shared: &Shared,
) -> Result<(), String> {
    let header_size = size_of::<RAWINPUTHEADER>() as u32;
    let mut bytes = 0;
    if (api.raw)(raw, RID_INPUT, null_mut(), &mut bytes, header_size) == u32::MAX
        || bytes < header_size + 8
        || bytes > 262144
    {
        return Err("Invalid Raw Input buffer length.".into());
    }
    let mut storage = words(bytes as usize);
    if (api.raw)(
        raw,
        RID_INPUT,
        storage.as_mut_ptr().cast(),
        &mut bytes,
        header_size,
    ) != bytes
        || bytes < header_size + 8
        || bytes as usize > storage.len() * size_of::<usize>()
    {
        return Err(win_error("Raw Input report"));
    }
    let header = &*(storage.as_ptr() as *const RAWINPUTHEADER);
    if header.dwType != RIM_TYPEHID {
        return Ok(());
    }
    let Some(device) = devices.get(&(header.hDevice as usize)) else {
        return Ok(());
    };
    let raw = std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), bytes as usize);
    let payload = &raw[header_size as usize..];
    let size = u32::from_ne_bytes(payload[..4].try_into().unwrap()) as usize;
    let count = u32::from_ne_bytes(payload[4..8].try_into().unwrap()) as usize;
    if size != device.report_len
        || size == 0
        || count > 4096
        || size
            .checked_mul(count)
            .is_none_or(|n| n > payload.len() - 8)
    {
        return Err("Malformed HID report batch.".into());
    }
    for report in payload[8..8 + size * count].chunks_exact(size) {
        let mut report = report.to_vec();
        let mut data = vec![HIDP_DATA::default(); MAX_CONTROLS];
        let mut length = data.len() as u32;
        let status = (api.data)(
            HidP_Input,
            data.as_mut_ptr(),
            &mut length,
            device.preparsed.as_ptr() as isize,
            report.as_mut_ptr(),
            report.len() as u32,
        );
        let mut r = shared.lock().unwrap_or_else(|p| p.into_inner());
        let Some(id) = r.id_for_path(&device.path) else {
            continue;
        };
        if status != HIDP_STATUS_SUCCESS || length as usize > MAX_CONTROLS {
            r.fault(
                &id,
                "HidP report decoding failed; waiting for a fresh baseline.",
            );
            continue;
        }
        let values = data
            .iter()
            .take(length as usize)
            .map(|d| (d.DataIndex, d.Anonymous.RawValue))
            .collect();
        let report_id = if device.controls.iter().any(|c| c.report_id != 0) {
            report[0]
        } else {
            0
        };
        if let Err(error) = r.report(&id, report_id, &values) {
            r.fault(&id, &error);
        }
    }
    Ok(())
}
pub fn run(shared: Shared, stop: Arc<AtomicBool>) -> Result<(), String> {
    let api = Api::native();
    unsafe {
        let class = wide(&format!("OpenSimGearInput-{}", uuid::Uuid::new_v4()));
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(win_error("Register controller input window"));
        }
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            UnregisterClassW(class.as_ptr(), instance);
            return Err(win_error("Create controller input window"));
        }
        let registrations: Vec<_> = [4, 5, 8]
            .iter()
            .map(|usage| RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: *usage,
                dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                hwndTarget: window,
            })
            .collect();
        if RegisterRawInputDevices(
            registrations.as_ptr(),
            3,
            size_of::<RAWINPUTDEVICE>() as u32,
        ) == 0
        {
            DestroyWindow(window);
            UnregisterClassW(class.as_ptr(), instance);
            return Err(win_error("Register background controller capture"));
        }
        let mut devices = BTreeMap::new();
        let mut rescan = true;
        let mut next_scan = std::time::Instant::now();
        shared
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .backend_status = "ready".into();
        while !stop.load(Ordering::Acquire) {
            if rescan || std::time::Instant::now() >= next_scan {
                next_scan = std::time::Instant::now() + std::time::Duration::from_secs(2);
                match enumerate(&api, &mut devices, &shared) {
                    Ok(()) => rescan = false,
                    Err(e) => shared.lock().unwrap_or_else(|p| p.into_inner()).diagnostic = e,
                }
            }
            let mut message = MSG::default();
            for _ in 0..256 {
                if PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) == 0 {
                    break;
                }
                if message.message == WM_INPUT {
                    if let Err(e) = input(&api, message.lParam as HRAWINPUT, &devices, &shared) {
                        let mut r = shared.lock().unwrap_or_else(|p| p.into_inner());
                        // A loss that cannot be attributed invalidates all observed baselines.
                        let ids: Vec<_> = devices
                            .values()
                            .filter_map(|d| r.id_for_path(&d.path))
                            .collect();
                        for id in ids {
                            r.fault(&id, &e);
                        }
                    }
                } else if message.message == WM_INPUT_DEVICE_CHANGE {
                    if message.wParam == GIDC_REMOVAL as usize {
                        if let Some(device) = devices.remove(&(message.lParam as usize)) {
                            let mut r = shared.lock().unwrap_or_else(|p| p.into_inner());
                            if let Some(id) = r.id_for_path(&device.path) {
                                r.disconnect(&id);
                            }
                        }
                    }
                    rescan = true;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            MsgWaitForMultipleObjectsEx(0, null(), 16, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
        let removal: Vec<_> = [4, 5, 8]
            .iter()
            .map(|usage| RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: *usage,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: null_mut(),
            })
            .collect();
        RegisterRawInputDevices(removal.as_ptr(), 3, size_of::<RAWINPUTDEVICE>() as u32);
        DestroyWindow(window);
        UnregisterClassW(class.as_ptr(), instance);
        Ok(())
    }
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
