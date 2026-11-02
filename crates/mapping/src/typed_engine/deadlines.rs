//! The reducer and host scheduler use the same pending-deadline selection.
use super::*;

pub(super) struct Deadlines {
    pub stale: Option<u64>,
    pub internal: Option<u64>,
    pub external: Option<u64>,
}
impl Deadlines {
    pub(super) fn next(&self) -> Option<u64> {
        [self.stale, self.internal, self.external]
            .into_iter()
            .flatten()
            .min()
    }
}
impl Phase {
    /// Earliest pending freshness expiry, built-in transition or script timer,
    /// on the same clock as `at_us`. A deadline is due inclusively at its time.
    /// Query again after admitting input, liveness or asynchronous completion.
    pub fn next_deadline_us(&self) -> Option<Counter> {
        if !self.active || self.fault.is_some() {
            return None;
        }
        self.pending_deadlines().next().map(Counter)
    }

    pub(super) fn pending_deadlines(&self) -> Deadlines {
        let stale = self
            .compiled
            .contracts
            .iter()
            .filter_map(|(endpoint, contract)| match contract {
                PortContract::State { freshness_ms, .. }
                    if matches!(self.values[endpoint].sample, SignalSample::Known(_))
                        && !self.compiled.program.nodes.iter().any(|node| {
                            node.id == endpoint.node_id
                                && matches!(node.operation, Operation::Constant { .. })
                        }) =>
                {
                    self.values[endpoint]
                        .observed
                        .checked_add(*freshness_ms as u64 * 1000)
                }
                _ => None,
            })
            .min();
        Deadlines {
            stale,
            internal: self
                .states
                .values()
                .filter_map(|state| state.rule.next())
                .min(),
            external: self.timers.first_key_value().map(|((at, ..), _)| *at),
        }
    }
}

#[cfg(test)]
#[path = "deadline_tests.rs"]
mod tests;
