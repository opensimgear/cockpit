//! Read-only discovery work never retains the vehicle-state mutex across I/O.
use super::*;
pub(crate) struct VehicleObservation {
    id: String,
    epoch: u64,
    generation: u64,
    signature: String,
    integration: String,
    connection: Value,
    process: Option<ProcessIdentity>,
    inspection: Vec<String>,
    reader: Option<Result<Reader, String>>,
    prepared_watch: Option<Result<CaptureBinding, String>>,
    installation: String,
    setup_revision: String,
    identity: Option<ProcessIdentity>,
    verified: Option<bool>,
}
impl VehicleObservation {
    #[cfg(test)]
    pub(crate) fn test_opening_pid(&self) -> u32 {
        self.process.as_ref().unwrap().pid
    }
    fn is_opening(&self) -> bool {
        self.process.is_some()
    }
    pub(crate) fn can_open(&self) -> bool {
        self.verified == Some(true)
    }
    pub(crate) fn prepare_watch(mut self, wake: &Arc<osg_streams::NativeWake>) -> Self {
        if let Some(Ok(reader)) = &self.reader {
            self.prepared_watch = Some(CaptureBinding::require_live(reader, wake));
        }
        self
    }
    pub(crate) fn read(self) -> Self {
        self.read_with(Reader::open)
    }
    fn read_with(
        mut self,
        open: impl FnOnce(&str, &Value, &ProcessIdentity) -> Result<Reader, String>,
    ) -> Self {
        if let Some(process) = &self.process {
            if self.integration.starts_with("x-plane-") {
                self.connection["providerSetupRevision"] = json!(self.setup_revision);
            }
            self.reader = Some(open(&self.integration, &self.connection, process).map(
                |mut reader| {
                    let _ = reader.inspect(&self.integration, &self.inspection);
                    reader
                },
            ));
        }
        self
    }
    pub(crate) fn verify_ready(
        self,
        games: &[Value],
        revision: impl FnOnce(&Value) -> String,
        alive: impl FnMut(&ProcessIdentity) -> Result<bool, String>,
    ) -> Self {
        let current = games.iter().find(|game| game["id"] == self.id);
        let revision = current.map_or_else(String::new, revision);
        self.verify(games, &revision, alive)
    }

    pub(crate) fn verify(
        mut self,
        games: &[Value],
        revision: &str,
        mut alive: impl FnMut(&ProcessIdentity) -> Result<bool, String>,
    ) -> Self {
        self.verified = if self.setup_revision == revision && games.iter().any(|game| {
                game["id"] == self.id && Tracker::installation(game) == self.installation
            }) {
            self.identity.as_ref().map_or(Ok(true), &mut alive).ok()
        } else {
            Some(false)
        };
        self
    }
}
impl Tracker {
    pub(crate) fn watch_openers(&mut self, wakes: Arc<CaptureWakes>) {
        self.open_wakes = Some(wakes);
    }
    pub(crate) fn stop_openers(&mut self) {
        self.open_wakes = None;
        for entry in self.entries.values_mut() {
            entry.queued_open = None;
            entry.opening = None;
        }
    }
    pub(crate) fn defer_open(&mut self, observation: VehicleObservation) -> Option<VehicleObservation> {
        let Some(wake) = self.open_wakes.as_ref().and_then(|wakes| wakes.get(&observation.integration))
            .filter(|_| observation.is_opening()) else { return Some(observation); };
        let entry = self.entries.get_mut(&observation.id).expect("Observation must own a tracker entry");
        entry.queued_open = Some(Box::new(observation));
        wake.notify();
        None
    }
    pub(crate) fn take_open(&mut self, integration: &str) -> Option<VehicleObservation> {
        self.entries.values_mut().filter(|entry| entry.integration == integration)
            .find_map(|entry| entry.queued_open.take().map(|work| *work))
    }

    #[cfg(test)]
    pub(crate) fn observe(
        &mut self,
        game: &Value,
        connection: &Value,
        process: Option<ProcessIdentity>,
        setup_revision: &str,
        enabled: bool,
        epoch: u64,
    ) {
        let work =
            self.begin_observation(game, connection, process, setup_revision, enabled, epoch);
        drop(self.take_retired());
        if let Some(work) = work {
            self.apply_observation(work.read());
        }
        drop(self.take_retired());
    }
    pub(crate) fn begin_inventory_observation(
        &mut self,
        game: &Value,
        connection: &Value,
        process: crate::game_discovery::ProcessLookup,
        setup_revision: &str,
        enabled: bool,
        observation_epoch: u64,
    ) -> Option<VehicleObservation> {
        let integration = crate::game_discovery::integration_key(game).unwrap_or("");
        let enabled = enabled && !integration.is_empty();
        if matches!(process, Err(crate::game_discovery::ProcessLookupError::Unavailable(_)))
            && game["id"].as_str().and_then(|id| self.entries.get(id)).is_some_and(|entry| {
            entry.enabled == enabled && entry.integration == integration
                && entry.setup_revision == setup_revision && Self::installation_matches(&entry.installation_fields, game)
        }) {
            // An unavailable lookup cannot revoke or renew existing proof.
            return None;
        }
        self.begin_observation(game, connection, process.unwrap_or(None), setup_revision, enabled, observation_epoch)
    }
    pub(crate) fn begin_observation(
        &mut self,
        game: &Value,
        connection: &Value,
        process: Option<ProcessIdentity>,
        setup_revision: &str,
        enabled: bool,
        observation_epoch: u64,
    ) -> Option<VehicleObservation> {
        let id = game["id"].as_str().unwrap_or("");
        if !crate::game_discovery::valid_id(id)
            || (!self.entries.contains_key(id) && self.entries.len() >= 512)
        {
            return None;
        }
        let integration = crate::game_discovery::integration_key(game).unwrap_or("");
        let enabled = enabled && !integration.is_empty();
        let now = Instant::now();
        let signature = json!([
            process.as_ref().map(ProcessIdentity::value),
            setup_revision,
            Self::installation(game),
            enabled
        ])
        .to_string();
        let entry = self.entries.entry(id.into()).or_insert_with(|| Entry {
            #[cfg(test)]
            injected: false,
            signature: String::new(),
            integration: String::new(),
            installation: String::new(),
            installation_fields: Self::installation_fields(game),
            setup_revision: String::new(),
            enabled: false,
            process: None,
            reader: None,
            current: None,
            telemetry: None,
            inspection: inspection::Demand::default(),
            error: None,
            retry: now,
            observed: now,
            observation_epoch: 0,
            generation: 0,
            polling: false,
            opening: None,
            queued_open: None,
            detached_physics: None,
            capture_watch: None,
            inventory_revision: 0,
            published: Visible::default(),
            published_capture: None,
        });
        if observation_epoch <= entry.observation_epoch {
            return None;
        }
        entry.observation_epoch = observation_epoch;
        if entry.signature != signature {
            entry.generation = entry.generation.wrapping_add(1);
            entry.polling = false;
            entry.opening = None;
            entry.queued_open = None;
            entry.detached_physics = None;
            if entry.installation != Self::installation(game) || entry.setup_revision != setup_revision {
                entry.inspection = inspection::Demand::default();
            }
            entry.integration = integration.into();
            entry.capture_watch = None;
            self.retired.extend(entry.reader.take());
            entry.current = None;
            entry.telemetry = None;
            entry.error = None;
            entry.process = process;
            entry.installation = Self::installation(game);
            entry.installation_fields = Self::installation_fields(game);
            entry.setup_revision = setup_revision.into();
            entry.enabled = enabled;
            entry.signature = signature;
            entry.retry = now;
        }
        entry.observed = now;
        let process = (enabled
            && entry.reader.is_none()
            && !entry.polling
            && entry.opening.is_none()
            && now >= entry.retry)
            .then(|| entry.process.clone())
            .flatten();
        if process.is_some() {
            entry.opening = Some(observation_epoch);
        }
        let observation = VehicleObservation {
            id: id.into(),
            epoch: observation_epoch,
            generation: entry.generation,
            signature: entry.signature.clone(),
            integration: integration.into(),
            connection: connection.clone(),
            process,
            inspection: entry.inspection.selection(now).to_vec(),
            reader: None,
            prepared_watch: None,
            installation: entry.installation.clone(),
            setup_revision: entry.setup_revision.clone(),
            identity: entry.process.clone(),
            verified: Some(true),
        };
        self.publish_visible(id);
        Some(observation)
    }
    fn wake_open_retry(&self) {
        if let Some(changes) = &self.changes { changes.context_update_wake.notify(); }
    }
    pub(crate) fn apply_observation(&mut self, observation: VehicleObservation) {
        let Some(entry) = self.entries.get_mut(&observation.id) else {
            self.retired.extend(observation.reader.and_then(Result::ok));
            self.publish_visible(&observation.id);
            return;
        };
        let owns_opening = entry.opening == Some(observation.epoch);
        if owns_opening {
            entry.opening = None;
        }
        if (entry.observation_epoch != observation.epoch && !owns_opening)
            || entry.generation != observation.generation
            || entry.signature != observation.signature
        {
            self.retired.extend(observation.reader.and_then(Result::ok));
            self.publish_visible(&observation.id);
            return;
        }
        if observation.verified == Some(false) {
            entry.queued_open = None;
            entry.generation = entry.generation.wrapping_add(1);
            entry.polling = false;
            entry.opening = None;
            entry.detached_physics = None;
            entry.capture_watch = None;
            self.retired.extend(entry.reader.take());
            entry.current = None;
            entry.telemetry = None;
            entry.retry = Instant::now() + Duration::from_secs(2);
            if owns_opening { self.wake_open_retry(); }
            self.retired.extend(observation.reader.and_then(Result::ok));
            self.publish_visible(&observation.id);
            return;
        }
        if observation.verified.is_none() || entry.observed.elapsed() > Duration::from_secs(2) {
            // Late process work cannot revoke an admitted reader or make its
            // proof current. Only an unadmitted opener must retry.
            if owns_opening {
                entry.retry = Instant::now() + Duration::from_secs(2);
                self.wake_open_retry();
            }
            self.retired.extend(observation.reader.and_then(Result::ok));
            self.publish_visible(&observation.id);
            return;
        }
        if let Some(reader) = observation.reader {
            let wake = self.capture_wakes.as_ref().and_then(|wakes| wakes.get(&entry.integration));
            let admitted = reader.and_then(|reader| {
                let watch = match observation.prepared_watch {
                    Some(watch) => watch.map(Some),
                    None => wake.map(|wake| CaptureBinding::require_live(&reader, wake)).transpose(),
                };
                match watch {
                    Ok(watch) => Ok((reader, watch)),
                    Err(error) => {
                        self.retired.push(reader);
                        Err(error)
                    }
                }
            });
            match admitted {
                Ok((reader, watch)) => {
                    entry.capture_watch = watch;
                    self.retired.extend(entry.reader.replace(reader));
                    entry.error = None;
                    if let Some(wake) = wake { wake.notify(); }
                }
                Err(error) => {
                    entry.error = Some(error);
                    entry.retry = Instant::now() + Duration::from_secs(2);
                    if let Some(changes) = &self.changes { changes.context_update_wake.notify(); }
                }
            }
        }
        self.publish_visible(&observation.id);
    }
}

#[cfg(test)]
#[path = "observation_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "observation_stream_tests.rs"]
mod stream_tests;
