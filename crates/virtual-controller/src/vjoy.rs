//! Exact vJoy 2.2.2 capability and V3 report contract.
use osg_mapping::{
    engine::TargetState,
    profile::{AxisRange, Layout},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const VERSION: u16 = 0x222;
pub const AXIS_USAGES: [u32; 16] = [
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0xc4, 0xc5, 0xc6, 0xc8, 0xb0, 0xba, 0xbb,
];
const AXIS_WORDS: [usize; 16] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 2, 1, 0];
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Owned,
    Free,
    Busy,
    Missing,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Axis {
    pub usage: u32,
    pub min: i32,
    pub max: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Slot {
    pub id: u8,
    pub status: Status,
    pub owner_pid: Option<u32>,
    pub buttons: u16,
    pub axes: Vec<Axis>,
    pub discrete_hats: u8,
    pub continuous_hats: u8,
    pub force_feedback: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub target: String,
    pub slot: u8,
    pub axes: Vec<u32>,
    pub layout: Layout,
}
impl Binding {
    pub fn validate(&self) -> Result<(), String> {
        self.layout.validate()?;
        if !(1..=16).contains(&self.slot)
            || self.target.is_empty()
            || self.target.len() > 64
            || !self
                .target
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || self.axes.len() != self.layout.axes.len()
            || self.axes.iter().collect::<BTreeSet<_>>().len() != self.axes.len()
            || self.axes.iter().any(|u| !AXIS_USAGES.contains(u))
        {
            return Err("Invalid vJoy slot, target or explicit axis assignment.".into());
        }
        Ok(())
    }
    pub fn match_slot(&self, slot: &Slot) -> Result<(), String> {
        self.validate()?;
        if slot.id != self.slot
            || slot.buttons != self.layout.buttons
            || slot.continuous_hats != self.layout.hats
            || slot.discrete_hats != 0
            || slot.force_feedback
            || slot.axes.len() != self.axes.len()
            || self
                .axes
                .iter()
                .any(|u| !slot.axes.iter().any(|a| a.usage == *u))
            || slot.axes.iter().any(|a| a.min != 0 || a.max != 32767)
        {
            return Err(format!("Slot {} does not match the requested complete layout, or has unsupported FFB/POV/range capabilities.", self.slot));
        }
        Ok(())
    }
    pub fn fallback(&self) -> TargetState {
        TargetState {
            buttons: vec![false; self.layout.buttons as usize],
            axes: self.layout.axes.iter().map(|a| a.fallback).collect(),
            hats: vec![None; self.layout.hats as usize],
        }
    }
    pub fn report(&self, state: &TargetState) -> Result<Position, String> {
        self.validate()?;
        if state.buttons.len() != self.layout.buttons as usize
            || state.axes.len() != self.axes.len()
            || state.hats.len() != self.layout.hats as usize
            || state.hats.iter().any(|h| h.is_some_and(|v| v > 7))
            || state
                .axes
                .iter()
                .zip(&self.layout.axes)
                .any(|(v, a)| !a.range.contains(*v))
        {
            return Err("Complete virtual report does not match the target layout.".into());
        }
        let mut report = Position {
            device: self.slot,
            padding: [0; 3],
            words: [0; 30],
        };
        for index in 19..=22 {
            report.words[index] = -1;
        }
        for (index, pressed) in state.buttons.iter().enumerate() {
            if *pressed {
                report.words[[18, 23, 24, 25][index / 32]] |= (1u32 << (index % 32)) as i32;
            }
        }
        for ((usage, value), axis) in self.axes.iter().zip(&state.axes).zip(&self.layout.axes) {
            let value = if axis.range == AxisRange::Bipolar {
                ((i64::from(*value) + 32768) * 32767 / 65535) as i32
            } else {
                *value
            };
            report.words[AXIS_WORDS[AXIS_USAGES.iter().position(|u| u == usage).unwrap()]] = value;
        }
        for (index, hat) in state.hats.iter().enumerate() {
            report.words[19 + index] = hat.map_or(-1, |h| i32::from(h) * 4500);
        }
        Ok(report)
    }
}
/// The selected SDK uses four-byte packing, a byte ID, and thirty 32-bit words.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Position {
    pub device: u8,
    pub padding: [u8; 3],
    pub words: [i32; 30],
}
const _: () = assert!(std::mem::size_of::<Position>() == 124);
pub trait Driver {
    /// Persisted templates are prepared before acquisition; installed vJoy slots need no setup.
    fn prepare(&mut self, _bindings: &[Binding]) -> Result<(), String> {
        Ok(())
    }
    /// Creator-handle devices disappear on process exit and must never be recreated for recovery.
    fn ephemeral(&self) -> bool {
        false
    }
    fn check(&mut self) -> Result<(), String>;
    fn inspect(&mut self, slot: u8) -> Result<Slot, String>;
    fn acquire(&mut self, slot: u8) -> Result<(), String>;
    fn submit(&mut self, slot: u8, position: &Position) -> Result<(), String>;
    fn release(&mut self, slot: u8);
}
pub fn inventory(driver: &mut impl Driver) -> Result<Vec<Slot>, String> {
    driver.check()?;
    let mut slots = Vec::new();
    for id in 1..=16 {
        let slot = driver.inspect(id)?;
        if slot.status != Status::Missing {
            slots.push(slot);
        }
    }
    Ok(slots)
}
#[cfg(test)]
#[path = "vjoy_tests.rs"]
pub(crate) mod tests;
