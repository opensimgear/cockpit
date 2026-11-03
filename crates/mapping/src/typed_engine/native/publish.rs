//! Publish bounded portable effects and reports.
use super::super::*;
impl Phase {
    pub(in crate::engine::typed) fn publish(
        &mut self,
        events: BTreeSet<Endpoint>,
        reason: ReportReason,
        force: bool,
    ) {
        let mut sinks = self.compiled.program.sinks.clone();
        sinks.sort_by(|a, b| a.id.cmp(&b.id));
        let updates = sinks
            .into_iter()
            .filter_map(|sink| {
                let sample = self.values[&sink.from].sample.clone();
                // A real unchanged release can precede a press in this same
                // native batch. Preserve that neutral witness only in the
                // desktop projection, and never manufacture one from liveness.
                let observed_neutral = self.desktop_neutrals.as_ref().is_some_and(|previous| {
                    matches!(sink.kind, SinkKind::Desktop { .. })
                        && (boolean(&sample) == Some(false) || number(&sample) == Some(0.0))
                        && self.endpoint_token(&sink.from).ok().is_some_and(|token| {
                            let observations = self
                                .effect_observations(&self.values[&sink.from].proof, Some(&token));
                            !observations.is_empty()
                                && previous.get(&sink.id) != Some(&observations)
                        })
                });
                let occurrence = matches!(
                    sink.contract,
                    PortContract::Event { .. } | PortContract::Action { .. }
                );
                let fire = events.contains(&sink.from);
                if occurrence
                    && matches!(sample, SignalSample::Known(_))
                    && !fire
                    && !observed_neutral
                {
                    return None;
                }
                if !force && !fire && !observed_neutral && self.last.get(&sink.id) == Some(&sample)
                {
                    return None;
                }
                Some((sink, sample))
            })
            .collect::<Vec<_>>();
        if updates.iter().any(|(sink,sample)|matches!(sample,SignalSample::Known(value)if !accepts(&sink.contract,value))){self.fault=Some("A typed sink value exceeded its exact advertised contract.".into());self.stop();return}
        if self.active
            && updates.iter().any(|(sink, _)| {
                !self.values[&sink.from].proof.bindings.is_empty()
                    || !self.values[&sink.from].proof.unproven.is_empty()
            })
        {
            if let Err(error) = self.check_native_retained_bytes(
                &self.native,
                updates
                    .iter()
                    .map(|(sink, _)| &self.values[&sink.from].proof),
            ) {
                self.native_memory_fault(error);
                return;
            }
        }
        if self.effects.len()
            + self.reports.len()
            + updates.len()
            + self.compiled.program.controllers.len()
            > MAX_REPORTS
        {
            self.effects.clear();
            self.reports.clear();
            self.fault =
                Some("Typed output consumer fell behind; safe fallbacks were emitted.".into());
            self.active = false;
            self.timers.clear();
            for state in self.states.values_mut() {
                state.pending.clear();
                state.inflight = None;
                state.rule = crate::rules::State::default();
            }
            for state in self.values.values_mut() {
                state.sample = SignalSample::Unavailable;
                state.epoch = state.epoch.saturating_add(1);
            }
            self.publish(BTreeSet::new(), ReportReason::Overflow, true);
            return;
        }
        for (sink, sample) in updates {
            let producer = if matches!(sink.kind, SinkKind::Desktop { .. }) {
                let endpoints = self
                    .compiled
                    .dependencies
                    .get(&sink.from.node_id)
                    .cloned()
                    .unwrap_or_else(|| BTreeSet::from([sink.from.clone()]));
                Some(Token {
                    run: self.run.clone(),
                    node_id: sink.from.node_id.clone(),
                    dependencies: endpoints
                        .into_iter()
                        .map(|endpoint| Dependency {
                            generation: Counter(
                                self.sources
                                    .get(&endpoint.node_id)
                                    .map_or(0, |source| source.generation),
                            ),
                            epoch: Counter(self.values[&endpoint].epoch),
                            endpoint,
                        })
                        .collect(),
                })
            } else {
                None
            };
            let observations =
                self.effect_observations(&self.values[&sink.from].proof, producer.as_ref());
            if let Some(previous) = &mut self.desktop_neutrals {
                previous.insert(sink.id.clone(), observations.clone());
            }
            self.sequence = self.sequence.saturating_add(1);
            self.last.insert(sink.id.clone(), sample.clone());
            self.effects.push_back(Effect {
                run: self.run.clone(),
                sequence: Counter(self.sequence),
                at_us: Counter(self.clock),
                sink_id: sink.id,
                endpoint: sink.endpoint,
                sample,
                reason,
                producer,
                observations,
                native_proof: self.values[&sink.from].proof.clone(),
            });
        }
        let mut controllers = self.compiled.program.controllers.clone();
        controllers.sort_by(|a, b| a.id.cmp(&b.id));
        for controller in controllers {
            let mut state = TargetState {
                buttons: vec![false; controller.layout.buttons.len()],
                axes: controller
                    .layout
                    .axes
                    .iter()
                    .map(|axis| axis.fallback)
                    .collect(),
                hats: vec![None; controller.layout.hats.len()],
            };
            for sink in &self.compiled.program.sinks {
                let SinkKind::Virtual {
                    controller_id,
                    control,
                } = &sink.kind
                else {
                    continue;
                };
                if *controller_id != controller.id {
                    continue;
                }
                let sample = &self.values[&sink.from].sample;
                match control {
                    crate::profile::OutputControl::Button { index } => {
                        state.buttons[*index as usize] = boolean(sample).unwrap_or(false)
                    }
                    crate::profile::OutputControl::Axis { index } => {
                        if let Some(value) = number(sample) {
                            state.axes[*index as usize] = value as i32
                        }
                    }
                    crate::profile::OutputControl::Hat { index } => {
                        if let Some(value) = number(sample) {
                            state.hats[*index as usize] = (value >= 0.0).then_some(value as u8)
                        }
                    }
                }
            }
            if force || self.last_controllers.get(&controller.id) != Some(&state) {
                self.report_sequence = self.report_sequence.saturating_add(1);
                self.last_controllers
                    .insert(controller.id.clone(), state.clone());
                self.reports.push_back(Report {
                    session: self.run.session.clone(),
                    run_generation: self.run.generation,
                    sequence: Counter(self.report_sequence),
                    at_us: Counter(self.clock),
                    target: controller.id,
                    reason,
                    state,
                });
            }
        }
    }
}
