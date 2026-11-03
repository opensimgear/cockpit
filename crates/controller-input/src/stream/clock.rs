use std::time::Instant;

/// A read-only copy of the journal's monotonic clock. Reading the time never
/// acquires capture state, advances a cursor or creates input evidence.
#[derive(Clone, Copy)]
pub struct Clock {
    pub(super) origin: Instant,
}
impl Clock {
    pub fn now_us(self) -> u64 {
        u64::try_from(self.origin.elapsed().as_micros())
            .unwrap_or(u64::MAX - 1)
            .saturating_add(1)
    }
}
