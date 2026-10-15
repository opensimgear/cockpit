//! Script ownership belongs to the reducer, never the helper.
use super::*;
use crate::script::{Invocation, Ticket, Transaction, IPC_LIMIT, QUEUE_LIMIT};
impl Engine {
    pub(super) fn clear_scripts(&mut self) {
        for instance in self.scripts.values_mut() {
            instance.epoch = instance.epoch.saturating_add(1);
            instance.state = crate::script::empty();
            instance.pending.clear();
            instance.inflight = None;
            instance.timers.clear();
        }
    }
    pub fn script_fault(&mut self, id: &str, reason: &str) {
        if let Some(script) = self.compiled.profile.scripts.iter().find(|s| s.id == id) {
            for output in &script.outputs {
                self.contributions.remove(&script.owner(&output.id));
            }
        }
        if let Some(instance) = self.scripts.get_mut(id) {
            instance.epoch = instance.epoch.saturating_add(1);
            instance.state = crate::script::empty();
            instance.pending.clear();
            instance.inflight = None;
            instance.timers.clear();
            instance.fault = Some(reason.chars().take(512).collect());
        }
        if self.active {
            self.publish(ReportReason::Change, self.clock, false);
        }
    }
    pub(super) fn invalidate_scripts(&mut self, source: &Source) {
        let ids = self
            .compiled
            .profile
            .scripts
            .iter()
            .filter(|script| self.affected(&script.owner(&script.outputs[0].id), source))
            .map(|s| s.id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            if self.scripts[&id].started {
                self.script_fault(
                    &id,
                    "A declared source changed or was lost; start again with fresh baselines.",
                );
            }
        }
    }
    fn enqueue_script(&mut self, id: &str, callback: &str, event: serde_json::Value, at: u64) {
        let queued = self
            .scripts
            .values()
            .map(|s| s.pending.len() + usize::from(s.inflight.is_some()))
            .sum::<usize>();
        if queued >= QUEUE_LIMIT {
            self.script_fault(id, "Script callback queue overflow.");
            return;
        }
        let script = self
            .compiled
            .profile
            .scripts
            .iter()
            .find(|s| s.id == id)
            .expect("declared script");
        let Ok(producer) = self.token(&script.owner(&script.outputs[0].id)) else {
            return;
        };
        let mut inputs = BTreeMap::<String, BTreeMap<String, Value>>::new();
        for input in &script.inputs {
            if let Input::Source { role, control } = input {
                let Some(value) = self.input(input) else {
                    return;
                };
                inputs
                    .entry(role.clone())
                    .or_default()
                    .insert(control.clone(), value);
            }
        }
        let instance = self.scripts.get_mut(id).expect("script state");
        if instance.fault.is_some() {
            return;
        }
        instance.sequence = instance.sequence.saturating_add(1);
        let invocation = Invocation {
            ticket: Ticket {
                producer,
                epoch: Counter(instance.epoch),
                sequence: Counter(instance.sequence),
            },
            callback: callback.into(),
            at_us: Counter(at),
            inputs,
            event,
            parameters: script.parameters.clone(),
            state: crate::script::empty(),
        };
        instance.started = true;
        instance.pending.push_back(invocation);
    }
    pub(super) fn queue_scripts(&mut self, source: Option<&Source>, event: serde_json::Value) {
        let scripts = self
            .compiled
            .profile
            .scripts
            .iter()
            .filter(|s| {
                source.is_none_or(|source| self.affected(&s.owner(&s.outputs[0].id), source))
            })
            .map(|s| s.id.clone())
            .collect::<Vec<_>>();
        for id in scripts {
            let callback = if self.scripts[&id].started {
                "on_event"
            } else {
                "on_start"
            };
            self.enqueue_script(&id, callback, event.clone(), self.clock);
        }
    }
    pub(super) fn queue_script_timers(&mut self) {
        let mut due = Vec::new();
        for (id, instance) in &mut self.scripts {
            while let Some(((at, timer), ())) = instance.timers.first_key_value() {
                if *at > self.clock {
                    break;
                }
                due.push((id.clone(), *at, timer.clone()));
                instance.timers.pop_first();
            }
        }
        due.sort();
        for (id, at, timer) in due {
            self.enqueue_script(&id, "on_timer", serde_json::json!({"timer":timer}), at);
        }
    }
    pub fn script_requests(&mut self) -> Vec<(String, Invocation)> {
        let mut requests = Vec::new();
        let mut faults = Vec::new();
        if !self.active {
            return requests;
        }
        for (id, instance) in &mut self.scripts {
            if instance.inflight.is_some() || instance.fault.is_some() {
                continue;
            }
            if let Some(mut request) = instance.pending.pop_front() {
                request.state = if instance.state.is_null() {
                    crate::script::empty()
                } else {
                    instance.state.clone()
                };
                if serde_json::to_vec(&request).map_or(true, |bytes| bytes.len() > IPC_LIMIT) {
                    faults.push(id.clone());
                    continue;
                }
                instance.inflight = Some(request.clone());
                requests.push((id.clone(), request));
            }
        }
        for id in faults {
            self.script_fault(&id, "Script invocation exceeds 32 KiB.");
        }
        requests
    }
    pub fn script_ticket_current(&self, id: &str, ticket: &Ticket) -> bool {
        self.active
            && self
                .scripts
                .get(id)
                .and_then(|s| s.inflight.as_ref())
                .is_some_and(|request| request.ticket == *ticket)
            && self
                .token(&ticket.producer.node)
                .is_ok_and(|token| token == ticket.producer)
    }
    pub fn script_reply(
        &mut self,
        id: &str,
        ticket: &Ticket,
        transaction: Transaction,
    ) -> Result<(), String> {
        if !self.script_ticket_current(id, ticket) {
            return Err("Obsolete script worker reply.".into());
        }
        let result: Result<(), String> = (|| {
            let script = self
                .compiled
                .profile
                .scripts
                .iter()
                .find(|s| s.id == id)
                .ok_or("Unknown script.")?;
            transaction.validate(script, &self.compiled.profile)?;
            let total = self.timers.len()
                + self.compiled.timer_reserve
                + self
                    .scripts
                    .iter()
                    .filter(|(key, _)| *key != id)
                    .map(|(_, s)| s.timers.len())
                    .sum::<usize>()
                + transaction.timers.len();
            if total > MAX_TIMERS {
                return Err("Shared mapping timer budget exceeded.".into());
            }
            let instance = &self.scripts[id];
            let count = if self.clock.saturating_sub(instance.log_window) >= 1_000_000 {
                0
            } else {
                instance.log_count
            };
            if count + transaction.logs.len() > 16 {
                return Err("Script log rate exceeds 16 messages per second.".into());
            }
            // No mutation happens before every field and shared budget has passed validation.
            for output in &script.outputs {
                self.contributions.remove(&script.owner(&output.id));
            }
            for intent in transaction.outputs {
                self.contributions
                    .insert(script.owner(&intent.output), intent.value);
            }
            let instance = self.scripts.get_mut(id).expect("script state");
            instance.state = transaction.state;
            instance.inflight = None;
            instance.timers.clear();
            instance
                .pending
                .retain(|request| request.callback != "on_timer");
            for timer in transaction.timers {
                instance
                    .timers
                    .insert((self.clock.saturating_add(timer.after_us), timer.id), ());
            }
            if count == 0 {
                instance.log_window = self.clock;
                instance.log_count = 0;
            }
            instance.log_count += transaction.logs.len();
            for log in transaction.logs {
                if instance.logs.len() == 16 {
                    instance.logs.pop_front();
                }
                instance.logs.push_back(log);
            }
            self.publish(ReportReason::Change, self.clock, false);
            Ok(())
        })();
        if let Err(error) = &result {
            self.script_fault(id, error);
        }
        result
    }
    pub fn script_enabled(&self, id: &str) -> bool {
        self.active && self.scripts.get(id).is_some_and(|s| s.fault.is_none())
    }
    pub fn script_inflight(&self, id: &str) -> Option<&Invocation> {
        self.scripts.get(id)?.inflight.as_ref()
    }
    pub fn script_status(&self) -> serde_json::Value {
        serde_json::json!(self.scripts.iter().map(|(id,s)|serde_json::json!({"id":id,"started":s.started,"fault":s.fault,"pending":s.pending.len(),"inflight":s.inflight.is_some(),"timers":s.timers.len(),"logs":s.logs.iter().rev().take(4).collect::<Vec<_>>()})).collect::<Vec<_>>())
    }
}
