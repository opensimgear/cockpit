//! Ephemeral helper evidence is never restored from a recovery journal.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ScopeUpdate {
    pub run: RunToken,
    pub receipt: ScopeReceipt,
}

#[cfg(test)]
#[path = "scope_test_support.rs"]
mod test_support;
#[cfg(test)]
#[path = "scope_receipt_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "proof_recovery_tests.rs"]
mod proof_recovery_tests;
pub(super) struct ScopeStage {
    pub(super) next: Option<BTreeMap<u64, ScopeUpdate>>,
    pub(super) paused: Vec<RunToken>,
    updates: Vec<ScopeUpdate>,
}

impl Coordinator {
    pub(super) fn admission_is_stale(&self, command: &Command) -> bool {
        let Some(admission) = self.admission else { return false };
        let run = match command {
            Command::Arm { producer, .. } => &producer.run,
            Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. } => &delivery.producer.run,
            Command::DeliverKeyPair { deliveries } => &deliveries[0].producer.run,
            Command::DeliverPointerBatch { deliveries } => {
                let Some(first) = deliveries.first() else { return false }; &first.producer.run
            }
            _ => return false,
        };
        self.scope_receipt(run).map(|value| value.admission) != Some(admission)
    }
    pub fn set_scope_observer(&mut self, observer: Arc<dyn Fn(ScopeUpdate) + Send + Sync>) {
        self.scope_observer = Some(observer);
    }
    pub fn scope_receipt(&self, run: &RunToken) -> Option<ScopeReceipt> {
        self.scope_epochs.values().find(|value| value.run == *run).map(|value| value.receipt)
    }
    /// One immutable scheduler shadow supplies the proof for an entire job.
    /// Clearing this context restores synchronous command composition only.
    pub fn set_admission(&mut self, admission: Option<ScopeAdmission>) {
        self.admission = admission;
    }
    pub(super) fn request_admission(&self, command: &Command) -> Option<ScopeAdmission> {
        let run = match command {
            Command::Arm { producer, .. } => &producer.run,
            Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. } => &delivery.producer.run,
            Command::DeliverKeyPair { deliveries } => &deliveries[0].producer.run,
            Command::DeliverPointerBatch { deliveries } => &deliveries.first()?.producer.run,
            _ => return None,
        };
        self.admission.or_else(|| self.scope_receipt(run).map(|value| value.admission))
    }
    pub(super) fn stage_scope_receipts(&self, request: &Request, ack: &Ack) -> Result<ScopeStage, String> {
        if ack.scopes.len() > 64 {
            return Err("Desktop scope receipt budget exceeded.".into());
        }
        let rejectable = match &request.command {
            Command::Arm { .. } | Command::DeliverPointerBatch { .. } | Command::DeliverKeyPair { .. } => true,
            Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. } =>
                !matches!(delivery.intent, Intent::Cleanup)
                    && !matches!(&delivery.intent, Intent::Held { controls } if controls.is_empty()),
            _ => false,
        };
        if ack.scope_rejected && (!ack.ok || !ack.skipped || ack.collision
            || ack.attempted != 0 || ack.confirmed != 0
            || !rejectable) {
            return Err("Invalid desktop scope rejection acknowledgement.".into());
        }
        // Uncertain delivery has no remaining authority. The helper has already
        // attempted terminal cleanup; no future input may use these epochs.
        if ack.uncertain {
            if !ack.scopes.is_empty() {
                return Err("Uncertain desktop acknowledgement retained live scope evidence.".into());
            }
            return Ok(ScopeStage {
                next: (!self.scope_epochs.is_empty()).then(BTreeMap::new),
                // An uncertain reply is not an attestation of physical release.
                // Retain the conservative reducer/held evidence for recovery.
                paused: vec![],
                updates: self.scope_epochs.values().map(|old| ScopeUpdate {
                    run: old.run.clone(), receipt: ScopeReceipt { live: false, eligible: false, ..old.receipt },
                }).collect(),
            });
        }
        let stopping = ack.ok && matches!(request.command, Command::Stop);
        let activating = match &request.command {
            Command::Activate { run, .. } if ack.ok => Some(run),
            _ => None,
        };
        if let Some(run) = activating {
            if self.scope_epochs.contains_key(&request.sequence.0)
                || self.scope_epochs.values().any(|value| value.run == *run) {
                return Err("Desktop scope registration was reused.".into());
            }
        }
        let expected = if stopping { 0 } else { self.scope_epochs.len() + usize::from(activating.is_some()) };
        if ack.scopes.len() != expected {
            return Err("Desktop scope acknowledgement has missing or extra registrations.".into());
        }
        let mut stage = ScopeStage { next: None, paused: vec![], updates: vec![] };
        if stopping {
            stage.next = (!self.scope_epochs.is_empty()).then(BTreeMap::new);
            stage.paused = self.scope_epochs.values().map(|value| value.run.clone()).collect();
            stage.updates = self.scope_epochs.values().map(|old| ScopeUpdate {
                run: old.run.clone(), receipt: ScopeReceipt { live: false, eligible: false, ..old.receipt },
            }).collect();
            return Ok(stage);
        }
        // Avoid an allocation on each unchanged pointer acknowledgement.
        let mut seen = [0u64; 64];
        for (index, receipt) in ack.scopes.iter().enumerate() {
            let id = receipt.admission.registration.0;
            if id == 0 || receipt.admission.epoch.0 == 0 || seen[..index].contains(&id) {
                return Err("Invalid or duplicate desktop scope receipt.".into());
            }
            seen[index] = id;
            let old = self.scope_epochs.get(&id);
            let run = if let Some(old) = old {
                if receipt.admission.epoch < old.receipt.admission.epoch
                    || (receipt.admission.epoch == old.receipt.admission.epoch
                        && ((!receipt.eligible && old.receipt.eligible) || !receipt.live)) {
                    return Err("Regressed desktop scope evidence or unreported loss.".into());
                }
                &old.run
            } else if id == request.sequence.0 && activating.is_some() {
                if receipt.admission.epoch.0 != 1 || !receipt.live {
                    return Err("Invalid initial desktop scope receipt.".into());
                }
                activating.unwrap()
            } else {
                return Err("Unknown desktop scope receipt registration.".into());
            };
            if !receipt.live && receipt.eligible {
                return Err("Expired desktop scope remained eligible.".into());
            }
            if old.is_some_and(|old| old.receipt.admission != receipt.admission) || !receipt.live {
                stage.paused.push(run.clone());
            }
            if old.is_none_or(|old| old.receipt != *receipt) {
                let update = ScopeUpdate { run: run.clone(), receipt: *receipt };
                let next = stage.next.get_or_insert_with(|| self.scope_epochs.clone());
                if receipt.live { next.insert(id, update.clone()); } else { next.remove(&id); }
                stage.updates.push(update);
            }
        }
        Ok(stage)
    }
    pub(super) fn apply_scope_stage(&mut self, stage: ScopeStage) -> Result<(), String> {
        if let Some(next) = stage.next { self.scope_epochs = next; }
        if let Some(observer) = &self.scope_observer {
            for update in stage.updates {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer(update)))
                    .map_err(|_| "Desktop scope observer panicked.".to_string())?;
            }
        }
        Ok(())
    }
}
