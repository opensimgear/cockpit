//! Project shared process inventory through independent integration workers.
use super::*;
use crate::game_discovery::{ProcessLookup, ProcessLookupError};
use crate::game_vehicles::CaptureWakes;
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy)]
struct Scope {
    process: (u64, u64),
    setup: u64,
    watched: bool,
}

impl Scope {
    fn current(self, inner: &Inner) -> bool {
        !inner.closed.load(Ordering::Acquire)
            && self.process == inner.context_epochs()
            && (!self.watched || inner.current_process_epoch() == Some(self.process))
            && self.setup == inner.changes.vehicle_scope_wake.observed()
    }
}

struct Work {
    game: Value,
    epoch: u64,
    supported: bool,
    eligible: Result<bool, String>,
    process: ProcessLookup,
}

#[derive(Default)]
pub(super) struct Workers {
    wakes: CaptureWakes,
    pending: Mutex<BTreeMap<String, VecDeque<(Scope, Work)>>>,
}

impl Workers {
    pub(super) fn start(self: &Arc<Self>, inner: &Arc<Inner>) -> Vec<thread::JoinHandle<()>> {
        self.wakes
            .iter()
            .map(|(integration, wake)| {
                let workers = self.clone();
                let inner = inner.clone();
                let wake = wake.clone();
                thread::spawn(move || {
                    let _ = wake.bind_thread(thread::current());
                    while !inner.closed.load(Ordering::Acquire) {
                        let observed = wake.observed();
                        let job = workers
                            .pending
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .get_mut(integration)
                            .and_then(VecDeque::pop_front);
                        if let Some((scope, work)) = job {
                            inner.project_context(work, Some((&workers, scope)));
                        } else {
                            wake.wait_scheduled(observed, None);
                        }
                    }
                })
            })
            .collect()
    }

    fn submit(&self, scope: Scope, work: Vec<Work>) {
        let mut next = BTreeMap::<String, VecDeque<_>>::new();
        for job in work {
            if let Some(integration) = crate::game_discovery::integration_key(&job.game) {
                next.entry(integration.into())
                    .or_default()
                    .push_back((scope, job));
            }
        }
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        *pending = next;
        for (integration, wake) in self.wakes.iter() {
            if pending.contains_key(integration) {
                wake.notify();
            }
        }
    }

    fn superseded(&self, integration: &str, id: &str, epoch: u64, process: &ProcessLookup) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(integration)
            .is_some_and(|jobs| {
                jobs.iter().any(|(_, job)| {
                    job.epoch > epoch && job.game["id"] == id && &job.process != process
                })
            })
    }

    pub(super) fn stop(&self) {
        self.wakes.notify_all();
    }
}

impl Inner {
    pub(super) fn context_epochs(&self) -> (u64, u64) {
        (
            self.changes.context_wake.observed(),
            self.changes.context_process_wake.observed(),
        )
    }

    pub(super) fn refresh_game_contexts_with_inventory(
        &self,
        setup: SetupRefresh,
        inventory: impl FnOnce(&[&Value]) -> Vec<ProcessLookup>,
    ) -> bool {
        self.collect_game_contexts(setup, inventory, None)
    }

    pub(super) fn collect_game_contexts(
        &self,
        setup: SetupRefresh,
        inventory: impl FnOnce(&[&Value]) -> Vec<ProcessLookup>,
        background: Option<(&Workers, (u64, u64))>,
    ) -> bool {
        let epoch = match self
            .contexts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .begin_observation()
        {
            Ok(epoch) => epoch,
            Err(_) => return false,
        };
        let installation_metadata = self.guard_installation_metadata();
        let mut games: Vec<_> = self
            .settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .catalogue
            .iter()
            .filter(|game| crate::game_discovery::integration_key(game).is_some())
            .cloned()
            .collect();
        {
            let mut contexts = self.contexts.lock().unwrap_or_else(|p| p.into_inner());
            if background.is_some_and(|(_, process)| process != self.context_epochs()) {
                return false;
            }
            let bound_ids = games
                .iter()
                .filter_map(|game| game["id"].as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            contexts.retain_bindings(&bound_ids);
        }
        match setup {
            SetupRefresh::Cached => {}
            SetupRefresh::Explicit => self.setup.refresh(&games),
            SetupRefresh::Library => {
                // Explicit preparation owns its setup admission.
                if !self.setup.refresh_background(&games) {
                    return false;
                }
            }
        }
        let setup_epoch = self.changes.vehicle_scope_wake.observed();
        self.setup.decorate_source(&mut games);
        let admission: Vec<_> = games
            .iter()
            .map(|game| {
                let supported = self.platform.integration_setup_supported()
                    && game["processMonitoringSupported"]
                        .as_bool()
                        .unwrap_or(game["launchSupported"] == true);
                let eligible = installation_metadata.clone().and_then(|()| {
                    if !supported || game["installed"] != true {
                        return Ok(false);
                    }
                    if background.is_none() {
                        self.validate_context_installation(game)?;
                    }
                    Ok(true)
                });
                (supported, eligible)
            })
            .collect();
        let selected: Vec<_> = games
            .iter()
            .zip(&admission)
            .filter_map(|(game, (_, eligible))| matches!(eligible, Ok(true)).then_some(game))
            .collect();
        let watched =
            background.is_some_and(|(_, process)| self.current_process_epoch() == Some(process));
        let mut processes = inventory(&selected).into_iter();
        let work = games
            .into_iter()
            .zip(admission)
            .map(|(game, (supported, eligible))| {
                let process = if matches!(eligible, Ok(true)) {
                    processes.next().unwrap_or_else(|| {
                        Err(ProcessLookupError::Unavailable(
                            "Game process inventory was incomplete.".into(),
                        ))
                    })
                } else {
                    Ok(None)
                };
                Work {
                    game,
                    epoch,
                    supported,
                    eligible,
                    process,
                }
            });
        if let Some((workers, process)) = background {
            let scope = Scope {
                process,
                setup: setup_epoch,
                watched,
            };
            if !scope.current(self) {
                return false;
            }
            workers.submit(scope, work.collect());
        } else {
            for row in work {
                self.project_context(row, None);
            }
        }
        true
    }

    fn validate_context_installation(&self, game: &Value) -> Result<(), String> {
        self.platform.validate(
            game["id"].as_str().ok_or("Choose a game identity.")?,
            game["executable"]
                .as_str()
                .ok_or("Choose a game executable before process verification.")?,
        )
    }

    fn project_context(&self, work: Work, background: Option<(&Workers, Scope)>) {
        let Work {
            game,
            epoch,
            supported,
            eligible,
            process: captured,
        } = work;
        let (Some(id), Some(integration)) = (
            game["id"].as_str(),
            crate::game_discovery::integration_key(&game),
        ) else {
            return;
        };
        let current = || {
            background.is_none_or(|(workers, scope)| {
                scope.current(self) && !workers.superseded(integration, id, epoch, &captured)
            })
        };
        if !current() {
            return;
        }
        let eligible = eligible.and_then(|eligible| {
            if eligible && background.is_some() {
                self.validate_context_installation(&game)?;
            }
            Ok(eligible)
        });
        let eligible_for_retention = matches!(eligible, Ok(true));
        let process =
            eligible
                .map_err(ProcessLookupError::Unavailable)
                .and_then(|_| match captured.clone()? {
                    Some(process)
                        if self
                            .platform
                            .alive(&process)
                            .map_err(ProcessLookupError::Unavailable)? =>
                    {
                        Ok(Some(process))
                    }
                    _ => Ok(None),
                });
        if !current() {
            return;
        }
        let revision = self.setup.provider_setup_revision(integration);
        let setup_matches =
            game["integrationSetup"]["providerSetupRevision"].as_str() == Some(revision.as_str());
        let ready = setup_matches && game["integrationSetup"]["state"] == "ready";
        if let Ok((connection, connection_revision, _)) =
            self.setup.provider_connection_snapshot(integration)
        {
            if !current() {
                return;
            }
            let work = self.update_vehicles(|vehicles| {
                if !current() {
                    return None;
                }
                vehicles
                    .begin_inventory_observation(
                        &game,
                        &connection,
                        process.clone(),
                        &revision,
                        supported && ready && connection_revision == revision,
                        epoch,
                    )
                    .and_then(|work| vehicles.defer_open(work))
            });
            if let Some(work) = work {
                vehicle_openers::finish_inline(self, integration, work);
            }
        }
        let vehicle_id = self.runtime_vehicle_id(id);
        let catalogue = self
            .setup
            .integration_catalogue_metadata_for_vehicle(integration, vehicle_id.as_deref());
        if !current() || (background.is_some() && vehicle_id != self.runtime_vehicle_id(id)) {
            return;
        }
        let lookup_unavailable =
            eligible_for_retention && matches!(process, Err(ProcessLookupError::Unavailable(_)));
        let error = process.as_ref().err().map(ToString::to_string);
        let observation = Observation {
            installation: preparation_installation(&game),
            process: process.unwrap_or(None),
            supported,
            setup_ready: ready
                && (!integration.starts_with("assetto-corsa")
                    || game["integrationSetup"]["verified"] == true),
            provider_setup_revision: revision,
            catalogue_revision: catalogue
                .ok()
                .and_then(|value| value["catalogueRevision"].as_str().map(str::to_owned)),
            vehicle_id,
            error,
        };
        let mut contexts = self.contexts.lock().unwrap_or_else(|p| p.into_inner());
        if current() {
            let _ = contexts.observe(id, epoch, observation, lookup_unavailable);
        }
    }
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
