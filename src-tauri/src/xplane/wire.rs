fn subscriptions() -> Vec<String> {
    subscriptions_for(&legacy_channels())
}
fn subscriptions_for(channels: &[datarefs::Channel]) -> Vec<String> {
    GUARDS
        .iter()
        .map(|s| s.to_string())
        .chain((0..PATH_BYTES).map(|i| format!("sim/aircraft/view/acf_relative_path[{i}]")))
        .chain(channels.iter().map(|c| c.dataref.clone()))
        .collect()
}
fn request(index: i32, hz: i32, path: &str) -> Vec<u8> {
    let mut bytes = vec![0; 413];
    bytes[..5].copy_from_slice(b"RREF\0");
    bytes[5..9].copy_from_slice(&hz.to_le_bytes());
    bytes[9..13].copy_from_slice(&index.to_le_bytes());
    bytes[13..13 + path.len()].copy_from_slice(path.as_bytes());
    bytes
}
fn decode(bytes: &[u8], base: i32, count: usize) -> Result<Vec<(usize, f64)>, String> {
    decode_wire(bytes)?.into_iter().map(|(wire, number)| {
        let index = wire.checked_sub(base)
            .and_then(|n| usize::try_from(n).ok()).filter(|n| *n < count)
            .ok_or(UNSUBSCRIBED)?;
        Ok((index, number))
    }).collect()
}

fn decode_wire(bytes: &[u8]) -> Result<Vec<(i32, f64)>, String> {
    if bytes.len() < 13
        || bytes.len() > MAX_PACKET
        || &bytes[..5] != b"RREF,"
        || (bytes.len() - 5) % 8 != 0
    {
        return Err("Malformed X-Plane RREF packet.".into());
    }
    let mut seen = BTreeSet::new();
    bytes[5..]
        .chunks_exact(8)
        .map(|pair| {
            let wire = i32::from_le_bytes(pair[..4].try_into().unwrap());
            let number = f32::from_le_bytes(pair[4..].try_into().unwrap()) as f64;
            if !number.is_finite() || !seen.insert(wire) {
                return Err("Non-finite or duplicate X-Plane dataref.".into());
            }
            Ok((wire, number))
        })
        .collect()
}

/// A retired index can share a datagram with a healthy subscription. Validate
/// the entire wire report before mapping any values, then discard retired rows.
fn decode_selected(bytes: &[u8], indices: &BTreeMap<i32, Vec<usize>>) -> Result<Vec<(usize, f64)>, String> {
    Ok(decode_wire(bytes)?.into_iter().flat_map(|(wire, value)| {
        indices.get(&wire).into_iter().flatten().map(move |index| (*index, value))
    }).collect())
}
