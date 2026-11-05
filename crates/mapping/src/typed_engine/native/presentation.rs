//! Read-only timestamps for presentation consumers of reducer samples.
use super::super::*;

/// Freshness evidence retained alongside one cached reducer sample.
///
/// `observed_at` follows [`Phase::inspected_sample`]. `captured_at` is the
/// latest source-capture or graph-evaluation stamp; `captured_sequence` is
/// source capture evidence and does not change on liveness refreshes. Constant
/// nodes are marked timeless because their known values do not expire by age.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampleObservation<'a> {
    pub sample: &'a SignalSample,
    pub observed_at: Option<Counter>,
    pub captured_at: Option<Counter>,
    pub captured_sequence: Counter,
    pub timeless: bool,
}

impl Phase {
    /// Inspect cached presentation state without advancing or mutating it.
    pub fn inspected_observation(&self, endpoint: &Endpoint) -> Option<SampleObservation<'_>> {
        let state = self.values.get(endpoint)?;
        let observed_at = (state.epoch > 1
            || state.observed > 0
            || !matches!(state.sample, SignalSample::Unknown))
        .then_some(Counter(state.observed));
        let has_capture = state.captured_sequence > 0
            || state.captured > 0
            || state.epoch > 1
            || matches!(state.sample, SignalSample::Known(_));
        let timeless = self.active
            && matches!(state.sample, SignalSample::Known(_))
            && state.timeless;
        Some(SampleObservation {
            sample: &state.sample,
            observed_at,
            captured_at: has_capture.then_some(Counter(state.captured)),
            captured_sequence: Counter(state.captured_sequence),
            timeless,
        })
    }
}

#[cfg(test)]
#[path = "presentation_tests.rs"]
mod tests;
