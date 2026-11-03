//! Portable reducer output_work.
use super::super::*;
impl Phase {
    pub(in crate::engine::typed) fn validate_outputs(
        &self,
        node: &str,
        outputs: &BTreeMap<String, SignalValue>,
        allow_one_shot: bool,
    ) -> Result<(), String> {
        let definition = self
            .compiled
            .program
            .nodes
            .iter()
            .find(|n| n.id == node)
            .ok_or("Unknown output producer.")?;
        if outputs.len() > definition.outputs.len() {
            return Err("Too many producer outputs.".into());
        }
        for (id, value) in outputs {
            let port = definition
                .outputs
                .iter()
                .find(|p| p.id == *id)
                .ok_or("Undeclared producer output port.")?;
            if !accepts(&port.contract, value)
                || !allow_one_shot
                    && matches!(
                        port.contract,
                        PortContract::Invocation {
                            semantics: ActionSemantics::OneShot,
                            ..
                        }
                    )
            {
                return Err("Invalid producer output or one-shot replay from a baseline.".into());
            }
        }
        Ok(())
    }
    pub fn schedule(
        &mut self,
        token: Token,
        id: String,
        at: Counter,
        outputs: BTreeMap<String, SignalValue>,
    ) -> Result<(), String> {
        self.schedule_checked(token, id, at, outputs, false)
    }
    pub(in crate::engine::typed) fn schedule_checked(
        &mut self,
        token: Token,
        id: String,
        at: Counter,
        outputs: BTreeMap<String, SignalValue>,
        allow_one_shot: bool,
    ) -> Result<(), String> {
        self.validate_token(&token)?;
        crate::signal::text(&id, 80)?;
        if at.0 < self.clock || at.0 - self.clock > 60_000_000 {
            return Err("Typed timer exceeds its explicit time bound.".into());
        }
        self.validate_outputs(&token.node_id, &outputs, allow_one_shot)?;
        let replacing = self
            .timers
            .keys()
            .any(|(_, node, known_id)| node == &token.node_id && known_id == &id);
        if !replacing && self.timers.len() + self.compiled.timer_reserve >= MAX_TIMERS {
            return Err("Typed shared timer budget exceeded.".into());
        }
        self.timers
            .retain(|(_, node, known_id), _| node != &token.node_id || known_id != &id);
        let proof = if self.causal.bindings.is_empty() {
            self.proof_for_token(&token)
        } else {
            self.causal.clone()
        };
        if !proof.bindings.is_empty() || !proof.unproven.is_empty() {
            self.check_native_retained_bytes(&self.native, std::iter::once(&proof))?;
        }
        self.timers.insert(
            (at.0, token.node_id.clone(), id),
            Timer {
                token,
                outputs,
                allow_one_shot,
                proof,
            },
        );
        Ok(())
    }
    pub(in crate::engine::typed) fn apply_outputs(
        &mut self,
        node: &str,
        outputs: BTreeMap<String, SignalValue>,
        allow_events: bool,
        replace_outputs: bool,
    ) -> Result<(), String> {
        let mut dirty = BTreeSet::new();
        let mut events = BTreeSet::new();
        if replace_outputs {
            let omitted = self
                .compiled
                .program
                .nodes
                .iter()
                .find(|definition| definition.id == node)
                .unwrap()
                .outputs
                .iter()
                .filter(|port| !outputs.contains_key(&port.id))
                .map(|port| Endpoint {
                    node_id: node.into(),
                    port_id: port.id.clone(),
                })
                .collect::<Vec<_>>();
            for endpoint in omitted {
                self.put(&endpoint, SignalSample::Unknown);
                dirty.insert(endpoint);
            }
        }
        for (id, value) in outputs {
            let endpoint = Endpoint {
                node_id: node.into(),
                port_id: id,
            };
            self.put(&endpoint, SignalSample::Known(value));
            dirty.insert(endpoint.clone());
            if matches!(
                self.compiled.contracts[&endpoint],
                PortContract::Invocation {
                    semantics: ActionSemantics::Setter,
                    ..
                }
            ) || allow_events
                && matches!(
                    self.compiled.contracts[&endpoint],
                    PortContract::Event { .. } | PortContract::Invocation { .. }
                )
            {
                events.insert(endpoint);
            }
        }
        self.cancel_invalid();
        let events = self.evaluate(dirty, events, !allow_events)?;
        self.publish(events, ReportReason::Timer, false);
        Ok(())
    }
    pub fn advance(&mut self, now: Counter) -> Result<(), String> {
        if !self.active {
            return Err("Typed mapping is stopped.".into());
        }
        if now.0 < self.clock || now.0 > u64::MAX - 600_000_000 {
            return Err(
                "Typed mapping time cannot move backwards or exceed its safe deadline bound."
                    .into(),
            );
        }
        let mut work = 0;
        loop {
            let deadlines = self.pending_deadlines();
            let (stale, internal, external, native) = (
                deadlines.stale,
                deadlines.internal,
                deadlines.external,
                deadlines.native,
            );
            let Some(at) = deadlines.next().filter(|at| *at <= now.0) else {
                break;
            };
            work += 1;
            if work > MAX_REPORTS {
                self.fault = Some("Typed timer work exceeded its bound.".into());
                self.stop();
                return Err("Typed timer work exceeded its bound.".into());
            }
            self.clock = at;
            if native == Some(at) {
                self.expire_native()?;
            }
            if stale == Some(at) {
                let endpoints=self.compiled.contracts.iter().filter(|(endpoint,contract)|matches!(contract,PortContract::State{freshness_ms,..} if self.values[*endpoint].observed.saturating_add(*freshness_ms as u64*1000)<=at)&&self.values[*endpoint].proof.bindings.is_empty()&&matches!(self.values[*endpoint].sample,SignalSample::Known(_))&&!self.compiled.program.nodes.iter().any(|node|node.id==endpoint.node_id&&matches!(node.operation,Operation::Constant{..}))).map(|(endpoint,_)|endpoint.clone()).collect::<Vec<_>>();
                let dirty = endpoints.iter().cloned().collect();
                for endpoint in endpoints {
                    self.put(&endpoint, SignalSample::Stale);
                }
                self.cancel_invalid();
                let events = self.evaluate(dirty, BTreeSet::new(), true)?;
                self.publish(events, ReportReason::SourceLost, false);
            }
            if internal == Some(at) {
                let events = self.evaluate(BTreeSet::new(), BTreeSet::new(), false)?;
                self.publish(events, ReportReason::Timer, false);
            }
            // Expiry or a built-in transition above may have revoked this timer.
            // Never pop a later deadline using the head captured before revocation.
            if external == Some(at)
                && self
                    .timers
                    .first_key_value()
                    .is_some_and(|((deadline, ..), _)| *deadline == at)
            {
                let (_, timer) = self.timers.pop_first().unwrap();
                if self.validate_token(&timer.token).is_ok()
                    && self
                        .validate_outputs(
                            &timer.token.node_id,
                            &timer.outputs,
                            timer.allow_one_shot,
                        )
                        .is_ok()
                {
                    self.causal = timer.proof;
                    self.apply_outputs(
                        &timer.token.node_id,
                        timer.outputs,
                        timer.allow_one_shot,
                        false,
                    )?;
                    self.causal = native::NativeProof::default();
                }
            }
            if !self.active {
                return Err("Typed output queue overflow.".into());
            }
        }
        self.clock = now.0;
        Ok(())
    }
}
