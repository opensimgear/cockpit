#![allow(dead_code)] // Shared helpers are intentionally used by different integration test targets.
use serde_json::Value;
use std::{fs, path::PathBuf};

pub fn corpus(folder: &str) -> Vec<Value> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/hid-contract")
        .join(folder);
    let mut paths: Vec<_> = fs::read_dir(directory)
        .expect("H0 fixture directory")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "H0 corpus must not disappear");
    paths
        .into_iter()
        .map(|path| {
            serde_json::from_slice(&fs::read(path).expect("fixture bytes")).expect("fixture JSON")
        })
        .collect()
}

pub fn hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "complete hex bytes");
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("lowercase fixture hex"),
            };
            digit(pair[0]) * 16 + digit(pair[1])
        })
        .collect()
}

/// Independent decimal oracle: accumulate unsigned little-endian bits in base 10.
/// Negative two's-complement values first invert/add one inside the declared width.
/// No machine-width integer or implementation bigint is used for golden expectations.
pub fn decimal(payload: &[u8], offset: usize, width: usize, signed: bool) -> String {
    assert!(width > 0 && offset + width <= payload.len() * 8);
    let mut bits: Vec<bool> = (0..width)
        .map(|bit| payload[(offset + bit) / 8] & (1 << ((offset + bit) % 8)) != 0)
        .collect();
    let negative = signed && bits[width - 1];
    if negative {
        let mut carry = true;
        for bit in &mut bits {
            let inverted = !*bit;
            *bit = inverted ^ carry;
            carry &= inverted;
        }
    }
    let mut digits = vec![0_u8];
    for bit in bits.into_iter().rev() {
        let mut carry = u8::from(bit);
        for digit in &mut digits {
            let next = *digit * 2 + carry;
            *digit = next % 10;
            carry = next / 10;
        }
        if carry != 0 {
            digits.push(carry);
        }
    }
    let mut text: String = digits
        .into_iter()
        .rev()
        .map(|digit| char::from(b'0' + digit))
        .collect();
    if negative {
        text.insert(0, '-');
    }
    text
}

pub fn put_bits(payload: &mut [u8], offset: usize, width: usize, bytes: &[u8]) {
    assert!(offset + width <= payload.len() * 8);
    assert!(width <= bytes.len() * 8);
    for bit in 0..width {
        let mask = 1 << ((offset + bit) % 8);
        if bytes[bit / 8] & (1 << (bit % 8)) != 0 {
            payload[(offset + bit) / 8] |= mask;
        } else {
            payload[(offset + bit) / 8] &= !mask;
        }
    }
}

pub struct Generator(pub u64);
impl Generator {
    pub fn next(&mut self) -> u64 {
        // Fixed seed and wrapping arithmetic make failed cases reproducible on every target.
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    pub fn bytes(&mut self, length: usize) -> Vec<u8> {
        (0..length).map(|_| (self.next() >> 32) as u8).collect()
    }
}
