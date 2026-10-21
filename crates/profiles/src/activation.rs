//! Deterministic game-start selection. Verified process observations are native
//! inputs; this policy never discovers a process, starts a provider or applies a
//! graph. A decision is an intent which still needs normal native admission.
use crate::{session::SelectionMode, ProfileDocument, DEFAULT_PROFILE_ID};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SUPPORTED_GAME_IDS: [&str; 7] = ["dcs", "assetto-corsa-rally", "x-plane-11", "x-plane-12", "assetto-corsa", "assetto-corsa-competizione", "assetto-corsa-evo"];
pub const MAX_RULES: usize = SUPPORTED_GAME_IDS.len();

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameIdentity {
    pub game_id: String,
    /// Stable validated installation executable, never a PID or launch argument.
    pub executable: String,
}
impl GameIdentity {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(
            self.game_id.as_str(),
            "dcs" | "assetto-corsa-rally" | "x-plane-11" | "x-plane-12" | "assetto-corsa" | "assetto-corsa-competizione" | "assetto-corsa-evo"
        ) || self.executable.is_empty()
            || self.executable.len() > 32768
            || self.executable.chars().any(char::is_control)
            || !((self.executable.as_bytes().get(1) == Some(&b':')
                && matches!(self.executable.as_bytes().get(2), Some(b'\\' | b'/')))
                || self.executable.starts_with("\\\\"))
        {
            return Err(
                "A game rule needs a supported game and an absolute native executable identity."
                    .into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rule {
    pub identity: GameIdentity,
    pub profile_id: String,
    pub priority: i16,
    pub enabled: bool,
}
pub fn validate_rules(rules: &[Rule]) -> Result<(), String> {
    if rules.len() > MAX_RULES {
        return Err("Choose at most one activation rule for each supported game.".into());
    }
    let mut games = BTreeSet::new();
    for rule in rules {
        rule.identity.validate()?;
        if !games.insert(&rule.identity.game_id)
            || !uuid::Uuid::parse_str(&rule.profile_id)
                .is_ok_and(|id| id.to_string() == rule.profile_id)
        {
            return Err(
                "Activation rules need unique games and canonical saved profile identities.".into(),
            );
        }
    }
    Ok(())
}

/// Constructed only after the native watcher verifies executable, creation epoch
/// and current process liveness. Not an IPC request or persisted rule field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedGame {
    pub identity: GameIdentity,
    pub process_session: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub profile_id: String,
    pub game_id: Option<String>,
    pub process_session: Option<String>,
}
impl Default for Target {
    fn default() -> Self {
        Self {
            profile_id: DEFAULT_PROFILE_ID.into(),
            game_id: None,
            process_session: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Hold(String),
    Conflict(Vec<String>),
    Select(Target),
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Locks {
    pub emergency_stopped: bool,
    pub recovery_needed: bool,
    pub faulted: bool,
}

pub fn select(
    mode: SelectionMode,
    locks: Locks,
    rules: &[Rule],
    observed: &[VerifiedGame],
    profiles: &[ProfileDocument],
) -> Result<Decision, String> {
    validate_rules(rules)?;
    if observed.len() > MAX_RULES {
        return Err("The verified game observation exceeds its bound.".into());
    }
    let mut games = BTreeSet::new();
    for game in observed {
        game.identity.validate()?;
        if !games.insert(&game.identity.game_id)
            || game.process_session.is_empty()
            || game.process_session.len() > 256
            || game.process_session.chars().any(char::is_control)
        {
            return Err(
                "Verified game contexts must be unique and have bounded native process sessions."
                    .into(),
            );
        }
    }
    if locks.recovery_needed || locks.faulted {
        return Ok(Decision::Hold(
            "Explicit native recovery is required before automatic activation.".into(),
        ));
    }
    if locks.emergency_stopped {
        return Ok(Decision::Hold(
            "Stop prevents automatic activation.".into(),
        ));
    }
    if mode != SelectionMode::Automatic {
        return Ok(Decision::Hold(
            "Manual profile selection stays pinned until Automatic is selected.".into(),
        ));
    }
    let mut matches = rules
        .iter()
        .filter_map(|rule| {
            rule.enabled
                .then(|| {
                    observed
                        .iter()
                        .find(|game| game.identity == rule.identity)
                        .map(|game| (rule, game))
                })
                .flatten()
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Ok(Decision::Select(Target::default()));
    }
    matches.sort_by(|(left, _), (right, _)| {
        right
            .priority
            .cmp(&left.priority)
            .then(left.identity.game_id.cmp(&right.identity.game_id))
    });
    let top = matches[0].0.priority;
    let highest = matches
        .iter()
        .take_while(|(rule, _)| rule.priority == top)
        .collect::<Vec<_>>();
    if highest.len() != 1 {
        return Ok(Decision::Conflict(
            highest
                .iter()
                .map(|(rule, _)| rule.identity.game_id.clone())
                .collect(),
        ));
    }
    let (rule, game) = highest[0];
    let Some(profile) = profiles
        .iter()
        .find(|profile| profile.id == rule.profile_id)
    else {
        return Ok(Decision::Hold(format!(
            "The activation rule for {} references a removed profile.",
            rule.identity.game_id
        )));
    };
    if !profile.eligibility.game_ids.is_empty()
        && !profile
            .eligibility
            .game_ids
            .contains(&rule.identity.game_id)
    {
        return Ok(Decision::Hold(format!(
            "The selected profile is not eligible for {}.",
            rule.identity.game_id
        )));
    }
    Ok(Decision::Select(Target {
        profile_id: rule.profile_id.clone(),
        game_id: Some(rule.identity.game_id.clone()),
        process_session: Some(game.process_session.clone()),
    }))
}

/// Attempts are idempotent even after a failed admission. Polling the same
/// context never retries effects; a new verified session, rule/operator epoch or
/// explicit retry is required. Saving a draft does not silently reapply it.
#[derive(Default)]
pub struct Reconciler {
    attempted: Option<(Target, u64)>,
}
impl Reconciler {
    pub fn next(&mut self, decision: &Decision, epoch: u64) -> Option<Target> {
        let Decision::Select(target) = decision else {
            return None;
        };
        if self
            .attempted
            .as_ref()
            .is_some_and(|(previous, at)| previous == target && *at == epoch)
        {
            return None;
        }
        self.attempted = Some((target.clone(), epoch));
        Some(target.clone())
    }
}

#[cfg(test)]
#[path = "activation_tests.rs"]
mod tests;
