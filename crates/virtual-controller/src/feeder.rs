//! Feeder ownership and renewable lease. All report transitions are submitted in order.
use crate::vjoy::{Binding, Driver, Status};
use osg_mapping::engine::{Counter, Report, RunToken};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const LEASE_US: u64 = 250_000;
pub const RENEW_US: u64 = 50_000;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Message {
    Start {
        token: RunToken,
        bindings: Vec<Binding>,
    },
    Renew {
        token: RunToken,
        sequence: Counter,
    },
    Report {
        token: RunToken,
        report: Report,
    },
    Stop {
        token: RunToken,
    },
}
impl Message {
    pub fn token(&self) -> &RunToken {
        match self {
            Self::Start { token, .. }
            | Self::Renew { token, .. }
            | Self::Report { token, .. }
            | Self::Stop { token } => token,
        }
    }
}
pub struct Feeder<D: Driver> {
    driver: D,
    token: Option<RunToken>,
    retired: BTreeSet<String>,
    bindings: BTreeMap<String, Binding>,
    deadline: u64,
    sequence: u64,
    renewal: u64,
    fault: Option<String>,
}
impl<D: Driver> Feeder<D> {
    pub fn new(driver: D) -> Self {
        Self {
            driver,
            token: None,
            retired: BTreeSet::new(),
            bindings: BTreeMap::new(),
            deadline: 0,
            sequence: 0,
            renewal: 0,
            fault: None,
        }
    }
    pub fn active(&self) -> bool {
        self.token.is_some()
    }
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }
    fn same(&self, token: &RunToken) -> Result<(), String> {
        if self.token.as_ref() != Some(token) {
            Err("Stale feeder session/run token.".into())
        } else {
            Ok(())
        }
    }
    pub fn receive(&mut self, message: Message, now: u64) -> Result<(), String> {
        self.tick(now)?;
        match message {
            Message::Start { token, bindings } => self.start(token, bindings, now),
            Message::Renew { token, sequence } => {
                self.same(&token)?;
                if sequence.0 == 0 || sequence.0 <= self.renewal {
                    return Err("Stale lease renewal.".into());
                }
                self.renewal = sequence.0;
                self.deadline = now.checked_add(LEASE_US).ok_or("Feeder clock exhausted.")?;
                Ok(())
            }
            Message::Report { token, report } => {
                self.same(&token)?;
                if report.session != token.session
                    || report.run_generation != token.generation
                    || report.sequence.0 == 0
                    || Some(report.sequence.0) != self.sequence.checked_add(1)
                    || report.at_us.0 == 0
                {
                    return Err("Stale or invalid complete virtual report.".into());
                }
                let binding = self
                    .bindings
                    .get(&report.target)
                    .ok_or("Unknown feeder target.")?;
                let position = binding.report(&report.state)?;
                if let Err(error) = self.driver.submit(binding.slot, &position) {
                    self.stop();
                    let failure = self
                        .fault
                        .take()
                        .map_or_else(|| error.clone(), |recovery| format!("{error}; {recovery}"));
                    self.fault = Some(failure.clone());
                    return Err(failure);
                }
                self.sequence = report.sequence.0;
                Ok(())
            }
            Message::Stop { token } => {
                self.same(&token)?;
                self.stop();
                self.fault.clone().map_or(Ok(()), Err)
            }
        }
    }
    fn start(&mut self, token: RunToken, bindings: Vec<Binding>, now: u64) -> Result<(), String> {
        if self.active()
            || token.generation.0 == 0
            || uuid::Uuid::parse_str(&token.session)
                .ok()
                .map(|u| u.to_string())
                .as_deref()
                != Some(&token.session)
            || self.retired.contains(&token.session)
            || self.retired.len() >= 128
            || bindings.is_empty()
            || bindings.len() > 8
        {
            return Err(
                "Feeder start requires a fresh canonical session, 1–8 targets, and no active run."
                    .into(),
            );
        }
        self.driver.check()?;
        let mut targets = BTreeSet::new();
        let mut slots = BTreeSet::new();
        for binding in &bindings {
            binding.validate()?;
            if !targets.insert(&binding.target) || !slots.insert(binding.slot) {
                return Err("Targets and slots must be unique.".into());
            }
            let actual = self.driver.inspect(binding.slot)?;
            binding.match_slot(&actual)?;
            if actual.status != Status::Free {
                return Err(format!(
                    "Slot {} is {:?}; never take over another feeder.",
                    binding.slot, actual.status
                ));
            }
        }
        self.deadline = now.checked_add(LEASE_US).ok_or("Feeder clock exhausted.")?;
        // Record ownership before submission so even a failed first write releases it.
        self.token = Some(token);
        self.sequence = 0;
        self.renewal = 0;
        self.fault = None;
        for binding in bindings {
            let result = (|| {
                // Recheck after preflight: another feeder/configurator may have raced.
                let actual = self.driver.inspect(binding.slot)?;
                binding.match_slot(&actual)?;
                if actual.status != Status::Free {
                    return Err("A target became owned during acquisition.".into());
                }
                self.driver.acquire(binding.slot)?;
                self.bindings
                    .insert(binding.target.clone(), binding.clone());
                self.driver
                    .submit(binding.slot, &binding.report(&binding.fallback())?)
            })();
            if let Err(error) = result {
                self.stop();
                let failure = self
                    .fault
                    .take()
                    .map_or_else(|| error.clone(), |recovery| format!("{error}; {recovery}"));
                self.fault = Some(failure.clone());
                return Err(failure);
            }
        }
        Ok(())
    }
    pub fn tick(&mut self, now: u64) -> Result<(), String> {
        if self.active() && now >= self.deadline {
            self.stop();
            self.fault = Some(match self.fault.take() {
                Some(error) => format!("Mapping heartbeat expired; {error}"),
                None => {
                    "Mapping heartbeat expired; fallback submitted and ownership released.".into()
                }
            });
            return Err(self.fault.clone().unwrap());
        }
        Ok(())
    }
    pub fn stop(&mut self) {
        if let Some(token) = self.token.take() {
            self.retired.insert(token.session);
        }
        for binding in self.bindings.values() {
            if let Ok(position) = binding.report(&binding.fallback()) {
                if let Err(error) = self.driver.submit(binding.slot, &position) {
                    self.fault = Some(format!("Fallback could not be confirmed: {error}"));
                }
            }
            self.driver.release(binding.slot);
        }
        self.bindings.clear();
    }
    /// Supervisor-only recovery of previously recorded targets. No BUSY takeover.
    pub fn recover(&mut self, bindings: &[Binding]) -> Result<(), String> {
        if self.active() || bindings.len() > 8 {
            return Err("Stop mapping before recovery.".into());
        }
        self.driver.check()?;
        let mut errors = Vec::new();
        for binding in bindings {
            let result = (|| {
                let actual = self.driver.inspect(binding.slot)?;
                binding.match_slot(&actual)?;
                if actual.status != Status::Free {
                    return Err(format!(
                        "Slot {} is not FREE; recovery preserved its owner.",
                        binding.slot
                    ));
                }
                self.driver.acquire(binding.slot)?;
                let result = binding
                    .report(&binding.fallback())
                    .and_then(|p| self.driver.submit(binding.slot, &p));
                self.driver.release(binding.slot);
                result
            })();
            if let Err(error) = result {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}
impl<D: Driver> Drop for Feeder<D> {
    fn drop(&mut self) {
        self.stop();
    }
}
#[cfg(test)]
#[path = "feeder_tests.rs"]
mod tests;
