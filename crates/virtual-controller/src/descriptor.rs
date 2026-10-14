//! Bounded, non-FFB vJoy 2.2.2 descriptors. Generation is implemented but native
//! custom-layout qualification is still required; the elevated entry remains gated.
use crate::vjoy::{Slot, AXIS_USAGES};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Layout {
    pub axes: u16,
    pub buttons: u8,
    pub hats: u8,
}
impl Layout {
    pub const LEGACY: Self = Self {
        axes: u16::MAX,
        buttons: 8,
        hats: 0,
    };
    pub const STARTER: Self = Self {
        axes: 255,
        buttons: 32,
        hats: 1,
    };
    pub fn validate(self) -> Result<(), String> {
        if self.buttons > 128
            || self.hats > 4
            || (self.axes == 0 && self.buttons == 0 && self.hats == 0)
        {
            return Err(
                "Choose at least one control: up to 16 axes, 128 buttons and 4 continuous hats."
                    .into(),
            );
        }
        Ok(())
    }
    pub fn from_slot(slot: &Slot) -> Result<Self, String> {
        if slot.force_feedback
            || slot.discrete_hats != 0
            || slot.buttons > 128
            || slot.axes.len() > 16
        {
            return Err("Existing FFB, discrete POV or unknown layouts are preserved and cannot be provisioned.".into());
        }
        let mut axes = 0;
        for axis in &slot.axes {
            let index = AXIS_USAGES
                .iter()
                .position(|u| *u == axis.usage)
                .ok_or("Unknown vJoy axis.")?;
            if axes & (1 << index) != 0 || axis.min != 0 || axis.max != 32767 {
                return Err("Duplicate axis or unsupported vJoy range.".into());
            }
            axes |= 1 << index;
        }
        let layout = Self {
            axes,
            buttons: slot.buttons as u8,
            hats: slot.continuous_hats,
        };
        layout.validate()?;
        Ok(layout)
    }
    pub fn descriptor(self, slot: u8) -> Result<Vec<u8>, String> {
        self.validate()?;
        if !(1..=16).contains(&slot) {
            return Err("vJoy slot must be 1–16.".into());
        }
        let mut bytes = vec![
            0x05, 0x01, 0x15, 0x00, 0x09, 0x04, 0xa1, 0x01, 0x05, 0x01, 0x85, slot, 0x09, 0x01,
            0x15, 0x00, 0x26, 0xff, 0x7f, 0x75, 0x20, 0x95, 0x01, 0xa1, 0x00,
        ];
        // Keep every native field in place, including absent axes and hats.
        for (i, usage) in AXIS_USAGES.iter().enumerate() {
            if self.axes & (1 << i) != 0 {
                bytes.extend([0x09, *usage as u8, 0x81, 0x02]);
            } else {
                bytes.extend([0x81, 0x01]);
            }
        }
        bytes.push(0xc0);
        if self.hats == 0 {
            bytes.extend([0x75, 0x20, 0x95, 0x04, 0x81, 0x01]);
        } else {
            bytes.extend([
                0x15, 0x00, 0x27, 0x3c, 0x8c, 0x00, 0x00, 0x35, 0x00, 0x47, 0x3c, 0x8c, 0x00, 0x00,
                0x65, 0x14, 0x75, 0x20, 0x95, 0x01,
            ]);
            for _ in 0..self.hats {
                bytes.extend([0x09, 0x39, 0x81, 0x02]);
            }
            if self.hats < 4 {
                bytes.extend([0x95, 4 - self.hats, 0x81, 0x01]);
            }
        }
        bytes.extend([
            0x05,
            0x09,
            0x15,
            0x00,
            0x25,
            0x01,
            0x55,
            0x00,
            0x65,
            0x00,
            0x19,
            u8::from(self.buttons != 0),
            0x29,
            self.buttons,
            0x75,
            0x01,
            0x95,
            self.buttons,
            0x81,
            0x02,
        ]);
        if self.buttons < 128 {
            bytes.extend([0x75, 128 - self.buttons, 0x95, 0x01, 0x81, 0x01]);
        }
        bytes.push(0xc0);
        Ok(bytes)
    }
    /// Recognize only the exact canonical grammar we emit, never arbitrary HID bytes.
    /// Regeneration validates every byte before recovery is allowed to remove a key.
    pub fn from_descriptor(slot: u8, bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 70 || bytes.len() > 180 {
            return Err("Unsupported vJoy descriptor size.".into());
        }
        let mut cursor = 25;
        let mut axes = 0;
        for (i, usage) in AXIS_USAGES.iter().enumerate() {
            if bytes.get(cursor..cursor + 4) == Some(&[0x09, *usage as u8, 0x81, 0x02]) {
                axes |= 1 << i;
                cursor += 4;
            } else if bytes.get(cursor..cursor + 2) == Some(&[0x81, 0x01]) {
                cursor += 2;
            } else {
                return Err("Unsupported axis descriptor.".into());
            }
        }
        cursor += 1; // End collection, verified by complete regeneration below.
        let mut hats = 0;
        if bytes.get(cursor..cursor + 2) == Some(&[0x15, 0x00]) {
            cursor += 20;
            while bytes.get(cursor..cursor + 4) == Some(&[0x09, 0x39, 0x81, 0x02]) && hats < 4 {
                hats += 1;
                cursor += 4;
            }
            if hats < 4 {
                cursor += 4;
            }
        } else {
            cursor += 6;
        }
        let buttons = *bytes.get(cursor + 13).ok_or("Missing button descriptor.")?;
        let layout = Self {
            axes,
            buttons,
            hats,
        };
        if layout.descriptor(slot)? != bytes {
            return Err("Descriptor is not a supported canonical layout.".into());
        }
        Ok(layout)
    }
}
#[cfg(test)]
#[path = "descriptor_tests.rs"]
mod tests;
