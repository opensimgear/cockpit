//! Bounded latency histogram used by the reactive qualification reducer.

use serde_json::{json, Value};

const BIN_WIDTH_US: u64 = 100;
const BIN_COUNT: usize = 10_000;
const CAP_US: u64 = BIN_WIDTH_US * BIN_COUNT as u64;

/// A fixed-size histogram with 100 µs bins through 1 second.
pub(super) struct Histogram {
    bins: Vec<u64>,
    overflow: u64,
    count: u64,
    max_us: Option<u64>,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            bins: vec![0; BIN_COUNT],
            overflow: 0,
            count: 0,
            max_us: None,
        }
    }
}

impl Histogram {
    pub(super) fn record(&mut self, elapsed_us: u64) {
        self.count = self.count.saturating_add(1);
        self.max_us = Some(self.max_us.map_or(elapsed_us, |max| max.max(elapsed_us)));

        if elapsed_us > CAP_US {
            self.overflow = self.overflow.saturating_add(1);
        } else {
            // Exact boundaries belong to the bin ending at that boundary.
            let index = elapsed_us.saturating_sub(1) / BIN_WIDTH_US;
            self.bins[index as usize] = self.bins[index as usize].saturating_add(1);
        }
    }

    /// Return count and nearest-rank p50/p95/p99 estimates in milliseconds.
    /// Estimates round up to the containing bin's upper edge, capped by the
    /// observed maximum. Overflow quantiles use the observed maximum.
    pub(super) fn summary(&self) -> Value {
        json!({
            "count": self.count,
            "p50": self.quantile_ms(50, 100),
            "p95": self.quantile_ms(95, 100),
            "p99": self.quantile_ms(99, 100),
            "max": self.max_us.map(|us| us as f64 / 1_000.0),
        })
    }

    fn quantile_ms(&self, numerator: u64, denominator: u64) -> Option<f64> {
        let max_us = self.max_us?;
        // Nearest rank, calculated without multiplying count by numerator.
        let rank = (self.count / denominator) * numerator
            + ((self.count % denominator) * numerator + denominator - 1) / denominator;
        let mut seen = 0u64;
        for (index, count) in self.bins.iter().enumerate() {
            seen = seen.saturating_add(*count);
            if seen >= rank {
                let upper_us = (index as u64 + 1) * BIN_WIDTH_US;
                return Some(upper_us.min(max_us) as f64 / 1_000.0);
            }
        }
        if seen.saturating_add(self.overflow) >= rank {
            return Some(max_us as f64 / 1_000.0);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::Histogram;

    #[test]
    fn rounds_quantiles_up_at_bin_boundaries_and_keeps_exact_max() {
        let mut histogram = Histogram::default();
        histogram.record(0);
        histogram.record(100);
        histogram.record(101);
        histogram.record(999);

        let summary = histogram.summary();
        assert_eq!(summary["count"], 4);
        assert_eq!(summary["p50"], 0.1);
        assert_eq!(summary["p95"], 0.999);
        assert_eq!(summary["p99"], 0.999);
        assert_eq!(summary["max"], 0.999);
    }

    #[test]
    fn cap_boundary_is_binned_and_overflow_quantiles_use_observed_max() {
        let mut histogram = Histogram::default();
        histogram.record(1_000_000);
        histogram.record(1_000_001);

        let summary = histogram.summary();
        assert_eq!(summary["p50"], 1_000.0);
        assert_eq!(summary["p95"], 1_000.001);
        assert_eq!(summary["p99"], 1_000.001);
        assert_eq!(summary["max"], 1_000.001);
    }

    #[test]
    fn empty_histogram_has_null_quantiles_and_max() {
        let summary = Histogram::default().summary();
        assert_eq!(summary["count"], 0);
        assert!(summary["p50"].is_null());
        assert!(summary["p95"].is_null());
        assert!(summary["p99"].is_null());
        assert!(summary["max"].is_null());
    }
}
