use super::super::*;
use super::*;

impl Phase {
    pub(in crate::engine::typed) fn preflight_native_expiry(
        &mut self,
        now: Counter,
    ) -> Result<(), String> {
        let roots = self
            .native
            .ports
            .keys()
            .filter(|endpoint| {
                matches!(self.values[*endpoint].sample, SignalSample::Known(_))
                    && !self.values[*endpoint]
                        .proof
                        .valid(&self.native, now.0, false)
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        if roots.is_empty() {
            return Ok(());
        }
        for endpoint in &roots {
            self.put(endpoint, SignalSample::Stale);
        }
        self.cancel_invalid();
        self.retire_native_work();
        let affected = self
            .compiled
            .program
            .sinks
            .iter()
            .filter(|sink| {
                roots.contains(&sink.from)
                    || self
                        .compiled
                        .dependencies
                        .get(&sink.from.node_id)
                        .is_some_and(|dependencies| {
                            dependencies.iter().any(|endpoint| roots.contains(endpoint))
                        })
            })
            .map(|sink| sink.id.clone())
            .collect::<BTreeSet<_>>();
        self.effects
            .retain(|effect| !affected.contains(&effect.sink_id));
        let events = self.evaluate(roots.clone(), BTreeSet::new(), true)?;
        self.publish(events, ReportReason::SourceLost, false);
        self.clamp_native_reports(&roots);
        Ok(())
    }
    pub(super) fn recover_native_levels(
        &mut self,
        changed: &BTreeSet<Endpoint>,
        now: Counter,
    ) -> Result<(), String> {
        let recovered = self
            .native
            .ports
            .iter()
            .filter(|(endpoint, port)| {
                !changed.contains(*endpoint)
                    && matches!(self.values[*endpoint].sample, SignalSample::Stale)
                    && port.active
                    && matches!(port.sample, Some(SignalSample::Known(_)))
                    && port.proof.valid(&self.native, now.0, false)
            })
            .map(|(endpoint, port)| {
                (
                    endpoint.clone(),
                    port.sample.clone().unwrap(),
                    port.proof.clone(),
                )
            })
            .collect::<Vec<_>>();
        let dirty = recovered
            .iter()
            .map(|(endpoint, ..)| endpoint.clone())
            .collect::<BTreeSet<_>>();
        for (endpoint, sample, proof) in recovered {
            self.causal = proof;
            self.put(&endpoint, sample);
            if let Some(capture) = self
                .causal
                .bindings
                .iter()
                .flat_map(|binding| binding.captures.iter().map(|(_, capture, _)| capture))
                .min_by_key(|capture| (capture.captured_us, capture.sequence, capture.byte_offset))
            {
                let state = self.values.get_mut(&endpoint).unwrap();
                state.captured = capture.captured_us.0;
                state.captured_sequence = capture.sequence.0;
            }
        }
        self.causal = NativeProof::default();
        if !dirty.is_empty() {
            self.cancel_invalid();
            let events = self.evaluate_baseline(dirty, BTreeSet::new())?;
            self.publish(events, ReportReason::Baseline, false);
        }
        Ok(())
    }
}
