//! USB HID keyboard page 0x07 to IBM set-1 scan codes. No character substitution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanCode {
    pub code: u16,
    pub extended: bool,
}
pub fn scan_code(usage: u16) -> Result<ScanCode, String> {
    let (code, extended) = match usage {
        0x04..=0x1d => (
            [
                0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31,
                0x18, 0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
            ][(usage - 4) as usize],
            false,
        ),
        0x1e..=0x26 => (usage - 0x1c, false),
        0x27 => (0x0b, false),
        0x28 => (0x1c, false),
        0x29 => (0x01, false),
        0x2a => (0x0e, false),
        0x2b => (0x0f, false),
        0x2c => (0x39, false),
        0x2d => (0x0c, false),
        0x2e => (0x0d, false),
        0x2f => (0x1a, false),
        0x30 => (0x1b, false),
        0x31 | 0x32 => (0x2b, false),
        0x33 => (0x27, false),
        0x34 => (0x28, false),
        0x35 => (0x29, false),
        0x36 => (0x33, false),
        0x37 => (0x34, false),
        0x38 => (0x35, false),
        0x39 => (0x3a, false),
        0x3a..=0x43 => (usage + 1, false),
        0x44 => (0x57, false),
        0x45 => (0x58, false),
        0x46 => (0x37, true),
        0x47 => (0x46, false),
        // Pause has an E1 multi-event encoding and is intentionally unsupported.
        0x49 => (0x52, true),
        0x4a => (0x47, true),
        0x4b => (0x49, true),
        0x4c => (0x53, true),
        0x4d => (0x4f, true),
        0x4e => (0x51, true),
        0x4f => (0x4d, true),
        0x50 => (0x4b, true),
        0x51 => (0x50, true),
        0x52 => (0x48, true),
        0x53 => (0x45, false),
        0x54 => (0x35, true),
        0x55 => (0x37, false),
        0x56 => (0x4a, false),
        0x57 => (0x4e, false),
        0x58 => (0x1c, true),
        0x59..=0x61 => (
            [0x4f, 0x50, 0x51, 0x4b, 0x4c, 0x4d, 0x47, 0x48, 0x49][(usage - 0x59) as usize],
            false,
        ),
        0x62 => (0x52, false),
        0x63 => (0x53, false),
        0x64 => (0x56, false),
        0x65 => (0x5d, true),
        0xe0 => (0x1d, false),
        0xe1 => (0x2a, false),
        0xe2 => (0x38, false),
        0xe3 => (0x5b, true),
        0xe4 => (0x1d, true),
        0xe5 => (0x36, false),
        0xe6 => (0x38, true),
        0xe7 => (0x5c, true),
        _ => {
            return Err(format!(
                "Unsupported physical keyboard HID usage 0x{usage:04x}."
            ))
        }
    };
    Ok(ScanCode { code, extended })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_layouts_modifiers_and_keypad_are_distinct() {
        assert_eq!(scan_code(0x1c).unwrap().code, 0x15); // physical Y, German Z
        assert_eq!(scan_code(0x1d).unwrap().code, 0x2c);
        assert_eq!(
            scan_code(0xe6).unwrap(),
            ScanCode {
                code: 0x38,
                extended: true
            }
        ); // AltGr
        assert_ne!(scan_code(0x28), scan_code(0x58));
        assert_ne!(scan_code(0x52), scan_code(0x60));
        assert_eq!(scan_code(0x64).unwrap().code, 0x56);
        assert!(scan_code(0x48).is_err());
        assert!(scan_code(0xff).is_err());
    }
    #[test]
    fn every_supported_usage_has_an_explicit_physical_mapping() {
        for usage in 0..=255 {
            let supported = matches!(usage,0x04..=0x47|0x49..=0x65|0xe0..=0xe7);
            let result = scan_code(usage);
            assert_eq!(result.is_ok(), supported, "usage {usage:x}");
            if let Ok(scan) = result {
                assert!((1..=0x7f).contains(&scan.code));
            }
        }
    }
}
