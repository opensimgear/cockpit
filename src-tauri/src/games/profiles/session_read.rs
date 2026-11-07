impl GameSources {
    pub(crate) fn acquire(&mut self, games: &GamesRuntime) -> Result<(), String> {
        let game = self
            .game
            .as_ref()
            .and_then(|game| crate::game_telemetry::Game::from_id(&game.integration_key).ok())
            .unwrap_or(crate::game_telemetry::Game::Rally);
        self.acquire_with_acr(games, |identity, _| {
            crate::acr_windows::SharedSource::open(identity, game)
                .map(|source| Box::new(source) as Box<dyn crate::game_telemetry::Source>)
        })
    }
    fn acquire_with_acr(
        &mut self,
        games: &GamesRuntime,
        mut open_acr: impl FnMut(&crate::game_discovery::ProcessIdentity, &str) -> Result<Box<dyn crate::game_telemetry::Source>, String>,
    ) -> Result<(), String> {
        self.validate_catalogue(true)?;
        let Some(game) = &self.game else {
            return Ok(());
        };
        games.validate_game_preparation(&game.proof)?;
        if self.acquired {
            return Err("This native game reader was already acquired; prepare a fresh reader instead of replaying acquisition.".into());
        }
        self.acquired = true;
        if self.reader.is_none() && (!self.sources.is_empty() || !self.commands.is_empty()) {
            return Err(
                "The source was stopped; prepare a new native game session before reacquiring."
                    .into(),
            );
        }
        match self.reader.as_mut() {
            Some(Reader::Dcs {
                network, transport, decoder, ..
            }) => *transport = Some(crate::dcs_bios::decoded::Source::open(network, decoder.decode_spec()?)?),
            Some(Reader::Xplane {
                connection,
                outputs,
                source,
                ..
            }) => *source = Some(crate::xplane::Source::new(connection, outputs.clone())?),
            Some(Reader::Acr { source, .. }) => {
                let path = &game.proof.process.path;
                let image = path.rsplit(['\\', '/']).next().unwrap_or(path);
                *source = Some(open_acr(&game.proof.process, image)?);
            }
            None => {}
        }
        self.lost = false;
        let _ = self.refresh_source_wakes(games);
        Ok(())
    }
    pub(crate) fn admit(&mut self, games: &GamesRuntime) -> Result<(), String> {
        let began = Instant::now();
        self.validate_catalogue(true)?;
        let Some(game) = &self.game else {
            return Ok(());
        };
        let context = games.admit_prepared_game_context(&game.proof)?;
        if self.context.as_ref().is_some_and(|old| old != &context) {
            return Err(
                "This game reader belongs to an earlier applied owner; prepare a new session."
                    .into(),
            );
        }
        self.context = Some(context);
        self.validation.seed(began);
        Ok(())
    }
    fn validate_catalogue(&mut self, force: bool) -> Result<(), String> {
        if let Some(Reader::Dcs {
            catalogue_guard, ..
        }) = &mut self.reader
        {
            let began = Instant::now();
            catalogue_guard.validate(force)?;
            if force { self.catalogue_validation.seed(began); }
        }
        Ok(())
    }
    fn validate(&mut self, games: &GamesRuntime, force_catalogue: bool) -> Result<(), String> {
        self.validate_catalogue(force_catalogue)?;
        self.validate_context(games)
    }
    fn validate_context(&self, games: &GamesRuntime) -> Result<(), String> {
        let Some(game) = &self.game else {
            return Ok(());
        };
        if let Some(context) = &self.context {
            games.validate_admitted_game_context(context, &game.proof).map_err(Into::into)
        } else {
            games.validate_game_preparation(&game.proof)
        }
    }
    pub(crate) fn poll(&mut self, games: &GamesRuntime, clock: impl FnOnce() -> u64) -> GamePoll {
        self.poll_with_policy(games, clock, false)
    }
    fn poll_with_policy(
        &mut self,
        games: &GamesRuntime,
        clock: impl FnOnce() -> u64,
        baseline: bool,
    ) -> GamePoll {
        if self.lost {
            return GamePoll {
                context_lost: true,
                ..Default::default()
            };
        }
        let rejection = match self.command_worker.as_ref().map(command_worker::Worker::poll) {
            Some(Err(command_worker::Failure::ProcessUnavailable(reason))) => {
                self.stop_commands();
                Some(format!("DCS command process verification is unavailable. Pending commands were discarded. {reason}"))
            }
            Some(Err(command_worker::Failure::Terminal(error))) => return self.lose(clock(), error),
            _ => None,
        };
        let mut result = self.poll_native(games, clock, baseline);
        result.diagnostics.extend(rejection);
        result
    }
    fn poll_native(&mut self, games: &GamesRuntime, clock: impl FnOnce() -> u64, baseline: bool) -> GamePoll {
        let wake_error = self.refresh_source_wakes(games).err();
        if baseline {
            let began = Instant::now();
            if let Err(error) = self.validate(games, true) { return self.lose(clock(), error); }
            self.validation.seed(began);
        } else if self.game.is_some() {
            if !self.validation.started() {
                let game = self.game.as_ref().unwrap();
                let wake = self.notification_wake().as_ref().map(std::sync::Arc::downgrade);
                self.validation.start(
                    wake,
                    games.profile_validation_task(game.proof.clone(), self.context.clone()),
                );
            }
            self.start_catalogue_validation();
            match self.validation_ready() {
                Ok(true) => {},
                Ok(false) => return self.pending(clock(), "Game identity validation is pending; its branches await fresh evidence.".into()),
                Err(error) => return self.lose(clock(), error),
            }
        }
        // Process and catalogue validation may outlast the telemetry lease.
        // Correlate native capture after those checks. Queueing/validation time
        // must not become new source evidence when the consumer finally reads.
        let now_us = clock();
        let snapshot = self.read(now_us, baseline);
        let mut result = match snapshot {
            Ok(Some(mut snapshot)) => {
                if let (
                    Some(game),
                    Some(Reader::Dcs {
                        physics_outputs, ..
                    }),
                ) = (&self.game, &self.reader)
                {
                    if !physics_outputs.is_empty() {
                        let physics = games.dcs_physics_snapshot(&game.proof.process);
                        self.physics = Some(merge_dcs_physics(
                            &mut snapshot,
                            physics_outputs,
                            physics.as_ref().filter(|physics| game.proof.vehicle_id.as_ref().is_none_or(|expected| physics.aircraft.as_ref() == Some(expected))),
                        ));
                    }
                }
                self.frames(snapshot, now_us)
            }
            Ok(None) => GamePoll::default(),
            Err(error) => self.lose(now_us, error),
        };
        if let Some(error) = wake_error {
            result.diagnostics.push(format!("Native source notifications are unavailable; adapter service remains active: {error}"));
        }
        result
    }
    fn validation_ready(&mut self) -> Result<bool, String> {
        let catalogue = matches!(self.reader, Some(Reader::Dcs { .. }))
            .then_some(&mut self.catalogue_validation);
        self.validation.poll_with(catalogue)
    }
    fn start_catalogue_validation(&mut self) {
        if self.catalogue_validation.started() { return; }
        if let Some(Reader::Dcs { catalogue_guard, .. }) = &self.reader {
            let mut guard = catalogue_guard.clone();
            let wake = self.notification_wake().as_ref().map(std::sync::Arc::downgrade);
            self.catalogue_validation.start(wake, move || {
                let began = Instant::now();
                guard.validate(true)?;
                Ok(Some(began))
            });
        }
    }
    /// Drain native acquisition into one current baseline immediately before
    /// arming. Pre-Start transitions never acquire event/one-shot authority.
    pub(crate) fn baseline(
        &mut self,
        games: &GamesRuntime,
        clock: impl FnOnce() -> u64,
    ) -> GamePoll {
        for source in self.sources.values_mut() {
            source.available = false;
            for family in source.families.values_mut() { family.available = false; }
        }
        self.poll_baseline(games, clock)
    }
    /// One bounded pre-arm read. The host owns waiting, cancellation and output
    /// heartbeats, including after discarding a queued pre-activation backlog.
    pub(crate) fn poll_baseline(
        &mut self,
        games: &GamesRuntime,
        clock: impl FnOnce() -> u64,
    ) -> GamePoll {
        self.poll_with_policy(games, clock, true)
    }
    pub(crate) fn requires_baseline(&self) -> bool {
        self.reader.is_some()
    }
    pub(crate) fn fail(&mut self, now_us: u64, error: String) -> GamePoll {
        self.lose(now_us, error)
    }
    /// Temporary uncertainty withdraws samples and commands without retiring
    /// the exact reader. Resumption requires a new source baseline.
    pub(crate) fn pending(&mut self, now_us: u64, message: String) -> GamePoll {
        self.fresh = false;
        self.witness = None;
        self.witnessed_progress = false;
        let mut result = GamePoll { diagnostics: vec![message], ..Default::default() };
        result.frames = self.withdraw_native(now_us, Reason::Gap);
        result.fatal = self.fatal.clone();
        result
    }
    fn read(&mut self, now_us: u64, baseline: bool) -> Result<Option<ExportSnapshot>, String> {
        let authored_fixture = self.authored_transport_fixture();
        match self.reader.as_mut() {
            Some(Reader::Dcs {
                network,
                transport,
                decoder,
                ..
            }) => {
                if transport.is_none() && self.bios_retry_at.is_some_and(|at| Instant::now() >= at) {
                    self.bios_retry_at = Some(Instant::now() + std::time::Duration::from_secs(1));
                    if let Ok(source) = crate::dcs_bios::decoded::Source::open(network, decoder.decode_spec()?) {
                        *transport = Some(source);
                        self.bios_retry_at = None;
                    }
                }
                let Some(receiver) = transport.as_ref() else {
                    return Ok(self.bios_retry_at.map(|_| decoder.snapshot(now_us / 1000)));
                };
                let snapshot = match receiver.snapshot() {
                    Ok(snapshot) => snapshot,
                    Err(_) if !authored_fixture => {
                        self.command_worker = None;
                        self.command_catalogue = None;
                        self.command_fences.clear();
                        // Subscriber errors are sticky. Reopening subscribes to
                        // the healthy canonical publisher's current baseline, or
                        // replaces a terminal publisher. Retain its old owner
                        // until opening finishes so a healthy socket stays live.
                        decoder.reset_stream(now_us / 1000);
                        let replacement = crate::dcs_bios::decoded::Source::open(network, decoder.decode_spec()?);
                        drop(transport.take());
                        match replacement {
                            Ok(source) => { *transport = Some(source); self.bios_retry_at = None; }
                            Err(_) => self.bios_retry_at = Some(Instant::now() + std::time::Duration::from_secs(1)),
                        }
                        self.witness = None;
                        self.witnessed_progress = false;
                        self.fresh = false;
                        // Withdraw BIOS before accepting its new baseline.
                        // Ownship is merged independently by the caller.
                        return Ok(Some(decoder.snapshot(now_us / 1000)));
                    }
                    Err(_) if baseline => {
                        self.command_worker = None;
                        self.command_catalogue = None;
                        self.command_fences.clear();
                        // Recovery registers a new coherent source baseline;
                        // it cannot replay the lost reader's partial history.
                        decoder.reset_stream(now_us / 1000);
                        drop(transport.take());
                        *transport = Some(crate::dcs_bios::decoded::Source::open(network, decoder.decode_spec()?)?);
                        self.witness = None;
                        self.witnessed_progress = false;
                        self.fresh = false;
                        return Ok(None);
                    }
                    Err(error) => return Err(error),
                };
                if self.game.as_ref().and_then(|game| game.proof.vehicle_id.as_ref())
                    .zip(snapshot.aircraft.as_ref()).is_some_and(|(expected, actual)| expected != actual) {
                    return Err("DCS aircraft changed from the captured game context; prepare a new reader.".into());
                }
                decoder.accept_shared(snapshot)?;
                Ok(Some(decoder.snapshot(now_us / 1000)))
            }
            Some(Reader::Xplane {
                source: Some(source),
                canonical_outputs,
                ..
            }) => {
                let mut snapshot = source.snapshot()?.0;
                if self.game.as_ref().and_then(|game| game.proof.vehicle_id.as_ref())
                    .zip(snapshot.aircraft.as_ref()).is_some_and(|(expected, actual)| expected.replace('\\', "/") != actual.replace('\\', "/")) {
                    return Err("X-Plane aircraft changed from the captured game context; prepare a new reader.".into());
                }
                if let Some(game) = &self.game {
                    crate::game_channels::captured_project_rows(
                        &game.integration_key,
                        canonical_outputs,
                        &mut snapshot.values,
                        Instant::now(),
                    );
                }
                Ok(Some(snapshot))
            }
            Some(Reader::Acr {
                source: Some(source),
                decoder,
                outputs,
            }) => {
                let snapshot = decoder.poll(source.as_mut(), now_us / 1000);
                let channels = snapshot.values();
                let values = outputs
                    .iter()
                    .filter_map(|output| {
                        channels
                            .get(output["channel"].as_str()?)
                            .map(|value| (output["id"].as_str().unwrap().to_owned(), value.clone()))
                    })
                    .collect();
                Ok(Some(ExportSnapshot {
                    epoch: snapshot.epoch,
                    aircraft: snapshot
                        .context
                        .as_ref()
                        .map(|context| context.car_model.clone()),
                    mission_active: snapshot.fresh,
                    fresh: snapshot.fresh,
                    age_ms: snapshot.age_ms,
                    counter: snapshot.packet_id.map(|id| (id & 65535) as u16),
                    values,
                    malformed_frames: 0,
                }))
            }
            _ => Ok(None),
        }
    }
    fn frames(&mut self, snapshot: ExportSnapshot, now_us: u64) -> GamePoll {
        #[cfg(test)]
        if self.authored_fixture { return self.fixture_frames(snapshot, now_us); }
        self.native_frames(snapshot, now_us)
    }
    #[cfg(test)]
    fn fixture_frames(&mut self, mut snapshot: ExportSnapshot, now_us: u64) -> GamePoll {
        let shared_baseline = matches!(&self.reader, Some(Reader::Dcs { decoder, .. }) if decoder.shared_baseline_ready());
        if snapshot.fresh || snapshot.mission_active && shared_baseline {
            if let Some(counter) = snapshot.counter {
                match self.witness {
                    Some((epoch, previous)) if epoch == snapshot.epoch => {
                        if counter != previous {
                            self.witnessed_progress = true;
                        }
                    }
                    _ => {
                        self.witnessed_progress = false;
                    }
                }
                self.witness = Some((snapshot.epoch, counter));
                snapshot.fresh &= self.witnessed_progress;
            } else {
                snapshot.fresh = false;
            }
        } else {
            self.witness = None;
            self.witnessed_progress = false;
        }
        self.fresh = snapshot.fresh;
        self.epoch = snapshot.epoch.max(1);
        let mut result = GamePoll {
            fresh: self.fresh,
            ..Default::default()
        };
        for (resource, state) in &mut self.sources {
            let physics = self.physics.as_ref().filter(|physics| {
                state
                    .ports
                    .iter()
                    .any(|port| physics.keys.contains(&port.native))
            });
            let physics_update = physics
                .is_some_and(|physics| state.physics_witness.as_ref() != Some(&physics.witness));
            let physics_baseline = physics.is_some_and(|physics| {
                physics.witness.fresh
                    && (state
                        .physics_witness
                        .as_ref()
                        .is_none_or(|previous| !previous.fresh)
                        || state.physics_epoch.as_ref()
                            != Some(&(physics.witness.producer_id.clone(), physics.witness.session.clone(), physics.witness.epoch)))
            });
            let baseline = snapshot.fresh
                && (!state.available || state.epoch != snapshot.epoch || physics_baseline);
            let update = baseline
                || state.available != snapshot.fresh
                || snapshot.fresh && state.counter != snapshot.counter
                || physics_update;
            if !update {
                continue;
            }
            if baseline {
                state.generation = state.generation.saturating_add(1);
                state.epoch = snapshot.epoch;
            }
            state.sequence = state.sequence.saturating_add(1);
            state.available = snapshot.fresh;
            state.counter = snapshot.counter;
            if let Some(physics) = physics {
                state.physics_witness = Some(physics.witness.clone());
                if physics.witness.fresh {
                    state.physics_epoch =
                        Some((physics.witness.producer_id.clone(), physics.witness.session.clone(), physics.witness.epoch));
                }
            }
            let changes=state.ports.iter().map(|port|Change {port_id:port.logical.clone(),sample:if !snapshot.fresh||physics.is_some_and(|physics|physics.stale.contains(&port.native)) {SignalSample::Stale} else {snapshot.values.get(&port.native).and_then(scalar).map(SignalSample::scalar).filter(|sample|matches!(sample,SignalSample::Known(value)if osg_mapping::engine::typed::accepts(&port.contract,value))).unwrap_or(SignalSample::Unknown)}}).collect();
            result.frames.push(Frame {
                source: resource.clone(),
                generation: Counter(state.generation),
                sequence: Counter(state.sequence),
                captured_us: Counter(now_us),
                reason: if baseline {
                    Reason::Baseline
                } else {
                    Reason::Change
                },
                changes,
            }.into());
        }
        result
    }
    fn lose(&mut self, now_us: u64, error: String) -> GamePoll {
        self.stop();
        self.lost = true;
        let mut result = GamePoll {
            context_lost: true,
            diagnostics: vec![error],
            ..Default::default()
        };
        result.frames = self.withdraw_native(now_us, Reason::Disconnect);
        result.fatal = self.fatal.clone();
        result
    }
}
