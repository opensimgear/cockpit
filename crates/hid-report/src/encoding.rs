use serde::Serializer;

/// Keep binary evidence in wire order when a diagnostic is exported as JSON.
pub(crate) fn hex<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    serializer.serialize_str(&text)
}

pub(crate) fn bound<S: Serializer>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => serializer.collect_str(value),
        None => serializer.serialize_none(),
    }
}
