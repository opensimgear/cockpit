//! Linux templates and complete event batches, sharing the bounded feeder protocol.
//! Slot numbers are local template identities; they are never /dev/input/eventN.
use crate::vjoy::{Axis, Binding, Driver, Position, Slot, Status};
use std::collections::BTreeMap;
pub const QUALIFIED: bool = false;
pub const REASON: &str = "Linux virtual output is disabled until native uinput, lifetime and game-binding qualification is complete. Recording preview remains available.";
pub const MAX_BUTTONS: u16 = 32;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}
#[derive(Clone, Debug)]
pub struct Template {
    pub binding: Binding,
    pub name: String,
    pub physical: String,
}
impl Template {
    pub fn new(binding: Binding) -> Result<Self, String> {
        binding.validate()?;
        if binding.layout.buttons > MAX_BUTTONS
            || binding.axes.iter().any(|a| !(0x30..=0x37).contains(a))
        {
            return Err("This virtual backend supports eight standard axes, 32 buttons and four hats; unsupported layouts are never truncated.".into());
        }
        Ok(Self {
            name: format!("OpenSimGear Cockpit {}", binding.slot),
            physical: format!("opensimgear-virtual:uinput:slot:{}", binding.slot),
            binding,
        })
    }
    pub fn slot(&self, owned: bool) -> Slot {
        Slot {
            id: self.binding.slot,
            status: if owned { Status::Owned } else { Status::Free },
            owner_pid: owned.then_some(std::process::id()),
            buttons: self.binding.layout.buttons,
            axes: self
                .binding
                .axes
                .iter()
                .map(|usage| Axis {
                    usage: *usage,
                    min: 0,
                    max: 32767,
                })
                .collect(),
            discrete_hats: 0,
            continuous_hats: self.binding.layout.hats,
            force_feedback: false,
        }
    }
    pub fn events(&self, report: &Position) -> Result<Vec<Event>, String> {
        if report.device != self.binding.slot {
            return Err("Wrong uinput template identity.".into());
        }
        let mut result = vec![];
        for usage in &self.binding.axes {
            let value = report.words[(*usage - 0x30) as usize + 3];
            if !(0..=32767).contains(&value) {
                return Err("uinput axis outside configured bounds.".into());
            }
            result.push(Event {
                kind: 3,
                code: (usage - 0x30) as u16,
                value,
            });
        }
        for index in 0..self.binding.layout.buttons {
            result.push(Event {
                kind: 1,
                code: 0x120 + index,
                value: ((report.words[18] as u32 >> index) & 1) as i32,
            });
        }
        for index in 0..self.binding.layout.hats {
            let value = report.words[19 + index as usize];
            let (x, y) = match value {
                -1 => (0, 0),
                0 => (0, -1),
                4500 => (1, -1),
                9000 => (1, 0),
                13500 => (1, 1),
                18000 => (0, 1),
                22500 => (-1, 1),
                27000 => (-1, 0),
                31500 => (-1, -1),
                _ => return Err("Invalid uinput hat direction.".into()),
            };
            result.push(Event {
                kind: 3,
                code: 16 + 2 * u16::from(index),
                value: x,
            });
            result.push(Event {
                kind: 3,
                code: 17 + 2 * u16::from(index),
                value: y,
            });
        }
        result.push(Event {
            kind: 0,
            code: 0,
            value: 0,
        });
        Ok(result)
    }
}
/// A creator must close its handle on Drop, including failed setup or write paths.
pub trait Device {
    fn emit(&mut self, events: &[Event]) -> Result<(), String>;
}
pub trait Backend {
    type Device: Device;
    fn ready(&mut self) -> Result<(), String>;
    fn create(&mut self, template: &Template) -> Result<Self::Device, String>;
}
pub struct Output<B: Backend> {
    backend: B,
    templates: BTreeMap<u8, Template>,
    devices: BTreeMap<u8, B::Device>,
}
impl<B: Backend> Output<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            templates: BTreeMap::new(),
            devices: BTreeMap::new(),
        }
    }
}
impl<B: Backend> Drop for Output<B> {
    fn drop(&mut self) {
        // Close creator handles while the backend still holds the process lock.
        self.devices.clear();
    }
}
impl<B: Backend> Driver for Output<B> {
    fn prepare(&mut self, bindings: &[Binding]) -> Result<(), String> {
        if !self.devices.is_empty() || bindings.is_empty() || bindings.len() > 8 {
            return Err("Stop all uinput devices before preparing 1-8 templates.".into());
        }
        let mut templates = BTreeMap::new();
        for binding in bindings {
            let t = Template::new(binding.clone())?;
            if templates.insert(binding.slot, t).is_some() {
                return Err("Duplicate uinput template slot.".into());
            }
        }
        self.templates = templates;
        Ok(())
    }
    fn check(&mut self) -> Result<(), String> {
        self.backend.ready()
    }
    fn inspect(&mut self, slot: u8) -> Result<Slot, String> {
        self.templates
            .get(&slot)
            .map(|t| t.slot(self.devices.contains_key(&slot)))
            .ok_or("Missing uinput template.".into())
    }
    fn acquire(&mut self, slot: u8) -> Result<(), String> {
        if self.devices.contains_key(&slot) {
            return Err("uinput template is already owned.".into());
        }
        let template = self
            .templates
            .get(&slot)
            .ok_or("Missing uinput template.")?;
        let device = self.backend.create(template)?;
        self.devices.insert(slot, device);
        Ok(())
    }
    fn submit(&mut self, slot: u8, position: &Position) -> Result<(), String> {
        let events = self
            .templates
            .get(&slot)
            .ok_or("Missing uinput template.")?
            .events(position)?;
        self.devices
            .get_mut(&slot)
            .ok_or("uinput creator handle is not owned.")?
            .emit(&events)
    }
    fn release(&mut self, slot: u8) {
        self.devices.remove(&slot);
    }
    fn ephemeral(&self) -> bool {
        true
    }
}
#[cfg(test)]
#[path = "uinput_tests.rs"]
pub(crate) mod tests;
