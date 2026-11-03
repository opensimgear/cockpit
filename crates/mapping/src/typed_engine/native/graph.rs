//! Portable reducer graph.
use super::super::*;
impl Phase {
    pub(in crate::engine::typed) fn put(
        &mut self,
        endpoint: &Endpoint,
        sample: SignalSample,
    ) -> bool {
        let state = self.values.get_mut(endpoint).expect("compiled endpoint");
        let changed = state.sample != sample;
        if !same_status(&state.sample, &sample) {
            state.epoch = state.epoch.saturating_add(1);
        }
        state.proof = if matches!(sample, SignalSample::Known(_)) {
            self.causal.clone()
        } else {
            native::NativeProof::default()
        };
        state.sample = sample;
        state.observed = self.clock;
        state.captured = self.clock;
        changed
    }
    pub(in crate::engine::typed) fn invalidate(&mut self, source: &str, status: SignalSample) {
        if let Some(state) = self.sources.get_mut(source) {
            state.available = false;
        }
        let endpoints = self
            .compiled
            .program
            .sources
            .iter()
            .filter(|port| port.endpoint.node_id == source)
            .map(|port| port.endpoint.clone())
            .collect::<Vec<_>>();
        for endpoint in endpoints {
            self.put(&endpoint, status.clone());
        }
        self.cancel_invalid();
    }
    pub(in crate::engine::typed) fn cancel_invalid(&mut self) {
        let tokens = self
            .compiled
            .program
            .nodes
            .iter()
            .filter_map(|node| {
                self.token(&node.id)
                    .ok()
                    .map(|token| (node.id.clone(), token))
            })
            .collect::<BTreeMap<_, _>>();
        self.timers
            .retain(|_, timer| tokens.get(&timer.token.node_id) == Some(&timer.token));
        self.retire_native_work();
        for (node, state) in &mut self.states {
            if state
                .inflight
                .as_ref()
                .is_some_and(|(ticket, _)| tokens.get(node) != Some(&ticket.producer))
            {
                state.pending.clear();
                state.inflight = None;
                state.script_state.clear();
            }
            state
                .pending
                .retain(|request| tokens.get(node) == Some(&request.ticket.producer));
        }
    }
    pub(in crate::engine::typed) fn refresh_source(
        &mut self,
        source: &str,
        reason: ReportReason,
    ) -> Result<(), String> {
        let dirty = self
            .compiled
            .program
            .sources
            .iter()
            .filter(|port| port.endpoint.node_id == source)
            .map(|port| port.endpoint.clone())
            .collect();
        let events = self.evaluate(dirty, BTreeSet::new(), true)?;
        self.publish(events, reason, false);
        Ok(())
    }
    pub(in crate::engine::typed) fn refresh_all(
        &mut self,
        reason: ReportReason,
    ) -> Result<(), String> {
        let dirty = self
            .compiled
            .program
            .sources
            .iter()
            .map(|p| p.endpoint.clone())
            .collect();
        let events = self.evaluate(dirty, BTreeSet::new(), true)?;
        self.publish(events, reason, false);
        Ok(())
    }
    pub(in crate::engine::typed) fn inputs(&self, node: &Node) -> BTreeMap<String, SignalSample> {
        node.inputs
            .iter()
            .map(|input| {
                (
                    input.port_id.clone(),
                    self.values[&input.from].sample.clone(),
                )
            })
            .collect()
    }
    pub fn token(&self, node_id: &str) -> Result<Token, String> {
        if !self.active {
            return Err("Typed mapping is stopped.".into());
        }
        let endpoints = self
            .compiled
            .dependencies
            .get(node_id)
            .ok_or("Unknown typed producer.")?;
        if endpoints
            .iter()
            .any(|endpoint| self.values[endpoint].epoch == u64::MAX)
        {
            return Err("Typed port validity epoch exhausted.".into());
        }
        let dependencies = endpoints
            .iter()
            .map(|endpoint| Dependency {
                endpoint: endpoint.clone(),
                generation: Counter(
                    self.sources
                        .get(&endpoint.node_id)
                        .map_or(0, |source| source.generation),
                ),
                epoch: Counter(self.values[endpoint].epoch),
            })
            .collect();
        Ok(Token {
            run: self.run.clone(),
            node_id: node_id.into(),
            dependencies,
        })
    }
    /// Capture validity for an exact executable endpoint, including a direct
    /// native source port which has no processing-node entry in the graph.
    pub fn endpoint_token(&self, endpoint: &Endpoint) -> Result<Token, String> {
        if !self.active || !self.values.contains_key(endpoint) {
            return Err("Unknown or stopped typed endpoint producer.".into());
        }
        if self.compiled.dependencies.contains_key(&endpoint.node_id) {
            return self.token(&endpoint.node_id);
        }
        Ok(Token {
            run: self.run.clone(),
            node_id: endpoint.node_id.clone(),
            dependencies: vec![Dependency {
                endpoint: endpoint.clone(),
                generation: Counter(
                    self.sources
                        .get(&endpoint.node_id)
                        .map_or(0, |source| source.generation),
                ),
                epoch: Counter(self.values[endpoint].epoch),
            }],
        })
    }
    pub(in crate::engine::typed) fn validate_token(&self, token: &Token) -> Result<(), String> {
        if self.token(&token.node_id)? != *token {
            return Err(
                "Stale typed producer run, source generation or port validity epoch.".into(),
            );
        }
        Ok(())
    }
}
