impl Sources {
    fn usb_observations(&mut self) -> Result<Vec<Observation>, String> {
        let slots = if self.native {
            self.usb.scan()?
        } else {
            self.usb.snapshots()
        };
        slots
            .into_iter()
            .map(Self::usb_observation)
            .collect::<Result<Vec<_>, _>>()
            .map(|observations| observations.into_iter().flatten().collect())
    }
    fn usb_observation(
        slot: crate::module_usb_fleet::SlotSnapshot,
    ) -> Result<Option<Observation>, String> {
        let publication = hid_publication::usb_status(&slot);
        let snapshot = slot.snapshot;
        let route = json!(snapshot.route);
        let events_current = snapshot.connected
            && snapshot
                .events
                .as_ref()
                .is_some_and(|events| events["route"] == route && events["ok"] == true);
        let events_known = events_current
            || snapshot.events.as_ref().is_some_and(|events| {
                events["route"] == route && events["status"] == "unsupported"
            });
        let actions_current = snapshot.connected
            && snapshot
                .actions
                .as_ref()
                .is_some_and(|actions| actions["route"] == route && actions["status"] == "ready");
        let actions_known = actions_current
            || snapshot.actions.as_ref().is_some_and(|actions| {
                actions["route"] == route && actions["status"] == "unsupported"
            });
        let writable = slot.admissible
            && snapshot.error.is_none()
            && snapshot
                .actions
                .as_ref()
                .is_none_or(|actions| actions["status"] != "uncertain");
        let Some(identity) = snapshot.identity else {
            return Ok(None);
        };
        let mut item = observation(slot.key.clone(), identity.profile, ResourceType::OsgModule);
        item.evidence = vec![
            evidence(EvidenceKind::Serial, identity.serial.clone()),
            evidence(
                EvidenceKind::VerifiedPhysical,
                format!("osg-mcu:{}", identity.module_id),
            ),
        ];
        item.attributes = attributes([
            ("model", identity.model),
            ("board", identity.board),
            ("moduleId", identity.module_id),
            ("usbSerial", identity.serial),
        ]);
        item.diagnostics = slot.diagnostics;
        if let Some(error) = snapshot.error {
            item.diagnostics.push(error);
        }
        let mut descriptors = vec![];
        if events_current {
            descriptors.extend(ports::events(snapshot.events.as_ref().unwrap())?);
        }
        if actions_current {
            descriptors.extend(ports::actions(snapshot.actions.as_ref().unwrap())?);
        }
        item.ports = (events_known && actions_known)
            .then(|| PortUpdate::complete(descriptors))
            .transpose()?;
        if item.ports.is_none() && snapshot.connected {
            item.diagnostics.push(
                "Waiting for the complete current control and action capability snapshot.".into(),
            );
        }
        if slot.present && slot.admissible && snapshot.connected {
            if let Some(route) = snapshot.route {
                item.route = Some(Route {
                    id: format!("usb:{}", slot.key),
                    transport: Transport::Usb,
                    provider_generation: format!(
                        "{}:{}:{}",
                        route.connection_generation, route.boot, route.configuration_generation
                    ),
                    attributes: attributes([
                        ("path", slot.port.path),
                        ("boot", route.boot.to_string()),
                    ]),
                    baseline_ready: events_current,
                    writable,
                    priority: 100,
                });
                let route = item.route.as_ref().unwrap();
                item.attributes.insert(
                    osg_profiles::compiler::hid_policy::attribute(&route.id),
                    osg_profiles::compiler::hid_policy::evidence(route, publication),
                );
            }
        }
        Ok(Some(item))
    }
    fn companion_observations(&self) -> Result<Vec<Observation>, String> {
        let paired = self.host.resource_companions()?;
        let mut observations = self
            .companions
            .resource_inventory()
            .into_iter()
            .filter(|source| {
                paired
                    .iter()
                    .any(|companion| companion.instances.contains(&source.definition.source_id))
            })
            .map(|source| {
                let publication = self.host.resource_publication(&source.definition.source_id);
                let transport = self.host.resource_transport(&source.definition.source_id);
                let companion = paired
                    .iter()
                    .find(|companion| companion.instances.contains(&source.definition.source_id))
                    .unwrap();
                let mut item = Self::companion_observation(source, publication, transport)?;
                item.attributes
                    .insert("companionId".into(), companion.companion_id.clone());
                Ok(item)
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut components = vec![];
        for companion in paired {
            let mut item = observations
                .iter()
                .find(|item| item.key == companion.instance_id)
                .cloned()
                .unwrap_or_else(|| {
                    let mut item = observation(
                        String::new(),
                        companion.name.clone(),
                        ResourceType::Companion,
                    );
                    // No assignment is authoritative absence of panel controls,
                    // unlike an assigned panel whose publication is offline.
                    item.ports = Some(PortUpdate::complete(vec![]).expect("Empty catalogue is valid"));
                    item
                });
            item.key = format!("paired:{}", companion.companion_id);
            item.name = companion.name;
            item.evidence = vec![evidence(
                EvidenceKind::CompanionInstance,
                companion.instance_id.clone(),
            )];
            item.attributes
                .insert("componentKind".into(), "paired-companion".into());
            item.attributes
                .insert("companionId".into(), companion.companion_id);
            item.attributes
                .insert("companionKind".into(), companion.kind);
            item.attributes.insert(
                "pairedIdentityFingerprint".into(),
                companion.identity_fingerprint.clone(),
            );
            if item.route.is_none() && companion.connected {
                item.route = Some(Route {
                    id: format!("companion:{}", companion.instance_id),
                    transport: Transport::Integration,
                    provider_generation: companion.identity_fingerprint,
                    attributes: attributes([("preparationOnly", "paired-companion".into())]),
                    baseline_ready: false,
                    writable: false,
                    priority: 0,
                });
            }
            components.push(item);
        }
        observations.extend(components);
        Ok(observations)
    }
    fn companion_observation(
        source: osg_mapping_host::companions::ResourceDescriptor,
        publication: Result<osg_companion_service::Publication, String>,
        transport: Option<osg_companion_service::SourceTransport>,
    ) -> Result<Observation, String> {
        let definition = source.definition;
        let mut item = observation(
            definition.source_id.clone(),
            definition.name.clone(),
            ResourceType::Companion,
        );
        item.evidence = vec![evidence(
            EvidenceKind::CompanionInstance,
            definition.source_id.clone(),
        )];
        item.attributes = attributes([("publicationHash", definition.publication_hash.clone())]);
        if let Some(fault) = source.fault {
            item.diagnostics.push(fault);
        }
        let mut publication_verified = false;
        item.ports = match publication {
            Ok(publication) if publication.publication_hash == definition.publication_hash => {
                let descriptors = ports::companion(&definition, &publication.feedback)?;
                item.attributes.insert(
                    "capabilityFingerprint".into(),
                    publication.capability_fingerprint,
                );
                publication_verified = true;
                Some(PortUpdate::complete(descriptors)?)
            }
            Ok(_) => {
                item.diagnostics.push("Companion publication changed during inspection; capture its complete replacement descriptors.".into());
                None
            }
            Err(error) => {
                item.diagnostics.push(error);
                None
            }
        };
        let transport = transport.map(|transport| match transport {
            osg_companion_service::SourceTransport::Usb => Transport::Usb,
            osg_companion_service::SourceTransport::Network => Transport::Network,
        });
        item.route = source
            .online
            .then_some(transport)
            .flatten()
            .map(|transport| Route {
                id: format!("companion:{}", definition.source_id),
                transport,
                provider_generation: source.generation.to_string(),
                attributes: BTreeMap::new(),
                baseline_ready: source.baseline_ready,
                writable: source.baseline_ready && publication_verified,
                priority: 0,
            });
        Ok(item)
    }
}
