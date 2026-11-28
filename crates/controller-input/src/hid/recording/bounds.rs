use serde_json::Value;

pub(super) const VALUE_BYTES: usize = 2 * 1024 * 1024;
pub(super) const WRAPPER_BYTES: usize = "{\"recording\":}".len();

fn shape(value: &Value, max_depth: usize) -> bool {
    fn walk(value: &Value, depth: usize, max_depth: usize, nodes: &mut usize) -> bool {
        *nodes += 1;
        if depth > max_depth || *nodes > 2_000_000 {
            return false;
        }
        match value {
            Value::Array(values) => values.iter().all(|v| walk(v, depth + 1, max_depth, nodes)),
            Value::Object(values) => values
                .values()
                .all(|v| walk(v, depth + 1, max_depth, nodes)),
            _ => true,
        }
    }
    walk(value, 1, max_depth, &mut 0)
}

pub(super) fn descriptor(value: &Value) -> bool {
    // The downloadable wrapper and archive add two containers around this value.
    value.is_object() && shape(value, 46) && value.to_string().len() <= VALUE_BYTES
}

pub(super) fn row(value: &Value) -> bool {
    // Wrapper, archive, and records array surround every replay observation.
    shape(value, 45)
        && value["samples"].is_object()
        && value.get("kind").is_none()
        && value.get("wireHex").is_none_or(|hex| {
            hex.as_str().is_some_and(|hex| {
                hex.len() <= 131072
                    && hex.len() % 2 == 0
                    && hex.bytes().all(|b| b.is_ascii_hexdigit())
            })
        })
}

pub(super) fn reason(value: &Value) -> bool {
    value.is_object() && shape(value, 44) && value.to_string().len() <= 2048
}
