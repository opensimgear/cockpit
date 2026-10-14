use super::*;
use crate::vjoy::{Axis, Status};
// Independent short-item HID reader: verify fields and fixed report offsets,
// rather than only round-tripping the generator's own recognizer.
#[derive(Debug)]
struct Field {
    page: u32,
    usage: u32,
    offset: u32,
    size: u32,
    count: u32,
    maximum: u32,
}
fn fields(bytes: &[u8], slot: u8) -> Vec<Field> {
    let (mut pos, mut offset, mut size, mut count, mut page, mut usage, mut maximum) =
        (0, 0, 0, 0, 0, 0, 0);
    let mut fields = vec![];
    while pos < bytes.len() {
        let key = bytes[pos];
        pos += 1;
        let length = match key & 3 {
            3 => 4,
            n => n as usize,
        };
        let mut value = 0;
        for n in 0..length {
            value |= u32::from(bytes[pos + n]) << (8 * n);
        }
        pos += length;
        match key & 0xfc {
            0x04 => page = value,
            0x08 => usage = value,
            0x18 => usage = value,
            0x24 => maximum = value,
            0x74 => size = value,
            0x94 => count = value,
            0x84 => assert_eq!(value, u32::from(slot)),
            0x80 => {
                if value & 1 == 0 && count != 0 {
                    fields.push(Field {
                        page,
                        usage,
                        offset,
                        size,
                        count,
                        maximum,
                    });
                }
                offset += size * count;
                usage = 0;
            }
            _ => {}
        }
    }
    assert_eq!(
        offset, 768,
        "the driver's fixed 96-byte input payload must never move"
    );
    fields
}
#[test]
fn layouts_have_exact_controls_and_stable_native_offsets() {
    for axes in [0, 1, 255, 0x8000, u16::MAX] {
        for buttons in [0, 1, 32, 128] {
            for hats in [0, 1, 4] {
                let layout = Layout {
                    axes,
                    buttons,
                    hats,
                };
                if axes == 0 && buttons == 0 && hats == 0 {
                    continue;
                }
                for slot in [1, 16] {
                    let bytes = layout.descriptor(slot).unwrap();
                    assert_eq!(Layout::from_descriptor(slot, &bytes).unwrap(), layout);
                    let actual = fields(&bytes, slot);
                    assert_eq!(
                        actual.len(),
                        axes.count_ones() as usize + usize::from(hats) + usize::from(buttons > 0)
                    );
                    for (i, usage) in AXIS_USAGES
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| axes & (1 << i) != 0)
                    {
                        let f = actual
                            .iter()
                            .find(|f| f.page == 1 && f.usage == *usage)
                            .unwrap();
                        assert_eq!(
                            (f.offset, f.size, f.count, f.maximum),
                            (i as u32 * 32, 32, 1, 32767)
                        );
                    }
                    let povs: Vec<_> = actual
                        .iter()
                        .filter(|f| f.page == 1 && f.usage == 0x39)
                        .collect();
                    assert_eq!(povs.len(), hats as usize);
                    for (i, f) in povs.iter().enumerate() {
                        assert_eq!(
                            (f.offset, f.size, f.count, f.maximum),
                            (512 + i as u32 * 32, 32, 1, 35900)
                        );
                    }
                    if buttons > 0 {
                        let f = actual.iter().find(|f| f.page == 9).unwrap();
                        assert_eq!(
                            (f.offset, f.size, f.count, f.maximum),
                            (640, 1, buttons.into(), 1)
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn unknown_corrupted_truncated_descriptors_and_invalid_bounds_are_refused() {
    for layout in [
        Layout::LEGACY,
        Layout::STARTER,
        Layout {
            axes: 0,
            buttons: 128,
            hats: 4,
        },
    ] {
        let bytes = layout.descriptor(2).unwrap();
        for len in 0..bytes.len() {
            assert!(Layout::from_descriptor(2, &bytes[..len]).is_err());
        }
        for index in 0..bytes.len() {
            let mut changed = bytes.clone();
            changed[index] ^= 0x80;
            assert!(
                Layout::from_descriptor(2, &changed).is_err(),
                "mutation {index}"
            );
        }
        assert!(Layout::from_descriptor(1, &bytes).is_err());
    }
    for layout in [
        Layout {
            axes: 0,
            buttons: 0,
            hats: 0,
        },
        Layout {
            axes: 1,
            buttons: 129,
            hats: 0,
        },
        Layout {
            axes: 1,
            buttons: 1,
            hats: 5,
        },
    ] {
        assert!(layout.validate().is_err());
    }
    for slot in [0, 17, 255] {
        assert!(Layout::STARTER.descriptor(slot).is_err());
    }
    assert!(Layout::from_descriptor(1, &[0; 181]).is_err());
}
#[test]
fn capability_readback_rejects_unknown_axes_ranges_and_ffb() {
    let slot = Slot {
        id: 2,
        status: Status::Free,
        owner_pid: None,
        buttons: 32,
        axes: AXIS_USAGES[..8]
            .iter()
            .map(|usage| Axis {
                usage: *usage,
                min: 0,
                max: 32767,
            })
            .collect(),
        continuous_hats: 1,
        discrete_hats: 0,
        force_feedback: false,
    };
    assert_eq!(Layout::from_slot(&slot).unwrap(), Layout::STARTER);
    let mut bad = slot.clone();
    bad.force_feedback = true;
    assert!(Layout::from_slot(&bad).is_err());
    let mut bad = slot.clone();
    bad.discrete_hats = 1;
    assert!(Layout::from_slot(&bad).is_err());
    let mut bad = slot.clone();
    bad.buttons = 256;
    assert!(Layout::from_slot(&bad).is_err());
    let mut bad = slot.clone();
    bad.axes[0].usage = 0xffff;
    assert!(Layout::from_slot(&bad).is_err());
    let mut bad = slot.clone();
    bad.axes[0].min = -1;
    assert!(Layout::from_slot(&bad).is_err());
    let mut bad = slot.clone();
    bad.axes[0].max = 255;
    assert!(Layout::from_slot(&bad).is_err());
    let mut bad = slot.clone();
    bad.axes.push(bad.axes[0].clone());
    assert!(Layout::from_slot(&bad).is_err());
    let mut bad = slot;
    bad.axes = vec![bad.axes[0].clone(); 17];
    assert!(Layout::from_slot(&bad).is_err());
}
