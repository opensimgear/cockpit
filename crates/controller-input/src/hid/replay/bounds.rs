use serde_json::Value;
use std::io::{self, Write};
pub(super) const ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
pub(super) const TOTAL_BYTES: usize = 128 * 1024 * 1024;
pub(super) const CHUNK_BYTES: usize = 24 * 1024;
// Catalog detail may gain a fingerprint and snapshot identity before capture.
pub(super) const DESCRIPTOR_BYTES: usize = 2 * 1024 * 1024 + 16 * 1024;
const NODES: usize = 2_000_000;
/// Scan untrusted UTF-8 before allocating a JSON tree. Serde then checks grammar.
pub(super) fn text(text: &str) -> Result<(), String> {
    if text.len() > ARCHIVE_BYTES {
        return Err("HID recording exceeds 64 MiB".into());
    }
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for byte in text.bytes() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => {
                quoted = true;
            }
            b'[' | b'{' => {
                depth += 1;
            }
            b']' | b'}' => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        if depth > 48 {
            return Err("HID recording structure exceeds native limits".into());
        }
    }
    Ok(())
}
fn structure(value: &Value, max_nodes: usize) -> Result<(), String> {
    fn walk(v: &Value, depth: usize, nodes: &mut usize, max_nodes: usize) -> Result<(), String> {
        *nodes += 1;
        if depth > 48 || *nodes > max_nodes {
            return Err("HID recording structure exceeds native limits".into());
        }
        match v {
            Value::Array(a) => {
                for child in a {
                    walk(child, depth + 1, nodes, max_nodes)?;
                }
            }
            Value::Object(o) => {
                for child in o.values() {
                    walk(child, depth + 1, nodes, max_nodes)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    walk(value, 0, &mut 0, max_nodes)
}
pub(super) fn archive_value(value: &Value) -> Result<usize, String> {
    structure(value, usize::MAX)?;
    encoded_size(value)
}
pub(super) fn value(value: &Value) -> Result<usize, String> {
    structure(value, NODES)?;
    encoded_size(value)
}
pub(super) fn encoded_size(value: &Value) -> Result<usize, String> {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            if self.0 > ARCHIVE_BYTES {
                return Err(io::Error::other("HID recording exceeds 64 MiB"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut count = Counter(0);
    serde_json::to_writer(&mut count, value).map_err(|e| e.to_string())?;
    Ok(count.0)
}
pub(super) fn decimal(value: &Value, name: &str) -> Result<u64, String> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("Invalid replay {name}; expected unsigned decimal string"))
}
pub(super) fn integer(value: &Value, name: &str) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|n| n.try_into().ok())
        .ok_or_else(|| format!("Invalid replay {name}"))
}
