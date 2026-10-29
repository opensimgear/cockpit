//! Read-only discovery work never retains the vehicle-state mutex across I/O.
use super::*;
pub(crate) struct VehicleObservation {
    id: String,
    epoch: u64,
    generation: u64,
    signature: String,
    integration: String,
    game: Value,
    connection: Value,
    catalogue: Option<Vec<Value>>,
    process: Option<ProcessIdentity>,
    inspection: Vec<String>,
    reader: Option<Result<Reader, String>>,
    installation: String,
    setup_revision: String,
    identity: Option<ProcessIdentity>,
    verified: bool,
}
impl VehicleObservation {
    pub(crate) fn read(self) -> Self {
        self.read_with(Reader::open)
    }
    fn read_with(
        mut self,
        open: impl FnOnce(&str, &Value, &ProcessIdentity) -> Result<Reader, String>,
    ) -> Self {
        if self.catalogue.is_some() {
            self.catalogue = Some(match self.integration.as_str() {
                "dcs" => dcs_catalogue(&self.connection).unwrap_or_default(),
                "x-plane-11" | "x-plane-12" => xplane_catalogue(&self.game),
                _ => vec![],
            });
        }
        if let Some(process) = &self.process {
            self.reader = Some(open(&self.integration, &self.connection, process).map(
                |mut reader| {
                    let _ = reader.inspect(&self.integration, &self.inspection);
                    reader
                },
            ));
        }
        self
    }
    pub(crate) fn verify(
        mut self,
        games: &[Value],
        revision: &str,
        mut alive: impl FnMut(&ProcessIdentity) -> bool,
    ) -> Self {
        self.verified = self.setup_revision == revision
            && games.iter().any(|game| {
                game["id"] == self.id && Tracker::installation(game) == self.installation
            })
            && self.identity.as_ref().is_none_or(&mut alive);
        self
    }
}
impl Tracker {
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
            setup_revision: String::new(),
            enabled: false,
            process: None,
            reader: None,
            current: None,
            telemetry: None,
            inspection: vec![],
            inspected: now,
            catalogued: now - Duration::from_secs(31),
            vehicles: vec![],
            error: None,
            retry: now,
            observed: now,
            observation_epoch: 0,
            generation: 0,
            polling: false,
            opening: None,
            detached_physics: None,
        });
        if observation_epoch <= entry.observation_epoch {
            return None;
        }
        entry.observation_epoch = observation_epoch;
        if entry.signature != signature {
            entry.generation = entry.generation.wrapping_add(1);
            entry.polling = false;
            entry.opening = None;
            entry.detached_physics = None;
            entry.integration = integration.into();
            self.retired.extend(entry.reader.take());
            entry.current = None;
            entry.telemetry = None;
            entry.error = None;
            entry.process = process;
            entry.installation = Self::installation(game);
            entry.setup_revision = setup_revision.into();
            entry.enabled = enabled;
            entry.signature = signature;
            entry.retry = now;
            entry.catalogued = now - Duration::from_secs(31);
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
        Some(VehicleObservation {
            id: id.into(),
            epoch: observation_epoch,
            generation: entry.generation,
            signature: entry.signature.clone(),
            integration: integration.into(),
            game: game.clone(),
            connection: connection.clone(),
            catalogue: (entry.catalogued.elapsed() >= Duration::from_secs(30)).then_some(vec![]),
            process,
            inspection: entry.inspection.clone(),
            reader: None,
            installation: entry.installation.clone(),
            setup_revision: entry.setup_revision.clone(),
            identity: entry.process.clone(),
            verified: true,
        })
    }
    pub(crate) fn apply_observation(&mut self, observation: VehicleObservation) {
        let Some(entry) = self.entries.get_mut(&observation.id) else {
            self.retired.extend(observation.reader.and_then(Result::ok));
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
            return;
        }
        if !observation.verified || entry.observed.elapsed() > Duration::from_secs(2) {
            entry.generation = entry.generation.wrapping_add(1);
            entry.polling = false;
            entry.opening = None;
            entry.detached_physics = None;
            self.retired.extend(entry.reader.take());
            entry.current = None;
            entry.telemetry = None;
            self.retired.extend(observation.reader.and_then(Result::ok));
            return;
        }
        if let Some(catalogue) = observation
            .catalogue
            .filter(|_| entry.observation_epoch == observation.epoch)
        {
            entry.vehicles = catalogue;
            entry.catalogued = Instant::now();
        }
        if let Some(reader) = observation.reader {
            match reader {
                Ok(reader) => {
                    self.retired.extend(entry.reader.replace(reader));
                    entry.error = None;
                }
                Err(error) => {
                    entry.error = Some(error);
                    entry.retry = Instant::now() + Duration::from_secs(2);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "observation_tests.rs"]
mod tests;
