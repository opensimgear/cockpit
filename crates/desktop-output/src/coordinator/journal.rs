//! Durable evidence commits finish before helper dispatch; the OS lock spans the session.
use super::*;
use std::fs::{File, OpenOptions};
pub(super) struct Store {
    path: PathBuf,
    evidence: osg_persistence::Evidence,
    lock: File,
}
impl Store {
    fn open(path: &Path) -> Result<Self, String> {
        let lock_path = path.with_extension("lock");
        osg_persistence::check_path(&lock_path, true)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| "Cannot open desktop recovery ownership lock.")?;
        lock.try_lock()
            .map_err(|_| "Another desktop output session owns this recovery evidence.")?;
        let evidence =
            osg_persistence::Evidence::open_live(path, "recovery-desktop", MAX_JOURNAL_BYTES)?;
        if evidence.payload().is_some() {
            Coordinator::inspect_journal(path)?;
            evidence.verify()?;
        }
        Ok(Self {
            path: path.into(),
            evidence,
            lock,
        })
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = self.lock.unlock();
    }
}
impl Coordinator {
    pub fn spawn_with_journal(
        enabled: bool,
        qualified: bool,
        path: PathBuf,
    ) -> Result<Self, String> {
        Self::spawn_initial(enabled, qualified, path, None, None)
    }
    fn spawn_initial(
        enabled: bool,
        qualified: bool,
        path: PathBuf,
        owner: Option<RunToken>,
        scope: Option<Scope>,
    ) -> Result<Self, String> {
        let store = Store::open(&path)?;
        if store.evidence.payload().is_some() {
            if let Some(owner) = &owner {
                Self::inspect_owned_journal(&path, owner, scope.as_ref())?;
            }
            let previous = Self::inspect_journal(&path)?;
            if previous.uncertain
                || !previous.held.is_empty()
                || previous.in_flight.is_some()
                || !previous.cleanup_complete
            {
                return Err(
                    "Interrupted desktop output requires explicit recovery before activation."
                        .into(),
                );
            }
        }
        store.evidence.verify()?;
        let mut coordinator = Self::spawn(enabled, qualified)?;
        *coordinator.journal_store.borrow_mut() = Some(store);
        coordinator.journal = Some(path);
        coordinator.snapshot.journal_owner = owner;
        coordinator.snapshot.journal_scope = scope;
        coordinator.persist()?;
        Ok(coordinator)
    }
    /// ProfileSession composition pins receipt identity before Start can arm or
    /// deliver. The identity survives clean Stop and explicit recovery.
    pub fn spawn_with_owned_journal(
        enabled: bool,
        qualified: bool,
        path: PathBuf,
        owner: RunToken,
        scope: Option<Scope>,
    ) -> Result<Self, String> {
        Self::spawn_initial(enabled, qualified, path, Some(owner), scope)
    }
    pub fn inspect_journal(path: &Path) -> Result<Snapshot, String> {
        let bytes =
            osg_persistence::Evidence::inspect(path, "recovery-desktop", MAX_JOURNAL_BYTES)?
                .ok_or("Desktop recovery evidence is missing.")?;
        let snapshot: Snapshot = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid desktop recovery journal: {e}"))?;
        crate::reducer::recovery_releases(&snapshot.possible_held)?;
        crate::reducer::recovery_releases(&snapshot.held)?;
        Ok(snapshot)
    }
    /// Bind a journal to the owning ProfileSession receipt before explicit
    /// recovery. A well-formed record for another run must never release keys.
    pub fn inspect_owned_journal(
        path: &Path,
        expected: &RunToken,
        scope: Option<&Scope>,
    ) -> Result<Snapshot, String> {
        let record = Self::inspect_journal(path)?;
        if record.journal_owner.as_ref() != Some(expected) {
            return Err("Desktop recovery journal is not bound to this exact ProfileSession run and generation.".into());
        }
        if scope.is_some() && record.journal_scope.as_ref() != scope {
            return Err(
                "Desktop recovery journal application scope does not match its receipt.".into(),
            );
        }
        let validate_run = |run: &RunToken| {
            if run == expected {
                Ok(())
            } else {
                Err(
                    "Desktop recovery journal belongs to a different run or generation."
                        .to_string(),
                )
            }
        };
        let validate_scope = |actual: &Scope| {
            if scope.is_none_or(|expected| expected == actual) {
                Ok(())
            } else {
                Err(
                    "Desktop recovery journal application scope does not match its receipt."
                        .to_string(),
                )
            }
        };
        for run in &record.runs {
            validate_run(&run.run)?;
            validate_scope(&run.scope)?;
        }
        for owner in &record.owners {
            validate_run(&owner.producer.run)?;
            crate::reducer::recovery_releases(&owner.controls)?;
        }
        if let Some(request) = &record.in_flight {
            match &request.command {
                Command::Activate { run, scope } => {
                    validate_run(run)?;
                    validate_scope(scope)?;
                }
                Command::Arm { producer, .. } => validate_run(&producer.run)?,
                Command::Deliver { delivery } => validate_run(&delivery.producer.run)?,
                Command::Renew { run } | Command::Pause { run } => validate_run(run)?,
                Command::Stop | Command::Recover { .. } => {}
            }
        }
        Ok(record)
    }
    pub fn recover_journal(enabled: bool, qualified: bool, path: PathBuf) -> Result<(), String> {
        Self::recover_exact(enabled, qualified, path, None)
    }
    pub fn recover_owned_journal(
        enabled: bool,
        qualified: bool,
        path: PathBuf,
        owner: &RunToken,
        scope: Option<&Scope>,
    ) -> Result<(), String> {
        Self::recover_exact(enabled, qualified, path, Some((owner, scope)))
    }
    fn recover_exact(
        enabled: bool,
        qualified: bool,
        path: PathBuf,
        owner: Option<(&RunToken, Option<&Scope>)>,
    ) -> Result<(), String> {
        let store = Store::open(&path)?;
        let original = match owner {
            Some((owner, scope)) => Self::inspect_owned_journal(&path, owner, scope)?,
            None => Self::inspect_journal(&path)?,
        };
        let mut controls = original.held.clone();
        controls.extend(original.possible_held.clone());
        store.evidence.verify()?;
        let mut coordinator = Self::spawn(enabled, qualified)?;
        *coordinator.journal_store.borrow_mut() = Some(store);
        coordinator.snapshot = original;
        coordinator.journal = Some(path);
        coordinator.recover(controls)?;
        coordinator.stop_all()
    }
    /// Motion may omit writes, but must still hold the exact retained evidence
    /// authority before native dispatch. Verification never creates a store.
    pub(super) fn verify_current_evidence(&self) -> Result<(), String> {
        let Some(path) = &self.journal else { return Ok(()); };
        let slot = self.journal_store.borrow();
        let store = slot.as_ref().ok_or("Desktop recovery evidence is not acquired.")?;
        if store.path != *path {
            return Err("Desktop recovery storage identity changed.".into());
        }
        store.evidence.verify()
    }
    pub(super) fn persist(&self) -> Result<(), String> {
        let result = self.persist_inner();
        if result.is_err() && self.channel.audit_id().is_some() {
            crate::audit::fault(crate::audit::Fault::Io);
        }
        result
    }
    fn persist_inner(&self) -> Result<(), String> {
        if let Some(path) = &self.journal {
            #[cfg(test)]
            {
                let attempt = self.persist_attempts.get() + 1;
                self.persist_attempts.set(attempt);
                if self.fail_persist_at.get() == Some(attempt) {
                    return Err("Injected desktop journal write failure.".into());
                }
            }
            let mut audit_times = self
                .channel
                .audit_id()
                .map(|channel| (channel, Vec::with_capacity(7)));
            let mark = |times: &mut Option<(u64, Vec<u64>)>| {
                if let Some((_, values)) = times {
                    values.push(crate::audit::stamp().map_or(0, |at| at.qpc));
                }
            };
            mark(&mut audit_times);
            let bytes = serde_json::to_vec(&self.snapshot).map_err(|e| e.to_string())?;
            mark(&mut audit_times);
            if bytes.len() > MAX_JOURNAL_BYTES {
                return Err(
                    "Desktop recovery journal exceeds its 128 KiB bound; output was refused."
                        .into(),
                );
            }
            mark(&mut audit_times);
            let mut slot = self.journal_store.borrow_mut();
            if slot.is_none() {
                *slot = Some(Store::open(path)?);
            }
            let store = slot.as_mut().unwrap();
            if store.path != *path {
                return Err("Desktop recovery storage identity changed.".into());
            }
            mark(&mut audit_times);
            store.evidence.verify()?;
            mark(&mut audit_times);
            store.evidence.write(&bytes)?;
            mark(&mut audit_times);
            mark(&mut audit_times);
            if let Some((channel, times)) = audit_times {
                if times.iter().any(|time| *time == 0)
                    || times.windows(2).any(|pair| pair[0] > pair[1])
                {
                    crate::audit::fault(crate::audit::Fault::Clock);
                } else {
                    let mut fields = vec![channel, self.sequence];
                    fields.extend_from_slice(&times);
                    crate::audit::record_at(
                        crate::audit::Stamp { qpc: times[6] },
                        crate::audit::Kind::JournalPersist,
                        &fields,
                    );
                }
            }
        }
        Ok(())
    }
}
