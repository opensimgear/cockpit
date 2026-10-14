use super::*;
use std::sync::atomic::{AtomicI32, Ordering};
static MODE: AtomicI32 = AtomicI32::new(0);
unsafe extern "C" fn enabled() -> i32 {
    if MODE.load(Ordering::Relaxed) == 1 {
        0
    } else {
        1
    }
}
unsafe extern "C" fn version() -> u16 {
    VERSION
}
unsafe extern "C" fn matches(dll: *mut u16, driver: *mut u16) -> i32 {
    *dll = VERSION;
    *driver = if MODE.load(Ordering::Relaxed) == 2 {
        0
    } else {
        VERSION
    };
    1
}
unsafe extern "C" fn status(_: u32) -> i32 {
    match MODE.load(Ordering::Relaxed) {
        3 => 3,
        4 => 4,
        5 => 2,
        6 => 0,
        _ => 1,
    }
}
unsafe extern "C" fn owner(_: u32) -> i32 {
    17
}
unsafe extern "C" fn buttons(_: u32) -> i32 {
    if MODE.load(Ordering::Relaxed) == 7 {
        -1
    } else {
        128
    }
}
unsafe extern "C" fn zero(_: u32) -> i32 {
    0
}
unsafe extern "C" fn ffb(_: u32) -> i32 {
    if MODE.load(Ordering::Relaxed) == 8 {
        -1
    } else {
        0
    }
}
unsafe extern "C" fn axis(_: u32, usage: u32) -> i32 {
    match MODE.load(Ordering::Relaxed) {
        9 => -2,
        10 => -10,
        _ => {
            if usage == 0x30 {
                1
            } else {
                0
            }
        }
    }
}
unsafe extern "C" fn minimum(_: u32, _: u32, value: *mut i32) -> i32 {
    *value = 0;
    if MODE.load(Ordering::Relaxed) == 11 {
        0
    } else {
        1
    }
}
unsafe extern "C" fn maximum(_: u32, _: u32, value: *mut i32) -> i32 {
    *value = 32767;
    1
}
unsafe extern "C" fn acquire(_: u32) -> i32 {
    if MODE.load(Ordering::Relaxed) == 12 {
        0
    } else {
        1
    }
}
unsafe extern "C" fn submit(_: u32, _: *mut Position) -> i32 {
    0
}
unsafe extern "C" fn release(_: u32) {}
#[test]
fn sdk_boundary_distinguishes_absence_negative_errors_and_ownership() {
    let mut native = Native {
        api: Api {
            module: std::ptr::null_mut(),
            enabled,
            version,
            matches,
            status,
            owner,
            buttons,
            discrete: zero,
            continuous: zero,
            ffb,
            axis,
            minimum,
            maximum,
            acquire,
            submit,
            release,
        },
    };
    MODE.store(0, Ordering::Relaxed);
    native.check().unwrap();
    assert_eq!(native.inspect(1).unwrap().axes.len(), 1);
    native.acquire(1).unwrap();
    assert!(native.inspect(0).is_err());
    for mode in [1, 2] {
        MODE.store(mode, Ordering::Relaxed);
        assert!(native.check().is_err());
    }
    for (mode, expected) in [
        (3, Status::Missing),
        (4, Status::Unknown),
        (5, Status::Busy),
        (6, Status::Owned),
    ] {
        MODE.store(mode, Ordering::Relaxed);
        assert_eq!(native.inspect(1).unwrap().status, expected);
        assert!(native.acquire(1).is_err());
    }
    for mode in [7, 8, 9, 11] {
        MODE.store(mode, Ordering::Relaxed);
        assert!(native.inspect(1).is_err());
    }
    MODE.store(10, Ordering::Relaxed);
    assert!(native.inspect(1).unwrap().axes.is_empty());
    MODE.store(12, Ordering::Relaxed);
    assert!(native.acquire(1).is_err());
    let report = Position {
        device: 1,
        padding: [0; 3],
        words: [0; 30],
    };
    assert!(native.submit(0, &report).is_err());
    MODE.store(0, Ordering::Relaxed);
    assert!(native.submit(1, &report).is_err());
    MODE.store(6, Ordering::Relaxed);
    assert!(native.submit(1, &report).is_err());
    native.release(1);
    native.release(0);
    MODE.store(0, Ordering::Relaxed);
}
#[test]
fn artifact_pins_and_explicit_loader_reject_wrong_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fixture.dll");
    std::fs::write(&path, b"fixture").unwrap();
    let hash = format!("{:x}", Sha256::digest(b"fixture"));
    assert!(pinned_file(&path, &hash).is_ok());
    assert!(pinned_file(&path, DLL_HASH).is_err());
    assert!(pinned_file(Path::new(r"C:\missing-osg-vjoy-fixture"), DLL_HASH).is_err());
    assert!(unsafe { load_api(&path) }.is_err());
    // A real system DLL loads but cannot masquerade as the required vJoy API.
    assert!(unsafe { load_api(Path::new(r"C:\Windows\System32\kernel32.dll")) }.is_err());
}
