use std::time::{Duration, Instant};

/// A read-only copy of the journal's monotonic clock. Reading the time never
/// acquires capture state, advances a cursor or creates input evidence.
#[derive(Clone, Copy)]
pub struct Clock {
    pub(super) origin: Instant,
    pub(super) id: uuid::Uuid,
}
impl Clock {
    pub fn id(self) -> uuid::Uuid { self.id }

    pub fn now_us(self) -> u64 {
        u64::try_from(self.origin.elapsed().as_micros())
            .unwrap_or(u64::MAX - 1)
            .saturating_add(1)
    }

    /// Convert native acquisition evidence into this exact journal domain.
    /// Failed conversion cannot be replaced with receipt time or a zero stamp.
    pub fn captured_us(self, captured_at: Instant) -> Result<u64, &'static str> {
        let elapsed = captured_at.checked_duration_since(self.origin)
            .ok_or("Capture predates the journal clock.")?;
        if captured_at > Instant::now() {
            return Err("Capture is in the future.");
        }
        encode(elapsed)
    }
}

fn encode(elapsed: Duration) -> Result<u64, &'static str> {
    u64::try_from(elapsed.as_micros()).ok().and_then(|value| value.checked_add(1))
        .ok_or("Capture exceeds the journal clock range.")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_preserves_origin_and_conservatively_rounds_fractional_microseconds() {
        let origin = Instant::now() - Duration::from_secs(1);
        let clock = Clock { origin, id: uuid::Uuid::new_v4() };
        assert_eq!(clock.captured_us(origin), Ok(1));
        assert_eq!(clock.captured_us(origin + Duration::from_nanos(3999)), Ok(4));
        assert!(clock.captured_us(origin - Duration::from_micros(1)).is_err());
        assert!(clock.captured_us(Instant::now() + Duration::from_secs(1)).is_err());
    }
    #[test]
    fn conversion_rejects_overflow_without_saturation_or_wrapping() {
        assert_eq!(encode(Duration::from_micros(u64::MAX - 1)), Ok(u64::MAX));
        assert!(encode(Duration::from_micros(u64::MAX)).is_err());
        assert!(encode(Duration::MAX).is_err());
    }
}
