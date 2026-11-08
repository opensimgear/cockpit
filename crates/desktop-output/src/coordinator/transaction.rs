//! Ordered journal, native acknowledgement and owner projection transaction.
use super::*;

impl Coordinator {
    pub(super) fn send(
        &mut self,
        command: Command,
        deadline_us: Option<Counter>,
        possible: Vec<Control>,
    ) -> Result<Ack, String> {
        self.send_with_commit(command, deadline_us, possible, |_, _| Ok(false))
    }
    /// Apply a validated delivery's owner projection inside its ACK commit.
    /// The callback is memory-only and runs after successful native validation.
    pub(super) fn send_with_commit(
        &mut self,
        command: Command,
        deadline_us: Option<Counter>,
        possible: Vec<Control>,
        commit: impl FnOnce(&mut Self, &Ack) -> Result<bool, String>,
    ) -> Result<Ack, String> {
        if self.terminal_stopped {
            return Err("Desktop worker is stopped; acquire a fresh coordinator before activation or recovery.".into());
        }
        if self.snapshot.uncertain && !matches!(command, Command::Stop | Command::Recover { .. }) {
            return Err(
                "Desktop delivery is uncertain; explicit stopped-session recovery is required."
                    .into(),
            );
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Desktop channel sequence exhausted.")?;
        // Relative motion cannot acquire or release a held control and is never
        // replayed. Its operation counter is volatile, so successful pointer
        // updates need no recovery-evidence transaction on the lease path.
        let pointer = matches!(&command, Command::Deliver { delivery } if matches!(delivery.intent, Intent::Pointer { .. }))
            || matches!(&command, Command::DeliverPointerBatch { .. });
        // Arm replaces an owner's contribution with an empty one. The old
        // durable holds remain a recovery superset until arm() commits the
        // resulting owners. No control can be acquired on this path.
        let release_only_arm = matches!(&command, Command::Arm { .. });
        // The validated reducer projection cannot acquire a native hold when
        // every resulting control was already held. Retained durable holds
        // cover release uncertainty until the final ACK/owner commit below.
        let release_only_delivery = matches!(&command, Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. }
            if matches!(delivery.intent, Intent::Held { .. } | Intent::Cleanup))
            && possible.iter().all(|control| self.snapshot.held.contains(control));
        let changes_ledger = !pointer && !release_only_arm && !matches!(command, Command::Renew { .. });
        let mut request = Request {
            sequence: Counter(self.sequence),
            deadline_us,
            admission: self.request_admission(&command),
            command,
        };
        if let Some(channel) = self.channel.audit_id() {
            let (operation, command, run) = match &request.command {
                Command::Activate { run, .. } => (0, 1, Some(run)),
                Command::Arm { producer, .. } => (0, 2, Some(&producer.run)),
                Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. } => (delivery.operation_id.0, 3, Some(&delivery.producer.run)),
                Command::DeliverPointerBatch { deliveries } => (deliveries[0].operation_id.0, 8, Some(&deliveries[0].producer.run)),
                Command::Renew { run } => (0, 4, Some(run)),
                Command::Pause { run } => (0, 5, Some(run)),
                Command::Recover { .. } => (0, 6, None),
                Command::Stop => (0, 7, None),
            };
            let run_alias = run.and_then(|run| crate::audit::alias(crate::audit::AliasKind::Run, run.session.as_bytes())).unwrap_or(0);
            crate::audit::record(crate::audit::Kind::RequestIssued,
                &[channel, request.sequence.0, operation, command, run_alias, run.map_or(0, |run| run.generation.0)]);
        }
        if pointer || release_only_arm || release_only_delivery {
            if let Err(error) = self.verify_current_evidence() {
                self.audit_outcome(&request, None, 1);
                return Err(error);
            }
        }
        if changes_ledger && !release_only_delivery {
            if !self.snapshot.uncertain {
                self.snapshot.possible_held = self.snapshot.held.clone();
            }
            self.snapshot.possible_held.extend(possible);
            self.snapshot.in_flight = Some(request.clone());
            if let Err(error) = self.persist() {
                self.audit_outcome(&request, None, 1);
                return Err(error);
            }
        }
        let revocable = match &request.command {
            Command::Deliver { delivery } | Command::DeliverGuarded { delivery, .. } => !matches!(&delivery.intent, Intent::Cleanup)
                && !matches!(&delivery.intent, Intent::Held { controls } if controls.is_empty()),
            Command::DeliverPointerBatch { .. } | Command::Arm { .. } => true,
            _ => false,
        };
        if revocable && self.dispatch_gate.as_ref().is_some_and(|gate| !gate(&request)) {
            // The journal deliberately retains the conservative original request;
            // recovery releases possible holds and never replays its deadline.
            request.deadline_us = Some(Counter(0));
        }
        let resolving_timeout = matches!(request.command, Command::Stop) && self.channel.has_timed_out_reply();
        let result = self.channel.request(&request).and_then(|ack| {
            if ack.sequence != request.sequence {
                return Err("Stale desktop worker acknowledgement.".into());
            }
            if let Some(delivery) = request.command.delivery() {
                if ack.operation_id != Some(delivery.operation_id) {
                    return Err("Desktop acknowledgement has the wrong operation identity.".into());
                }
            }
            pointer_batch::validate_ack(&request, &ack)?;
            if ack.attempted > MAX_BATCH
                || ack.confirmed > ack.attempted
                || (ack.ok && ack.attempted != ack.confirmed)
            {
                return Err(
                    "Desktop worker acknowledged an incomplete or invalid native batch.".into(),
                );
            }
            crate::reducer::recovery_releases(&ack.held)?;
            if matches!(request.command, Command::Stop | Command::Recover { .. })
                && (!ack.cleanup_complete || !ack.held.is_empty()) {
                return Err("Desktop worker cleanup acknowledgement is incomplete.".into());
            }
            let stopping = ack.ok && matches!(request.command, Command::Stop);
            let mut reconciled = if stopping && resolving_timeout {
                // An unacknowledged delivery may have installed a guard that
                // released before Stop. The exact empty terminal ACK proves
                // all releases without trusting intermediate owner receipts.
                if ack.released_guards.len() > MAX_OWNERS {
                    return Err("Desktop release receipt budget exceeded.".into());
                }
                Some(self.reducer.clone())
            } else {
                self.stage_guard_receipts(&ack)?
            };
            let scopes = self.stage_scope_receipts(&request, &ack)?;
            if !scopes.paused.is_empty() {
                let next = reconciled.get_or_insert_with(|| self.reducer.clone());
                for run in &scopes.paused { next.pause(run); }
            }
            if ack.ok && matches!(request.command, Command::Stop | Command::Recover { .. }) {
                if let Some(next) = &mut reconciled { next.stop(); }
            }
            if pointer && !reconciled.as_ref().map_or_else(|| ack.held == self.snapshot.held,
                |next| ack.held == next.held()) {
                return Err("Desktop pointer acknowledgement changed the held-control ledger.".into());
            }
            if ack.scope_rejected && ack.held != reconciled.as_ref().unwrap_or(&self.reducer).held() {
                return Err("Desktop scope rejection changed unexplained held controls.".into());
            }
            if release_only_arm && ack.held.iter().any(|control| !self.snapshot.held.contains(control)) {
                return Err("Desktop arm acknowledgement acquired an unexpected held control.".into());
            }
            if release_only_delivery && ack.held.iter().any(|control| !self.snapshot.held.contains(control)) {
                return Err("Desktop release acknowledgement acquired an unexpected held control.".into());
            }
            Ok((ack, reconciled, scopes))
        });
        match result {
            Ok((ack, reconciled, scopes)) => {
                self.snapshot.held = ack.held.clone();
                self.snapshot.cleanup_complete = ack.cleanup_complete;
                self.snapshot.error = ack.error.clone();
                let reconciled_changed = reconciled.is_some();
                if !scopes.paused.is_empty() { self.snapshot.phase = "paused".into(); }
                let owners_changed = if let Some(reconciled) = reconciled {
                    self.reducer = reconciled;
                    let Some(revision) = self.projection_revision.checked_add(1) else {
                        let error = "Desktop projection revision exhausted.".to_string();
                        self.snapshot.uncertain = true;
                        self.snapshot.cleanup_complete = false;
                        self.snapshot.phase = "faulted".into();
                        self.snapshot.error = Some(error.clone());
                        let _ = self.apply_scope_stage(scopes);
                        let _ = self.persist();
                        return Err(error);
                    };
                    self.projection_revision = revision;
                    self.update_owners()
                } else { false };
                if ack.uncertain {
                    self.snapshot.uncertain = true;
                    self.snapshot.phase = "faulted".into();
                    let _ = self.apply_scope_stage(scopes);
                    let _ = self.persist();
                    self.audit_outcome(&request, Some(&ack), 2);
                    return Err(ack.error.unwrap_or("Uncertain native submission.".into()));
                }
                if ack.ok {
                    // Install only after the complete ACK has validated. Every
                    // projection below starts from these exact released owners.
                    let projection = commit(self, &ack).and_then(|changed| {
                        if self.reducer.held() != ack.held {
                            return Err("Desktop receipt disagrees with the acknowledged owner projection.".into());
                        }
                        self.finish_guard_receipts(&request, &ack);
                        // This callback is memory-only. Revocation reaches the
                        // host before any potentially blocking final persistence.
                        self.apply_scope_stage(scopes)?;
                        Ok(changed)
                    });
                    let metadata_changed = match projection {
                        Ok(changed) => changed,
                        Err(error) => {
                            self.snapshot.uncertain = true;
                            self.snapshot.cleanup_complete = false;
                            self.snapshot.phase = "faulted".into();
                            self.snapshot.error = Some(error.clone());
                            let _ = self.persist();
                            self.audit_outcome(&request, Some(&ack), 3);
                            return Err(error);
                        }
                    };
                    // Only a fully validated projection may narrow recovery
                    // evidence. Forged/invalid ACKs retain the prepared request.
                    self.snapshot.in_flight = None;
                    self.snapshot.possible_held = ack.held.clone();
                    if changes_ledger || metadata_changed || owners_changed || reconciled_changed {
                        if let Err(error) = self.persist() {
                            self.snapshot.uncertain = true;
                            self.snapshot.cleanup_complete = false;
                            self.snapshot.phase = "faulted".into();
                            self.snapshot.error = Some(error.clone());
                            self.audit_outcome(&request, Some(&ack), 3);
                            return Err(error);
                        }
                    }
                    self.audit_outcome(&request, Some(&ack), 0);
                    Ok(ack)
                } else {
                    let error = if self.reducer.held() != ack.held {
                        Some("Rejected desktop acknowledgement changed unexplained held controls.".to_string())
                    } else { self.apply_scope_stage(scopes).err() };
                    if let Some(error) = error {
                        self.snapshot.uncertain = true;
                        self.snapshot.cleanup_complete = false;
                        self.snapshot.phase = "faulted".into();
                        self.snapshot.error = Some(error.clone());
                        let _ = self.persist();
                        return Err(error);
                    }
                    self.snapshot.in_flight = None;
                    self.finish_guard_receipts(&request, &ack);
                    self.snapshot.phase = "paused".into();
                    let _ = self.persist();
                    self.audit_outcome(&request, Some(&ack), 4);
                    Err(ack
                        .error
                        .unwrap_or("Desktop worker rejected output.".into()))
                }
            }
            Err(error) => {
                self.snapshot.uncertain = true;
                self.snapshot.cleanup_complete = false;
                self.snapshot.phase = "faulted".into();
                self.snapshot.error = Some(error.clone());
                let _ = self.persist();
                self.audit_outcome(&request, None, 5);
                Err(error)
            }
        }
    }
}
