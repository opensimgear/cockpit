use super::*;
use std::cell::RefCell;

struct Fixture {
    mode: &'static str,
    present: bool,
    kind: u32,
    vid: u32,
    usage: u16,
    buttons: Vec<HIDP_BUTTON_CAPS>,
    values: Vec<HIDP_VALUE_CAPS>,
    packet: Vec<u8>,
    closed: usize,
    write_requested: bool,
}
fn value(usage: u16, index: u16, report: u8) -> HIDP_VALUE_CAPS {
    let mut c = HIDP_VALUE_CAPS::default();
    c.UsagePage = 1;
    c.ReportID = report;
    c.IsAbsolute = true;
    c.BitSize = 16;
    c.ReportCount = 1;
    c.LogicalMin = -32768;
    c.LogicalMax = 32767;
    c.Anonymous.NotRange = HIDP_VALUE_CAPS_0_1 {
        Usage: usage,
        DataIndex: index,
        ..Default::default()
    };
    c
}
fn packet(reports: &[[u8; 4]]) -> Vec<u8> {
    let size = size_of::<RAWINPUTHEADER>() + 8 + 4 * reports.len();
    let header = RAWINPUTHEADER {
        dwType: RIM_TYPEHID,
        dwSize: size as u32,
        hDevice: 1usize as HANDLE,
        wParam: 0,
    };
    let mut bytes = unsafe {
        std::slice::from_raw_parts(
            (&header as *const RAWINPUTHEADER).cast::<u8>(),
            size_of::<RAWINPUTHEADER>(),
        )
    }
    .to_vec();
    bytes.extend_from_slice(&4u32.to_ne_bytes());
    bytes.extend_from_slice(&(reports.len() as u32).to_ne_bytes());
    for report in reports {
        bytes.extend_from_slice(report);
    }
    bytes
}
impl Default for Fixture {
    fn default() -> Self {
        let mut buttons = HIDP_BUTTON_CAPS::default();
        buttons.UsagePage = 9;
        buttons.ReportID = 1;
        buttons.IsRange = true;
        buttons.Anonymous.Range = HIDP_BUTTON_CAPS_0_0 {
            UsageMin: 1,
            UsageMax: 2,
            DataIndexMin: 0,
            DataIndexMax: 1,
            ..Default::default()
        };
        let mut hat = value(0x39, 3, 2);
        hat.LogicalMin = 1;
        hat.LogicalMax = 8;
        hat.BitSize = 4;
        hat.HasNull = true;
        hat.LinkCollection = 1;
        let mut unknown = value(2, 4, 1);
        unknown.UsagePage = 0xff00;
        Self {
            mode: "",
            present: true,
            kind: RIM_TYPEHID,
            vid: 0x1111,
            usage: 4,
            buttons: vec![buttons],
            values: vec![value(0x30, 2, 1), hat, unknown],
            packet: packet(&[[1, 0xff, 0xff, 0], [2, 0, 0, 0]]),
            closed: 0,
            write_requested: false,
        }
    }
}
thread_local! {static FIXTURE:RefCell<Fixture>=RefCell::new(Fixture::default());}
fn reset(mode: &'static str) {
    FIXTURE.with(|f| {
        *f.borrow_mut() = Fixture::default();
        f.borrow_mut().mode = mode;
    });
}
unsafe extern "system" fn info(
    _handle: HANDLE,
    command: u32,
    output: *mut core::ffi::c_void,
    length: *mut u32,
) -> u32 {
    FIXTURE.with(|f| {
        let f = f.borrow();
        match command {
            RIDI_DEVICEINFO => {
                if f.mode == "info-error" {
                    return u32::MAX;
                }
                let o = &mut *output.cast::<RID_DEVICE_INFO>();
                o.dwType = f.kind;
                o.Anonymous.hid = RID_DEVICE_INFO_HID {
                    dwVendorId: f.vid,
                    dwProductId: 0xbead,
                    usUsagePage: 1,
                    usUsage: f.usage,
                    ..Default::default()
                };
                *length
            }
            RIDI_DEVICENAME => {
                let text = wide("\\\\?\\hid#vid_1111&pid_bead&mi_00&col01#fixture#{guid}");
                if output.is_null() {
                    *length = match f.mode {
                        "path-zero" => 0,
                        "path-long" => 2049,
                        _ => text.len() as u32,
                    };
                    if f.mode == "path-query" {
                        u32::MAX
                    } else {
                        0
                    }
                } else if f.mode == "path-read" {
                    u32::MAX
                } else {
                    std::ptr::copy_nonoverlapping(text.as_ptr(), output.cast(), text.len());
                    text.len() as u32
                }
            }
            _ => {
                if output.is_null() {
                    *length = if f.mode == "preparse-large" {
                        262145
                    } else {
                        8
                    };
                    if f.mode == "preparse-query" {
                        u32::MAX
                    } else {
                        0
                    }
                } else if f.mode == "preparse-read" {
                    u32::MAX
                } else {
                    8
                }
            }
        }
    })
}
unsafe extern "system" fn list(
    output: *mut RAWINPUTDEVICELIST,
    count: *mut u32,
    _size: u32,
) -> u32 {
    FIXTURE.with(|f| {
        let f = f.borrow();
        if f.mode == "list-error" {
            return u32::MAX;
        }
        if output.is_null() {
            *count = if f.mode == "list-large" {
                1025
            } else {
                u32::from(f.present)
            };
            return 0;
        }
        if f.mode == "list-race" {
            return u32::MAX;
        }
        if f.present {
            *output = RAWINPUTDEVICELIST {
                hDevice: 1usize as HANDLE,
                dwType: RIM_TYPEHID,
            };
            1
        } else {
            0
        }
    })
}
unsafe extern "system" fn raw(
    _handle: HRAWINPUT,
    _command: u32,
    output: *mut core::ffi::c_void,
    size: *mut u32,
    _header: u32,
) -> u32 {
    FIXTURE.with(|f| {
        let f = f.borrow();
        if output.is_null() {
            *size = match f.mode {
                "raw-short" => 1,
                "raw-large" => 262145,
                _ => f.packet.len() as u32,
            };
            if f.mode == "raw-query" {
                u32::MAX
            } else {
                0
            }
        } else if f.mode == "raw-shrink" {
            *size = 1;
            1
        } else if f.mode == "raw-read" {
            u32::MAX
        } else {
            std::ptr::copy_nonoverlapping(f.packet.as_ptr(), output.cast(), f.packet.len());
            f.packet.len() as u32
        }
    })
}
unsafe extern "system" fn caps(_pp: isize, output: *mut HIDP_CAPS) -> i32 {
    FIXTURE.with(|f| {
        let f = f.borrow();
        if f.mode == "caps-error" {
            return HIDP_STATUS_INVALID_PREPARSED_DATA;
        }
        (*output).UsagePage = 1;
        (*output).Usage = 4;
        (*output).InputReportByteLength = if f.mode == "report-large" { 4097 } else { 4 };
        (*output).NumberInputButtonCaps = if f.mode == "caps-large" {
            257
        } else {
            f.buttons.len() as u16
        };
        (*output).NumberInputValueCaps = f.values.len() as u16;
        HIDP_STATUS_SUCCESS
    })
}
unsafe extern "system" fn buttons(
    _kind: i32,
    output: *mut HIDP_BUTTON_CAPS,
    count: *mut u16,
    _pp: isize,
) -> i32 {
    FIXTURE.with(|f| {
        let f = f.borrow();
        if f.mode == "buttons-error" {
            return HIDP_STATUS_INVALID_PREPARSED_DATA;
        }
        std::ptr::copy_nonoverlapping(f.buttons.as_ptr(), output, f.buttons.len());
        *count = f.buttons.len() as u16;
        HIDP_STATUS_SUCCESS
    })
}
unsafe extern "system" fn values(
    _kind: i32,
    output: *mut HIDP_VALUE_CAPS,
    count: *mut u16,
    _pp: isize,
) -> i32 {
    FIXTURE.with(|f| {
        let f = f.borrow();
        if f.mode == "values-error" {
            return HIDP_STATUS_INVALID_PREPARSED_DATA;
        }
        std::ptr::copy_nonoverlapping(f.values.as_ptr(), output, f.values.len());
        *count = f.values.len() as u16;
        HIDP_STATUS_SUCCESS
    })
}
unsafe extern "system" fn data(
    _kind: i32,
    output: *mut HIDP_DATA,
    count: *mut u32,
    _pp: isize,
    report: *mut u8,
    _length: u32,
) -> i32 {
    FIXTURE.with(|f| {
        let f = f.borrow();
        if f.mode == "data-error" {
            return HIDP_STATUS_INVALID_REPORT_LENGTH;
        }
        if f.mode == "data-large" {
            *count = 257;
            return HIDP_STATUS_SUCCESS;
        }
        let values = if *report == 2 {
            vec![(3, if f.mode == "hat-invalid" { 14 } else { 0 })]
        } else {
            vec![(0, 1), (2, 0xffff), (4, 7)]
        };
        *count = values.len() as u32;
        for (index, (field, value)) in values.into_iter().enumerate() {
            *output.add(index) = HIDP_DATA {
                DataIndex: field,
                Reserved: 0,
                Anonymous: HIDP_DATA_0 { RawValue: value },
            };
        }
        HIDP_STATUS_SUCCESS
    })
}
unsafe extern "system" fn open(
    _path: *const u16,
    access: u32,
    _share: u32,
    _security: *const windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
    _creation: u32,
    _flags: u32,
    _template: HANDLE,
) -> HANDLE {
    FIXTURE.with(|f| {
        let mut f = f.borrow_mut();
        f.write_requested = access != 0;
        if f.mode == "open-denied" {
            INVALID_HANDLE_VALUE
        } else {
            2usize as HANDLE
        }
    })
}
unsafe extern "system" fn product(
    _file: HANDLE,
    output: *mut core::ffi::c_void,
    _bytes: u32,
) -> bool {
    FIXTURE.with(|f| {
        let f = f.borrow();
        if f.mode == "strings-missing" {
            return false;
        }
        let text = wide(if f.mode == "strings-empty" {
            ""
        } else {
            "Authored controller"
        });
        std::ptr::copy_nonoverlapping(text.as_ptr(), output.cast(), text.len());
        true
    })
}
unsafe extern "system" fn serial(
    _file: HANDLE,
    output: *mut core::ffi::c_void,
    bytes: u32,
) -> bool {
    product(_file, output, bytes)
}
unsafe extern "system" fn close(_file: HANDLE) -> i32 {
    FIXTURE.with(|f| f.borrow_mut().closed += 1);
    1
}
fn api() -> Api {
    Api {
        info,
        list,
        raw,
        caps,
        buttons,
        values,
        data,
        open,
        product,
        serial,
        close,
    }
}

#[test]
fn native_descriptor_ranges_aliases_arrays_and_diagnostics() {
    unsafe {
        reset("");
        let (_, definition) = device(&api(), 1usize as HANDLE).unwrap().unwrap();
        assert_eq!(definition.name, "Authored controller");
        assert_eq!(
            definition.identity.serial.as_deref(),
            Some("Authored controller")
        );
        assert_eq!(definition.controls.len(), 5);
        assert_eq!(definition.controls[2].kind, Kind::Axis);
        assert_eq!(definition.controls[3].collection, 1);
        assert_eq!(definition.controls[4].kind, Kind::Diagnostic);
        assert!(!definition.diagnostics.is_empty());
        FIXTURE.with(|f| {
            assert_eq!(f.borrow().closed, 1);
            assert!(!f.borrow().write_requested);
        });
        for mode in ["open-denied", "strings-missing", "strings-empty"] {
            reset(mode);
            let (_, d) = device(&api(), 1usize as HANDLE).unwrap().unwrap();
            assert!(d.identity.serial.is_none());
            assert!(d.name.starts_with("Controller"));
        }
        reset("");
        FIXTURE.with(|f| {
            let mut f = f.borrow_mut();
            let mut c = f.buttons[0];
            c.IsAlias = true;
            f.buttons.push(c);
            f.buttons[0].IsRange = false;
            f.buttons[0].UsagePage = 0xff00;
            f.buttons[0].Anonymous.NotRange = HIDP_BUTTON_CAPS_0_1 {
                Usage: 1,
                DataIndex: 0,
                ..Default::default()
            };
            f.values[0].IsRange = true;
            f.values[0].LogicalMin = 0;
            f.values[0].LogicalMax = -1;
            f.values[0].BitSize = 32;
            f.values[0].Anonymous.Range = HIDP_VALUE_CAPS_0_0 {
                UsageMin: 0x30,
                UsageMax: 0x31,
                DataIndexMin: 2,
                DataIndexMax: 3,
                ..Default::default()
            };
            f.values[1].IsAbsolute = false;
            f.values[2].ReportCount = 4;
        });
        let (_, d) = device(&api(), 1usize as HANDLE).unwrap().unwrap();
        assert_eq!(d.controls[0].kind, Kind::Diagnostic);
        assert_eq!(d.controls[1].max, 4294967295);
        assert_eq!(d.controls[3].kind, Kind::Diagnostic);
    }
}
#[test]
fn native_discovery_exclusion_and_failure_bounds() {
    unsafe {
        for mode in [
            "info-error",
            "path-query",
            "path-zero",
            "path-long",
            "path-read",
            "preparse-query",
            "preparse-large",
            "preparse-read",
            "caps-error",
            "caps-large",
            "report-large",
            "buttons-error",
            "values-error",
        ] {
            reset(mode);
            assert!(device(&api(), 1usize as HANDLE).is_err(), "{mode}");
        }
        reset("");
        FIXTURE.with(|f| f.borrow_mut().kind = RIM_TYPEKEYBOARD);
        assert!(device(&api(), 1usize as HANDLE).unwrap().is_none());
        reset("");
        FIXTURE.with(|f| f.borrow_mut().vid = 0x1234);
        assert!(device(&api(), 1usize as HANDLE).unwrap().is_none());
        reset("");
        FIXTURE.with(|f| f.borrow_mut().usage = 6);
        assert!(device(&api(), 1usize as HANDLE).unwrap().is_none());
        assert!(!eligible(2, 4, 1, 2));
        assert!(eligible(1, 8, 1, 2));
        for is_button in [true, false] {
            reset("");
            FIXTURE.with(|f| {
                let mut f = f.borrow_mut();
                if is_button {
                    f.buttons[0].Anonymous.Range.DataIndexMax = 300;
                } else {
                    f.values[0].IsRange = true;
                    f.values[0].Anonymous.Range = HIDP_VALUE_CAPS_0_0 {
                        DataIndexMin: 0,
                        DataIndexMax: 300,
                        ..Default::default()
                    };
                }
            });
            assert!(device(&api(), 1usize as HANDLE).is_err());
        }
    }
}
#[test]
fn enumeration_reconnects_and_reports_preserve_atomic_control_state() {
    unsafe {
        reset("");
        let shared = Arc::new(Mutex::new(Registry::default()));
        let mut devices = BTreeMap::new();
        enumerate(&api(), &mut devices, &shared).unwrap();
        assert_eq!(devices.len(), 1);
        enumerate(&api(), &mut devices, &shared).unwrap();
        assert_eq!(shared.lock().unwrap().list(0)["total"], 1);
        input(&api(), 1usize as HRAWINPUT, &devices, &shared).unwrap();
        let id = shared.lock().unwrap().list(0)["sources"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let detail = shared.lock().unwrap().inspect(&id, 0).unwrap();
        assert_eq!(detail["source"]["baselineReady"], true);
        assert_eq!(detail["controls"][0]["value"]["normalized"], 1);
        assert_eq!(detail["controls"][1]["value"]["normalized"], 0);
        assert_eq!(detail["controls"][2]["value"]["native"], -1);
        assert!(detail["controls"][3]["value"]["normalized"].is_null());
        for mode in ["data-error", "data-large"] {
            FIXTURE.with(|f| f.borrow_mut().mode = mode);
            input(&api(), 1usize as HRAWINPUT, &devices, &shared).unwrap();
            assert_eq!(
                shared.lock().unwrap().inspect(&id, 0).unwrap()["source"]["baselineReady"],
                false
            );
        }
        FIXTURE.with(|f| {
            let mut f = f.borrow_mut();
            f.mode = "";
            f.present = false;
        });
        enumerate(&api(), &mut devices, &shared).unwrap();
        assert!(devices.is_empty());
        assert_eq!(
            shared.lock().unwrap().inspect(&id, 0).unwrap()["source"]["online"],
            false
        );
        FIXTURE.with(|f| f.borrow_mut().present = true);
        enumerate(&api(), &mut devices, &shared).unwrap();
        assert_eq!(shared.lock().unwrap().list(0)["total"], 1);
        for mode in ["list-error", "list-large", "list-race"] {
            FIXTURE.with(|f| f.borrow_mut().mode = mode);
            assert!(enumerate(&api(), &mut devices, &shared).is_err());
        }
        reset("info-error");
        devices.clear();
        enumerate(&api(), &mut devices, &shared).unwrap();
        assert!(!shared.lock().unwrap().diagnostic.is_empty());
    }
}
#[test]
fn malformed_raw_input_never_reads_outside_the_frame() {
    unsafe {
        reset("");
        let shared = Arc::new(Mutex::new(Registry::default()));
        let mut devices = BTreeMap::new();
        enumerate(&api(), &mut devices, &shared).unwrap();
        for mode in [
            "raw-short",
            "raw-large",
            "raw-query",
            "raw-read",
            "raw-shrink",
        ] {
            FIXTURE.with(|f| f.borrow_mut().mode = mode);
            assert!(input(&api(), 1usize as HRAWINPUT, &devices, &shared).is_err());
        }
        for (field, value) in [(0, 0u32), (0, 5), (4, 4097), (4, 10)] {
            reset("");
            FIXTURE.with(|f| {
                f.borrow_mut().packet
                    [size_of::<RAWINPUTHEADER>() + field..size_of::<RAWINPUTHEADER>() + field + 4]
                    .copy_from_slice(&value.to_ne_bytes())
            });
            assert!(input(&api(), 1usize as HRAWINPUT, &devices, &shared).is_err());
        }
        reset("");
        FIXTURE
            .with(|f| f.borrow_mut().packet[..4].copy_from_slice(&RIM_TYPEKEYBOARD.to_ne_bytes()));
        assert!(input(&api(), 1usize as HRAWINPUT, &devices, &shared).is_ok());
        reset("");
        devices.clear();
        assert!(input(&api(), 1usize as HRAWINPUT, &devices, &shared).is_ok());
    }
}
#[test]
fn real_message_window_starts_and_stops_without_optional_drivers() {
    let service = crate::ControllerService::new(None, true);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while service.status()["backendStatus"] == "starting" && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(service.status()["backendStatus"], "ready");
    std::thread::sleep(std::time::Duration::from_millis(50));
    service.shutdown();
    assert_eq!(service.status()["backendStatus"], "stopped");
}
