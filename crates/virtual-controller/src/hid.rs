//! Generic joystick descriptors and complete HID input reports for the Apple helper.
use crate::uinput::{Event, Template};
use crate::vjoy::Binding;
use std::collections::BTreeMap;
#[derive(Clone)]
pub struct Joystick {
    pub template: Template,
    pub descriptor: Vec<u8>,
}
impl Joystick {
    pub fn new(binding: Binding) -> Result<Self, String> {
        let template = Template::new(binding)?;
        let b = &template.binding;
        let mut d = vec![0x05, 1, 0x09, 4, 0xa1, 1];
        if b.layout.buttons > 0 {
            d.extend([
                0x05,
                9,
                0x19,
                1,
                0x29,
                b.layout.buttons as u8,
                0x15,
                0,
                0x25,
                1,
                0x75,
                1,
                0x95,
                b.layout.buttons as u8,
                0x81,
                2,
            ]);
            let pad = (8 - b.layout.buttons % 8) % 8;
            if pad > 0 {
                d.extend([0x75, 1, 0x95, pad as u8, 0x81, 3]);
            }
        }
        if !b.axes.is_empty() {
            d.extend([0x05, 1, 0x15, 0, 0x26, 0xff, 0x7f, 0x75, 16, 0x95, 1]);
            for usage in &b.axes {
                d.extend([0x09, *usage as u8, 0x81, 2]);
            }
        }
        if b.layout.hats > 0 {
            d.extend([
                0x05,
                1,
                0x15,
                0,
                0x25,
                7,
                0x35,
                0,
                0x46,
                0x3b,
                1,
                0x65,
                0x14,
                0x75,
                4,
                0x95,
                b.layout.hats,
                0x09,
                0x39,
                0x81,
                0x42,
            ]);
            if b.layout.hats % 2 == 1 {
                d.extend([0x75, 4, 0x95, 1, 0x81, 3]);
            }
            d.extend([0x65, 0]);
        }
        d.push(0xc0);
        Ok(Self {
            template,
            descriptor: d,
        })
    }
    pub fn serial(&self) -> String {
        format!(
            "opensimgear-virtual:macos-hid:slot:{}",
            self.template.binding.slot
        )
    }
    pub fn report(&self, events: &[Event]) -> Result<Vec<u8>, String> {
        let b = &self.template.binding;
        let expected =
            b.axes.len() + usize::from(b.layout.buttons) + 2 * usize::from(b.layout.hats) + 1;
        if events.len() != expected
            || events.last()
                != Some(&Event {
                    kind: 0,
                    code: 0,
                    value: 0,
                })
        {
            return Err("Incomplete HID report batch.".into());
        }
        let mut values = BTreeMap::new();
        for e in &events[..events.len() - 1] {
            if values.insert((e.kind, e.code), e.value).is_some() {
                return Err("Duplicate HID report control.".into());
            }
        }
        let mut bytes = vec![0u8; usize::from(b.layout.buttons).div_ceil(8)];
        for n in 0..b.layout.buttons {
            match values.remove(&(1, 0x120 + n)) {
                Some(0) => {}
                Some(1) => bytes[usize::from(n / 8)] |= 1 << (n % 8),
                _ => return Err("Missing or invalid HID button.".into()),
            }
        }
        for axis in &b.axes {
            let value = values
                .remove(&(3, (*axis - 0x30) as u16))
                .filter(|n| (0..=32767).contains(n))
                .ok_or("Missing or invalid HID axis.")?;
            bytes.extend((value as u16).to_le_bytes());
        }
        let mut hats = vec![0u8; usize::from(b.layout.hats).div_ceil(2)];
        for n in 0..b.layout.hats {
            let x = values
                .remove(&(3, 16 + 2 * u16::from(n)))
                .ok_or("Missing HID hat X.")?;
            let y = values
                .remove(&(3, 17 + 2 * u16::from(n)))
                .ok_or("Missing HID hat Y.")?;
            let direction = match (x, y) {
                (0, 0) => 8,
                (0, -1) => 0,
                (1, -1) => 1,
                (1, 0) => 2,
                (1, 1) => 3,
                (0, 1) => 4,
                (-1, 1) => 5,
                (-1, 0) => 6,
                (-1, -1) => 7,
                _ => return Err("Invalid HID hat vector.".into()),
            };
            hats[usize::from(n / 2)] |= direction << ((n % 2) * 4);
        }
        if !values.is_empty() {
            return Err("Unknown HID report controls.".into());
        }
        bytes.extend(hats);
        Ok(bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::uinput::tests::binding;
    #[test]
    fn complete_reports_and_descriptors_agree_for_all_bounded_layouts() {
        for buttons in [0, 1, 7, 8, 9, 32] {
            for axes in [0, 1, 8] {
                for hats in [0, 1, 2, 3, 4] {
                    let mut b = binding(1);
                    b.layout.buttons = buttons;
                    b.axes = (0..axes).map(|n| 0x30 + n).collect();
                    b.layout.axes.resize(
                        axes as usize,
                        osg_mapping::profile::Axis {
                            range: osg_mapping::profile::AxisRange::Unipolar,
                            fallback: 0,
                        },
                    );
                    b.layout.hats = hats;
                    if buttons == 0 && axes == 0 && hats == 0 {
                        assert!(Joystick::new(b).is_err());
                        continue;
                    }
                    let j = Joystick::new(b.clone()).unwrap();
                    let report = j
                        .report(
                            &j.template
                                .events(&b.report(&b.fallback()).unwrap())
                                .unwrap(),
                        )
                        .unwrap();
                    // Interpret HID ReportSize/Count and Input items independently, including padding.
                    let (mut size, mut count, mut bits) = (0, 0, 0);
                    let mut offset = 0;
                    while offset < j.descriptor.len() {
                        let prefix = j.descriptor[offset];
                        let len = match prefix & 3 {
                            3 => 4,
                            n => n as usize,
                        };
                        let value = (0..len).fold(0u32, |v, n| {
                            v | ((j.descriptor[offset + 1 + n] as u32) << (8 * n))
                        });
                        match prefix & 0xfc {
                            0x74 => size = value,
                            0x94 => count = value,
                            0x80 => bits += size * count,
                            _ => {}
                        };
                        offset += len + 1;
                    }
                    assert_eq!(bits as usize, report.len() * 8);
                    assert!(j.serial().starts_with("opensimgear-virtual:"));
                }
            }
        }
    }
    #[test]
    fn exact_edges_diagonal_hats_ranges_and_invalid_batches() {
        let b = binding(1);
        let j = Joystick::new(b.clone()).unwrap();
        let mut state = b.fallback();
        state.buttons[0] = true;
        state.buttons[31] = true;
        state.axes = vec![0x1234, 32767];
        for hat in (0..8).map(Some).chain([None]) {
            state.hats = vec![hat; 4];
            let events = j.template.events(&b.report(&state).unwrap()).unwrap();
            let report = j.report(&events).unwrap();
            assert_eq!(&report[..8], &[1, 0, 0, 128, 0x34, 0x12, 0xff, 0x7f]);
            let v = hat.unwrap_or(8);
            assert_eq!(&report[8..], &[v | (v << 4); 2]);
            for index in 0..events.len() {
                let mut invalid = events.clone();
                invalid.remove(index);
                assert!(j.report(&invalid).is_err());
            }
            for index in 0..events.len() - 1 {
                let mut invalid = events.clone();
                invalid[index].value = i32::MAX;
                assert!(j.report(&invalid).is_err());
                let mut invalid = events.clone();
                invalid[index].code = 65535;
                assert!(j.report(&invalid).is_err());
            }
            let mut duplicate = events.clone();
            duplicate[0] = duplicate[1].clone();
            assert!(j.report(&duplicate).is_err());
        }
    }
}
