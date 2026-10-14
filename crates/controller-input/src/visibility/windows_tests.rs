use super::super::journal::Backend;
use super::*;
#[test]
fn paths_use_existing_os_handles_and_invalid_paths_are_inert() {
    let current = std::env::current_exe().unwrap();
    let image = executable(current.to_str().unwrap()).unwrap();
    assert!(image.to_lowercase().starts_with(r"\device\"));
    for path in ["", "relative.exe", r"C:\not-an-osg-executable.exe"] {
        assert!(executable(path).is_err());
    }
    for path in ["", "x\0y", &"x".repeat(4097)] {
        assert!(open(path, 0).is_err());
    }
    for interface in [
        "",
        "x",
        r"\\?\HID#NOT_PRESENT#1#{00000000-0000-0000-0000-000000000000}",
    ] {
        assert!(instance(interface).is_err());
    }
}
#[test]
fn absent_unsupported_and_installed_dependency_inspection_never_mutates() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("HidHide.sys");
    assert!(artifact(&path).is_err());
    std::fs::write(&path, b"unqualified").unwrap();
    assert!(artifact(&path).is_err());
    std::fs::File::create(&path)
        .unwrap()
        .set_len(4_194_305)
        .unwrap();
    assert!(artifact(&path).is_err());
    if let Ok(mut driver) = installed() {
        driver.read().unwrap().validate().unwrap();
        assert!(driver
            .0
            .call(2053, Some(&[1]))
            .unwrap_err()
            .contains("disabled"));
    }
}
#[test]
fn native_transport_rejects_writes_and_malformed_requests_before_io() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let mut kernel = Kernel(open(temp.path().to_str().unwrap(), 0).unwrap());
    assert!(kernel
        .call(2049, Some(&[0, 0]))
        .unwrap_err()
        .contains("disabled"));
    assert!(kernel.call(9000, None).is_err());
    assert!(kernel.call(2048, Some(&[0])).is_err());
    assert!(kernel.call(2049, None).is_err());
    assert!(kernel.call(2048, None).is_err()); // A regular file cannot answer HidHide IOCTLs.
}
