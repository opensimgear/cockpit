impl GameSources {
    pub(crate) fn acquire(&mut self, games: &GamesRuntime) -> Result<(), String> {
        let game = self
            .game
            .as_ref()
            .and_then(|game| crate::game_telemetry::Game::from_id(&game.integration_key).ok())
            .unwrap_or(crate::game_telemetry::Game::Rally);
        self.acquire_with_acr(games, |pid, image| {
            crate::acr_windows::WindowsSource::new_for_game(pid, image, game)
                .map(|source| Box::new(source) as Box<dyn crate::game_telemetry::Source>)
        })
    }
    fn acquire_with_acr(
        &mut self,
        games: &GamesRuntime,
        mut open_acr: impl FnMut(u32, &str) -> Result<Box<dyn crate::game_telemetry::Source>, String>,
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
                network, transport, ..
            }) => *transport = Some(crate::dcs_bios::Transport::open_network(network)?),
            Some(Reader::Xplane {
                connection,
                outputs,
                source,
                ..
            }) => *source = Some(crate::xplane::Source::new(connection, outputs.clone())?),
            Some(Reader::Acr { source, .. }) => {
                let path = &game.proof.process.path;
                let image = path.rsplit(['\\', '/']).next().unwrap_or(path);
                *source = Some(open_acr(game.proof.process.pid, image)?);
            }
            None => {}
        }
        self.lost = false;
        Ok(())
    }
    pub(crate) fn admit(&mut self, games: &GamesRuntime) -> Result<(), String> {
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
        Ok(())
    }
    fn validate_catalogue(&mut self, force: bool) -> Result<(), String> {
        if let Some(Reader::Dcs {
            catalogue_guard, ..
        }) = &mut self.reader
        {
            catalogue_guard.validate(force)?;
        }
        Ok(())
    }
    fn validate(&mut self, games: &GamesRuntime) -> Result<(), String> {
        self.validate_catalogue(false)?;
        let Some(game) = &self.game else {
            return Ok(());
        };
        if let Some(context) = &self.context {
            games.validate_admitted_game_context(context, &game.proof)
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
        if let Err(error) = self.validate(games) {
            return self.lose(clock(), error);
        }
        // Process and catalogue validation may outlast the telemetry lease.
        // Timestamp actual acquisition after those checks, using the host's
        // source clock, rather than backdating newly received simulator bytes.
        let now_us = clock();
        let snapshot = self.read(now_us, baseline);
        match snapshot {
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
                            physics.as_ref(),
                        ));
                    }
                }
                self.frames(snapshot, now_us)
            }
            Ok(None) => GamePoll::default(),
            Err(error) => self.lose(now_us, error),
        }
    }
    /// Drain native acquisition into one current baseline immediately before
    /// arming. Pre-Start transitions never acquire event/one-shot authority.
    pub(crate) fn baseline(
        &mut self,
        games: &GamesRuntime,
        clock: impl FnOnce() -> u64,
    ) -> GamePoll {
        if let Err(error) = self.validate_catalogue(true) {
            return self.lose(clock(), error);
        }
        for source in self.sources.values_mut() {
            source.available = false;
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
    fn read(&mut self, now_us: u64, baseline: bool) -> Result<Option<ExportSnapshot>, String> {
        match self.reader.as_mut() {
            Some(Reader::Dcs {
                network,
                transport,
                decoder,
                ..
            }) => {
                let Some(receiver) = transport.as_ref() else {
                    return Ok(None);
                };
                let mut drained = false;
                for _ in 0..64 {
                    let Some(bytes) = receiver.receive()? else {
                        drained = true;
                        break;
                    };
                    decoder.feed(&bytes, now_us / 1000);
                }
                if !drained && receiver.receive()?.is_some() {
                    decoder.reset_stream(now_us / 1000);
                    if baseline {
                        // Activation can spend longer than the receive bound in
                        // other phases. Drop queued pre-activation datagrams and
                        // require new aircraft identity and advancing counters.
                        // Live polling still disarms on the same overflow.
                        drop(transport.take());
                        *transport = Some(crate::dcs_bios::Transport::open_network(network)?);
                        self.witness = None;
                        self.witnessed_progress = false;
                        self.fresh = false;
                        return Ok(None);
                    }
                    return Err("The native simulator receive queue exceeded its bound; the branch was disarmed instead of replaying a backlog.".into());
                }
                decoder.tick(now_us / 1000);
                Ok(Some(decoder.snapshot(now_us / 1000)))
            }
            Some(Reader::Xplane {
                source: Some(source),
                canonical_outputs,
                ..
            }) => {
                let mut snapshot = source.poll(Instant::now()).0;
                if let Some(game) = &self.game {
                    crate::game_channels::project_rows(
                        &game.integration_key,
                        canonical_outputs,
                        &mut snapshot.values,
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
    fn frames(&mut self, mut snapshot: ExportSnapshot, now_us: u64) -> GamePoll {
        if snapshot.fresh {
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
                snapshot.fresh = self.witnessed_progress;
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
                            != Some(&(physics.witness.session.clone(), physics.witness.epoch)))
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
                        Some((physics.witness.session.clone(), physics.witness.epoch));
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
            });
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
        for (resource, state) in &mut self.sources {
            state.sequence = state.sequence.saturating_add(1);
            state.available = false;
            result.frames.push(Frame {
                source: resource.clone(),
                generation: Counter(state.generation),
                sequence: Counter(state.sequence),
                captured_us: Counter(now_us),
                reason: Reason::Disconnect,
                changes: vec![],
            });
        }
        result
    }
}
