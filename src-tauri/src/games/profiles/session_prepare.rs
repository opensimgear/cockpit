impl GameSources {
    pub(crate) fn needs_context(&self) -> bool {
        self.game.is_none() || self.lost
    }
    pub(crate) fn preparation_session(&self) -> Option<&str> {
        self.game.as_ref().map(|game| game.proof.session.as_str())
    }
    pub(crate) fn reader_session(&self) -> &str { &self.reader_session }
    pub(crate) fn accepts_replacement(&self, game: &PreparedGame) -> bool {
        self.game.as_ref().is_none_or(|old| self.lost
            && (old.proof.session != game.proof.session
                || old.proof == game.proof && old.integration_key == game.integration_key
                    && old.aircraft_module == game.aircraft_module))
    }
    pub(crate) fn accepts_replacement_from(&self, previous: &str, game: &PreparedGame) -> bool {
        // The source may have changed while a read-only worker was validating.
        // Its captured predecessor must still be the reader being replaced.
        self.reader_session == previous && self.accepts_replacement(game)
    }
    pub(crate) fn inherit_source_counters(&mut self, previous: &Self) {
        for (id, source) in &mut self.sources {
            if let Some(old) = previous.sources.get(id) {
                source.generation = old.generation.saturating_add(1);
                source.sequence = old.sequence;
            }
        }
    }
    pub(crate) fn can_command(&self) -> bool {
        !self.lost && self.fresh && self.context.is_some()
    }
    pub(crate) fn has_delivery_publication(&self) -> bool {
        self.revision > 0
    }
    pub(crate) fn prepare(
        plan: &ExecutionPlan,
        game: Option<PreparedGame>,
    ) -> Result<Self, String> {
        let mut result = Self {
            game,
            reader: None,
            sources: BTreeMap::new(),
            commands: BTreeMap::new(),
            delivery_catalogues: BTreeMap::new(),
            context: None,
            fresh: false,
            lost: false,
            epoch: 1,
            revision: 0,
            last_command_sequence: 0,
            revoked_commands: BTreeSet::new(),
            witness: None,
            witnessed_progress: false,
            reader_session: uuid::Uuid::new_v4().to_string(),
            acquired: false,
            published: None,
            physics: None,
            validation: live_validation::Validation::default(),
            catalogue_validation: live_validation::Validation::catalogue(),
        };
        let Some(game) = &result.game else {
            return Ok(result);
        };
        let catalogue = crate::profile_resource_ports::integration(&game.catalogue)?
            .into_iter()
            .map(|port| (port.id.clone(), port))
            .collect::<BTreeMap<_, _>>();
        let mut rows = vec![];
        let mut delivery_catalogues: BTreeMap<String, DeliveryCatalogue> = BTreeMap::new();
        let mut capture_port = |route: Option<&Route>,
                                native: &str,
                                descriptor: &PortDescriptor|
         -> Result<(), String> {
            let module = (game.integration_key == "dcs")
                .then(|| {
                    route
                        .and_then(|route| route.attributes.get("aircraftModule"))
                        .cloned()
                })
                .flatten();
            let key = module
                .as_ref()
                .filter(|module| module.as_str() != "CommonData")
                .map(|module| format!("{}:{module}", game.proof.game_id))
                .unwrap_or_else(|| game.proof.game_id.clone());
            let revision = route
                .and_then(|route| route.attributes.get("catalogueRevision"))
                .cloned()
                .unwrap_or_else(|| game.proof.catalogue_revision.clone());
            let captured = delivery_catalogues
                .entry(key)
                .or_insert_with(|| DeliveryCatalogue {
                    aircraft_module: module.clone(),
                    revision: revision.clone(),
                    ports: BTreeMap::new(),
                });
            if captured.revision != revision || captured.aircraft_module != module {
                return Err(
                    "The captured game resource combines different native aircraft catalogues."
                        .into(),
                );
            }
            captured.ports.insert(native.into(), descriptor.clone());
            Ok(())
        };
        for source in &plan.sources {
            if !compatible_game_route(source.route.as_ref(), game) {
                continue;
            }
            let native = source
                .observed_port_id
                .as_ref()
                .ok_or("Game source has no exact captured native port.")?;
            let descriptor = catalogue
                .get(native)
                .ok_or("The game source disappeared from its captured catalogue.")?;
            if semantic_contract(descriptor.contract.clone())
                != semantic_contract(source.contract.clone())
            {
                return Err("The game source contract changed before acquisition.".into());
            }
            let key = native_key(&source.endpoint.node_id, &source.endpoint.port_id);
            if game.integration_key == "dcs" {
                let channel = native
                    .strip_prefix("channel:")
                    .ok_or("Invalid captured DCS channel.")?;
                if game.catalogue["channels"]
                    .as_array()
                    .is_some_and(|channels| channels.iter().any(|row| row["id"] == channel))
                {
                    rows.push(json!({"id":key,"channel":channel,"physics":true}));
                } else {
                    let (channel, index) = native
                        .strip_prefix("channel:")
                        .and_then(|value| value.rsplit_once(':'))
                        .ok_or("Invalid captured DCS channel.")?;
                    let index = index
                        .parse::<usize>()
                        .map_err(|_| "Invalid captured DCS output index.")?;
                    rows.push(json!({"id":key,"channel":channel,"output":index}));
                }
            } else {
                let channel = native
                    .strip_prefix("channel:")
                    .ok_or("Invalid captured telemetry channel.")?;
                rows.push(json!({"id":key,"channel":channel}));
            }
            result
                .sources
                .entry(source.endpoint.node_id.clone())
                .or_insert_with(|| SourceState {
                    generation: 1,
                    ..Default::default()
                })
                .ports
                .push(SourcePort {
                    logical: source.endpoint.port_id.clone(),
                    native: key,
                    contract: source.contract.clone(),
                });
            capture_port(source.route.as_ref(), native, descriptor)?;
        }
        for sink in plan.sinks.iter().filter(|sink| {
            sink.adapter == SinkAdapter::SimulatorCommand
                && compatible_game_route(sink.route.as_ref(), game)
        }) {
            if game.integration_key != "dcs" {
                return Err("This provider does not advertise native simulator commands.".into());
            }
            let native = sink
                .observed_port_id
                .as_ref()
                .ok_or("Game command has no exact native port.")?;
            let descriptor = catalogue
                .get(native)
                .ok_or("The game command disappeared from its captured catalogue.")?;
            if semantic_contract(descriptor.contract.clone())
                != semantic_contract(sink.contract.clone())
            {
                return Err("The game command contract changed before acquisition.".into());
            }
            let PortContract::Action { endpoint, .. } = &descriptor.contract else {
                return Err("The game command is not an action port.".into());
            };
            result.commands.insert(
                (sink.endpoint.node_id.clone(), sink.endpoint.port_id.clone()),
                CommandPort {
                    descriptor: descriptor.clone(),
                    command: endpoint.clone(),
                },
            );
            capture_port(sink.route.as_ref(), native, descriptor)?;
        }
        result.delivery_catalogues = delivery_catalogues;
        if result.sources.is_empty() && result.commands.is_empty() {
            return Ok(result);
        }
        result.reader = Some(match game.integration_key.as_str() {
            "dcs" => {
                let module = game
                    .aircraft_module
                    .as_deref()
                    .ok_or("The current DCS aircraft has no native module catalogue.")?;
                let path = Path::new(
                    game.connection["cataloguePath"]
                        .as_str()
                        .ok_or("DCS catalogue path is missing.")?,
                );
                let (sources, catalogue_guard, catalogue) =
                    crate::dcs_profile::guarded_telemetry_sources(
                        path,
                        module,
                        &rows
                            .iter()
                            .filter(|row| row["physics"] != true)
                            .cloned()
                            .collect::<Vec<_>>(),
                        game.catalogue["sourceRevision"]
                            .as_str()
                            .ok_or("The DCS source catalogue has no captured native revision.")?,
                    )?;
                let network = crate::dcs_bios::Network::parse(&game.connection)?;
                let decoder = crate::dcs_profile::ExportSession::from_sources(
                    catalogue,
                    network.freshness_ms,
                    sources,
                )?;
                Reader::Dcs {
                    network,
                    transport: None,
                    decoder,
                    catalogue_guard,
                    physics_outputs: rows
                        .into_iter()
                        .filter(|row| row["physics"] == true)
                        .collect(),
                }
            }
            "x-plane-11" | "x-plane-12" => {
                let mut connection = game.connection.clone();
                connection["processIdentity"] = game.proof.process.value();
                Reader::Xplane {
                    connection,
                    outputs: crate::game_channels::source_rows(&game.integration_key, &rows),
                    canonical_outputs: rows,
                    source: None,
                }
            }
            "assetto-corsa"
            | "assetto-corsa-competizione"
            | "assetto-corsa-evo"
            | "assetto-corsa-rally" => {
                let freshness = game.connection["freshnessMs"]
                    .as_u64()
                    .ok_or("Missing telemetry freshness policy.")?;
                Reader::Acr {
                    outputs: rows,
                    source: None,
                    decoder: crate::game_telemetry::Session::new_for_game(
                        freshness,
                        crate::game_telemetry::Game::from_id(&game.integration_key)?,
                    )?,
                }
            }
            _ => return Err("Unsupported native game provider.".into()),
        });
        Ok(result)
    }
}
