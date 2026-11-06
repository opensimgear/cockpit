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
    retained_process: bool,
    qualified: Option<bool>,
    capture_changed: bool,
    capture_expired: bool,
}
impl Qualification {
    pub(crate) fn without_renewal(mut self) -> Self {
        if self.qualified == Some(true) { self.qualified = None; }
        self
    }
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
                && self.process.as_ref().is_some_and(|process| self.retained_process || alive(process)),
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
    previous_capture: Option<Arc<Capture>>,
    capture_changed: bool,
    result: Option<Result<(Option<String>, Capture), String>>,
    qualification: Qualification,
}
impl VehiclePoll {
    pub(crate) fn read(mut self) -> Self {
        // Inspection requests made while the reader was detached are picked up
        // on its next poll. No reader may hold the state mutex during provider I/O.
        let _ = self.reader.inspect(&self.integration, &self.inspection);
        if self.qualification.qualified == Some(true) {
            if let Reader::Dcs { resolved_aircraft, .. } = &mut self.reader {
                *resolved_aircraft = None;
            }
        }
        let read_ms = self.now_ms.saturating_add(u64::try_from(self.at.elapsed().as_millis()).unwrap_or(u64::MAX));
        let capture = if self.qualification.capture_expired && !self.qualification.capture_changed {
            self.reader.expire_assetto(read_ms)
        } else {
            self.reader.poll_telemetry(read_ms)
        };
        self.result = Some(capture.map(
            |(current, mut capture)| {
                capture.at = self.at;
                self.capture_changed = self.previous_capture.as_ref()
                    .is_none_or(|previous| !capture.same_observation(previous));
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
    pub(crate) fn poll_candidates(&mut self) -> Vec<Qualification> {
        self.candidates(false)
    }
    pub(crate) fn capture_candidates(&mut self) -> Vec<Qualification> {
        self.candidates(true)
    }
    fn candidates(&mut self, capture_only: bool) -> Vec<Qualification> {
        let now = Instant::now();
        self.entries
            .iter_mut()
            .filter_map(|(id, entry)| {
                #[cfg(test)]
                if entry.injected {
                    return None;
                }
                let changed = entry.reader.is_some()
                    && entry.capture_watch.as_mut().is_some_and(CaptureBinding::begin_drain);
                let expired = entry.capture_watch.is_some()
                    && entry.reader.as_ref().and_then(Reader::assetto_deadline).is_some_and(|at| now >= at);
                let retained_process = entry.assetto_process_current().is_some();
                let inspection_due = entry.reader.is_some() && entry.inspection.deadline(now).is_some_and(|deadline| now >= deadline);
                if (capture_only || retained_process) && !changed && !expired && !inspection_due {
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
                    retained_process,
                    qualified: None,
                    capture_changed: changed,
                    capture_expired: expired,
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
            if qualification.qualified == Some(true) && !qualification.retained_process {
                entry.observed = Instant::now();
            }
            if qualification.qualified == Some(false)
                || (!qualification.retained_process && !entry.process_current())
            {
                entry.generation = entry.generation.wrapping_add(1);
                entry.polling = false;
                entry.opening = None;
                entry.detached_physics = None;
                entry.capture_watch = None;
                self.retired.extend(entry.reader.take());
                entry.current = None;
                entry.telemetry = None;
                self.publish_visible(&qualification.id);
                continue;
            }
            let inspection = entry.inspection.selection(Instant::now()).to_vec();
            if !qualification.capture_changed && entry.capture_watch.is_some() {
                if matches!(&entry.reader, Some(Reader::Rally { .. })) && !qualification.capture_expired {
                    self.publish_visible(&qualification.id);
                    continue;
                }
                if let Some(Reader::Xplane(source)) = &entry.reader {
                    // The native owner publishes expiry and terminal changes.
                    // Maintenance still applies inspection lease changes.
                    let ids = crate::game_channels::native_ids(&entry.integration, &inspection);
                    if source.inspection_matches(&ids) {
                        entry.inspection.pending(false);
                        self.publish_visible(&qualification.id);
                        continue;
                    }
                }
            }
            let Some(reader) = entry.reader.take() else {
                self.publish_visible(&qualification.id);
                continue;
            };
            entry.inspection.pending(false);
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
                inspection,
                now_ms,
                at: Instant::now(),
                reader,
                previous_capture: entry.telemetry.clone(),
                capture_changed: true,
                result: None,
                qualification,
            });
            self.publish_visible(&id);
        }
        polls
    }
    pub(crate) fn apply_poll(&mut self, mut poll: VehiclePoll) {
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
            || (!entry.process_current() && (entry.assetto_process_current().is_none() || !matches!(poll.result, Some(Err(_)))))
        {
            entry.generation = entry.generation.wrapping_add(1);
            entry.current = None;
            entry.telemetry = None;
            entry.capture_watch = None;
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
                if poll.reader.take_capture_change() {
                    entry.capture_watch = self.capture_wake.as_ref()
                        .and_then(|wake| CaptureBinding::open(&poll.reader, wake));
                }
                self.retired.extend(entry.reader.replace(poll.reader));
                let capture = Arc::new(capture);
                if !poll.capture_changed {
                    entry.published_capture = Some(Arc::downgrade(&capture));
                }
                entry.telemetry = Some(capture);
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
                entry.capture_watch = None;
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
