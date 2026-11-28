use super::*;
use native::{Api, Callback};
use std::{ffi::c_void, sync::atomic::AtomicI32};

unsafe extern "C" fn yes() -> i32 {
    1
}
unsafe extern "C" fn no() -> i32 {
    0
}
unsafe extern "C" fn version() -> u16 {
    0x222
}
unsafe extern "C" fn matches(dll: *mut u16, driver: *mut u16) -> i32 {
    *dll = 0x222;
    *driver = 0x222;
    1
}
unsafe extern "C" fn out(value: *mut i32) -> i32 {
    *value = 1;
    1
}
unsafe extern "C" fn maximum(value: *mut i32) -> i32 {
    *value = 16;
    1
}
unsafe extern "C" fn status(slot: u32) -> i32 {
    match slot {
        2 | 3 => 2,
        4 => 3,
        5 => 4,
        _ => 1,
    }
}
unsafe extern "C" fn owner(slot: u32) -> i32 {
    if slot == 3 {
        -13
    } else {
        1234
    }
}
unsafe extern "C" fn exists(slot: u32) -> i32 {
    i32::from(slot != 4)
}
unsafe extern "C" fn count(_: u32) -> i32 {
    4
}
unsafe extern "C" fn query(_: u32) -> i32 {
    1
}
unsafe extern "C" fn axis(_: u32, _: u32) -> i32 {
    1
}
unsafe extern "C" fn minimum(_: u32, _: u32, value: *mut i32) -> i32 {
    *value = -123;
    1
}
unsafe extern "C" fn maximum_axis(_: u32, _: u32, value: *mut i32) -> i32 {
    *value = 456;
    1
}
unsafe extern "C" fn release(_: u32) {}
unsafe extern "C" fn register(_: Option<Callback>, _: *mut c_void) {}

pub(super) fn api() -> Api {
    unsafe {
        Api::from_symbols(
            |name| {
                let name = std::str::from_utf8(&name[..name.len() - 1]).unwrap();
                Ok(match name {
                    "vJoyEnabled" => yes as *const (),
                    "GetvJoyVersion" => version as *const (),
                    "DriverMatch" => matches as *const (),
                    "vJoyFfbCap" | "GetNumberExistingVJD" => out as *const (),
                    "GetvJoyMaxDevices" => maximum as *const (),
                    "GetVJDStatus" => status as *const (),
                    "GetOwnerPid" => owner as *const (),
                    "isVJDExists" => exists as *const (),
                    "GetVJDButtonNumber" | "GetVJDDiscPovNumber" | "GetVJDContPovNumber" => {
                        count as *const ()
                    }
                    "IsDeviceFfb" | "AcquireVJD" => query as *const (),
                    "IsDeviceFfbEffect" | "GetVJDAxisExist" => axis as *const (),
                    "GetVJDAxisMin" => minimum as *const (),
                    "GetVJDAxisMax" => maximum_axis as *const (),
                    "RelinquishVJD" => release as *const (),
                    "FfbRegisterGenCB" => register as *const (),
                    name => panic!("Unexpected SDK export {name}"),
                }
                .cast())
            },
            vec![],
        )
        .unwrap()
    }
}

#[test]
fn inspection_covers_all_sdk_axes_effect_usages_and_exact_v3_layout_without_acquisition() {
    let value = inspection::inspect(&api(), 1).unwrap();
    assert_eq!(value["status"], "free");
    assert_eq!(value["captureAvailable"], true);
    let axes = value["axes"].as_array().unwrap();
    assert_eq!(axes.len(), 16);
    assert!(axes
        .iter()
        .any(|a| a["usage"] == 0xc8 && a["usagePage"] == 2));
    assert!(axes
        .iter()
        .all(|a| a["minimum"] == -123 && a["maximum"] == 456));
    let effects = value["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 12);
    assert_eq!(effects.last().unwrap()["usage"], 0x28);
    assert_eq!(value["reportLayout"]["bytes"], 124);
    assert_eq!(value["reportLayout"]["fields"][18]["offset"], 76);
    assert_eq!(value["reportLayout"]["fields"][29]["offset"], 120);
    assert_eq!(value["reportLayout"]["ffbCallback"]["bytes"], 16);
    assert_eq!(inspection::inspect(&api(), 2).unwrap()["ownerPid"], 1234);
    assert!(inspection::inspect(&api(), 3)
        .unwrap_err()
        .contains("owner PID"));
    assert_eq!(inspection::inspect(&api(), 4).unwrap()["exists"], false);
    assert_eq!(inspection::inspect(&api(), 5).unwrap()["status"], "unknown");
    assert_eq!(
        inspection::inspect(&api(), 16).unwrap()["captureAvailable"],
        false
    );
}

#[test]
fn capability_query_failures_are_never_reported_as_unsupported() {
    unsafe extern "C" fn bad(_: u32) -> i32 {
        -3
    }
    unsafe extern "C" fn no_axis(_: u32, _: u32) -> i32 {
        -10
    }
    unsafe extern "C" fn bad_axis(_: u32, _: u32) -> i32 {
        -2
    }
    unsafe extern "C" fn bad_range(_: u32, _: u32, _: *mut i32) -> i32 {
        0
    }
    let mut fixture = api();
    fixture.enabled = no;
    assert!(fixture.check().unwrap_err().contains("disabled"));
    unsafe extern "C" fn bad_version() -> u16 {
        0
    }
    fixture = api();
    fixture.version = bad_version;
    assert!(fixture.check().unwrap_err().contains("match"));
    fixture = api();
    fixture.buttons = bad;
    assert!(inspection::inspect(&fixture, 1)
        .unwrap_err()
        .contains("buttons"));
    fixture = api();
    fixture.axis = no_axis;
    assert_eq!(inspection::inspect(&fixture, 1).unwrap()["axes"], json!([]));
    fixture.axis = bad_axis;
    assert!(inspection::inspect(&fixture, 1)
        .unwrap_err()
        .contains("axis"));
    fixture = api();
    fixture.minimum = bad_range;
    assert!(inspection::inspect(&fixture, 1)
        .unwrap_err()
        .contains("range"));
}

fn child(script: &str) -> Child {
    Command::new(std::env::var("OSG_TEST_PYTHON").unwrap_or_else(|_| "python".into()))
        .args(["-u", "-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .unwrap()
}
fn sink() -> Sink {
    Arc::new(|_, _| {})
}

#[test]
fn bounded_protocol_rejects_partial_large_invalid_and_explicit_error_messages() {
    for bytes in [
        b"{\n".to_vec(),
        b"{}".to_vec(),
        vec![b'x'; MAX_LINE as usize + 1],
    ] {
        assert!(line(&mut &bytes[..]).is_err());
    }
    assert!(line(&mut &b""[..]).unwrap().is_none());
    assert!(response(json!({"ok":true,"backend":"other"})).is_err());
    assert_eq!(
        response(json!({"error":"driver unavailable"})).unwrap_err(),
        "driver unavailable"
    );
    for value in [
        json!({"command":0}),
        json!({"command":-1,"payload":[17]}),
        json!({"command":1,"payload":[]}),
        json!({"command":1,"payload":[256]}),
    ] {
        let bytes = format!("{value}\n");
        assert!(read_packets(&mut bytes.as_bytes(), &AtomicBool::new(false), &sink(), 3).is_err());
    }
    assert!(validate_slot(0).is_err());
    assert!(validate_slot(17).is_err());
    assert!(start(16, sink()).err().unwrap().contains("four-bit"));
}

#[test]
fn isolated_parent_handshake_copies_packets_and_joins_reader_on_close() {
    let (sender, receiver) = mpsc::channel();
    let script = "import json,sys\nprint(json.dumps({'ok':True,'backend':'vjoy-ffb-sdk','slotId':3,'acquired':True,'capabilities':{'slotId':3}}),flush=True)\nprint(json.dumps({'command':721303,'payload':[53,1,255]}),flush=True)\nsys.stdin.read()\n";
    let capture = start_child(
        child(script),
        3,
        Arc::new(move |command, bytes| {
            sender.send((command, bytes)).unwrap();
        }),
    )
    .unwrap();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
        (721303, vec![53, 1, 255])
    );
    assert!(capture.failure().is_none());
    drop(capture);
    assert!(receiver.recv().is_err());
    let value = inspect_child(
        child("print('{\"ok\":true,\"backend\":\"vjoy-ffb-sdk\",\"inspection\":{\"slotId\":3}}')"),
        3,
    )
    .unwrap();
    assert_eq!(value["slotId"], 3);
}

#[test]
fn isolated_parent_surfaces_acquisition_protocol_and_later_backend_failures() {
    for script in [
        "print('{\"error\":\"owned by feeder\"}')",
        "print('{\"ok\":true,\"backend\":\"vjoy-ffb-sdk\",\"slotId\":2,\"acquired\":true}')",
        "pass",
    ] {
        assert!(start_child(child(script), 3, sink()).is_err());
    }
    let script = "import sys\nprint('{\"ok\":true,\"backend\":\"vjoy-ffb-sdk\",\"slotId\":3,\"acquired\":true,\"capabilities\":{\"slotId\":3}}',flush=True)\nprint('{\"error\":\"device removed\"}',flush=True)\nsys.stdin.read()\n";
    let capture = start_child(child(script), 3, sink());
    if let Ok(capture) = capture {
        let deadline = Instant::now() + Duration::from_secs(3);
        while capture.failure().is_none() && Instant::now() < deadline {
            thread::yield_now();
        }
        assert_eq!(capture.failure().as_deref(), Some("device removed"));
    } else {
        assert_eq!(capture.err().unwrap(), "device removed");
    }
    let mut stuck = child("import time; time.sleep(60)");
    retire(&mut stuck);
    assert!(stuck.try_wait().unwrap().is_some());
}

static CAPTURE_STATUS: AtomicI32 = AtomicI32::new(1);
static RELEASED: AtomicBool = AtomicBool::new(false);
unsafe extern "C" fn capture_status(_: u32) -> i32 {
    CAPTURE_STATUS.load(Ordering::SeqCst)
}
unsafe extern "C" fn acquire(_: u32) -> i32 {
    CAPTURE_STATUS.store(0, Ordering::SeqCst);
    1
}
unsafe extern "C" fn relinquish(_: u32) {
    RELEASED.store(true, Ordering::SeqCst);
}
unsafe extern "C" fn callback_register(cb: Option<Callback>, data: *mut c_void) {
    let mut bytes = [0x15, 1, 2, 3];
    let mut packet = callback::FfbData {
        size: 12,
        command: 0xb0017,
        data: bytes.as_mut_ptr(),
    };
    cb.unwrap()((&mut packet as *mut callback::FfbData).cast(), data);
    bytes.fill(0); // A queued frame must own the earlier bytes.
}

#[test]
fn capture_acquires_only_free_slot_detaches_before_release_and_never_reuses_callback_context() {
    let mut fixture = api();
    let mut emitted = vec![];
    assert!(capture_with(&fixture, 2, |_| Ok(()), |_| {})
        .unwrap_err()
        .contains("FREE"));
    fixture.status = capture_status;
    fixture.acquire = acquire;
    fixture.release = relinquish;
    fixture.register = callback_register;
    capture_with(
        &fixture,
        1,
        |v| {
            emitted.push(v);
            Ok(())
        },
        |sender| {
            sender.send(callback::Message::Stop).unwrap();
        },
    )
    .unwrap();
    assert_eq!(emitted[0]["acquired"], true);
    assert_eq!(emitted[1]["payload"], json!([0x15, 1, 2, 3]));
    assert!(RELEASED.load(Ordering::SeqCst));
    unsafe {
        callback::receive(std::ptr::null_mut(), std::ptr::null_mut());
    }
    assert!(!callback::OVERFLOW.load(Ordering::Acquire));
    assert!(callback::install(1, mpsc::sync_channel(1).0).is_err());
}

#[test]
fn installed_pinned_sdk_can_be_inspected_read_only_when_present() {
    if !std::path::Path::new(r"C:\Program Files\vJoy\x64\vJoyInterface.dll").exists() {
        return;
    }
    let fixture = Api::open().unwrap();
    let value = inspection::inspect(&fixture, 1).unwrap();
    assert_eq!(value["dllVersion"], 0x222);
    assert_eq!(value["driverVersion"], 0x222);
    assert_eq!(value["reportLayout"]["bytes"], 124);
}

#[test]
fn helper_dispatch_validates_before_opening_and_preserves_read_only_query_results() {
    assert!(!run_helper());
    let args = |kind: &str, slot: &str| vec!["cockpit.exe".into(), kind.into(), slot.into()];
    for arguments in [vec![], args(INSPECT, "bad"), args(INSPECT, "0")] {
        assert!(helper_result(
            &arguments,
            || panic!("invalid arguments opened SDK"),
            |_| Ok(())
        )
        .is_err());
    }
    let mut output = None;
    helper_result(
        &args(INSPECT, "1"),
        || Ok(api()),
        |v| {
            output = Some(v);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(output.unwrap()["inspection"]["slotId"], 1);
    assert!(helper_result(&args(CAPTURE, "2"), || Ok(api()), |_| Ok(()))
        .unwrap_err()
        .contains("FREE"));
    assert_eq!(
        helper_result(
            &args(INSPECT, "1"),
            || Err("missing SDK".into()),
            |_| Ok(())
        )
        .unwrap_err(),
        "missing SDK"
    );
    assert_eq!(
        helper_result(
            &args(INSPECT, "1"),
            || Ok(api()),
            |_| Err("closed stdout".into())
        )
        .unwrap_err(),
        "closed stdout"
    );
}
