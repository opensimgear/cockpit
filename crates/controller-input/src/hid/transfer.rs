pub(super) const MAX_CHUNK_BYTES: usize = 32768;
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub(super) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if c.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
pub(super) fn decode(text: &str) -> Result<Vec<u8>, String> {
    if text.is_empty() || text.len() > MAX_CHUNK_BYTES.div_ceil(3) * 4 || text.len() % 4 != 0 {
        return Err("Invalid replay base64 chunk size".into());
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for (i, c) in text.as_bytes().chunks_exact(4).enumerate() {
        let mut n = 0u32;
        let padding = usize::from(c[3] == b'=') + usize::from(c[2] == b'=');
        if padding > 0 && (i + 1 != text.len() / 4 || c[3] != b'=') {
            return Err("Invalid replay base64 padding".into());
        }
        for (j, b) in c.iter().enumerate() {
            let digit = if *b == b'=' && j >= 4 - padding {
                0
            } else {
                ALPHABET
                    .iter()
                    .position(|v| v == b)
                    .ok_or("Invalid replay base64 chunk")? as u32
            };
            n = (n << 6) | digit;
        }
        if (padding == 1 && n & 255 != 0) || (padding == 2 && n & 65535 != 0) {
            return Err("Invalid replay base64 padding bits".into());
        }
        out.push((n >> 16) as u8);
        if padding < 2 {
            out.push((n >> 8) as u8);
        }
        if padding == 0 {
            out.push(n as u8);
        }
    }
    if out.len() > MAX_CHUNK_BYTES {
        return Err("Invalid replay base64 chunk size".into());
    }
    Ok(out)
}
