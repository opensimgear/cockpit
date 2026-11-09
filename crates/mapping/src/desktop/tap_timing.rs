use super::*;

impl Scheduler {
    /// Native output starts tap dwell from acknowledged physical transitions.
    /// The portable scheduler uses its explicit simulation clock directly.
    pub fn use_native_tap_timing(&mut self) -> Result<(), String> {
        if self.sequence != 0 || self.now != 0 || self.native_tap_timing {
            return Err("Configure native tap timing before starting the scheduler.".into());
        }
        self.capture(Command::NativeTapTiming)?;
        self.native_tap_timing = true;
        Ok(())
    }

    pub fn awaits_tap_receipt(&self, action_id: &str, operation_id: Counter) -> bool {
        self.states.get(action_id).and_then(|state| state.tap_pending) == Some(operation_id)
    }

    pub fn uses_native_tap_timing(&self, action_id: &str) -> bool {
        self.native_tap_timing
            && matches!(self.definitions.get(action_id), Some(Action::KeyTap { .. } | Action::MouseClick { .. }))
    }

    /// A successful receipt comes with the current admission clock time.
    /// A rejected transition retires its exact queued taps and requests cleanup.
    pub fn finish_native_tap(
        &mut self,
        action_id: &str,
        operation_id: Counter,
        at_us: Option<Counter>,
    ) -> Result<(), String> {
        if !self.awaits_tap_receipt(action_id, operation_id) {
            return Ok(());
        }
        self.capture(Command::TapReceipt { action_id: action_id.into(), operation_id, at_us })?;
        let Some(at_us) = at_us else {
            self.cancel(action_id, "Native tap transition was not acknowledged.");
            return Ok(());
        };
        let (down_ms, up_ms) = match self.definitions[action_id] {
            Action::KeyTap { down_ms, up_ms, .. } | Action::MouseClick { down_ms, up_ms, .. } => (down_ms, up_ms),
            _ => unreachable!("only tap actions await a receipt"),
        };
        let state = self.states.get_mut(action_id).expect("pending tap action");
        let dwell_ms = if state.tap_down { down_ms } else { up_ms };
        state.tap_pending = None;
        // The admission clock truncates fractional microseconds.
        state.due = Some(self.now.max(at_us.0).saturating_add(u64::from(dwell_ms) * 1000 + 1));
        Ok(())
    }

    pub(super) fn emit_tap(
        &mut self,
        id: &str,
        producer: Token,
        intent: Intent,
        dwell_ms: u32,
    ) -> Result<(), String> {
        self.emit(id, producer, intent)?;
        let state = self.states.get_mut(id).expect("tap action exists");
        if self.native_tap_timing {
            state.due = None;
            state.tap_pending = Some(Counter(self.sequence));
        } else {
            state.due = Some(self.now.saturating_add(u64::from(dwell_ms) * 1000));
        }
        Ok(())
    }
}
