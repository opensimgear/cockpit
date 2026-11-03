use super::super::*;
use super::ledger::{identity, Owner, Port};
use super::proof::{Authority, Binding};
use super::*;

pub(in crate::engine::typed) struct Prepared {
    pub frame: NativeFrame,
    pub proofs: BTreeMap<String, NativeProof>,
}
impl Ledger {
    pub(in crate::engine::typed) fn prepare(
        &mut self,
        frame: NativeFrame,
        now: u64,
        contracts: &BTreeMap<Endpoint, PortContract>,
    ) -> Result<Prepared, String> {
        identity(&frame.source)?;
        if self.clock.as_deref() != Some(&frame.clock_id)
            || frame.generation.0 == 0
            || frame.sequence.0 == 0
        {
            return Err("Native frame has an invalid clock or counter.".into());
        }
        let registration = self
            .registrations
            .get(&frame.source)
            .ok_or("Unregistered native source.")?
            .clone();
        if frame.captures.len() > MAX_NATIVE_CAPTURES
            || frame.leases.len() > MAX_NATIVE_LEASES
            || frame
                .changes
                .iter()
                .map(|change| change.contributors.len())
                .sum::<usize>()
                > MAX_NATIVE_REFERENCES
            || !super::budget::within_bytes(&frame, MAX_NATIVE_FRAME_BYTES)
        {
            return Err("Native frame exceeds its evidence budget.".into());
        }
        let previous = self.sources.get(&frame.source).copied().unwrap_or((0, 0));
        if frame.generation.0 < previous.0
            || frame.generation.0 == previous.0 && frame.sequence.0 <= previous.1
        {
            return Err("Native source counters regressed.".into());
        }
        let retained = self
            .inputs
            .values()
            .chain(self.leases.values().map(|(capture, _)| capture));
        let mut original_times = self.positions.clone();
        for capture in retained {
            original_times.insert(
                (
                    capture.producer_id.clone(),
                    capture.generation.0,
                    capture.sequence.0,
                    capture.byte_offset,
                ),
                capture.captured_us.0,
            );
        }
        let mut positions = BTreeSet::new();
        for capture in &frame.captures {
            identity(&capture.producer_id)?;
            if capture.generation.0 == 0 || capture.sequence.0 == 0 || capture.captured_us.0 == 0 || capture.captured_us.0 > now {
                return Err("Native acquisition capture is invalid or in the future.".into());
            }
            let key = (
                capture.producer_id.clone(),
                capture.generation.0,
                capture.sequence.0,
                capture.byte_offset,
            );
            if !positions.insert(key.clone()) {
                return Err("Native capture table is not deduplicated.".into());
            }
            if original_times
                .get(&key)
                .is_some_and(|time| *time != capture.captured_us.0)
                || self
                    .producers
                    .get(&capture.producer_id)
                    .is_some_and(|generation| capture.generation.0 < *generation)
            {
                return Err("Native acquisition generation or original position contradicts its retained fence.".into());
            }
            self.positions.insert(key, capture.captured_us.0);
            self.producers
                .entry(capture.producer_id.clone())
                .and_modify(|generation| *generation = (*generation).max(capture.generation.0))
                .or_insert(capture.generation.0);
        }
        let members = registration
            .ports
            .iter()
            .map(|port| (port.port_id.as_str(), port))
            .collect::<BTreeMap<_, _>>();
        let mut changes = BTreeMap::new();
        for change in &frame.changes {
            if !members.contains_key(change.port_id.as_str())
                || !changes.insert(change.port_id.as_str(), change).is_none()
                || change.continuity.0 == 0
            {
                return Err(
                    "Native change has unknown membership, duplicates or zero continuity.".into(),
                );
            }
            let endpoint = Endpoint {
                node_id: frame.source.clone(),
                port_id: change.port_id.clone(),
            };
            if matches!(&change.sample,SignalSample::Known(value) if !accepts(&contracts[&endpoint],value))
            {
                return Err("Native sample violates its prepared port contract.".into());
            }
            if matches!(change.sample, SignalSample::Known(_)) != !change.contributors.is_empty()
                || change.kind == NativeChangeKind::Withdraw
                    && matches!(change.sample, SignalSample::Known(_))
            {
                return Err("Native Known samples require acquisition proof; nonKnown samples cannot carry it.".into());
            }
        }
        let complete_source = registration.ports.iter().all(|port| {
            changes
                .get(port.port_id.as_str())
                .is_some_and(|change| change.kind == NativeChangeKind::Baseline)
        });
        if previous.0 != 0 && frame.generation.0 != previous.0 && !complete_source {
            return Err(
                "A native logical generation requires complete source baseline closure.".into(),
            );
        }
        if previous.0 != 0
            && frame.generation.0 == previous.0
            && previous.1.checked_add(1) != Some(frame.sequence.0)
            && !complete_source
        {
            return Err(
                "Native source sequence gap requires complete source baseline closure.".into(),
            );
        }
        for port in &registration.ports {
            let Some(change) = changes.get(port.port_id.as_str()) else {
                continue;
            };
            if change.kind == NativeChangeKind::Baseline {
                if !registration
                    .ports
                    .iter()
                    .filter(|member| member.family == port.family)
                    .all(|member| {
                        changes.get(member.port_id.as_str()).is_some_and(|other| {
                            other.kind == NativeChangeKind::Baseline
                                && other.continuity == change.continuity
                        })
                    })
                {
                    return Err(
                        "Native family baseline must include every prepared family member.".into(),
                    );
                }
            }
        }
        let mut lease_keys = BTreeSet::new();
        for lease in &frame.leases {
            identity(&lease.domain_id)?;
            let capture = frame
                .captures
                .get(lease.observation as usize)
                .ok_or("Native lease observation reference is out of range.")?;
            if lease.generation.0 == 0
                || lease.valid_until_us.0 <= capture.captured_us.0
                || lease.valid_until_us.0 - capture.captured_us.0 > MAX_NATIVE_VALIDITY_US
                || !lease_keys.insert((lease.domain_id.clone(), lease.generation.0))
            {
                return Err("Native lease authority is invalid or duplicated.".into());
            }
            let key = (
                frame.source.clone(),
                lease.domain_id.clone(),
                lease.generation.0,
            );
            if let Some((old, deadline)) = self.leases.get(&key) {
                if capture.producer_id != old.producer_id
                    || capture.generation != old.generation
                    || capture.byte_offset.is_some() != old.byte_offset.is_some()
                    || position(capture) < position(old)
                    || capture.captured_us < old.captured_us
                    || position(capture) == position(old)
                        && (capture.captured_us != old.captured_us
                            || lease.valid_until_us.0 != *deadline)
                {
                    return Err(
                        "Native lease observation regressed or contradicted a retained position."
                            .into(),
                    );
                }
            }
            self.leases
                .insert(key, (capture.clone(), lease.valid_until_us.0));
        }
        let mut proofs = BTreeMap::new();
        for change in &frame.changes {
            if change.kind == NativeChangeKind::Baseline {
                let endpoint = Endpoint {
                    node_id: frame.source.clone(),
                    port_id: change.port_id.clone(),
                };
                self.ports.get_mut(&endpoint).unwrap().owner = None;
            }
        }
        for change in &frame.changes {
            let endpoint = Endpoint {
                node_id: frame.source.clone(),
                port_id: change.port_id.clone(),
            };
            let registration = members[change.port_id.as_str()];
            let old = self.ports[&endpoint].clone();
            if change.kind == NativeChangeKind::Baseline {
                if previous.0 == frame.generation.0
                    && old.continuity != 0
                    && change.continuity.0 <= old.continuity
                {
                    return Err("Native family baseline must advance continuity.".into());
                }
            } else if change.continuity.0 != old.continuity
                || !old.active && change.kind != NativeChangeKind::Withdraw
            {
                return Err("Native update requires a live matching family baseline.".into());
            }
            let freshness = match contracts[&endpoint] {
                PortContract::State { freshness_ms, .. } => freshness_ms as u64 * 1000,
                _ => 600_000_000,
            };
            let mut captures = Vec::new();
            let mut owner: Option<Owner> = None;
            let mut inputs = BTreeSet::new();
            for contributor in &change.contributors {
                if let NativeInput::Channel(channel) = &contributor.input {
                    identity(channel)?;
                }
                if !inputs.insert(contributor.input.clone()) {
                    return Err("Duplicate native input contributor.".into());
                }
                let capture = frame
                    .captures
                    .get(contributor.capture as usize)
                    .ok_or("Native capture reference is out of range.")?
                    .clone();
                if matches!(
                    (&contributor.input, capture.byte_offset),
                    (NativeInput::Byte(_), None) | (NativeInput::Channel(_), Some(_))
                ) {
                    return Err("Native byte/channel contributor disagrees with its original stamp position.".into());
                }
                let key = (
                    capture.producer_id.clone(),
                    capture.generation.0,
                    contributor.input.clone(),
                );
                if let Some(previous) = self.inputs.get(&key) {
                    if position(&capture) < position(previous)
                        || capture.captured_us < previous.captured_us
                        || position(&capture) == position(previous)
                            && capture.captured_us != previous.captured_us
                    {
                        return Err("Native input acquisition regressed or contradicted its original position.".into());
                    }
                }
                let (authority, domain) = match (&contributor.validity, registration.policy) {
                    (NativeValidity::Sampled { valid_until_us }, NativePolicy::Sampled)
                        if valid_until_us.0 > capture.captured_us.0
                            && valid_until_us.0 - capture.captured_us.0 <= MAX_NATIVE_VALIDITY_US =>
                    {
                        (
                            Authority::Sampled(
                                valid_until_us
                                    .0
                                    .min(capture.captured_us.0.saturating_add(freshness)),
                            ),
                            None,
                        )
                    }
                    (NativeValidity::Latched { lease }, NativePolicy::Latched) => {
                        let lease = frame
                            .leases
                            .get(*lease as usize)
                            .ok_or("Native lease reference is out of range.")?;
                        let observation = &frame.captures[lease.observation as usize];
                        if observation.producer_id != capture.producer_id
                            || observation.generation != capture.generation
                            || observation.byte_offset.is_some() != capture.byte_offset.is_some()
                            || observation.captured_us < capture.captured_us
                            || position(&capture) > position(observation)
                        {
                            return Err(
                                "Native lease does not belong to the input acquisition domain."
                                    .into(),
                            );
                        }
                        (
                            Authority::Latched {
                                domain: lease.domain_id.clone(),
                                generation: lease.generation.0,
                                freshness,
                            },
                            Some((lease.domain_id.clone(), lease.generation.0)),
                        )
                    }
                    _ => return Err("Native validity disagrees with the prepared policy.".into()),
                };
                let next = Owner {
                    producer: capture.producer_id.clone(),
                    generation: capture.generation.0,
                    domain,
                };
                if owner.as_ref().is_some_and(|old| old != &next) {
                    return Err("Native family contributor ownership conflicts.".into());
                }
                owner = Some(next);
                self.inputs.insert(key, capture.clone());
                captures.push((contributor.input.clone(), capture, authority));
            }
            if change.kind != NativeChangeKind::Baseline
                && owner
                    .as_ref()
                    .is_some_and(|owner| old.owner.as_ref() != Some(owner))
            {
                return Err("Native ownership change requires a family baseline.".into());
            }
            let owner = owner.or_else(|| old.owner.clone());
            captures.sort_by(|a, b| a.0.cmp(&b.0));
            for (_, capture, authority) in &captures {
                if let Authority::Sampled(deadline) = authority {
                    if old.proof.bindings.iter().flat_map(|binding| &binding.captures).any(|(_,old_capture,old_authority)|
                        old_capture == capture && matches!(old_authority,Authority::Sampled(old_deadline) if old_deadline != deadline)) {
                        return Err("An original sampled acquisition cannot renew its original authority deadline.".into());
                    }
                }
            }
            let mut proof = NativeProof {
                bindings: owner
                    .as_ref()
                    .filter(|_| !captures.is_empty())
                    .map(|owner| {
                        vec![std::sync::Arc::new(Binding {
                            endpoint: endpoint.clone(),
                            continuity: change.continuity.0,
                            owner: owner.clone(),
                            captures,
                        })]
                    })
                    .unwrap_or_default(),
                ..Default::default()
            };
            if change.kind == NativeChangeKind::Update
                && old.proof == proof
                && !proof.bindings.is_empty()
            {
                if old.sample.as_ref() != Some(&change.sample) {
                    return Err(
                        "Native value contradicts its retained original acquisition.".into(),
                    );
                }
                proof = old.proof.clone();
            }
            self.ports.insert(
                endpoint,
                Port {
                    continuity: change.continuity.0,
                    active: change.kind != NativeChangeKind::Withdraw,
                    owner,
                    proof: proof.clone(),
                    sample: Some(change.sample.clone()),
                },
            );
            proofs.insert(change.port_id.clone(), proof);
        }
        // Every member of an acquisition family shares its pinned owner, even
        // when the first baseline member is absent/Unknown.
        for port in &registration.ports {
            let endpoint = Endpoint {
                node_id: frame.source.clone(),
                port_id: port.port_id.clone(),
            };
            let family_owner = registration
                .ports
                .iter()
                .filter(|other| other.family == port.family)
                .filter_map(|other| {
                    self.ports[&Endpoint {
                        node_id: frame.source.clone(),
                        port_id: other.port_id.clone(),
                    }]
                        .owner
                        .clone()
                })
                .next();
            if let Some(owner) = family_owner {
                let current = self.ports.get_mut(&endpoint).unwrap();
                if current
                    .owner
                    .as_ref()
                    .is_some_and(|current| current != &owner)
                {
                    return Err("Native family ownership conflicts.".into());
                }
                current.owner = Some(owner);
            }
        }
        self.sources
            .insert(frame.source.clone(), (frame.generation.0, frame.sequence.0));
        // Retain one input fence per current acquisition generation. Producer
        // generation fences remain even when its current input table is reset.
        self.inputs.retain(|(producer, generation, _), _| {
            self.producers.get(producer) == Some(generation)
        });
        let active_domains = self
            .ports
            .iter()
            .filter(|(_, port)| port.active)
            .filter_map(|(endpoint, port)| {
                port.owner
                    .as_ref()
                    .and_then(|owner| owner.domain.as_ref())
                    .map(|(domain, generation)| {
                        (endpoint.node_id.clone(), domain.clone(), *generation)
                    })
            })
            .collect::<BTreeSet<_>>();
        self.leases.retain(|key, _| {
            key.0 != frame.source
                || lease_keys.contains(&(key.1.clone(), key.2))
                || active_domains.contains(key)
        });
        if self.inputs.len() > MAX_NATIVE_REFERENCES
            || self.positions.len() > MAX_NATIVE_REFERENCES
            || self.producers.len() > MAX_NATIVE_CAPTURES
            || self
                .leases
                .keys()
                .filter(|(source, ..)| source == &frame.source)
                .count()
                > MAX_NATIVE_LEASES
        {
            return Err("Native retained acquisition fences exceed their bounded budget.".into());
        }
        Ok(Prepared { frame, proofs })
    }
}
fn position(capture: &NativeCapture) -> (u64, Option<u32>) {
    (capture.sequence.0, capture.byte_offset)
}
