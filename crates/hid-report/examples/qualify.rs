//! Offline verification adapter only. Reads bounded JSON stdin, never opens HID/OS handles.
use osg_hid_report::{
    decode_report, parse_descriptor, DecodedReport, Descriptor, Framing, ReportKind,
};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

const MAX_JSON: usize = 20 * 1024 * 1024;
const MAX_TOTAL_WIRE: usize = 8 * 1024 * 1024;
const MAX_VALUES: usize = 65_536;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    descriptor_hex: String,
    reports: Vec<Report>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Report {
    kind: String,
    framing: String,
    report_id: Option<u8>,
    wire_hex: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    provenance: &'static str,
    native_device_access: bool,
    descriptor: Descriptor,
    reports: Vec<DecodedReport>,
}
fn bytes(hex: &str, limit: usize) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 || hex.len() / 2 > limit {
        return Err("Hex byte length exceeds verification bound".into());
    }
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte| match byte {
                b'0'..=b'9' => Ok(byte - b'0'),
                b'a'..=b'f' => Ok(byte - b'a' + 10),
                _ => Err("Expected lowercase hexadecimal bytes".to_owned()),
            };
            Ok(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect()
}
fn verify(input: Input) -> Result<Output, String> {
    if input.reports.len() > 512 {
        return Err("At most 512 verification reports".into());
    }
    let descriptor = parse_descriptor(&bytes(&input.descriptor_hex, 65_536)?)
        .map_err(|error| format!("Descriptor budget: {} at {}", error.reason, error.offset))?;
    let mut reports = Vec::new();
    let mut total = 0usize;
    let mut values = 0usize;
    for report in input.reports {
        let kind = match report.kind.as_str() {
            "input" => ReportKind::Input,
            "output" => ReportKind::Output,
            "feature" => ReportKind::Feature,
            _ => return Err("Invalid report kind".into()),
        };
        let framing = match report.framing.as_str() {
            "id-prefix" => Framing::IdPrefix,
            "unnumbered" => Framing::Unnumbered,
            "os-zero-prefix" => Framing::OsZeroPrefix,
            "id-out-of-band" => Framing::OutOfBand(report.report_id),
            _ => return Err("Invalid report framing".into()),
        };
        let wire = bytes(&report.wire_hex, 65_536)?;
        total = total
            .checked_add(wire.len())
            .filter(|value| *value <= MAX_TOTAL_WIRE)
            .ok_or("Aggregate report-byte budget exceeded")?;
        let decoded = decode_report(&descriptor, kind, framing, &wire)
            .map_err(|error| format!("Report budget: {}", error.reason))?;
        values = values
            .checked_add(decoded.values.len())
            .filter(|value| *value <= MAX_VALUES)
            .ok_or("Aggregate decoded-value budget exceeded")?;
        reports.push(decoded);
    }
    Ok(Output {
        provenance: "offline-portable-decoder-verification",
        native_device_access: false,
        descriptor,
        reports,
    })
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let mut raw = Vec::new();
    io::stdin()
        .take((MAX_JSON + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|error| error.to_string())?;
    if raw.len() > MAX_JSON {
        return Err("Verification JSON byte budget exceeded".into());
    }
    let output = verify(serde_json::from_slice(&raw).map_err(|error| error.to_string())?)?;
    let encoded = serde_json::to_vec(&output).map_err(|error| error.to_string())?;
    if encoded.len() > 32 * 1024 * 1024 {
        return Err("Verification output exceeds byte budget".into());
    }
    io::stdout()
        .write_all(&encoded)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_original_h0_vectors_use_the_same_offline_adapter() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/hid-contract/authored");
        let mut cases = 0;
        for entry in std::fs::read_dir(root).unwrap() {
            let fixture: serde_json::Value =
                serde_json::from_slice(&std::fs::read(entry.unwrap().path()).unwrap()).unwrap();
            let reports=fixture["samples"].as_array().unwrap().iter().map(|sample|serde_json::json!({
                "kind":sample["kind"],"framing":if sample["reportId"].is_null(){"unnumbered"}else{"id-prefix"},"wireHex":sample["wireHex"]
            })).collect::<Vec<_>>();
            let input = serde_json::from_value(
                serde_json::json!({"descriptorHex":fixture["descriptorHex"],"reports":reports}),
            )
            .unwrap();
            let output = verify(input).unwrap();
            assert!(!output.native_device_access);
            assert!(output.descriptor.diagnostics.is_empty());
            for report in output.reports {
                assert!(report.diagnostics.is_empty());
                cases += 1;
            }
        }
        assert_eq!(cases, 18);
    }
    #[test]
    fn explicit_adapter_limits_and_unknown_request_fields_reject() {
        assert!(bytes("a", 10).is_err());
        assert!(bytes("FF", 10).is_err());
        assert!(bytes("aaaa", 1).is_err());
        assert!(serde_json::from_str::<Input>(
            r#"{"descriptorHex":"","reports":[],"device":"/dev/hidraw0"}"#
        )
        .is_err());
        assert!(verify(Input {
            descriptor_hex: "".into(),
            reports: (0..513)
                .map(|_| Report {
                    kind: "input".into(),
                    framing: "unnumbered".into(),
                    report_id: None,
                    wire_hex: "".into()
                })
                .collect()
        })
        .is_err());
        let input = |kind: &str, framing: &str| Input {
            descriptor_hex: "750895018102".into(),
            reports: vec![Report {
                kind: kind.into(),
                framing: framing.into(),
                report_id: None,
                wire_hex: "00".into(),
            }],
        };
        assert!(verify(input("invalid", "unnumbered")).is_err());
        assert!(verify(input("input", "invalid")).is_err());
    }
}
