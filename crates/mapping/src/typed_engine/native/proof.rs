use super::super::*;
use super::*;
use std::sync::Arc;

/// Immutable acquisition evidence. Its representation stays inside the reducer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeProof {
    pub(in crate::engine::typed) bindings: Vec<Arc<Binding>>,
    pub(in crate::engine::typed) unproven: BTreeSet<Endpoint>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::engine::typed) struct Binding {
    pub endpoint: Endpoint,
    pub continuity: u64,
    pub owner: ledger::Owner,
    pub captures: Vec<(NativeInput, NativeCapture, Authority)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::engine::typed) enum Authority {
    Sampled(u64),
    Latched {
        domain: String,
        generation: u64,
        freshness: u64,
    },
}
impl NativeProof {
    pub(in crate::engine::typed) fn merge(proofs: impl IntoIterator<Item = Self>) -> Self {
        let mut bindings = Vec::new();
        let mut unproven = BTreeSet::new();
        for proof in proofs {
            unproven.extend(proof.unproven);
            for binding in proof.bindings {
                if !bindings.contains(&binding) {
                    bindings.push(binding);
                }
            }
        }
        Self { bindings, unproven }
    }
    pub(in crate::engine::typed) fn deadline(&self, ledger: &Ledger) -> Option<u64> {
        self.bindings
            .iter()
            .flat_map(|binding| {
                binding
                    .captures
                    .iter()
                    .map(move |(_, _, authority)| match authority {
                        Authority::Sampled(deadline) => *deadline,
                        Authority::Latched {
                            domain,
                            generation,
                            freshness,
                        } => ledger
                            .leases
                            .get(&(
                                binding.endpoint.node_id.clone(),
                                domain.clone(),
                                *generation,
                            ))
                            .map_or(0, |(capture, deadline)| {
                                (*deadline).min(capture.captured_us.0.saturating_add(*freshness))
                            }),
                    })
            })
            .min()
    }
    pub(in crate::engine::typed) fn valid(&self, ledger: &Ledger, now: u64, current: bool) -> bool {
        (!current || self.unproven.is_empty())
            && self.deadline(ledger).is_none_or(|deadline| deadline > now)
            && self.bindings.iter().all(|binding| {
                ledger.ports.get(&binding.endpoint).is_some_and(|port| {
                    port.active
                        && port.continuity == binding.continuity
                        && port.owner.as_ref() == Some(&binding.owner)
                        && (!current || port.proof.bindings.contains(binding))
                })
            })
    }
}
impl Phase {
    pub(in crate::engine::typed) fn effect_observations(
        &self,
        proof: &NativeProof,
        producer: Option<&Token>,
    ) -> Vec<Observation> {
        let mut observations = producer
            .map_or_else(Vec::new, |token| self.source_observations(token))
            .into_iter()
            .filter(|observation| !self.native.ports.contains_key(&observation.endpoint))
            .collect::<Vec<_>>();
        for binding in &proof.bindings {
            if let Some((_, capture, _)) = binding.captures.iter().min_by_key(|(_, capture, _)| {
                (capture.captured_us, capture.sequence, capture.byte_offset)
            }) {
                observations.push(Observation {
                    endpoint: binding.endpoint.clone(),
                    at_us: capture.captured_us,
                    sequence: capture.sequence,
                });
            }
        }
        observations.sort_by(|a, b| {
            (&a.endpoint, a.at_us, a.sequence).cmp(&(&b.endpoint, b.at_us, b.sequence))
        });
        observations.dedup();
        observations
    }
    pub fn effect_has_current_native_level(&self, effect: &Effect) -> bool {
        effect.run == self.run && effect.native_proof.valid(&self.native, self.clock, true)
    }
    pub fn endpoint_has_current_native_level(&self, endpoint: &Endpoint) -> bool {
        self.values
            .get(endpoint)
            .is_some_and(|value| value.proof.valid(&self.native, self.clock, true))
    }
    pub(in crate::engine::typed) fn native_inputs(&self, node: &Node) -> NativeProof {
        let selected = match &node.operation {
            Operation::Priority { order } => order
                .iter()
                .find_map(|id| {
                    node.inputs.iter().find(|input| {
                        &input.port_id == id
                            && matches!(self.values[&input.from].sample, SignalSample::Known(_))
                    })
                })
                .map(|input| vec![input]),
            Operation::Any {} => node
                .inputs
                .iter()
                .find(|input| boolean(&self.values[&input.from].sample) == Some(true))
                .map(|input| vec![input]),
            Operation::All {} => node
                .inputs
                .iter()
                .find(|input| boolean(&self.values[&input.from].sample) == Some(false))
                .map(|input| vec![input]),
            _ => None,
        }
        .unwrap_or_else(|| node.inputs.iter().collect::<Vec<_>>());
        let mut proof = NativeProof::merge(
            selected
                .iter()
                .map(|input| self.values[&input.from].proof.clone()),
        );
        for input in selected {
            if !matches!(self.values[&input.from].sample, SignalSample::Known(_))
                && (self.native.ports.contains_key(&input.from)
                    || self
                        .compiled
                        .dependencies
                        .get(&input.from.node_id)
                        .is_some_and(|roots| {
                            roots
                                .iter()
                                .any(|root| self.native.ports.contains_key(root))
                        }))
            {
                proof.unproven.insert(input.from.clone());
            }
        }
        proof
    }
    pub(in crate::engine::typed) fn proof_for_token(&self, token: &Token) -> NativeProof {
        NativeProof::merge(token.dependencies.iter().filter_map(|dependency| {
            self.values
                .get(&dependency.endpoint)
                .map(|value| value.proof.clone())
        }))
    }
    pub(in crate::engine::typed) fn retire_native_work(&mut self) {
        let ledger = &self.native;
        let now = self.clock;
        self.timers
            .retain(|_, timer| timer.proof.valid(ledger, now, false));
        for state in self.states.values_mut() {
            if !state.rule_proof.valid(ledger, now, false) {
                state.rule = crate::rules::State::default();
                state.rule_proof = NativeProof::default();
            }
            state.pending.retain(|request| {
                state
                    .bindings
                    .get(&request.ticket.sequence.0)
                    .is_none_or(|proof| proof.valid(ledger, now, false))
            });
            if state.inflight.as_ref().is_some_and(|(ticket, _)| {
                state
                    .bindings
                    .get(&ticket.sequence.0)
                    .is_some_and(|proof| !proof.valid(ledger, now, false))
            }) {
                state.inflight = None;
            }
            state.bindings.retain(|sequence, _| {
                state
                    .pending
                    .iter()
                    .any(|request| request.ticket.sequence.0 == *sequence)
                    || state
                        .inflight
                        .as_ref()
                        .is_some_and(|(ticket, _)| ticket.sequence.0 == *sequence)
            });
        }
    }
    pub(in crate::engine::typed) fn native_deadline(&self) -> Option<u64> {
        let value_proofs = self
            .values
            .values()
            .filter(|value| matches!(value.sample, SignalSample::Known(_)))
            .map(|value| &value.proof);
        let request_proofs = self
            .states
            .values()
            .flat_map(|state| state.bindings.values());
        value_proofs
            .chain(request_proofs)
            .chain(self.timers.values().map(|timer| &timer.proof))
            .filter_map(|proof| proof.deadline(&self.native))
            .min()
    }
    pub(in crate::engine::typed) fn expire_native(&mut self) -> Result<(), String> {
        let endpoints = self
            .values
            .iter()
            .filter(|(_, value)| {
                matches!(value.sample, SignalSample::Known(_))
                    && !value.proof.valid(&self.native, self.clock, false)
            })
            .map(|(endpoint, _)| endpoint.clone())
            .collect::<BTreeSet<_>>();
        for endpoint in &endpoints {
            self.put(endpoint, SignalSample::Stale);
        }
        self.cancel_invalid();
        self.retire_native_work();
        self.effects.retain(|effect| {
            effect.native_proof.bindings.is_empty()
                || effect.native_proof.valid(&self.native, self.clock, false)
        });
        if !endpoints.is_empty() {
            let events = self.evaluate(endpoints.clone(), BTreeSet::new(), true)?;
            self.publish(events, ReportReason::SourceLost, false);
            self.clamp_native_reports(&endpoints);
        }
        Ok(())
    }
}
