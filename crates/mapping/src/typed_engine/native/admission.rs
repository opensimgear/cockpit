use super::super::*;
use super::*;

impl Phase {
    pub fn accept_admissions(
        &mut self,
        admissions: impl IntoIterator<Item = Admission>,
        now: Counter,
    ) -> Result<Vec<String>, String> {
        if !self.active || now.0 < self.clock || now.0 > u64::MAX - 600_000_000 {
            return Err("Invalid native drain clock or stopped phase.".into());
        }
        let admissions = admissions
            .into_iter()
            .take(MAX_REPORTS + 1)
            .collect::<Vec<_>>();
        if admissions.is_empty() {
            return Ok(vec![]);
        }
        if admissions.len() > MAX_REPORTS {
            return Err("Mixed admission drain exceeds its bounded budget.".into());
        }
        if admissions
            .iter()
            .all(|admission| matches!(admission, Admission::Direct(_)))
        {
            let seed_only = admissions.iter().all(|admission| matches!(admission,Admission::Direct(frame) if frame.reason == Reason::Baseline));
            return self.admit_batch(
                admissions.into_iter().map(|admission| match admission {
                    Admission::Direct(frame) => frame,
                    _ => unreachable!(),
                }),
                now,
                !seed_only,
            );
        }
        if !budget::within_bytes(&admissions, MAX_ADMISSION_BYTES) {
            return Err("Mixed admission drain exceeds its bounded budget.".into());
        }
        let mut ledger = self.native.clone();
        self.seed_native_positions(&mut ledger)?;
        let mut native = BTreeMap::new();
        let mut direct = Vec::new();
        let mut direct_indices = Vec::new();
        let mut barriers = BTreeMap::<Endpoint, (usize, SignalSample)>::new();
        for (index, admission) in admissions.into_iter().enumerate() {
            match admission {
                Admission::Direct(frame) => {
                    if ledger.registrations.contains_key(&frame.source) {
                        return Err(
                            "Native registered sources cannot use the Direct input protocol."
                                .into(),
                        );
                    }
                    direct.push(frame);
                    direct_indices.push(index);
                }
                Admission::Native(frame) => {
                    let prepared = ledger.prepare(frame, now.0, &self.compiled.contracts)?;
                    for change in &prepared.frame.changes {
                        let endpoint = Endpoint {
                            node_id: prepared.frame.source.clone(),
                            port_id: change.port_id.clone(),
                        };
                        let proof = &prepared.proofs[&change.port_id];
                        if change.kind != NativeChangeKind::Update
                            || !matches!(change.sample, SignalSample::Known(_))
                            || !proof.valid(&ledger, now.0, false)
                        {
                            barriers.insert(
                                endpoint,
                                (
                                    index,
                                    if change.kind == NativeChangeKind::Baseline {
                                        SignalSample::Unknown
                                    } else if matches!(change.sample, SignalSample::Known(_)) {
                                        SignalSample::Stale
                                    } else {
                                        change.sample.clone()
                                    },
                                ),
                            );
                        }
                    }
                    native.insert(index, prepared);
                }
            }
        }
        self.check_native_proof_budget(
            ledger.ports.values().map(|port| &port.proof).chain(
                native
                    .values()
                    .flat_map(|prepared| prepared.proofs.values()),
            ),
        )?;
        self.check_native_retained_bytes(
            &ledger,
            native
                .values()
                .flat_map(|prepared| prepared.proofs.values()),
        )?;
        // A final lease transaction can expire retained levels even when its
        // envelope has no changes. Install that loss before any authored work.
        for (endpoint, value) in &self.values {
            if self.native.ports.contains_key(endpoint)
                && matches!(value.sample, SignalSample::Known(_))
                && !value.proof.valid(&ledger, now.0, false)
            {
                barriers
                    .entry(endpoint.clone())
                    .or_insert((0, SignalSample::Stale));
            }
        }
        // No lease or ownership is installed until every native transaction is
        // validated. Direct preflight installs revocations but never timers.
        let seed_only = direct.iter().all(|frame| frame.reason == Reason::Baseline);
        let (prepared_direct, diagnostics) = self.prepare_direct(direct, now, !seed_only)?;
        self.native = ledger;
        let revoked = barriers.keys().cloned().collect::<BTreeSet<_>>();
        for (endpoint, (_, sample)) in &barriers {
            self.values.get_mut(endpoint).unwrap().epoch =
                self.values[endpoint].epoch.saturating_add(1);
            self.put(endpoint, sample.clone());
        }
        if !revoked.is_empty() {
            let affected = self
                .compiled
                .program
                .sinks
                .iter()
                .filter(|sink| {
                    revoked.contains(&sink.from)
                        || self
                            .compiled
                            .dependencies
                            .get(&sink.from.node_id)
                            .is_some_and(|dependencies| {
                                dependencies
                                    .iter()
                                    .any(|endpoint| revoked.contains(endpoint))
                            })
                })
                .map(|sink| sink.id.clone())
                .collect::<BTreeSet<_>>();
            self.effects
                .retain(|effect| !affected.contains(&effect.sink_id));
            self.cancel_invalid();
            self.retire_native_work();
            let events = self.evaluate(revoked.clone(), BTreeSet::new(), true)?;
            self.publish(events, ReportReason::SourceLost, false);
            self.clamp_native_reports(&revoked);
        }
        let changed = native
            .values()
            .flat_map(|prepared| {
                prepared.frame.changes.iter().map(|change| Endpoint {
                    node_id: prepared.frame.source.clone(),
                    port_id: change.port_id.clone(),
                })
            })
            .collect();
        self.recover_native_levels(&changed, now)?;
        self.advance(now)?;
        let mut direct = prepared_direct
            .into_iter()
            .map(|(index, frame)| (direct_indices[index], frame))
            .collect::<BTreeMap<_, _>>();
        let indices = direct
            .keys()
            .chain(native.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        for index in indices {
            if let Some(frame) = direct.remove(&index) {
                self.causal = NativeProof::default();
                self.accept_current(frame)?;
            }
            if let Some(prepared) = native.remove(&index) {
                self.apply_native(prepared, index, &barriers)?;
            }
        }
        self.causal = NativeProof::default();
        self.retire_native_work();
        self.prune_native_fences();
        Ok(diagnostics)
    }
    fn apply_native(
        &mut self,
        prepared: validate::Prepared,
        index: usize,
        barriers: &BTreeMap<Endpoint, (usize, SignalSample)>,
    ) -> Result<(), String> {
        let frame = prepared.frame;
        let state = self.sources.get_mut(&frame.source).unwrap();
        state.generation = frame.generation.0;
        state.sequence = frame.sequence.0;
        state.available = self
            .native
            .ports
            .iter()
            .any(|(endpoint, port)| endpoint.node_id == frame.source && port.active);
        let (baselines, updates): (Vec<_>, Vec<_>) = frame
            .changes
            .into_iter()
            .partition(|change| change.kind == NativeChangeKind::Baseline);
        self.apply_native_changes(
            &frame.source,
            baselines,
            &prepared.proofs,
            index,
            barriers,
            true,
        )?;
        self.apply_native_changes(
            &frame.source,
            updates,
            &prepared.proofs,
            index,
            barriers,
            false,
        )
    }
    fn apply_native_changes(
        &mut self,
        source: &str,
        changes: Vec<NativeChange>,
        proofs: &BTreeMap<String, NativeProof>,
        index: usize,
        barriers: &BTreeMap<Endpoint, (usize, SignalSample)>,
        baseline: bool,
    ) -> Result<(), String> {
        let mut dirty = BTreeSet::new();
        let mut events = BTreeSet::new();
        for change in changes {
            let endpoint = Endpoint {
                node_id: source.into(),
                port_id: change.port_id.clone(),
            };
            if barriers
                .get(&endpoint)
                .is_some_and(|(barrier, _)| index < *barrier)
            {
                continue;
            }
            let proof = proofs[&change.port_id].clone();
            if !baseline
                && !proof.bindings.is_empty()
                && self.values[&endpoint].proof == proof
                && self.values[&endpoint].sample == change.sample
            {
                continue;
            }
            let sample = if matches!(change.sample, SignalSample::Known(_))
                && !proof.valid(&self.native, self.clock, false)
            {
                SignalSample::Stale
            } else {
                change.sample
            };
            let event = change.kind == NativeChangeKind::Update
                && matches!(sample, SignalSample::Known(_))
                && matches!(
                    self.compiled.contracts[&endpoint],
                    PortContract::Event { .. }
                );
            self.causal = proof;
            self.put(&endpoint, sample);
            if let Some(binding) = self.causal.bindings.first() {
                if let Some((_, capture, _)) =
                    binding.captures.iter().min_by_key(|(_, capture, _)| {
                        (capture.captured_us, capture.sequence, capture.byte_offset)
                    })
                {
                    let state = self.values.get_mut(&endpoint).unwrap();
                    state.captured = capture.captured_us.0;
                    state.captured_sequence = capture.sequence.0;
                }
            }
            dirty.insert(endpoint.clone());
            if event {
                events.insert(endpoint);
            }
        }
        self.causal = NativeProof::default();
        self.cancel_invalid();
        if !dirty.is_empty() {
            let events = if baseline {
                self.evaluate_baseline(dirty, events)?
            } else {
                self.evaluate(dirty, events, false)?
            };
            self.publish(
                events,
                if baseline {
                    ReportReason::Baseline
                } else {
                    ReportReason::Change
                },
                false,
            );
        }
        Ok(())
    }
    pub fn mirror_native_state(&mut self, canonical: &Self) -> Result<(), String> {
        if self.run != canonical.run
            || self.native.registrations != canonical.native.registrations
            || self.native.clock != canonical.native.clock
        {
            return Err("Native mirror does not match the captured run registration.".into());
        }
        self.native.leases = canonical.native.leases.clone();
        self.retire_native_work();
        Ok(())
    }
}
