//! Shared Direct drain preflight.
use super::super::*;
impl Phase {
    pub(in crate::engine::typed) fn valid_frame(
        &self,
        frame: &Frame,
        previous: &SourceState,
    ) -> bool {
        let expected = self
            .compiled
            .program
            .sources
            .iter()
            .filter(|port| port.endpoint.node_id == frame.source)
            .map(|port| (&port.endpoint.port_id, &port.contract))
            .collect::<BTreeMap<_, _>>();
        let baseline = frame.reason == Reason::Baseline;
        let mut seen = BTreeSet::new();
        frame.generation.0 > 0
            && frame.sequence.0 > 0
            && frame.captured_us.0 <= u64::MAX - 600_000_000
            && frame.captured_us.0 >= previous.observed
            && frame.changes.len() <= expected.len()
            && frame.changes.iter().all(|change| {
                let Some(contract) = expected.get(&change.port_id) else {
                    return false;
                };
                seen.insert(&change.port_id)
                    && match &change.sample {
                        SignalSample::Known(value) => accepts(contract, value),
                        _ => true,
                    }
            })
            && (!baseline || seen.len() == expected.len())
    }
    /// Admit one bounded native capture batch at the current monotonic time.
    /// Every known revocation and expired sample is applied before due timers.
    /// Retained occurrences keep input order without replaying capture time.
    /// This is required when independent
    /// providers are drained together, rather than delivered as a causal stream.
    /// Handled continuity losses return bounded diagnostics and keep unrelated
    /// sources running; malformed envelopes fail without advancing their clocks.
    pub fn accept_batch(
        &mut self,
        frames: impl IntoIterator<Item = Frame>,
        admitted_now: Counter,
    ) -> Result<Vec<String>, String> {
        self.admit_batch(frames, admitted_now, true)
    }
    pub(in crate::engine::typed) fn admit_batch(
        &mut self,
        frames: impl IntoIterator<Item = Frame>,
        admitted_now: Counter,
        batch_baselines: bool,
    ) -> Result<Vec<String>, String> {
        let (frames, diagnostics) = self.prepare_direct(frames, admitted_now, batch_baselines)?;
        if frames.is_empty() && diagnostics.is_empty() {
            return Ok(diagnostics);
        }
        self.preflight_native_expiry(admitted_now)?;
        self.advance(admitted_now)?;
        for (_, frame) in frames {
            self.accept_current(frame)?;
        }
        Ok(diagnostics)
    }
    pub(in crate::engine::typed) fn prepare_direct(
        &mut self,
        frames: impl IntoIterator<Item = Frame>,
        admitted_now: Counter,
        batch_baselines: bool,
    ) -> Result<(Vec<(usize, Frame)>, Vec<String>), String> {
        if !self.active {
            return Err("Typed mapping is stopped.".into());
        }
        if admitted_now.0 < self.clock || admitted_now.0 > u64::MAX - 600_000_000 {
            return Err("Invalid typed source admission clock.".into());
        }
        let mut frames = frames.into_iter().take(MAX_REPORTS + 1).collect::<Vec<_>>();
        if frames.len() > MAX_REPORTS
            || !super::budget::within_bytes(&frames, super::MAX_ADMISSION_BYTES)
        {
            self.effects.clear();
            self.reports.clear();
            self.fault = Some("Typed source batch exceeded its frame budget.".into());
            self.stop();
            return Err("Typed source batch exceeded its frame budget.".into());
        }
        if frames.is_empty() {
            return Ok((vec![], vec![]));
        }
        if frames
            .iter()
            .any(|frame| self.native.registrations.contains_key(&frame.source))
        {
            return Err("Native registered sources cannot use the Direct input protocol.".into());
        }
        // Capture evidence never drives the processing clock. Reject future
        // evidence before touching state, then withdraw expired samples before
        // advancing any due timer or evaluating a retained input occurrence.
        if frames.iter().any(|frame| frame.captured_us > admitted_now) {
            return Err("Typed source capture is later than its admission clock.".into());
        }
        struct Fence {
            first: usize,
            state: SourceState,
            status: SignalSample,
        }
        let mut simulated = self.sources.clone();
        let mut fences = BTreeMap::<String, Fence>::new();
        let mut ports = BTreeMap::<Endpoint, (usize, SignalSample)>::new();
        let mut invalid_sources = BTreeSet::new();
        let mut invalid = false;
        let mut diagnostics = BTreeSet::new();
        for (index, frame) in frames.iter_mut().enumerate() {
            let Some(previous) = simulated.get(&frame.source).cloned() else {
                invalid = true;
                continue;
            };
            let baseline = frame.reason == Reason::Baseline;
            if !self.valid_frame(frame, &previous)
                || baseline
                    && (frame.generation.0 < previous.generation
                        || frame.generation.0 == previous.generation
                            && frame.sequence.0 <= previous.sequence)
            {
                invalid = true;
                invalid_sources.insert(frame.source.clone());
                continue;
            }
            // Validate the original envelope and Known value first. Otherwise
            // an expired wrong-type/out-of-range value could hide as Stale.
            for change in &mut frame.changes {
                let endpoint = Endpoint {
                    node_id: frame.source.clone(),
                    port_id: change.port_id.clone(),
                };
                if matches!(self.compiled.contracts.get(&endpoint), Some(PortContract::State { freshness_ms, .. })
                    if frame.captured_us.0.saturating_add(*freshness_ms as u64 * 1000) <= admitted_now.0)
                    && matches!(change.sample, SignalSample::Known(_))
                {
                    change.sample = SignalSample::Stale;
                }
            }
            let gap = previous.available
                && !baseline
                && (previous.generation != frame.generation.0
                    || previous.sequence.checked_add(1) != Some(frame.sequence.0));
            let loss = !baseline
                && (!previous.available
                    || gap
                    || matches!(frame.reason, Reason::Gap | Reason::Disconnect));
            let mut next = previous.clone();
            if loss {
                if frame.generation.0 >= next.generation {
                    next.sequence = if frame.generation.0 == next.generation {
                        next.sequence.max(frame.sequence.0)
                    } else {
                        frame.sequence.0
                    };
                    next.generation = frame.generation.0;
                }
                next.observed = frame.captured_us.0;
                next.available = false;
                fences.insert(
                    frame.source.clone(),
                    Fence {
                        first: index + 1,
                        state: next.clone(),
                        status: if frame.reason == Reason::Disconnect {
                            SignalSample::Unavailable
                        } else {
                            SignalSample::Unknown
                        },
                    },
                );
                diagnostics.insert(format!(
                    "{} lost source continuity; a newer complete baseline is required.",
                    frame.source
                ));
            } else {
                if baseline {
                    let mut state = previous;
                    state.available = false;
                    fences.insert(
                        frame.source.clone(),
                        Fence {
                            first: index,
                            state,
                            status: SignalSample::Unknown,
                        },
                    );
                }
                next.generation = frame.generation.0;
                next.sequence = frame.sequence.0;
                next.observed = frame.captured_us.0;
                next.available = true;
                for change in &frame.changes {
                    if !matches!(change.sample, SignalSample::Known(_)) {
                        ports.insert(
                            Endpoint {
                                node_id: frame.source.clone(),
                                port_id: change.port_id.clone(),
                            },
                            (index, change.sample.clone()),
                        );
                        if !baseline {
                            diagnostics.insert(format!(
                                "{} has unavailable input samples; dependent work was revoked.",
                                frame.source
                            ));
                        }
                    }
                }
            }
            simulated.insert(frame.source.clone(), next);
        }
        let mut revoked = BTreeMap::new();
        for source in &self.compiled.program.sources {
            if fences.contains_key(&source.endpoint.node_id)
                || invalid_sources.contains(&source.endpoint.node_id)
            {
                revoked.insert(
                    source.endpoint.clone(),
                    fences
                        .get(&source.endpoint.node_id)
                        .map_or(SignalSample::Unknown, |fence| fence.status.clone()),
                );
            }
        }
        for (endpoint, (index, sample)) in &ports {
            if fences
                .get(&endpoint.node_id)
                .is_none_or(|fence| *index >= fence.first)
            {
                revoked.insert(endpoint.clone(), sample.clone());
            }
        }
        let affected = self
            .compiled
            .program
            .sinks
            .iter()
            .filter(|sink| {
                revoked.contains_key(&sink.from)
                    || self
                        .compiled
                        .dependencies
                        .get(&sink.from.node_id)
                        .is_some_and(|dependencies| {
                            dependencies
                                .iter()
                                .any(|endpoint| revoked.contains_key(endpoint))
                        })
            })
            .map(|sink| sink.id.clone())
            .collect::<BTreeSet<_>>();
        let virtual_controls = self
            .compiled
            .program
            .sinks
            .iter()
            .filter(|sink| affected.contains(&sink.id))
            .filter_map(|sink| match &sink.kind {
                SinkKind::Virtual {
                    controller_id,
                    control,
                } => Some((controller_id.clone(), control.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        // An already-produced but unconsumed action is not grandfathered across
        // loss. Preserve queued work from independent branches and targets.
        self.effects
            .retain(|effect| !affected.contains(&effect.sink_id));
        // A single baseline keeps normal output deduplication: replacing its
        // source validity must not manufacture an Unknown/startup occurrence.
        if batch_baselines || frames[0].reason != Reason::Baseline {
            self.last.retain(|sink, _| !affected.contains(sink));
        }
        for (source, fence) in &fences {
            self.sources.insert(source.clone(), fence.state.clone());
        }
        for source in &invalid_sources {
            self.sources.get_mut(source).unwrap().available = false;
        }
        for (endpoint, sample) in &revoked {
            // Even Unknown -> Unknown revokes tickets made while an explicit
            // fallback masked the previous loss; validity must never resurrect.
            self.values.get_mut(endpoint).unwrap().epoch =
                self.values[endpoint].epoch.saturating_add(1);
            self.put(endpoint, sample.clone());
        }
        self.cancel_invalid();
        if !revoked.is_empty() {
            let events = self.evaluate(revoked.keys().cloned().collect(), BTreeSet::new(), true)?;
            self.publish(events, ReportReason::SourceLost, false);
            // Reports contain whole virtual controllers. Clamp only revoked
            // slots so an independent queued press/release is not discarded.
            for report in &mut self.reports {
                let Some(current) = self.last_controllers.get(&report.target) else {
                    continue;
                };
                for (target, control) in &virtual_controls {
                    if target != &report.target {
                        continue;
                    }
                    match control {
                        crate::profile::OutputControl::Button { index } => {
                            report.state.buttons[*index as usize] = current.buttons[*index as usize]
                        }
                        crate::profile::OutputControl::Axis { index } => {
                            report.state.axes[*index as usize] = current.axes[*index as usize]
                        }
                        crate::profile::OutputControl::Hat { index } => {
                            report.state.hats[*index as usize] = current.hats[*index as usize]
                        }
                    }
                }
            }
        }
        if invalid {
            return Err(
                "Invalid bounded typed source batch; acquire complete native baselines.".into(),
            );
        }
        if !self.active {
            return Err("Typed output queue overflow.".into());
        }
        // Known losses above must win against every same-time authored timer.
        // The entire drain is admitted now; capture history cannot replay old
        // timer steps or consume a newly authored pulse before it is emitted.
        let retained = frames
            .into_iter()
            .enumerate()
            .filter(|(index, frame)| {
                fences
                    .get(&frame.source)
                    .is_none_or(|fence| *index >= fence.first)
            })
            .collect::<Vec<_>>();
        let mut prepared = Vec::new();
        for (index, mut frame) in retained {
            if frame.reason == Reason::Baseline {
                for change in &mut frame.changes {
                    if let Some((_, sample)) = ports
                        .get(&Endpoint {
                            node_id: frame.source.clone(),
                            port_id: change.port_id.clone(),
                        })
                        .filter(|(after, _)| index < *after)
                    {
                        change.sample = sample.clone();
                    }
                }
            } else {
                frame.changes.retain(|change| {
                    ports
                        .get(&Endpoint {
                            node_id: frame.source.clone(),
                            port_id: change.port_id.clone(),
                        })
                        .is_none_or(|(after, _)| index >= *after)
                });
            }
            prepared.push((index, frame));
        }
        Ok((prepared, diagnostics.into_iter().collect()))
    }
}
