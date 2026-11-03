//! Portable reducer scripts.
use super::super::*;
impl Phase {
    pub(in crate::engine::typed) fn enqueue_script(
        &mut self,
        node: &Node,
        inputs: BTreeMap<String, SignalSample>,
        allow_one_shot: bool,
    ) -> Result<(), String> {
        if self.states[&node.id].fault {
            return Ok(());
        }
        let queued = self
            .states
            .values()
            .map(|s| s.pending.len() + usize::from(s.inflight.is_some()))
            .sum::<usize>();
        if queued >= crate::script::QUEUE_LIMIT {
            self.script_fault(&node.id, "Typed script queue overflow.");
            return Ok(());
        }
        let token = self.token(&node.id)?;
        let proof = self.native_inputs(node);
        if !proof.bindings.is_empty() || !proof.unproven.is_empty() {
            if let Err(error) =
                self.check_native_retained_bytes(&self.native, std::iter::once(&proof))
            {
                self.script_fault(&node.id, &error);
                return Ok(());
            }
        }
        let state = self.states.get_mut(&node.id).unwrap();
        state.script_sequence = state
            .script_sequence
            .checked_add(1)
            .ok_or("Script sequence exhausted.")?;
        let Operation::Script { parameters, .. } = &node.operation else {
            unreachable!()
        };
        let invocation = ScriptInvocation {
            node_id: node.id.clone(),
            ticket: ScriptTicket {
                producer: token,
                sequence: Counter(state.script_sequence),
            },
            at_us: Counter(self.clock),
            callback: if state.script_started {
                "on_event"
            } else {
                "on_start"
            }
            .into(),
            event_trigger: allow_one_shot,
            inputs,
            parameters: parameters.clone(),
            state: state.script_state.clone(),
        };
        if serde_json::to_vec(&invocation)
            .map_err(|e| e.to_string())?
            .len()
            > crate::script::IPC_LIMIT
        {
            self.script_fault(&node.id, "Typed script invocation exceeds its IPC budget.");
            return Ok(());
        }
        state.script_started = true;
        state.bindings.insert(state.script_sequence, proof);
        state.pending.push_back(invocation);
        Ok(())
    }
    pub fn script_requests(&mut self) -> Vec<ScriptInvocation> {
        self.retire_native_work();
        let mut requests = Vec::new();
        let mut oversized = Vec::new();
        for (node, state) in &mut self.states {
            if state.inflight.is_none() {
                if let Some(mut request) = state.pending.pop_front() {
                    request.state = state.script_state.clone();
                    if serde_json::to_vec(&request)
                        .map_or(true, |bytes| bytes.len() > crate::script::IPC_LIMIT)
                    {
                        oversized.push(node.clone());
                        continue;
                    }
                    state.inflight = Some((request.ticket.clone(), request.event_trigger));
                    requests.push(request);
                }
            }
        }
        for node in oversized {
            self.script_fault(
                &node,
                "Typed script invocation exceeds its IPC budget after state update.",
            );
        }
        requests
    }
    /// Native workers can discard stale replies and faults without affecting a
    /// newer callback from the same node.
    pub fn script_ticket_current(&self, ticket: &ScriptTicket) -> bool {
        self.active
            && self
                .states
                .get(&ticket.producer.node_id)
                .and_then(|state| state.inflight.as_ref())
                .is_some_and(|(expected, _)| expected == ticket)
            && self.validate_token(&ticket.producer).is_ok()
            && self.states[&ticket.producer.node_id]
                .bindings
                .get(&ticket.sequence.0)
                .is_none_or(|proof| proof.valid(&self.native, self.clock, false))
    }
    pub fn script_reply(
        &mut self,
        node: &str,
        ticket: &ScriptTicket,
        transaction: ScriptTransaction,
    ) -> Result<(), String> {
        if ticket.producer.node_id != node || !self.script_ticket_current(ticket) {
            return Err("Stale typed script ticket.".into());
        }
        self.causal = self.states[node]
            .bindings
            .get(&ticket.sequence.0)
            .cloned()
            .unwrap_or_default();
        let result = self.script_reply_inner(node, ticket, transaction);
        self.causal = native::NativeProof::default();
        self.states
            .get_mut(node)
            .unwrap()
            .bindings
            .remove(&ticket.sequence.0);
        if let Err(error) = &result {
            self.script_fault(node, error);
        }
        result
    }
    pub(in crate::engine::typed) fn script_reply_inner(
        &mut self,
        node: &str,
        ticket: &ScriptTicket,
        transaction: ScriptTransaction,
    ) -> Result<(), String> {
        let allow = self
            .states
            .get(node)
            .and_then(|state| state.inflight.as_ref())
            .filter(|(expected, _)| expected == ticket)
            .map(|(_, allow)| *allow)
            .ok_or("Stale or unexpected typed script ticket.")?;
        self.validate_token(&ticket.producer)?;
        self.validate_outputs(node, &transaction.outputs, allow)?;
        if transaction.timers.len() > MAX_TIMERS
            || self
                .timers
                .values()
                .filter(|timer| timer.token.node_id != node)
                .count()
                + self.compiled.timer_reserve
                + transaction.timers.len()
                > MAX_TIMERS
            || serde_json::to_vec(&transaction)
                .map_err(|e| e.to_string())?
                .len()
                > crate::script::IPC_LIMIT
            || serde_json::to_vec(&transaction.state)
                .map_err(|e| e.to_string())?
                .len()
                > crate::script::STATE_LIMIT
        {
            return Err("Typed script transaction exceeds a shared budget.".into());
        }
        for (key, value) in &transaction.state {
            crate::signal::text(key, 80)?;
            if matches!(value,PortValue::Number(value) if !value.is_finite()||value.abs()>crate::signal::MAX_SAFE_INTEGER as f64)
            {
                return Err("Invalid typed script state scalar.".into());
            }
        }
        let mut ids = BTreeSet::new();
        for timer in &transaction.timers {
            crate::signal::text(&timer.id, 80)?;
            if !ids.insert(&timer.id)
                || timer.after_us.0 == 0
                || timer.after_us.0 > 60_000_000
                || self.clock.checked_add(timer.after_us.0).is_none()
            {
                return Err("Invalid typed script timer.".into());
            }
            self.validate_outputs(node, &timer.outputs, allow)?;
        }
        // Every callback replaces its complete output contribution and timer set.
        // All fields and the final shared budget have passed validation above.
        self.timers.retain(|_, timer| timer.token.node_id != node);
        for timer in transaction.timers {
            self.schedule_checked(
                ticket.producer.clone(),
                timer.id,
                Counter(self.clock + timer.after_us.0),
                timer.outputs,
                allow,
            )?;
        }
        let state = self.states.get_mut(node).unwrap();
        state.script_state = transaction.state;
        state.inflight = None;
        self.apply_outputs(node, transaction.outputs, allow, true)
    }
    pub fn script_fault(&mut self, node: &str, _reason: &str) {
        if let Some(state) = self.states.get_mut(node) {
            state.fault = true;
            state.pending.clear();
            state.inflight = None;
            state.script_state.clear();
            state.bindings.clear();
            state.rule_proof = native::NativeProof::default();
        }
        self.timers.retain(|_, timer| timer.token.node_id != node);
        let outputs = self
            .compiled
            .program
            .nodes
            .iter()
            .find(|n| n.id == node)
            .map(|n| {
                n.outputs
                    .iter()
                    .map(|port| Endpoint {
                        node_id: node.into(),
                        port_id: port.id.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let dirty = outputs.iter().cloned().collect();
        for endpoint in outputs {
            self.put(&endpoint, SignalSample::Unknown);
        }
        if let Ok(events) = self.evaluate(dirty, BTreeSet::new(), true) {
            self.publish(events, ReportReason::SourceLost, false);
        }
    }
    pub fn stop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        self.timers.clear();
        for state in self.states.values_mut() {
            state.rule = crate::rules::State::default();
            state.pending.clear();
            state.inflight = None;
            state.script_state.clear();
            state.bindings.clear();
            state.rule_proof = native::NativeProof::default();
        }
        for state in self.values.values_mut() {
            state.sample = SignalSample::Unavailable;
            state.epoch = state.epoch.saturating_add(1);
            state.proof = native::NativeProof::default();
        }
        self.publish(BTreeSet::new(), ReportReason::Stop, true);
    }
}
