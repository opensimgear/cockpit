use super::super::*;
use super::*;

impl Phase {
    pub(in crate::engine::typed) fn retained_native_proofs(
        &self,
    ) -> impl Iterator<Item = &NativeProof> {
        self.values
            .values()
            .map(|value| &value.proof)
            .chain(self.effects.iter().map(|effect| &effect.native_proof))
            .chain(self.timers.values().map(|timer| &timer.proof))
            .chain(self.states.values().flat_map(|state| {
                state
                    .bindings
                    .values()
                    .chain(std::iter::once(&state.rule_proof))
            }))
            .chain(self.native.ports.values().map(|port| &port.proof))
    }
    pub(super) fn seed_native_positions(&self, ledger: &mut Ledger) -> Result<(), String> {
        ledger.positions.clear();
        for proof in self.retained_native_proofs() {
            for binding in &proof.bindings {
                for (_, capture, _) in &binding.captures {
                    let key = (
                        capture.producer_id.clone(),
                        capture.generation.0,
                        capture.sequence.0,
                        capture.byte_offset,
                    );
                    if ledger
                        .positions
                        .insert(key, capture.captured_us.0)
                        .is_some_and(|time| time != capture.captured_us.0)
                    {
                        return Err(
                            "Retained native causal proofs contradict an original position.".into(),
                        );
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn prune_native_fences(&mut self) {
        let mut owners = self
            .native
            .ports
            .values()
            .filter_map(|port| port.owner.as_ref())
            .map(|owner| (owner.producer.clone(), owner.generation))
            .collect::<BTreeSet<_>>();
        let mut domains = self
            .native
            .ports
            .iter()
            .filter_map(|(endpoint, port)| {
                port.owner
                    .as_ref()
                    .and_then(|owner| owner.domain.as_ref())
                    .map(|(domain, generation)| {
                        (endpoint.node_id.clone(), domain.clone(), *generation)
                    })
            })
            .collect::<BTreeSet<_>>();
        for proof in self.retained_native_proofs() {
            for binding in &proof.bindings {
                owners.insert((binding.owner.producer.clone(), binding.owner.generation));
                if let Some((domain, generation)) = &binding.owner.domain {
                    domains.insert((
                        binding.endpoint.node_id.clone(),
                        domain.clone(),
                        *generation,
                    ));
                }
            }
        }
        self.native.inputs.retain(|(producer, generation, _), _| {
            owners.contains(&(producer.clone(), *generation))
        });
        self.native
            .producers
            .retain(|producer, _| owners.iter().any(|(owner, _)| owner == producer));
        self.native.leases.retain(|key, _| domains.contains(key));
        self.native.positions.clear();
    }
}
