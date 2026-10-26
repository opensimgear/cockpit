impl Sources {
    fn collect_games(&mut self, revision: u64) -> Vec<(String, Result<ProviderBatch, String>)> {
        let mut batches = vec![];
        for game in self.games.resource_catalogue() {
            let Some(id) = game["id"].as_str() else {
                continue;
            };
            let scope = format!("game:{id}");
            let Some(integration) = crate::game_discovery::integration_binding(&game) else {
                continue;
            };
            let provider = game["provider"]
                .as_str()
                .ok_or("Missing integration provider")
                .unwrap();
            let mut resource = observation(
                id.into(),
                game["name"].as_str().unwrap_or(id).into(),
                ResourceType::GameIntegration,
            );
            resource.evidence = vec![evidence(EvidenceKind::GameScope, id.into())];
            resource.attributes = attributes([
                ("gameId", id.into()),
                ("integrationKey", integration.into()),
                ("provider", provider.into()),
                ("installed", game["installed"].to_string()),
                (
                    "processVerified",
                    game["integrationContext"]["processVerified"]
                        .as_bool()
                        .unwrap_or(false)
                        .to_string(),
                ),
            ]);
            let setup_revision = game["integrationSetup"]["providerSetupRevision"]
                .as_str()
                .unwrap_or("");
            for (attribute, value) in [
                ("providerSetupRevision", setup_revision),
                (
                    "catalogueRevision",
                    game["integrationSetup"]["catalogueRevision"]
                        .as_str()
                        .unwrap_or(""),
                ),
                (
                    "setupStatus",
                    game["integrationSetup"]["state"]
                        .as_str()
                        .unwrap_or("unknown"),
                ),
                (
                    "gameContextStatus",
                    game["integrationContext"]["status"]
                        .as_str()
                        .unwrap_or("waiting-for-game"),
                ),
            ] {
                if !value.is_empty() {
                    resource.attributes.insert(attribute.into(), value.into());
                }
            }
            // Catalogue presence and a discovered process never authorize input
            // consumption or commands. P4 admits a freshly captured profile run.
            resource.diagnostics.push(
                "Waiting for an applied profile in its matching verified game context.".into(),
            );
            if self
                .catalogues
                .get(id)
                .is_none_or(|(at, revision, result)| {
                    revision != setup_revision
                        || at.elapsed()
                            >= if result.is_ok() && integration.starts_with("x-plane-") {
                                Duration::from_secs(60)
                            } else {
                                Duration::from_secs(5)
                            }
                })
            {
                self.catalogues.insert(
                    id.into(),
                    (
                        Instant::now(),
                        setup_revision.into(),
                        self.games
                            .resource_integration_catalogue(id)
                            .and_then(GameCatalogue::parse),
                    ),
                );
            }
            match self.catalogues[id].2.clone() {
                Ok(catalogue) => {
                    resource
                        .attributes
                        .insert("providerSetupRevision".into(), catalogue.setup_revision);
                    resource
                        .attributes
                        .insert("catalogueRevision".into(), catalogue.catalogue_revision);
                    resource.ports = Some(catalogue.ports);
                    if let Some(module) = catalogue.aircraft_module {
                        resource.attributes.insert("aircraftModule".into(), module);
                    }
                }
                Err(error) => resource.diagnostics.push(error),
            }
            resource.route = Some(Route {
                id: scope.clone(),
                transport: Transport::Integration,
                provider_generation: format!(
                    "{}:{}",
                    resource
                        .attributes
                        .get("providerSetupRevision")
                        .map(String::as_str)
                        .unwrap_or("unconfigured"),
                    resource
                        .attributes
                        .get("catalogueRevision")
                        .map(String::as_str)
                        .unwrap_or("unavailable")
                ),
                attributes: resource
                    .attributes
                    .iter()
                    .filter(|(key, _)| {
                        [
                            "gameId",
                            "integrationKey",
                            "aircraftModule",
                            "providerSetupRevision",
                            "catalogueRevision",
                        ]
                        .contains(&key.as_str())
                    })
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
                baseline_ready: false,
                writable: false,
                priority: 0,
            });
            let base = resource.clone();
            batches.push((
                scope.clone(),
                Ok(batch(&scope, &self.session, revision, vec![resource])),
            ));
            if integration == "dcs" {
                let mut modules = self
                    .games
                    .dcs_vehicle_catalogues()
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                let mut active_module = None;
                // The native aircraft is exposed even when no editor has ever
                // registered its tab in this process.
                if let Some(vehicle) = self.games.current_vehicle_id(id) {
                    if let Ok(module) = self.games.register_dcs_vehicle_catalogue(&vehicle) {
                        modules.insert(module.clone());
                        active_module = Some(module);
                    }
                }
                for module in modules {
                    let key = format!("{id}:{module}");
                    let scope = format!("game:{key}");
                    if self.catalogues.get(&key).is_none_or(|(at, known, _)| {
                        known != setup_revision || at.elapsed() >= Duration::from_secs(5)
                    }) {
                        let capture = self
                            .games
                            .resource_dcs_vehicle_catalogue(&module)
                            .and_then(GameCatalogue::parse);
                        self.catalogues.insert(
                            key.clone(),
                            (Instant::now(), setup_revision.into(), capture),
                        );
                    }
                    let mut item = base.clone();
                    item.key = key.clone();
                    item.name = format!("DCS · {module}");
                    item.evidence = vec![evidence(EvidenceKind::GameScope, key)];
                    item.attributes
                        .insert("aircraftModule".into(), module.clone());
                    item.attributes.insert("vehicleId".into(), module.clone());
                    if active_module.as_ref() != Some(&module) {
                        item.attributes
                            .insert("processVerified".into(), "false".into());
                        item.attributes
                            .insert("gameContextStatus".into(), "waiting-vehicle".into());
                    }
                    item.ports = None;
                    match self.catalogues[&item.key].2.clone() {
                        Ok(catalogue) => {
                            item.attributes
                                .insert("providerSetupRevision".into(), catalogue.setup_revision);
                            item.attributes
                                .insert("catalogueRevision".into(), catalogue.catalogue_revision);
                            if let Some(aliases) = catalogue.vehicle_aliases {
                                item.attributes.insert("vehicleAliases".into(), aliases);
                            }
                            item.ports = Some(catalogue.ports);
                        }
                        Err(error) => item.diagnostics.push(error),
                    }
                    item.route.as_mut().unwrap().id = scope.clone();
                    item.route.as_mut().unwrap().provider_generation = format!(
                        "{}:{}",
                        item.attributes
                            .get("providerSetupRevision")
                            .map(String::as_str)
                            .unwrap_or("unconfigured"),
                        item.attributes
                            .get("catalogueRevision")
                            .map(String::as_str)
                            .unwrap_or("unavailable")
                    );
                    item.route.as_mut().unwrap().attributes = item
                        .attributes
                        .iter()
                        .filter(|(key, _)| {
                            [
                                "gameId",
                                "integrationKey",
                                "aircraftModule",
                                "providerSetupRevision",
                                "catalogueRevision",
                            ]
                            .contains(&key.as_str())
                        })
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect();
                    batches.push((
                        scope.clone(),
                        Ok(batch(&scope, &self.session, revision, vec![item])),
                    ));
                }
            }
        }

        batches
    }

    fn collect(&mut self, revision: u64) -> Vec<(String, Result<ProviderBatch, String>)> {
        let usb_result = self.usb_observations();
        let verified = self
            .usb
            .snapshots()
            .into_iter()
            .filter(|slot| usb_result.is_ok() && slot.present && slot.admissible)
            .filter_map(|slot| {
                let snapshot = slot.snapshot;
                if !snapshot.connected {
                    return None;
                }
                snapshot
                    .identity
                    .zip(snapshot.route)
                    .map(|(identity, route)| (identity.module_id, (identity.serial, route.boot)))
            })
            .collect::<BTreeMap<_, _>>();
        let host = self
            .controllers
            .authoritative_resource_inventory()
            .map(|controllers| {
                let host = controllers
                    .sources
                    .into_iter()
                    .map(|source| {
                        let mut result = observation(
                            source.saved.id.clone(),
                            source.saved.name,
                            ResourceType::HostController,
                        );
                        result.evidence.push(evidence(
                            EvidenceKind::LocalPath,
                            format!("host:{}", source.saved.id),
                        ));
                        if let Some(serial) = &source.saved.identity.serial {
                            result
                                .evidence
                                .push(evidence(EvidenceKind::Serial, serial.clone()));
                            if let Some((module, _)) = verified
                                .iter()
                                .find(|(_, (uid, _))| uid.eq_ignore_ascii_case(serial))
                            {
                                // Match the verified USB interface's VID/PID as well. A name
                                // or a familiar VID/PID by itself never establishes identity.
                                if self.usb.snapshots().iter().any(|slot| {
                                    slot.port
                                        .serial_number
                                        .as_deref()
                                        .is_some_and(|uid| uid.eq_ignore_ascii_case(serial))
                                        && hex(&slot.port.vendor_id)
                                            == Some(source.saved.identity.vendor_id)
                                        && hex(&slot.port.product_id)
                                            == Some(source.saved.identity.product_id)
                                }) {
                                    result.evidence.push(evidence(
                                        EvidenceKind::VerifiedPhysical,
                                        format!("osg-mcu:{module}"),
                                    ));
                                    result.resource_type = ResourceType::OsgModule;
                                }
                            }
                        }
                        result.attributes = attributes([
                            ("backend", source.saved.identity.backend),
                            ("collection", source.saved.identity.collection),
                        ]);
                        result.diagnostics = source.diagnostics;
                        if !controllers.diagnostic.is_empty() {
                            result.diagnostics.push(controllers.diagnostic.clone());
                        }
                        result.ports = (!source.controls.is_empty())
                            .then(|| PortUpdate::complete(ports::controls(&source.controls)))
                            .transpose()
                            .ok()
                            .flatten();
                        result.route = source.online.then(|| Route {
                            id: format!("hid:{}", source.saved.id),
                            transport: Transport::Hid,
                            provider_generation: source.generation.to_string(),
                            attributes: attributes([("path", source.saved.identity.path)]),
                            baseline_ready: source.baseline_ready,
                            writable: false,
                            priority: 10,
                        });
                        result
                    })
                    .collect();
                batch("host-controllers", &controllers.session, revision, host)
            });
        let companions = self.companion_observations();
        let mut batches = vec![
            ("host-controllers".into(), host),
            (
                "standalone-usb".into(),
                usb_result.map(|items| batch("standalone-usb", &self.session, revision, items)),
            ),
            (
                "companions".into(),
                companions.clone().map(|items| {
                    batch(
                        "companions",
                        &self.session,
                        revision,
                        items
                            .into_iter()
                            .filter(|item| {
                                item.attributes
                                    .get("componentKind")
                                    .is_none_or(|kind| kind != "paired-companion")
                            })
                            .collect(),
                    )
                }),
            ),
            (
                "paired-companions".into(),
                companions.map(|items| {
                    batch(
                        "paired-companions",
                        &self.session,
                        revision,
                        items
                            .into_iter()
                            .filter(|item| {
                                item.attributes
                                    .get("componentKind")
                                    .is_some_and(|kind| kind == "paired-companion")
                            })
                            .collect(),
                    )
                }),
            ),
        ];
        let gateways = (self.gateways)();
        let conflicts = gateways
            .iter()
            .filter(|(_, state)| state["connected"] == true)
            .flat_map(|(_, state)| {
                state["modules"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|module| module["online"] == true)
                    .filter_map(|module| {
                        module["id"]
                            .as_str()
                            .filter(|id| {
                                verified.get(*id).is_some_and(|(_, boot)| {
                                    module["boot"].as_u64() != Some(*boot as u64)
                                })
                            })
                            .map(str::to_owned)
                    })
            })
            .collect::<BTreeSet<_>>();
        for (id, state) in &gateways {
            let scope = format!("gateway:{id}");
            self.can_scopes.insert(scope.clone());
            let observed = match state["connected"].as_bool() {
                Some(true) => self.can_observations(id, state, &verified).map(|items| {
                    self.can_retained.insert(scope.clone(), items.clone());
                    batch(&scope, &self.session, revision, items)
                }),
                Some(false) => Ok(self.disconnected_can_batch(&scope, revision)),
                None => Err("Gateway connection state is unavailable; remembered module inventory is retained.".into()),
            };
            self.refresh_can_capabilities(id, state);
            batches.push((scope, observed));
        }
        for scope in &self.can_scopes {
            if !gateways
                .iter()
                .any(|(id, _)| scope == &format!("gateway:{id}"))
            {
                batches.push((
                    scope.clone(),
                    Ok(self.disconnected_can_batch(scope, revision)),
                ));
            }
        }
        batches.extend(self.collect_games(revision));
        for (_, batch) in &mut batches {
            if let Ok(batch) = batch {
                for resource in &mut batch.observations {
                    if resource
                        .attributes
                        .get("moduleId")
                        .is_some_and(|id| conflicts.contains(id))
                    {
                        if let Some(route) = &mut resource.route {
                            route.writable = false;
                        }
                        resource.diagnostics.push("Conflicting simultaneous USB/CAN boots block routed writes on both sightings until physical identity is reconciled.".into());
                    }
                }
            }
        }
        batches
    }
}
