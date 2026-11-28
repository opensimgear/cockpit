use super::digest;
use serde_json::{json, Map, Value};

/// Enumerated metadata fields only. Report deliveries, values, authorization and
/// session generations are deliberately outside the persisted document.
pub(super) fn sanitize(detail: &Value) -> Value {
    let mut metadata = Map::new();
    for key in [
        "interfaceId",
        "pathFingerprint",
        "identity",
        "backend",
        "attributes",
        "product",
        "name",
        "manufacturer",
        "serial",
        "vendorId",
        "productId",
        "versionNumber",
        "locationId",
        "transport",
        "usagePage",
        "usage",
        "capabilities",
        "capabilityFailures",
        "complete",
        "path",
        "sysfsPath",
        "hidParent",
        "hidAncestry",
        "physical",
        "unique",
        "udevProperties",
        "evdev",
        "evdevPaths",
        "restricted",
        "restrictionReasons",
        "restrictionReason",
        "failures",
        "descriptorSha256",
        "descriptorHex",
        "descriptor",
        "descriptorError",
        "descriptorProvenance",
        "elements",
        "elementsProvenance",
        "descriptorAvailable",
        "elementCount",
        "maxInputReportBytes",
        "maxFeatureReportBytes",
    ] {
        if let Some(value) = detail.get(key) {
            metadata.insert(key.into(), value.clone());
        }
    }
    Value::Object(metadata)
}

/// These identities identify one native interface/collection; VID/PID, product
/// and serial alone never combine sibling collections or unrelated devices.
pub(super) fn identity(id: &str, detail: &Value) -> String {
    if let Some(path) = detail["pathFingerprint"].as_str().filter(|p| !p.is_empty()) {
        return digest(&json!(["windows-interface-collection", path]));
    }
    if let Some(path) = detail["sysfsPath"].as_str().filter(|p| !p.is_empty()) {
        return digest(&json!([
            "linux-hidraw-interface",
            path,
            detail["hidParent"],
            detail["path"]
        ]));
    }
    if let Some(registry) = detail["identity"]["registryEntryId"]
        .as_str()
        .filter(|p| !p.is_empty())
    {
        return digest(&json!(["macos-iohid-service", registry]));
    }
    digest(&json!(["native-interface", id]))
}

pub(super) fn provenance(detail: &Value) -> Value {
    let mut sources = Map::new();
    if !detail["capabilities"].is_null() {
        sources.insert(
            "capabilities".into(),
            detail["capabilities"]
                .get("provenance")
                .cloned()
                .unwrap_or_else(|| json!("native-capabilities")),
        );
    }
    if detail["descriptorHex"].is_string() || !detail["descriptor"].is_null() {
        sources.insert(
            "descriptor".into(),
            detail
                .get("descriptorProvenance")
                .cloned()
                .unwrap_or_else(|| {
                    json!(if detail["sysfsPath"].is_string() {
                        "linux-sysfs-report-descriptor"
                    } else {
                        "native-report-descriptor"
                    })
                }),
        );
    }
    if !detail["elements"].is_null() {
        sources.insert(
            "elements".into(),
            detail
                .get("elementsProvenance")
                .cloned()
                .unwrap_or_else(|| json!("native-elements")),
        );
    }
    Value::Object(sources)
}

pub(super) fn fingerprint(detail: &Value, provenance: &Value) -> String {
    let mut contract = json!({"provenance":provenance});
    for key in [
        "descriptorHex",
        "descriptor",
        "capabilities",
        "elements",
        "usagePage",
        "usage",
        "maxInputReportBytes",
        "maxFeatureReportBytes",
    ] {
        if let Some(value) = detail.get(key) {
            contract[key] = value.clone();
        }
    }
    digest(&contract)
}
