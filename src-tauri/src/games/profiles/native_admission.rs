//! Per-family admission preserves independent native acquisition continuity.
use super::*;
use crate::native_capture::CapturedValue;

#[derive(Default)]
pub(super) struct FamilyState {
    pub available: bool,
    continuity: u64,
    last_packet: Option<u32>,
    context: Option<(Option<String>, Option<String>, u64)>,
    owner: Option<(String, u64, Option<(String, u64)>)>,
    previous: BTreeMap<String, (SignalSample, Option<CapturedValue>)>,
}
impl FamilyState {
    pub(super) fn inherit_continuity(&mut self, previous: &Self) {
        self.continuity = previous.continuity;
    }
}
fn same_level(
    a: &(SignalSample, Option<CapturedValue>),
    b: &(SignalSample, Option<CapturedValue>),
) -> bool {
    a.0 == b.0
        && match (&a.1, &b.1) {
            (Some(a), Some(b)) => a
                .contributors()
                .zip(b.contributors())
                .is_some_and(|(a, b)| {
                    a.len() == b.len()
                        && a.iter()
                            .zip(b)
                            .all(|(a, b)| a.input == b.input && a.capture == b.capture)
                }),
            (None, None) => true,
            _ => false,
        }
}
impl GameSources {
    pub(super) fn read_ready(&self) -> bool {
        #[cfg(test)]
        if self.authored_fixture { return self.fresh; }
        if self.sources.is_empty() { self.fresh } else {
            self.sources.values().any(|source| source.families.values().any(|family| family.available))
        }
    }
    pub(crate) fn native_registrations(&self) -> Vec<NativeSourceRegistration> {
        #[cfg(test)]
        if self.authored_fixture {
            return vec![];
        }
        self.sources
            .iter()
            .map(|(source, state)| NativeSourceRegistration {
                source: source.clone(),
                clock_id: self.clock.id().to_string(),
                ports: state
                    .ports
                    .iter()
                    .map(|port| NativePortRegistration {
                        port_id: port.logical.clone(),
                        family: port.family.clone(),
                        policy: port.policy,
                    })
                    .collect(),
            })
            .collect()
    }
    pub(super) fn native_frames(&mut self, snapshot: ExportSnapshot, now_us: u64) -> GamePoll {
        if let Some(error) = &self.fatal {
            return GamePoll {
                fatal: Some(error.clone()),
                context_lost: true,
                ..Default::default()
            };
        }
        // Command readiness follows BIOS readiness; ownship source availability
        // is decided independently below using the matching native frame.
        self.fresh = snapshot.fresh;
        self.epoch = snapshot.epoch.max(1);
        let mut result = GamePoll {
            fresh: self.fresh,
            ..Default::default()
        };
        let clock = self.clock;
        let physics = self.physics.as_ref();
        for (resource, state) in &mut self.sources {
            let emitted = (|| -> Result<Option<NativeFrame>, String> {
                let mut tables = native_tables::Tables::new(clock);
                let mut changes = vec![];
                let mut renewed = false;
                let mut retained = 0usize;
                let families = state
                    .ports
                    .iter()
                    .map(|p| p.family.clone())
                    .collect::<BTreeSet<_>>();
                for name in families {
                    let ports = state
                        .ports
                        .iter()
                        .filter(|p| p.family == name)
                        .collect::<Vec<_>>();
                    let family = state.families.entry(name.clone()).or_default();
                    let ready = if name == "dcs-ownship" {
                        physics.is_some_and(|physics| physics.witness.fresh)
                    } else {
                        snapshot.fresh
                    };
                    let observed_owner = ports
                        .iter()
                        .filter_map(|port| snapshot.values.get(&port.native))
                        .filter(|value| value.is_current(Instant::now()))
                        .filter_map(|value| value.contributors())
                        .find_map(|items| items.first())
                        .map(|item| {
                            (
                                item.capture.producer_id.to_string(),
                                item.capture.generation,
                                match &item.validity {
                                    crate::native_capture::Validity::Latched(lease) => Some((lease.domain_id.to_string(), lease.generation)),
                                    crate::native_capture::Validity::Sampled { .. } => None,
                                },
                            )
                        });
                    let available = ready;
                    let owner = observed_owner
                        .or_else(|| family.available.then(|| family.owner.clone()).flatten());
                    if !available {
                        if state.replacement {
                            family.continuity = family
                                .continuity
                                .checked_add(1)
                                .ok_or("Native family continuity exhausted")?;
                            for port in &ports {
                                changes.push(NativeChange {
                                    port_id: port.logical.clone(),
                                    continuity: Counter(family.continuity),
                                    kind: NativeChangeKind::Baseline,
                                    sample: SignalSample::Unknown,
                                    contributors: vec![],
                                });
                            }
                        } else if family.available {
                            for port in &ports {
                                changes.push(NativeChange {
                                    port_id: port.logical.clone(),
                                    continuity: Counter(family.continuity),
                                    kind: NativeChangeKind::Withdraw,
                                    sample: SignalSample::Stale,
                                    contributors: vec![],
                                });
                            }
                        }
                        family.available = false;
                        family.previous.clear();
                        continue;
                    }
                    let context = if name == "dcs-ownship" {
                        physics.map(|physics| (physics.witness.producer_id.clone(), physics.witness.session.clone(), physics.witness.epoch))
                    } else { Some((None, None, snapshot.epoch)) };
                    let baseline = !family.available || family.owner != owner || family.context != context;
                    if baseline {
                        family.continuity = family
                            .continuity
                            .checked_add(1)
                            .ok_or("Native family continuity exhausted")?;
                    }
                    family.available = true;
                    family.owner = owner;
                    family.context = context;
                    let new_packet = name == "dcs-ownship" && physics.is_some_and(|physics| family.last_packet != physics.witness.sequence);
                    for port in ports {
                        let captured = snapshot.values.get(&port.native).filter(|value| {
                            value.contributors().is_some() && value.is_current(Instant::now())
                        });
                        let mut sample = captured.and_then(scalar_current).map(SignalSample::scalar).filter(|sample| matches!(sample, SignalSample::Known(value) if osg_mapping::engine::typed::accepts(&port.contract,value))).unwrap_or(SignalSample::Unknown);
                        if !baseline && !new_packet && matches!(sample, SignalSample::Unknown)
                            && family.previous.get(&port.logical).is_some_and(|(previous, evidence)| {
                                matches!(previous, SignalSample::Stale) || evidence.as_ref().is_some_and(|old| !old.is_current(Instant::now()))
                            }) {
                            sample = SignalSample::Stale;
                        }
                        let captured = matches!(sample, SignalSample::Known(_))
                            .then_some(captured)
                            .flatten();
                        if let Some(value) = captured {
                            retained = retained
                                .checked_add(value.retained_bytes())
                                .ok_or("Native retained budget overflow")?;
                        }
                        if retained > MAX_NATIVE_FRAME_BYTES {
                            return Err("Native retained proof budget exceeded".into());
                        }
                        let current = (sample.clone(), captured.cloned());
                        let changed = family
                            .previous
                            .get(&port.logical)
                            .is_none_or(|old| !same_level(old, &current));
                        renewed |= family
                            .previous
                            .get(&port.logical)
                            .is_some_and(|old| old != &current);
                        if !baseline && !changed {
                            let previous = family.previous.get(&port.logical).unwrap();
                            if previous == &current { continue; }
                            if port.policy == NativePolicy::Sampled {
                                return Err("An original sampled acquisition cannot renew its validity".into());
                            }
                        }
                        let freshness = match &port.contract {
                            PortContract::State { freshness_ms, .. } => *freshness_ms,
                            _ => 1500,
                        };
                        let contributors = if let Some(value) = captured {
                            tables.contributors(
                                value.contributors().unwrap(),
                                port.policy,
                                u64::from(freshness),
                            )?
                        } else {
                            vec![]
                        };
                        if baseline || changed {
                            changes.push(NativeChange {
                                port_id: port.logical.clone(),
                                continuity: Counter(family.continuity),
                                kind: if baseline {
                                    NativeChangeKind::Baseline
                                } else {
                                    NativeChangeKind::Update
                                },
                                sample,
                                contributors,
                            });
                        }
                        family.previous.insert(port.logical.clone(), current);
                    }
                    family.last_packet = physics.filter(|_| name == "dcs-ownship").and_then(|physics| physics.witness.sequence);
                }
                if changes.is_empty() && !renewed {
                    return Ok(None);
                }
                state.sequence = state
                    .sequence
                    .checked_add(1)
                    .ok_or("Native source sequence exhausted")?;
                let frame = NativeFrame {
                    source: resource.clone(),
                    generation: Counter(state.generation),
                    sequence: Counter(state.sequence),
                    clock_id: clock.id().to_string(),
                    captures: tables.captures,
                    leases: tables.leases,
                    changes,
                };
                if serde_json::to_vec(&frame).map_err(|e| e.to_string())?.len()
                    > MAX_NATIVE_FRAME_BYTES
                {
                    return Err("Native encoded frame budget exceeded".into());
                }
                state.replacement = false;
                Ok(Some(frame))
            })();
            match emitted {
                Ok(Some(frame)) => result.frames.push(frame.into()),
                Ok(None) => {}
                Err(error) => {
                    self.fatal = Some(error.clone());
                    result.fatal = Some(error.clone());
                    result.diagnostics.push(error);
                    result.context_lost = true;
                }
            }
        }
        if self.commands.is_empty() && !self.sources.is_empty() {
            result.fresh = self.read_ready();
        }
        if result.context_lost {
            result.frames = self.withdraw_native(now_us, Reason::Gap);
            self.fresh = false;
            result.fresh = false;
        }
        result
    }
    pub(super) fn withdraw_native(&mut self, now_us: u64, reason: Reason) -> Vec<Admission> {
        let mut frames = vec![];
        for (resource, state) in &mut self.sources {
            #[cfg(test)]
            if self.authored_fixture {
                if reason == Reason::Gap && !state.available {
                    continue;
                }
                state.available = false;
                if let Some(sequence) = state.sequence.checked_add(1) {
                    state.sequence = sequence;
                    frames.push(
                        Frame {
                            source: resource.clone(),
                            generation: Counter(state.generation),
                            sequence: Counter(sequence),
                            captured_us: Counter(now_us),
                            reason: reason.clone(),
                            changes: vec![],
                        }
                        .into(),
                    );
                }
                continue;
            }
            let mut changes = vec![];
            for port in &state.ports {
                if let Some(family) = state
                    .families
                    .get(&port.family)
                    .filter(|family| family.available)
                {
                    changes.push(NativeChange {
                        port_id: port.logical.clone(),
                        continuity: Counter(family.continuity),
                        kind: NativeChangeKind::Withdraw,
                        sample: SignalSample::Stale,
                        contributors: vec![],
                    });
                }
            }
            for family in state.families.values_mut() {
                family.available = false;
                family.previous.clear();
            }
            if changes.is_empty() {
                continue;
            }
            if state.sequence == u64::MAX {
                self.fatal = Some("Native source sequence exhausted".into());
            }
            if let Some(sequence) = state.sequence.checked_add(1) {
                state.sequence = sequence;
                frames.push(
                    NativeFrame {
                        source: resource.clone(),
                        generation: Counter(state.generation),
                        sequence: Counter(sequence),
                        clock_id: self.clock.id().to_string(),
                        captures: vec![],
                        leases: vec![],
                        changes,
                    }
                    .into(),
                );
            }
        }
        frames
    }
}
