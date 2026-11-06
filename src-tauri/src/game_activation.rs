//! Background process identity capture for unified profile selection. No provider
//! setup, traffic, hardware effect, profile selection or UI polling is involved.
use crate::game_discovery::{Platform, ProcessIdentity};
use osg_profiles::activation::{GameIdentity, VerifiedGame};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{atomic::{AtomicBool, AtomicU8, Ordering},Arc,Mutex,OnceLock},
    time::{Duration, Instant},
};

const MAX_AGE: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct ProcessContinuity(AtomicU8);

impl ProcessContinuity {
    pub(crate) fn begin(&self) -> bool {
        self.0.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }

    pub(crate) fn is_live(&self) -> bool {
        self.0.load(Ordering::Acquire) == 1
    }

    pub(crate) fn retire(&self) {
        self.0.store(2, Ordering::Release);
    }
}
#[derive(Clone,PartialEq,Eq)]
struct Proof {
    identity: GameIdentity,
    process: ProcessIdentity,
    session: String,
}
#[derive(Clone,PartialEq,Eq)]
struct Entry {
    pending: Option<(GameIdentity, ProcessIdentity)>,
    confirmed: Option<Proof>,
    status: String,
    message: String,
}
#[derive(Default)]
struct State {
    at: Option<Instant>,
    tracks_processes: bool,
    continuity: Option<Arc<ProcessContinuity>>,
    entries: BTreeMap<String, Entry>,
}
#[derive(Default)]
pub(crate) struct Observer(Mutex<State>,OnceLock<Arc<crate::game_read_models::signals::Signal>>,AtomicBool);

impl State {
    fn fresh(&self) -> bool {
        self.continuity.as_ref().map_or_else(
            || self.at.is_some_and(|at| !self.tracks_processes || at.elapsed() <= MAX_AGE),
            |live| live.is_live(),
        )
    }

    fn exact_proof(&self, identity: &GameIdentity, process: &ProcessIdentity) -> Option<&Proof> {
        if !self.fresh() {
            return None;
        }
        self.entries
            .get(&identity.game_id)
            .and_then(|entry| entry.confirmed.as_ref())
            .filter(|proof| &proof.identity == identity && &proof.process == process)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CurrentProcessObservation {
    /// The background observer is publishing a result. Keep the last accepted
    /// scope and retry on the next health tick without treating contention as
    /// evidence that the process disappeared.
    Busy,
    /// No current observation is available, it is stale, or this game has no
    /// entry. Callers must continue to treat these cases as unavailable.
    Unavailable,
    /// A fresh observer cycle completed. The inner option is the confirmed
    /// process; `None` is fresh evidence that no matching process is running.
    Observed(Option<ProcessIdentity>),
}

pub(crate) fn process_candidate(game: &Value) -> bool {
    game["installed"] == true && game["processMonitoringSupported"] == true
}

pub(crate) fn identity(platform: &dyn Platform, game: &Value) -> Result<GameIdentity, String> {
    if !process_candidate(game) {
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
    pub(crate) fn watch_changes(&self,changes:Arc<crate::game_read_models::signals::Signal>) {let _=self.1.set(changes);}
    pub(crate) fn worker_live(&self) -> bool { self.2.load(Ordering::Acquire) }
    pub(crate) fn worker_scope(&self) -> impl Drop + '_ {
        struct Running<'a>(&'a Observer);
        impl Drop for Running<'_> {
            fn drop(&mut self) { self.0.set_worker_live(false); }
        }
        self.set_worker_live(true);
        Running(self)
    }
    fn set_worker_live(&self, live: bool) {
        self.2.store(live, Ordering::Release);
        if let Some(signal) = self.1.get() { signal.wake_launch(); }
    }
    pub(crate) fn view_deadline(&self)->Option<Instant> {
        let state = self.0.lock().unwrap_or_else(|p|p.into_inner());
        if state.continuity.is_some() { return None; }
        state.at.and_then(|at|at.checked_add(MAX_AGE)).filter(|at|*at>Instant::now())
    }
    /// Live desktop scope selection consumes the background process proof.
    /// Missing/contended evidence waits; it never starts a foreground scan on
    /// the mapping heartbeat. The desktop worker still checks exact process
    /// identity and foreground ownership before each native submission.
    pub(crate) fn try_current_process(&self, id: &str) -> CurrentProcessObservation {
        let state = match self.0.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return CurrentProcessObservation::Busy,
        };
        if !state.fresh() {
            return CurrentProcessObservation::Unavailable;
        }
        state.entries.get(id).map_or(
            CurrentProcessObservation::Unavailable,
            |entry| CurrentProcessObservation::Observed(
                entry.confirmed.as_ref().map(|proof| proof.process.clone()),
            ),
        )
    }
    #[cfg(test)]
    pub(crate) fn observe(&self, platform: &dyn Platform, games: &[Value]) {
        self.observe_with_continuity(platform, games, None);
    }

    pub(crate) fn observe_with_continuity(
        &self,
        platform: &dyn Platform,
        games: &[Value],
        continuity: Option<Arc<ProcessContinuity>>,
    ) {
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
        let previous=state.entries.clone();
        let was_fresh = state.fresh();
        let same_source = match (&state.continuity, &continuity) {
            (None, None) => true,
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            _ => false,
        };
        if !same_source { state.entries.clear(); }
        let event_owned = continuity.is_some();
        state.continuity = continuity;
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
                    } else if event_owned || entry.pending.as_ref() == Some(&(identity.clone(), process.clone())) {
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
        state.tracks_processes = games.iter().any(process_candidate);
        state.at = Some(Instant::now());
        let changed=state.entries!=previous || state.fresh()!=was_fresh;drop(state);
        if let Some(signal)=self.1.get() {
            if changed {signal.mark_process_activation();} else {signal.wake.notify();}
        }
    }
    pub(crate) fn snapshot(&self) -> Result<Vec<VerifiedGame>, String> {
        let state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if !state.fresh() {
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

    pub(crate) fn confirmed_process(
        &self,
        identity: &GameIdentity,
        process: &ProcessIdentity,
    ) -> Option<VerifiedGame> {
        let state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        state
            .exact_proof(identity, process)
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
            if !state.fresh() {
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
        let fresh = state.fresh();
        match state.entries.get(id).filter(|_| fresh) {
            Some(entry) => {
                json!({"status":entry.status,"message":entry.message,"processSession":entry.confirmed.as_ref().map(|proof| &proof.session)})
            }
            None => {
                json!({"status":"waiting","message":"Waiting for current native process observations.","processSession":null})
            }
        }
    }
    pub(crate) fn view_for(&self,game:&Value)->Value {
        let id=game["id"].as_str().unwrap_or("");
        let state=self.0.lock().unwrap_or_else(|p|p.into_inner());
        let known=state.entries.get(id).and_then(|entry|entry.confirmed.as_ref().map(|proof|&proof.identity)
            .or_else(||entry.pending.as_ref().map(|pending|&pending.0)));
        let matches=known.is_none_or(|known|game["executable"].as_str().is_some_and(|path|
            crate::game_discovery::normalized_path(path)==crate::game_discovery::normalized_path(&known.executable)));
        drop(state);
        if matches {self.view(id)} else {json!({"status":"waiting","message":"Waiting for observations of this installation.","processSession":null})}
    }
}

#[cfg(test)]
#[path = "game_activation_tests.rs"]
mod tests;
