impl GameSources {
    pub(crate) fn delivery_batch(&mut self) -> Option<ProviderBatch> {
        let game = self.game.as_ref()?;
        if !self.acquired
            || self.lost
            || self.reader.is_none()
            || (!self.fresh && self.revision == 0)
        {
            return None;
        }
        let id = &game.proof.game_id;
        let writable = game.integration_key == "dcs" && self.fresh && self.context.is_some();
        let publication = (self.epoch, self.fresh, writable);
        if self.published == Some(publication) {
            return None;
        }
        let revision = self.revision.checked_add(1)?;
        let attributes = BTreeMap::from([
            ("gameId".into(), id.clone()),
            ("integrationKey".into(), game.integration_key.clone()),
            (
                "providerSetupRevision".into(),
                game.proof.provider_setup_revision.clone(),
            ),
            (
                "catalogueRevision".into(),
                game.proof.catalogue_revision.clone(),
            ),
            ("gamePreparationSession".into(), game.proof.session.clone()),
            ("gameReaderSession".into(), self.reader_session.clone()),
        ]);
        let observations = self
            .delivery_catalogues
            .iter()
            .map(|(key, catalogue)| {
                let mut attributes = attributes.clone();
                attributes.insert("catalogueRevision".into(), catalogue.revision.clone());
                if let Some(module) = &catalogue.aircraft_module {
                    attributes.insert("aircraftModule".into(), module.clone());
                }
                if let Some(vehicle) = &game.proof.vehicle_id {
                    attributes.insert("vehicleId".into(), vehicle.clone());
                }
                Some(Observation {
                    key: key.clone(),
                    name: id.clone(),
                    resource_type: ResourceType::GameIntegration,
                    evidence: vec![IdentityEvidence {
                        kind: EvidenceKind::GameScope,
                        value: key.clone(),
                    }],
                    attributes: attributes.clone(),
                    diagnostics: vec![],
                    route: Some(Route {
                        id: format!("profile-game:{key}"),
                        transport: Transport::Integration,
                        provider_generation: format!("{}:{}", self.reader_session, self.epoch),
                        attributes,
                        baseline_ready: self.fresh,
                        writable: writable
                            && catalogue.aircraft_module.as_deref() != Some("CommonData"),
                        priority: 100,
                    }),
                    ports: Some(
                        PortUpdate::complete(catalogue.ports.values().cloned().collect()).ok()?,
                    ),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let batch = ProviderBatch {
            scope: format!("profile-game:{id}"),
            session: self.reader_session.clone(),
            revision,
            complete: true,
            observations,
        };
        self.revision = revision;
        self.published = Some(publication);
        Some(batch)
    }
    pub(crate) fn provider_identity(&self) -> Option<(&str, &str)> {
        self.game
            .as_ref()
            .map(|game| (game.proof.game_id.as_str(), self.reader_session.as_str()))
    }
    pub(crate) fn invoke(&mut self, games: &GamesRuntime, effect: &Effect) -> Result<(), String> {
        if self.command_revoked(effect) {
            return Err("Native simulator command publication authority was withdrawn; prepare a new reader to command again.".into());
        }
        self.validate(games)?;
        let (command, argument) = self.command_request(effect)?;
        let Some(Reader::Dcs {
            transport: Some(transport),
            ..
        }) = &self.reader
        else {
            return Err("The native simulator command transport is unavailable.".into());
        };
        transport.send_control(&command, &argument)
    }
    fn command_request(&mut self, effect: &Effect) -> Result<(String, String), String> {
        if self.command_revoked(effect) {
            return Err("Native simulator command publication authority was withdrawn; prepare a new reader to command again.".into());
        }
        self.validate_catalogue(true)?;
        let context = self
            .context
            .as_ref()
            .ok_or("Simulator commands require their applied profile context.")?;
        if self.lost
            || !self.fresh
            || effect.run.session != context.owner.session
            || effect.run.generation.0 != context.owner.run_generation
            || effect.sequence.0 <= self.last_command_sequence
        {
            return Err(
                "Simulator command owner or live baseline expired; the command was not replayed."
                    .into(),
            );
        }
        let command = self
            .commands
            .get(&(
                effect.endpoint.node_id.clone(),
                effect.endpoint.port_id.clone(),
            ))
            .ok_or("The effect is not an assigned native simulator command.")?;
        let SignalSample::Known(SignalValue::ActionPack(pack)) = &effect.sample else {
            return Err("Simulator commands require a complete known action tuple.".into());
        };
        let PortContract::Action {
            parameters,
            semantics,
            ..
        } = &command.descriptor.contract
        else {
            unreachable!()
        };
        if *semantics == osg_mapping::signal::ActionSemantics::OneShot
            && matches!(
                effect.reason,
                ReportReason::Baseline
                    | ReportReason::Start
                    | ReportReason::SourceLost
                    | ReportReason::Stop
            )
        {
            return Err(
                "A baseline or fallback cannot replay a one-shot simulator command.".into(),
            );
        }
        if pack.arguments.len() != parameters.len()
            || pack
                .arguments
                .iter()
                .zip(parameters)
                .any(|(argument, parameter)| argument.id != parameter.id)
        {
            return Err("Simulator action arguments changed identity or order.".into());
        }
        let arguments = pack
            .arguments
            .iter()
            .map(|argument| argument.value.clone())
            .collect::<Vec<_>>();
        command.descriptor.validate_arguments(&arguments)?;
        let argument = match arguments.as_slice() {
            [PortValue::Number(value)] if value.fract() == 0.0 => format!("{value:.0}"),
            [PortValue::Text(value)] => value.clone(),
            _ => {
                return Err(
                    "The native DCS command requires its one exact advertised argument.".into(),
                )
            }
        };
        if let Some(Reader::Dcs { decoder, .. }) = &mut self.reader {
            decoder.tick(effect.at_us.0 / 1000);
            if !decoder.snapshot(effect.at_us.0 / 1000).fresh {
                return Err(
                    "The simulator export baseline expired before command dispatch.".into(),
                );
            }
        }
        self.last_command_sequence = effect.sequence.0;
        Ok((command.command.clone(), argument))
    }

    fn command_revoked(&self, effect: &Effect) -> bool {
        self.revoked_commands.iter().any(|(resource, port)| {
            resource == &effect.endpoint.node_id && port == &effect.endpoint.port_id
        })
    }
    /// Publication loss withdraws only the affected logical command endpoints.
    /// Telemetry and unrelated commands retain their reader and freshness; a
    /// withdrawn endpoint cannot rearm within this reader instance.
    pub(crate) fn revoke_commands(&mut self, endpoints: &[osg_profiles::Endpoint]) {
        for key in self.commands.keys() {
            if endpoints
                .iter()
                .any(|endpoint| endpoint.node_id == key.0 && endpoint.port_id == key.1)
            {
                self.revoked_commands.insert(key.clone());
            }
        }
    }
    pub(crate) fn stop(&mut self) {
        if let Some(Reader::Acr {
            source: Some(source),
            ..
        }) = &mut self.reader
        {
            source.close();
        }
        self.reader = None;
        self.context = None;
        self.fresh = false;
        self.lost = true;
    }
}