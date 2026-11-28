//! Read-only persisted descriptor evidence; never a claim about the active HID device.
use osg_hid_report::parse_descriptor;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::ptr;
use windows_sys::Win32::{Foundation::ERROR_FILE_NOT_FOUND, System::Registry::*};

const PARAMETERS: &str = r"SYSTEM\CurrentControlSet\Services\vjoy\Parameters";
const MAX_DESCRIPTOR: usize = 65_536;
const MAX_EVIDENCE: usize = 168_000;
const MAX_DIAGNOSTICS: usize = 32;

#[derive(Clone, Debug, PartialEq)]
struct RegistryValue {
    kind: u32,
    bytes: usize,
    data: Option<Vec<u8>>,
}
type Values = (Option<RegistryValue>, Option<RegistryValue>);
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn registry_error(code: u32) -> String {
    format!("Registry read failed (Windows error {code})")
}

fn value(key: HKEY, name: &str) -> Result<Option<RegistryValue>, String> {
    let name = wide(name);
    let (mut kind, mut size) = (0, 0);
    let code = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            ptr::null(),
            &mut kind,
            ptr::null_mut(),
            &mut size,
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if code != 0 {
        return Err(registry_error(code));
    }
    if size as usize > MAX_DESCRIPTOR {
        return Ok(Some(RegistryValue {
            kind,
            bytes: size as usize,
            data: None,
        }));
    }
    let mut data = vec![0; size as usize];
    let code = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            ptr::null(),
            &mut kind,
            data.as_mut_ptr(),
            &mut size,
        )
    };
    if code != 0 {
        return Err(registry_error(code));
    }
    data.truncate(size as usize);
    Ok(Some(RegistryValue {
        kind,
        bytes: data.len(),
        data: Some(data),
    }))
}
fn read(hive: HKEY, path: &str) -> Result<Option<Values>, String> {
    let path = wide(path);
    let mut key = ptr::null_mut();
    let code = unsafe {
        RegOpenKeyExW(
            hive,
            path.as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_WOW64_64KEY,
            &mut key,
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if code != 0 {
        return Err(registry_error(code));
    }
    let key = Key(key);
    let values = (
        value(key.0, "HidReportDescriptor")?,
        value(key.0, "HidReportDescriptorSize")?,
    );
    let repeated = (
        value(key.0, "HidReportDescriptor")?,
        value(key.0, "HidReportDescriptorSize")?,
    );
    if repeated != values {
        return Err("Configured descriptor changed during inspection; inspect again".into());
    }
    Ok(Some(values))
}

fn bounded(mut result: Value) -> Value {
    if result.to_string().len() > MAX_EVIDENCE {
        result["reports"] = Value::Null;
        result["diagnostics"] = json!([]);
        result["reportsOmitted"] = result["reportCount"].clone();
        result["diagnosticsOmitted"] = result["diagnosticCount"].clone();
        result["detailsAvailable"] = json!(false);
        result["detailsUnavailableReason"] = json!("Configured report summaries exceed the 168000-byte projection limit; report rows and diagnostics are omitted, not truncated. Exact descriptor bytes, hash and counts remain available");
    }
    result
}
fn project(slot: u8, path: &str, observed: Result<Option<Values>, String>) -> Value {
    let mut result = json!({
        "slot":slot, "status":"missing", "detailsAvailable":true,
        "provenance":{"source":"windows-registry","hive":"HKEY_LOCAL_MACHINE","key":path,"view":"64-bit",
            "descriptorValue":"HidReportDescriptor","sizeValue":"HidReportDescriptorSize",
            "scope":"persisted configuration only","activeCorrelation":"unverified"},
        "matchesActiveDescriptor":null,
        "notice":"Persisted configuration may differ from the active device until a driver refresh or restart. Missing values may mean driver defaults; no default or active descriptor is inferred.",
        "descriptorHex":null,"descriptorBytes":null,"descriptorSha256":null,"declaredBytes":null,
        "reports":[],"reportCount":0,"diagnostics":[],"diagnosticCount":0,
        "limits":{"descriptorBytes":MAX_DESCRIPTOR,"projectionBytes":MAX_EVIDENCE,"diagnostics":MAX_DIAGNOSTICS}
    });
    let values = match observed {
        Err(error) => {
            result["status"] = json!("unreadable");
            result["error"] = json!(error);
            return result;
        }
        Ok(None) => return result,
        Ok(Some(values)) => values,
    };
    let (descriptor, size) = values;
    if descriptor.is_none() && size.is_none() {
        result["status"] = json!("default-or-unconfigured");
        return result;
    }
    let Some(descriptor) = descriptor else {
        result["status"] = json!("missing-descriptor");
        return result;
    };
    result["descriptorBytes"] = json!(descriptor.bytes);
    result["descriptorRegistryType"] = json!(descriptor.kind);
    let Some(data) = descriptor.data else {
        result["status"] = json!("descriptor-too-large");
        return result;
    };
    result["descriptorSha256"] = json!(format!("{:x}", Sha256::digest(&data)));
    if descriptor.kind != REG_BINARY {
        result["status"] = json!("invalid-descriptor-type");
        return result;
    }
    result["descriptorHex"] = json!(data
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>());
    let Some(size) = size else {
        result["status"] = json!("missing-size");
        return bounded(result);
    };
    let declared = size
        .data
        .as_deref()
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map(u32::from_le_bytes);
    result["declaredBytes"] = json!(declared);
    if size.kind != REG_DWORD || declared.is_none() {
        result["status"] = json!("invalid-size");
        return bounded(result);
    }
    if declared != Some(data.len() as u32) {
        result["status"] = json!("size-mismatch");
        return bounded(result);
    }
    if data.is_empty() {
        result["status"] = json!("empty-descriptor");
        return result;
    }
    match parse_descriptor(&data) {
        Err(error) => {
            result["status"] = json!("invalid-descriptor");
            result["diagnostics"] = json!([error]);
            result["diagnosticCount"] = json!(1);
        }
        Ok(parsed) => {
            result["status"] = json!(if parsed.diagnostics.is_empty() {
                "configured"
            } else {
                "configured-with-diagnostics"
            });
            result["reportCount"] = json!(parsed.layouts.len());
            result["reports"] = json!(parsed.layouts.iter().map(|layout|json!({
                "kind":layout.kind,"reportId":layout.report_id,"payloadBits":layout.payload_bits,
                "payloadBytes":layout.payload_bits.div_ceil(8),
                "wireBytes":layout.payload_bits.div_ceil(8)+usize::from(layout.report_id.is_some()),
                "framing":if layout.report_id.is_some(){"id-prefix"}else{"unnumbered"}
            })).collect::<Vec<_>>());
            result["diagnosticCount"] = json!(parsed.diagnostics.len());
            result["diagnostics"] = json!(parsed
                .diagnostics
                .iter()
                .take(MAX_DIAGNOSTICS)
                .collect::<Vec<_>>());
            result["diagnosticsOmitted"] =
                json!(parsed.diagnostics.len().saturating_sub(MAX_DIAGNOSTICS));
        }
    }
    bounded(result)
}

pub(super) fn inspect(slot: u8) -> Value {
    let path = format!(r"{PARAMETERS}\Device{slot:02}");
    if !(1..=16).contains(&slot) {
        return project(slot, &path, Err("Slot must be 1–16".into()));
    }
    project(slot, &path, read(HKEY_LOCAL_MACHINE, &path))
}

#[cfg(test)]
mod tests {
    use super::*;
    const VALID: &[u8] = &[
        0x05, 1, 0x09, 4, 0xa1, 1, 0x85, 0x11, 0x75, 8, 0x95, 2, 0x81, 2, 0x91, 2, 0xb1, 2, 0xc0,
    ];
    fn binary(data: &[u8]) -> RegistryValue {
        RegistryValue {
            kind: REG_BINARY,
            bytes: data.len(),
            data: Some(data.into()),
        }
    }
    fn size(number: usize) -> RegistryValue {
        RegistryValue {
            kind: REG_DWORD,
            bytes: 4,
            data: Some((number as u32).to_le_bytes().into()),
        }
    }
    fn evidence(data: &[u8]) -> Value {
        project(
            1,
            "fixture",
            Ok(Some((Some(binary(data)), Some(size(data.len()))))),
        )
    }
    #[test]
    fn configured_layout_has_exact_bytes_kinds_framing_and_persisted_provenance() {
        let result = evidence(VALID);
        assert_eq!(result["status"], "configured");
        assert_eq!(
            result["descriptorHex"],
            "05010904a10185117508950281029102b102c0"
        );
        assert_eq!(result["descriptorBytes"], VALID.len());
        assert_eq!(result["declaredBytes"], VALID.len());
        assert_eq!(result["descriptorSha256"].as_str().unwrap().len(), 64);
        assert_eq!(result["reportCount"], 3);
        assert_eq!(result["matchesActiveDescriptor"], Value::Null);
        assert_eq!(
            result["provenance"]["scope"],
            "persisted configuration only"
        );
        for (index, kind) in ["input", "output", "feature"].into_iter().enumerate() {
            assert_eq!(
                result["reports"][index],
                json!({"kind":kind,"reportId":17,"payloadBits":16,"payloadBytes":2,"wireBytes":3,"framing":"id-prefix"})
            );
        }
        let unnumbered = evidence(&[0x75, 1, 0x95, 9, 0x81, 2]);
        assert_eq!(unnumbered["reports"][0]["payloadBits"], 9);
        assert_eq!(unnumbered["reports"][0]["wireBytes"], 2);
        assert_eq!(unnumbered["reports"][0]["framing"], "unnumbered");
    }
    #[test]
    fn missing_default_invalid_and_unreadable_are_distinct() {
        assert_eq!(project(1, "fixture", Ok(None))["status"], "missing");
        assert_eq!(
            project(1, "fixture", Ok(Some((None, None))))["status"],
            "default-or-unconfigured"
        );
        assert_eq!(
            project(1, "fixture", Err("denied".into()))["error"],
            "denied"
        );
        assert_eq!(
            project(1, "fixture", Ok(Some((None, Some(size(1))))))["status"],
            "missing-descriptor"
        );
        assert_eq!(
            project(1, "fixture", Ok(Some((Some(binary(VALID)), None))))["status"],
            "missing-size"
        );
        let huge = RegistryValue {
            kind: REG_BINARY,
            bytes: 65537,
            data: None,
        };
        assert_eq!(
            project(1, "fixture", Ok(Some((Some(huge), None))))["status"],
            "descriptor-too-large"
        );
        let mut wrong = binary(VALID);
        wrong.kind = REG_SZ;
        assert_eq!(
            project(1, "fixture", Ok(Some((Some(wrong), None))))["status"],
            "invalid-descriptor-type"
        );
        let mut wrong = size(VALID.len());
        wrong.kind = REG_BINARY;
        assert_eq!(
            project(1, "fixture", Ok(Some((Some(binary(VALID)), Some(wrong)))))["status"],
            "invalid-size"
        );
        assert_eq!(
            project(
                1,
                "fixture",
                Ok(Some((Some(binary(VALID)), Some(binary(&[0])))))
            )["status"],
            "invalid-size"
        );
        assert_eq!(
            project(1, "fixture", Ok(Some((Some(binary(VALID)), Some(size(1))))))["status"],
            "size-mismatch"
        );
        assert_eq!(evidence(&[])["status"], "empty-descriptor");
        assert_eq!(inspect(0)["status"], "unreadable");
    }
    #[test]
    fn malformed_and_maximum_sized_evidence_remains_bounded_without_truncation() {
        let malformed = evidence(&[0x75]);
        assert!(
            malformed["status"] == "invalid-descriptor"
                || malformed["status"] == "configured-with-diagnostics"
        );
        assert!(malformed["diagnosticCount"].as_u64().unwrap() > 0);
        let maximum = evidence(&vec![0; MAX_DESCRIPTOR]);
        assert_eq!(maximum["descriptorBytes"], MAX_DESCRIPTOR);
        assert_eq!(maximum["detailsAvailable"], true);
        assert_eq!(
            maximum["descriptorHex"].as_str().unwrap(),
            "00".repeat(MAX_DESCRIPTOR)
        );
        let mut dense = vec![0x75, 8, 0x95, 1];
        for id in 1..=255 {
            dense.extend_from_slice(&[0x85, id, 0x81, 2, 0x91, 2, 0xb1, 2]);
        }
        dense.resize(MAX_DESCRIPTOR, 0);
        let dense = evidence(&dense);
        assert_eq!(dense["detailsAvailable"], false);
        assert_eq!(
            dense["descriptorHex"].as_str().unwrap().len(),
            MAX_DESCRIPTOR * 2
        );
        assert_eq!(dense["descriptorSha256"].as_str().unwrap().len(), 64);
        assert_eq!(dense["reportCount"], 765);
        assert_eq!(dense["reportsOmitted"], 765);
        assert_eq!(dense["diagnosticsOmitted"], dense["diagnosticCount"]);
        assert_eq!(dense["reports"], Value::Null);
        assert!(dense["detailsUnavailableReason"]
            .as_str()
            .unwrap()
            .contains("omitted, not truncated"));
        assert!(dense.to_string().len() <= MAX_EVIDENCE);
        assert_eq!(malformed["matchesActiveDescriptor"], Value::Null);
        assert_eq!(malformed["provenance"]["activeCorrelation"], "unverified");
        assert!(maximum.to_string().len() <= MAX_EVIDENCE);
    }
    struct Fixture {
        path: String,
        key: Key,
    }
    impl Fixture {
        fn new() -> Self {
            let path = format!(
                r"Software\OpenSimGearTests\vjoy-descriptor-{}",
                uuid::Uuid::new_v4()
            );
            let mut key = ptr::null_mut();
            assert_eq!(
                unsafe {
                    RegCreateKeyExW(
                        HKEY_CURRENT_USER,
                        wide(&path).as_ptr(),
                        0,
                        ptr::null(),
                        0,
                        KEY_ALL_ACCESS | KEY_WOW64_64KEY,
                        ptr::null(),
                        &mut key,
                        ptr::null_mut(),
                    )
                },
                0
            );
            Self {
                path,
                key: Key(key),
            }
        }
        fn write(&self, name: &str, kind: u32, bytes: &[u8]) {
            assert_eq!(
                unsafe {
                    RegSetValueExW(
                        self.key.0,
                        wide(name).as_ptr(),
                        0,
                        kind,
                        bytes.as_ptr(),
                        bytes.len() as u32,
                    )
                },
                0
            );
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe {
                RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.path).as_ptr());
            }
        }
    }
    #[test]
    fn isolated_hkcu_reads_no_real_vjoy_configuration_is_written() {
        let fixture = Fixture::new();
        assert_eq!(
            read(HKEY_CURRENT_USER, &fixture.path).unwrap(),
            Some((None, None))
        );
        fixture.write("HidReportDescriptor", REG_BINARY, VALID);
        fixture.write(
            "HidReportDescriptorSize",
            REG_DWORD,
            &(VALID.len() as u32).to_le_bytes(),
        );
        let found = read(HKEY_CURRENT_USER, &fixture.path).unwrap().unwrap();
        assert_eq!(found, (Some(binary(VALID)), Some(size(VALID.len()))));
        fixture.write(
            "HidReportDescriptor",
            REG_BINARY,
            &vec![0; MAX_DESCRIPTOR + 1],
        );
        assert!(read(HKEY_CURRENT_USER, &fixture.path)
            .unwrap()
            .unwrap()
            .0
            .unwrap()
            .data
            .is_none());
        assert_eq!(
            read(HKEY_CURRENT_USER, &format!(r"{}\absent", fixture.path)).unwrap(),
            None
        );
        assert!(read(ptr::null_mut(), "invalid-hive").is_err());
        assert!(value(ptr::null_mut(), "invalid-key").is_err());
        assert_eq!(registry_error(5), "Registry read failed (Windows error 5)");
    }
}
