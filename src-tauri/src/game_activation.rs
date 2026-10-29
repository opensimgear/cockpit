//! Background process identity capture for unified profile selection. No provider
//! setup, traffic, hardware effect, profile selection or UI polling is involved.
use crate::game_discovery::{Platform, ProcessIdentity};
use osg_profiles::activation::{GameIdentity, VerifiedGame};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
    time::{Duration, Instant},
};

const MAX_AGE: Duration = Duration::from_secs(2);
#[derive(Clone)]
struct Proof {
    identity: GameIdentity,
    process: ProcessIdentity,
    session: String,
}
struct Entry {
    pending: Option<(GameIdentity, ProcessIdentity)>,
    confirmed: Option<Proof>,
    status: String,
    message: String,
}
#[derive(Default)]
struct State {
    at: Option<Instant>,
    entries: BTreeMap<String, Entry>,
}
#[derive(Default)]
pub(crate) struct Observer(Mutex<State>);

pub(crate) fn identity(platform: &dyn Platform, game: &Value) -> Result<GameIdentity, String> {
    if game["installed"] != true || game["processMonitoringSupported"] != true {
        return Err("The selected installation has no native process monitoring support.".into());
    }
    let identity = GameIdentity {
        game_id: game["id"]
            .as_str()
            .ok_or("The native game identity is missing.")?
            .into(),
        executable: game["executable"]
            .as_str()
            .ok_or("Choose a validated game executable.")?
            .into(),
    };
    identity.validate()?;
    platform.validate(&identity.game_id, &identity.executable)?;
    Ok(identity)
}
fn process(
    platform: &dyn Platform,
    game: &Value,
) -> Result<(GameIdentity, Option<ProcessIdentity>), String> {
    let identity = identity(platform, game)?;
    let process = platform.find(game)?;
    checked_process(platform, identity, process)
}
fn checked_process(
    platform: &dyn Platform,
    identity: GameIdentity,
    process: Option<ProcessIdentity>,
) -> Result<(GameIdentity, Option<ProcessIdentity>), String> {
    if let Some(process) = &process {
        ProcessIdentity::parse(&process.value())?;
        if !platform.alive(process)? {
            return Ok((identity, None));
        }
    }
    Ok((identity, process))
}
impl Observer {
    /// Live desktop scope selection consumes the background process proof.
    /// Missing/contended evidence waits; it never starts a foreground scan on
    /// the mapping heartbeat. The desktop worker still checks exact process
    /// identity and foreground ownership before each native submission.
    pub(crate) fn try_current_process(&self, id: &str) -> Option<Option<ProcessIdentity>> {
        let state = match self.0.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        if state.at.is_none_or(|at| at.elapsed() > MAX_AGE) { return None; }
        state.entries.get(id).map(|entry| entry.confirmed.as_ref().map(|proof| proof.process.clone()))
    }
    pub(crate) fn observe(&self, platform: &dyn Platform, games: &[Value]) {
        // Capture one process inventory for the whole registered catalogue.
        // Reject duplicates before discovery so one cycle cannot satisfy the
        // consecutive-observation requirement for the same registration.
        let mut seen = BTreeSet::new();
        let mut duplicates = BTreeSet::new();
        for game in games.iter().take(osg_profiles::activation::MAX_GAMES + 1) {
            let id = game["id"].as_str().unwrap_or("").to_owned();
            if !seen.insert(id.clone()) { duplicates.insert(id); }
        }
        let identities = games
            .iter()
            .take(osg_profiles::activation::MAX_GAMES + 1)
            .map(|game| {
                let id = game["id"].as_str().unwrap_or("").to_owned();
                let result = if duplicates.contains(&id) || games.len() > osg_profiles::activation::MAX_GAMES {
                    Err("The native game catalogue is ambiguous or exceeds its bound.".into())
                } else if game["installed"] == false && game["processMonitoringSupported"] == true {
                    // Supported catalogue entries need not all be installed.
                    // Their absence is not a failed Windows process scan.
                    Ok(None)
                } else {
                    identity(platform, game).map(Some)
                };
                (id, result)
            })
            .collect::<Vec<_>>();
        let monitored = games.iter().zip(&identities)
            .filter(|(_, (_, identity))| matches!(identity, Ok(Some(_))))
            .map(|(game, _)| game.clone()).collect::<Vec<_>>();
        let observations = platform.find_many(&monitored);
        let complete = observations.len() == monitored.len();
        let mut observations = observations.into_iter();
        let scans = identities.into_iter().map(|(id, result)| {
            let result = result.and_then(|identity| match identity {
                None => Ok(None),
                Some(identity) if complete => observations.next()
                    .ok_or_else(|| "The native process inventory is incomplete.".to_owned())?
                    .and_then(|process| checked_process(platform, identity, process)).map(Some),
                Some(_) => Err("The native process inventory is incomplete.".into()),
            });
            (id, result)
        }).collect::<Vec<_>>();
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        state.entries.retain(|id, _| seen.contains(id));
        for (id, result) in scans {
            let entry = state.entries.entry(id).or_insert_with(|| Entry {
                pending: None,
                confirmed: None,
                status: "not-running".into(),
                message: String::new(),
            });
            match result {
                Ok(Some((identity, Some(process)))) => {
                    if entry
                        .confirmed
                        .as_ref()
                        .is_some_and(|proof| proof.identity == identity && proof.process == process)
                    {
                        entry.status = "verified".into();
                        entry.message.clear();
                    } else if entry.pending.as_ref() == Some(&(identity.clone(), process.clone())) {
                        entry.confirmed = Some(Proof {
                            identity,
                            process,
                            session: uuid::Uuid::new_v4().to_string(),
                        });
                        entry.pending = None;
                        entry.status = "verified".into();
                        entry.message.clear();
                    } else {
                        entry.confirmed = None;
                        entry.pending = Some((identity, process));
                        entry.status = "settling".into();
                        entry.message = "Waiting for a second verified observation of this exact executable and creation epoch.".into();
                    }
                }
                Ok(Some((_, None))) => {
                    entry.pending = None;
                    entry.confirmed = None;
                    entry.status = "not-running".into();
                    entry.message.clear();
                }
                Ok(None) => {
                    entry.pending = None;
                    entry.confirmed = None;
                    entry.status = "not-installed".into();
                    entry.message =
                        "Choose an installed executable in game details to monitor this game."
                            .into();
                }
                Err(error) => {
                    entry.pending = None;
                    entry.confirmed = None;
                    entry.status = "unavailable".into();
                    entry.message = error;
                }
            }
        }
        state.at = Some(Instant::now());
    }
    pub(crate) fn snapshot(&self) -> Result<Vec<VerifiedGame>, String> {
        let state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.at.is_none_or(|at| at.elapsed() > MAX_AGE) {
            return Err("Native game process observations are stale; automatic activation is waiting for the watcher.".into());
        }
        Ok(state
            .entries
            .values()
            .filter_map(|entry| {
                entry.confirmed.as_ref().map(|proof| VerifiedGame {
                    identity: proof.identity.clone(),
                    process_session: proof.session.clone(),
                })
            })
            .collect())
    }
    /// Matches an already observed native launch without advancing debounce or
    /// substituting another instance of the same executable.
    pub(crate) fn confirmed_process(
        &self,
        identity: &GameIdentity,
        process: &ProcessIdentity,
    ) -> Option<VerifiedGame> {
        let state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.at.is_none_or(|at| at.elapsed() > MAX_AGE) {
            return None;
        }
        state
            .entries
            .get(&identity.game_id)
            .and_then(|entry| entry.confirmed.as_ref())
            .filter(|proof| &proof.identity == identity && &proof.process == process)
            .map(|proof| VerifiedGame {
                identity: proof.identity.clone(),
                process_session: proof.session.clone(),
            })
    }
    pub(crate) fn validate(
        &self,
        platform: &dyn Platform,
        game: &Value,
        expected: &VerifiedGame,
    ) -> Result<(), String> {
        let proof = {
            let state = self.0.lock().unwrap_or_else(|p| p.into_inner());
            if state.at.is_none_or(|at| at.elapsed() > MAX_AGE) {
                return Err("The automatic game observation expired during preparation.".into());
            }
            state
                .entries
                .get(&expected.identity.game_id)
                .and_then(|entry| entry.confirmed.clone())
                .filter(|proof| {
                    proof.identity == expected.identity && proof.session == expected.process_session
                })
                .ok_or("The verified game context changed during profile preparation.")?
        };
        if process(platform, game)? != (proof.identity, Some(proof.process)) {
            return Err("The prepared game exited, changed executable or was replaced; no profile was activated.".into());
        }
        Ok(())
    }
    pub(crate) fn view(&self, id: &str) -> Value {
        let state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let fresh = state.at.is_some_and(|at| at.elapsed() <= MAX_AGE);
        match state.entries.get(id).filter(|_| fresh) {
            Some(entry) => {
                json!({"status":entry.status,"message":entry.message,"processSession":entry.confirmed.as_ref().map(|proof| &proof.session)})
            }
            None => {
                json!({"status":"waiting","message":"Waiting for current native process observations.","processSession":null})
            }
        }
    }
}

#[cfg(test)]
#[path = "game_activation_tests.rs"]
mod tests;
