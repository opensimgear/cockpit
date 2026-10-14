use super::*;
struct Fixture {
    path: String,
}
impl Fixture {
    fn new() -> Self {
        let path = format!(
            "Software\\OpenSimGearSecurityTest\\{}",
            uuid::Uuid::new_v4()
        );
        Key::create(HKEY_CURRENT_USER, &path, None).unwrap();
        Self { path }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.path.starts_with("Software\\OpenSimGearSecurityTest\\"));
        assert!(uuid::Uuid::parse_str(self.path.rsplit('\\').next().unwrap()).is_ok());
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.path).unwrap().as_ptr());
        }
    }
}
const TEST_POLICY: &str = "D:P(A;CI;KA;;;OW)(A;CI;KR;;;BU)";
#[test]
fn actual_security_descriptor_roundtrip_rejects_wrong_policy_without_modifying_it() {
    let f = Fixture::new();
    let child = format!("{}\\Journal", f.path);
    let (key, new) = create(
        HKEY_CURRENT_USER,
        &child,
        TEST_POLICY,
        DACL_SECURITY_INFORMATION,
    )
    .unwrap();
    assert!(new);
    let before = read(&key, DACL_SECURITY_INFORMATION).unwrap();
    verify(&key, TEST_POLICY, DACL_SECURITY_INFORMATION).unwrap();
    assert!(
        !create(
            HKEY_CURRENT_USER,
            &child,
            TEST_POLICY,
            DACL_SECURITY_INFORMATION
        )
        .unwrap()
        .1
    );
    assert!(create(
        HKEY_CURRENT_USER,
        &child,
        "D:P(A;CI;KA;;;OW)",
        DACL_SECURITY_INFORMATION
    )
    .is_err());
    assert_eq!(read(&key, DACL_SECURITY_INFORMATION).unwrap(), before);
    assert!(verify_protected(&key).is_err());
    // This existing test key is never accepted as a production machine journal.
    assert!(protected(HKEY_CURRENT_USER, &child).is_err());
    assert_eq!(read(&key, DACL_SECURITY_INFORMATION).unwrap(), before);
    assert!(descriptor("invalid").is_err());
    assert!(descriptor("nul\0tail").is_err());
    let invalid = Key(ptr::null_mut());
    assert!(read(&invalid, INFO).is_err());
    assert!(create(invalid.0, "invalid", TEST_POLICY, DACL_SECURITY_INFORMATION).is_err());
}
#[test]
fn machine_policy_has_admin_ownership_and_only_admin_system_writes() {
    let expected = descriptor(POLICY).unwrap();
    let text = canonical(expected.0, INFO).unwrap();
    assert_eq!(text, POLICY);
    for unsafe_policy in [
        "O:BAG:BAD:P(A;CI;KA;;;BU)",
        "O:BAG:BAD:(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)",
        "O:BUG:BAD:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)",
    ] {
        let different = descriptor(unsafe_policy).unwrap();
        assert_ne!(canonical(different.0, INFO).unwrap(), text);
    }
}
