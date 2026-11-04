//! Capture reader work under a short mutex and discard stale asynchronous results.
use super::*;

pub(crate) struct Qualification {
    id: String,
    generation: u64,
    signature: String,
    installation: String,
    setup_revision: String,
    enabled: bool,
    process: Option<ProcessIdentity>,
    qualified: Option<bool>,
}
impl Qualification {
    pub(crate) fn verify(
        mut self,
        games: &[Value],
        mut setup_revision: impl FnMut(&str) -> String,
        mut alive: impl FnMut(&ProcessIdentity) -> bool,
    ) -> Self {
        self.qualified = Some(
            self.enabled
                && games.iter().any(|game| {
                    game["id"] == self.id
                        && game["installed"] == true
                        && Tracker::installation(game) == self.installation
                })
                && setup_revision(&self.id) == self.setup_revision
                && self.process.as_ref().is_some_and(&mut alive),
        );
        self
    }
}
pub(crate) struct VehiclePoll {
    id: String,
    generation: u64,
    signature: String,
    integration: String,
    inspection: Vec<String>,
    now_ms: u64,
    at: Instant,
    reader: Reader,
    result: Option<Result<(Option<String>, Capture), String>>,
    qualification: Qualification,
}
impl VehiclePoll {
    pub(crate) fn read(mut self) -> Self {
        // Inspection requests made while the reader was detached are picked up
        // on its next poll. No reader may hold the state mutex during provider I/O.
        let _ = self.reader.inspect(&self.integration, &self.inspection);
        let read_ms = self.now_ms.saturating_add(u64::try_from(self.at.elapsed().as_millis()).unwrap_or(u64::MAX));
        self.result = Some(self.reader.poll_telemetry(read_ms).map(
            |(current, mut capture)| {
                capture.at = self.at;
                (current, capture)
            },
        ));
        self
    }
    pub(crate) fn verify(
        mut self,
        games: &[Value],
        setup_revision: impl FnMut(&str) -> String,
        alive: impl FnMut(&ProcessIdentity) -> bool,
    ) -> Self {
        self.qualification = self.qualification.verify(games, setup_revision, alive);
        self
    }
}
impl Tracker {
    pub(crate) fn poll_candidates(&self) -> Vec<Qualification> {
        self.entries
            .iter()
            .filter_map(|(id, entry)| {
                #[cfg(test)]
                if entry.injected {
                    return None;
                }
                Some(Qualification {
                    id: id.clone(),
                    generation: entry.generation,
                    signature: entry.signature.clone(),
                    installation: entry.installation.clone(),
                    setup_revision: entry.setup_revision.clone(),
                    enabled: entry.enabled,
                    process: entry.process.clone(),
                    qualified: None,
                })
            })
            .collect()
    }
    pub(crate) fn begin_polls(&mut self, qualified: Vec<Qualification>) -> Vec<VehiclePoll> {
        let now_ms = self.origin.elapsed().as_millis() as u64;
        let mut polls = vec![];
        for qualification in qualified {
            let Some(entry) = self.entries.get_mut(&qualification.id) else {
                continue;
            };
            if entry.generation != qualification.generation
                || entry.signature != qualification.signature
            {
                continue;
            }
            if qualification.qualified == Some(true) {
                entry.observed = Instant::now();
            }
            if qualification.qualified == Some(false)
                || entry.observed.elapsed() > Duration::from_secs(2)
            {
                entry.generation = entry.generation.wrapping_add(1);
                entry.polling = false;
                entry.opening = None;
                entry.detached_physics = None;
                self.retired.extend(entry.reader.take());
                entry.current = None;
                entry.telemetry = None;
                self.publish_visible(&qualification.id);
                continue;
            }
            let Some(reader) = entry.reader.take() else {
                self.publish_visible(&qualification.id);
                continue;
            };
            entry.detached_physics = match &reader {
                Reader::Dcs {
                    physics: Some(physics),
                    ..
                } => Some(physics.view()),
                _ => None,
            };
            entry.polling = true;
            let id = qualification.id.clone();
            polls.push(VehiclePoll {
                id: qualification.id.clone(),
                generation: entry.generation,
                signature: entry.signature.clone(),
                integration: entry.integration.clone(),
                inspection: entry.inspection.selection(Instant::now()).to_vec(),
                now_ms,
                at: Instant::now(),
                reader,
                result: None,
                qualification,
            });
            self.publish_visible(&id);
        }
        polls
    }
    pub(crate) fn apply_poll(&mut self, poll: VehiclePoll) {
        let Some(entry) = self.entries.get_mut(&poll.id) else {
            self.retired.push(poll.reader);
            self.publish_visible(&poll.id);
            return;
        };
        if entry.generation != poll.generation
            || entry.signature != poll.signature
            || !entry.polling
        {
            self.retired.push(poll.reader);
            self.publish_visible(&poll.id);
            return;
        }
        entry.polling = false;
        entry.detached_physics = None;
        // A slow read does not make old telemetry fresh on completion.
        if poll.qualification.qualified == Some(false)
            || poll.at.elapsed() > Duration::from_secs(2)
            || entry.observed.elapsed() > Duration::from_secs(2)
        {
            entry.generation = entry.generation.wrapping_add(1);
            entry.current = None;
            entry.telemetry = None;
            self.retired.push(poll.reader);
            self.publish_visible(&poll.id);
            return;
        }
        let id = poll.id.clone();
        let mut retain = false;
        match poll
            .result
            .expect("Reader work must finish before publication")
        {
            Ok((current, capture)) => {
                self.retired.extend(entry.reader.replace(poll.reader));
                entry.telemetry = Some(Arc::new(capture));
                entry.current = current.and_then(|value| vehicle(&entry.integration, &value));
                if let Some(value) = &entry.current {
                    if self.seen.contains_key(&poll.id) || self.seen.len() < 512 {
                        let seen = self.seen.entry(poll.id).or_default();
                        let key = value["id"].as_str().unwrap();
                        if !seen.contains_key(key) && seen.len() < MAX_VEHICLES {
                            seen.insert(key.into(), value.clone());
                            entry.inventory_revision = entry.inventory_revision.wrapping_add(1);
                            retain = true;
                        }
                    }
                }
            }
            Err(error) => {
                self.retired.push(poll.reader);
                entry.current = None;
                entry.telemetry = None;
                entry.generation = entry.generation.wrapping_add(1);
                entry.error = Some(error);
                entry.retry = Instant::now() + Duration::from_secs(2);
            }
        }
        if retain { self.retain_seen(); }
        self.publish_visible(&id);
    }
    fn retain_seen(&mut self) {
        let games = self
            .seen
            .iter()
            .map(|(id, values)| (id.clone(), json!(values.values().collect::<Vec<_>>())))
            .collect::<serde_json::Map<_, _>>();
        if let Some(writer) = &self.writer {
            if let Err(error) = writer.queue(json!({"schemaVersion":1,"games":games})) {
                if self.retention_error.as_ref() != Some(&error) { self.retention_error = Some(error); self.changed(); }
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn poll_verified(
        &mut self,
        games: &[Value],
        mut setup_revision: impl FnMut(&str) -> String,
        mut alive: impl FnMut(&ProcessIdentity) -> bool,
    ) {
        let qualified = self
            .poll_candidates()
            .into_iter()
            .map(|candidate| candidate.verify(games, &mut setup_revision, &mut alive))
            .collect();
        let polls = self.begin_polls(qualified);
        drop(self.take_retired());
        for poll in polls {
            self.apply_poll(poll.read());
            drop(self.take_retired());
        }
    }
    #[cfg(test)]
    pub(crate) fn poll(&mut self) {
        let candidates = self.poll_candidates();
        let polls = self.begin_polls(candidates);
        drop(self.take_retired());
        for poll in polls {
            self.apply_poll(poll.read());
            drop(self.take_retired());
        }
    }
}

#[cfg(test)]
#[path = "polling_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "polling_stream_tests.rs"]
mod stream_tests;
