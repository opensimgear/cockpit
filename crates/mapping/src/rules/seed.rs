use super::{Rule, State};
use crate::profile::{Kind, Value};

impl State {
    /// Reset this transform and establish its state from the current input baseline.
    ///
    /// A held button becomes the previous level while remaining unarmed, so it
    /// cannot create an edge-triggered action until it has been released.
    pub fn seed(&mut self, rule: &Rule, inputs: &[Value], kind: Kind, now: u64) -> Option<Value> {
        *self = Self::default();
        self.apply(rule, inputs, kind, now)
    }
}
