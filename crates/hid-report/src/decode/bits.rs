use num_bigint::{BigInt, BigUint, Sign};

/// Extract one bounded field. Result bytes are right-aligned little endian;
/// unused high bits are zero, including for a negative signed interpretation.
pub(super) fn extract(
    payload: &[u8],
    offset: usize,
    width: usize,
    signed: bool,
) -> Option<(BigInt, Vec<u8>)> {
    let bytes = extract_raw(payload, offset, width)?;
    let raw = BigUint::from_bytes_le(&bytes);
    let negative = signed && raw.bit((width - 1) as u64);
    let value = BigInt::from_biguint(Sign::Plus, raw);
    Some((
        if negative {
            value - (BigInt::from(1u8) << width)
        } else {
            value
        },
        bytes,
    ))
}

/// Byte-oriented extraction for opaque/constant data never constructs a number.
pub(super) fn extract_raw(payload: &[u8], offset: usize, width: usize) -> Option<Vec<u8>> {
    let end_bit = offset.checked_add(width)?;
    if width == 0 || end_bit > payload.len().checked_mul(8)? {
        return None;
    }
    let start = offset / 8;
    let shift = offset % 8;
    let mut bytes = vec![0; width.checked_add(7)? / 8];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = payload[start + index] >> shift;
        if shift != 0 {
            *byte |= payload.get(start + index + 1).copied().unwrap_or(0) << (8 - shift);
        }
    }
    if !width.is_multiple_of(8) {
        *bytes.last_mut()? &= (1 << (width % 8)) - 1;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_cross_byte_fields_preserve_raw_bits() {
        let payload = [0x85, 0xff, 0x3f, 0xa8];
        assert_eq!(
            extract(&payload, 0, 12, true),
            Some(((-123).into(), vec![0x85, 0x0f]))
        );
        assert_eq!(
            extract(&payload, 12, 12, true),
            Some((1023.into(), vec![0xff, 0x03]))
        );
        assert_eq!(extract(&payload, 24, 4, false), Some((8.into(), vec![8])));
    }

    #[test]
    fn arbitrary_width_is_exact_instead_of_rounding_through_machine_integers() {
        let mut payload = vec![0xff; 33];
        payload[32] = 1;
        let (value, raw) = extract(&payload, 0, 257, false).unwrap();
        assert_eq!(value, (BigInt::from(1u8) << 257) - 1u8);
        assert_eq!(raw, payload);
        assert_eq!(extract(&payload, 0, 257, true).unwrap().0, BigInt::from(-1));
        assert_eq!(extract(&[0; 8], 0, 64, false).unwrap().1, vec![0; 8]);
    }

    #[test]
    fn invalid_extents_and_arithmetic_overflow_are_rejected() {
        assert!(extract(&[], 0, 1, false).is_none());
        assert!(extract(&[0], 0, 0, false).is_none());
        assert!(extract(&[0], 1, 8, false).is_none());
        assert!(extract(&[0], usize::MAX, 1, false).is_none());
    }
}
