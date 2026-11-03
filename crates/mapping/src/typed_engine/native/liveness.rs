//! Portable reducer liveness.
use super::super::*;
impl Phase {
    pub fn sample(&self, endpoint: &Endpoint) -> Option<&SignalSample> {
        self.values.get(endpoint).map(|state| &state.sample)
    }
    /// Cached graph observation only: no clock advance, input polling or drain.
    pub fn inspected_sample(
        &self,
        endpoint: &Endpoint,
    ) -> Option<(&SignalSample, Option<Counter>)> {
        self.values.get(endpoint).map(|state| {
            let observed = (state.epoch > 1
                || state.observed > 0
                || !matches!(state.sample, SignalSample::Unknown))
            .then_some(Counter(state.observed));
            (&state.sample, observed)
        })
    }
    /// Actual accepted native samples only. Liveness extends a latched state's
    /// availability but never turns an old press into fresh rearming evidence.
    pub fn source_observations(&self, token: &Token) -> Vec<Observation> {
        if token.run != self.run {
            return vec![];
        }
        let roots = token
            .dependencies
            .iter()
            .filter(|dependency| self.sources.contains_key(&dependency.endpoint.node_id))
            .collect::<Vec<_>>();
        if roots.iter().any(|dependency| {
            self.sources
                .get(&dependency.endpoint.node_id)
                .is_none_or(|source| {
                    !source.available || source.generation != dependency.generation.0
                })
                || self.values.get(&dependency.endpoint).is_none_or(|state| {
                    state.epoch != dependency.epoch.0
                        || !matches!(state.sample, SignalSample::Known(_))
                })
        }) {
            return vec![];
        }
        roots
            .into_iter()
            .map(|dependency| {
                let state = &self.values[&dependency.endpoint];
                Observation {
                    endpoint: dependency.endpoint.clone(),
                    at_us: Counter(state.captured),
                    sequence: Counter(state.captured_sequence),
                }
            })
            .collect()
    }
    pub fn drain(&mut self) -> Vec<Effect> {
        self.effects.drain(..).collect()
    }
    pub fn drain_reports(&mut self) -> Vec<Report> {
        self.reports.drain(..).collect()
    }
    /// Whether this generation has a retained admitted baseline. This is not
    /// current native connection or authority evidence; liveness still requires
    /// the provider's independent current-generation validation.
    pub fn has_source_baseline(&self, source: &str, generation: Counter) -> bool {
        self.sources
            .get(source)
            .is_some_and(|state| state.available && state.generation == generation.0)
    }
    /// A native provider can confirm that a previously admitted latched source
    /// is still connected in this exact generation, even when a stationary HID
    /// control produces no reports. This is a liveness observation, never an
    /// input sample: it cannot rearm a source, fire events or renew scripts.
    /// Call before advancing the shared clock, using native connection evidence.
    pub fn observe_liveness(
        &mut self,
        source: &str,
        generation: Counter,
        now: Counter,
    ) -> Result<(), String> {
        self.observe_selected_liveness(source, generation, now, None)
    }
    /// Refresh only native latched controls, preserving independent freshness
    /// for sampled panel axes, telemetry and event ports on the same source.
    pub fn observe_port_liveness(
        &mut self,
        source: &str,
        generation: Counter,
        now: Counter,
        ports: &[String],
    ) -> Result<(), String> {
        self.observe_selected_liveness(source, generation, now, Some(ports))
    }
    pub(in crate::engine::typed) fn observe_selected_liveness(
        &mut self,
        source: &str,
        generation: Counter,
        now: Counter,
        ports: Option<&[String]>,
    ) -> Result<(), String> {
        if !self.active || now.0 < self.clock || now.0 > u64::MAX - 600_000_000 {
            return Err("Invalid native source liveness clock or inactive run.".into());
        }
        if self.native.registrations.contains_key(source) {
            return Err("Direct liveness cannot renew native registered ports.".into());
        }
        let state = self
            .sources
            .get(source)
            .ok_or("Unknown native liveness source.")?;
        if !state.available || state.generation != generation.0 {
            return Err(
                "Native liveness cannot replace a fresh current-generation baseline.".into(),
            );
        }
        let mut refreshed = BTreeSet::new();
        for port in &self.compiled.program.sources {
            if port.endpoint.node_id == source
                && ports.is_none_or(|ports| ports.contains(&port.endpoint.port_id))
                && matches!(port.contract, PortContract::State { .. })
                && matches!(self.values[&port.endpoint].sample, SignalSample::Known(_))
            {
                self.values.get_mut(&port.endpoint).unwrap().observed = now.0;
                refreshed.insert(port.endpoint.clone());
            }
        }
        for &index in &self.compiled.order {
            let node = &self.compiled.program.nodes[index];
            if matches!(node.operation, Operation::Script { .. })
                || !node.inputs.iter().any(|input| refreshed.contains(&input.from))
                || !node.inputs.iter().all(|input| {
                    matches!(self.values[&input.from].sample,SignalSample::Known(_))
                        && matches!(self.compiled.contracts[&input.from],PortContract::State{freshness_ms,..}
                            if self.values[&input.from].observed.saturating_add(freshness_ms as u64*1000)>now.0)
                })
            {
                continue;
            }
            for output in &node.outputs {
                let endpoint = Endpoint {
                    node_id: node.id.clone(),
                    port_id: output.id.clone(),
                };
                if matches!(output.contract, PortContract::State { .. })
                    && matches!(self.values[&endpoint].sample, SignalSample::Known(_))
                {
                    self.values.get_mut(&endpoint).unwrap().observed = now.0;
                    refreshed.insert(endpoint);
                }
            }
        }
        Ok(())
    }
}
