//! Read-only provider composition. Transport topology remains resource metadata.
use crate::{
    profile_resource_ports as ports,
    profile_resources::{attributes, batch, Collect},
};
use osg_profiles::resources::{
    EvidenceKind, IdentityEvidence, Observation, ProviderBatch, ResourceType, Route, Transport,
};
use osg_profiles::resources_ports::{PortUpdate, ScalarSchema, TextEncoding};
use osg_profiles::{Direction, PortValue};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

pub(crate) type Gateways = Arc<dyn Fn() -> Vec<(String, Value)> + Send + Sync>;
pub(crate) type Panel = Arc<dyn Fn(Value) -> Value + Send + Sync>;
#[derive(Clone)]
struct GameCatalogue {
    setup_revision: String,
    catalogue_revision: String,
    ports: PortUpdate,
}
struct Sources {
    session: String,
    controllers: Arc<osg_controller_input::ControllerService>,
    companions: Arc<osg_mapping_host::companions::CompanionProvider>,
    host: Arc<crate::companion_host::Runtime>,
    games: Arc<crate::games::GamesRuntime>,
    gateways: Gateways,
    panel: Panel,
    native: bool,
    usb: Arc<crate::module_usb_fleet::Fleet>,
    can_scopes: BTreeSet<String>,
    refreshing: Arc<Mutex<BTreeSet<String>>>,
    catalogues: BTreeMap<String, (Instant, String, Result<GameCatalogue, String>)>,
}
pub(crate) fn collector(
    controllers: Arc<osg_controller_input::ControllerService>,
    companions: Arc<osg_mapping_host::companions::CompanionProvider>,
    host: Arc<crate::companion_host::Runtime>,
    games: Arc<crate::games::GamesRuntime>,
    gateways: Gateways,
    panel: Panel,
    usb: Arc<crate::module_usb_fleet::Fleet>,
    native: bool,
) -> Collect {
    let sources = Mutex::new(Sources {
        session: uuid::Uuid::new_v4().to_string(),
        controllers,
        companions,
        host,
        games,
        gateways,
        panel,
        native,
        usb,
        can_scopes: BTreeSet::new(),
        refreshing: Arc::new(Mutex::new(BTreeSet::new())),
        catalogues: BTreeMap::new(),
    });
    Arc::new(move |revision| {
        sources
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .collect(revision)
    })
}
fn evidence(kind: EvidenceKind, value: String) -> IdentityEvidence {
    IdentityEvidence { kind, value }
}
fn observation(key: String, name: String, resource_type: ResourceType) -> Observation {
    Observation {
        key,
        name,
        resource_type,
        evidence: vec![],
        attributes: BTreeMap::new(),
        diagnostics: vec![],
        route: None,
        ports: None,
    }
}
impl Sources {
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
        let controllers = self.controllers.resource_inventory();
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
        let mut batches = vec![
            (
                "host-controllers".into(),
                Ok(batch(
                    "host-controllers",
                    &controllers.session,
                    revision,
                    host,
                )),
            ),
            (
                "standalone-usb".into(),
                usb_result.map(|items| batch("standalone-usb", &self.session, revision, items)),
            ),
            (
                "companions".into(),
                self.companion_observations()
                    .map(|items| batch("companions", &self.session, revision, items)),
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
            let observed = self.can_observations(id, state, &verified);
            self.refresh_can_capabilities(id, state);
            batches.push((
                scope.clone(),
                observed.map(|items| batch(&scope, &self.session, revision, items)),
            ));
        }
        for scope in &self.can_scopes {
            if !gateways
                .iter()
                .any(|(id, _)| scope == &format!("gateway:{id}"))
            {
                batches.push((
                    scope.clone(),
                    Ok(batch(scope, &self.session, revision, vec![])),
                ));
            }
        }
        for game in self.games.resource_catalogue() {
            let Some(id) = game["id"].as_str() else {
                continue;
            };
            let scope = format!("game:{id}");
            let provider = game["provider"].as_str().unwrap_or(match id {
                "dcs" => "dcs-bios",
                "assetto-corsa-rally" => "assetto-corsa-rally",
                _ => "x-plane",
            });
            let mut resource = observation(
                id.into(),
                game["name"].as_str().unwrap_or(id).into(),
                ResourceType::GameIntegration,
            );
            resource.evidence = vec![evidence(EvidenceKind::GameScope, id.into())];
            resource.attributes = attributes([
                ("gameId", id.into()),
                ("provider", provider.into()),
                ("installed", game["installed"].to_string()),
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
            if self.catalogues.get(id).is_none_or(|(at, revision, _)| {
                revision != setup_revision || at.elapsed() >= Duration::from_secs(5)
            }) {
                self.catalogues.insert(
                    id.into(),
                    (
                        Instant::now(),
                        setup_revision.into(),
                        self.games
                            .resource_integration_catalogue(id)
                            .and_then(|catalogue| {
                                let revision = |field: &str| {
                                    catalogue[field]
                                        .as_str()
                                        .filter(|value| {
                                            !value.is_empty()
                                                && value.len() <= 256
                                                && !value.chars().any(char::is_control)
                                        })
                                        .map(str::to_owned)
                                        .ok_or_else(|| {
                                            format!("The game-owned {field} is missing.")
                                        })
                                };
                                Ok(GameCatalogue {
                                    setup_revision: revision("providerSetupRevision")?,
                                    catalogue_revision: revision("catalogueRevision")?,
                                    ports: PortUpdate::complete(ports::integration(&catalogue)?)?,
                                })
                            }),
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
                attributes: BTreeMap::new(),
                baseline_ready: false,
                writable: false,
                priority: 0,
            });
            batches.push((
                scope.clone(),
                Ok(batch(&scope, &self.session, revision, vec![resource])),
            ));
        }
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
            }
        }
        Ok(Some(item))
    }
    fn companion_observations(&self) -> Result<Vec<Observation>, String> {
        self.companions
            .resource_inventory()
            .into_iter()
            .map(|source| {
                let publication = self.host.resource_publication(&source.definition.source_id);
                let transport = self.host.resource_transport(&source.definition.source_id);
                Self::companion_observation(source, publication, transport)
            })
            .collect()
    }
    fn companion_observation(
        source: osg_mapping_host::companions::ResourceDescriptor,
        publication: Result<osg_companion_service::Publication, String>,
        transport: Option<osg_companion_service::SourceTransport>,
    ) -> Result<Observation, String> {
        use osg_mapping::profile::{Kind, Value as Input};
        let definition = source.definition;
        let mut item = observation(
            definition.source_id.clone(),
            definition.name,
            ResourceType::Companion,
        );
        item.evidence = vec![evidence(
            EvidenceKind::CompanionInstance,
            definition.source_id.clone(),
        )];
        item.attributes = attributes([("publicationHash", definition.publication_hash)]);
        if let Some(fault) = source.fault {
            item.diagnostics.push(fault);
        }
        let mut descriptors = definition
            .controls
            .into_iter()
            .map(|control| {
                let schema = match control.kind {
                    Kind::Button => ScalarSchema::Boolean {},
                    Kind::Hat => ScalarSchema::Integer {
                        min: -1,
                        max: 7,
                        unit: None,
                    },
                    Kind::Axis { range } => ScalarSchema::Integer {
                        min: range.min() as i64,
                        max: 32767,
                        unit: None,
                    },
                };
                let fallback = match control.fallback {
                    Input::Button(value) => PortValue::Boolean(value),
                    Input::Axis(value) => PortValue::Number(value as f64),
                    Input::Hat(value) => PortValue::Number(value.map_or(-1, i32::from) as f64),
                };
                ports::state(
                    control.id.clone(),
                    control.id,
                    Direction::Output,
                    schema,
                    Some(fallback),
                )
            })
            .collect::<Vec<_>>();
        let mut publication_verified = false;
        match publication {
                Ok(publication) if publication.publication_hash == item.attributes["publicationHash"] => { publication_verified = true; for feedback in publication.feedback {
                    use osg_companion_service::FeedbackKind;
                    let schema = match feedback.kind {
                        FeedbackKind::Boolean => ScalarSchema::Boolean {},
                        FeedbackKind::Text => ScalarSchema::Text { max_bytes: 256, encoding: TextEncoding::Utf8 },
                        FeedbackKind::Number => { let range = feedback.range.ok_or("Published feedback numeric range is missing.")?; ScalarSchema::Number { min: range.min, max: range.max, unit: feedback.unit } },
                    };
                    for output in feedback.output_ids { descriptors.push(ports::state(format!("feedback:{}:{output}", feedback.channel_id), feedback.channel_id.clone(), Direction::Input, schema.clone(), None)); }
                } },
                Ok(_) => item.diagnostics.push("Companion publication changed during inspection; capture its complete replacement descriptors.".into()),
                Err(error) => item.diagnostics.push(error),
            }
        item.ports = publication_verified
            .then(|| PortUpdate::complete(descriptors))
            .transpose()?;
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
    fn can_observations(
        &self,
        gateway: &str,
        state: &Value,
        verified: &BTreeMap<String, (String, u32)>,
    ) -> Result<Vec<Observation>, String> {
        let mut observed = Vec::new();
        for module in state["modules"].as_array().into_iter().flatten() {
            let Some(id) = module["id"].as_str() else {
                continue;
            };
            let mut item = observation(
                id.into(),
                module["name"].as_str().unwrap_or(id).into(),
                ResourceType::OsgModule,
            );
            item.evidence = vec![evidence(EvidenceKind::Serial, id.into())];
            let mut identity_conflict = false;
            if let Some((_, boot)) = verified.get(id) {
                if module["boot"].as_u64() == Some(*boot as u64) && state["canProtocol"] == 3 {
                    item.evidence.push(evidence(
                        EvidenceKind::VerifiedPhysical,
                        format!("osg-mcu:{id}"),
                    ));
                } else {
                    identity_conflict = true;
                    item.diagnostics.push("USB and CAN identity sightings have different boots; reconcile the physical identity before routed writes.".into());
                }
            }
            item.attributes = attributes([("moduleId", id.into()), ("gatewayId", gateway.into())]);
            let events = &state["moduleEvents"][id];
            let actions = &state["moduleActions"][id];
            let expected_route = json!([
                state["epoch"],
                module["node"],
                module["lease"],
                module["boot"]
            ]);
            let events_current = events["ok"] == true
                && events["route"] == expected_route
                && events["configurationGeneration"] == module["configurationGeneration"];
            let events_known = events_current
                || (events["status"] == "unsupported"
                    && events["route"] == expected_route
                    && events["configurationGeneration"] == module["configurationGeneration"]);
            let actions_current = actions["status"] == "ready"
                && actions["route"] == expected_route
                && actions["configurationGeneration"] == module["configurationGeneration"];
            let actions_known = actions_current
                || (actions["status"] == "unsupported"
                    && actions["route"] == expected_route
                    && actions["configurationGeneration"] == module["configurationGeneration"]);
            let mut descriptors = vec![];
            if events_current {
                descriptors.extend(ports::events(events)?);
            }
            if actions_current {
                descriptors.extend(ports::actions(actions)?);
            }
            item.ports = (events_known && actions_known)
                .then(|| PortUpdate::complete(descriptors))
                .transpose()?;
            if item.ports.is_none() && state["connected"] == true && module["online"] == true {
                item.diagnostics.push(
                    "Waiting for the complete current control and action capability snapshot."
                        .into(),
                );
            }
            if state["connected"] == true && module["online"] == true {
                item.route = Some(Route {
                    id: format!("can:{gateway}:{id}"),
                    transport: Transport::Can,
                    provider_generation: json!([
                        state["epoch"],
                        module["boot"],
                        module["lease"],
                        module["configurationGeneration"],
                        events["stream"]
                    ])
                    .to_string(),
                    attributes: attributes([
                        ("gatewayId", gateway.into()),
                        ("moduleId", id.into()),
                        ("boot", module["boot"].to_string()),
                        ("node", module["node"].to_string()),
                    ]),
                    baseline_ready: events_current,
                    writable: !identity_conflict
                        && module["canConfigure"] == true
                        && actions["status"] != "uncertain",
                    priority: 50,
                });
            }
            observed.push(item);
        }
        Ok(observed)
    }
    fn refresh_can_capabilities(&self, id: &str, state: &Value) {
        if !self.native || state["connected"] != true {
            return;
        }
        let modules = state["modules"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["online"] == true && m["canConfigure"] == true)
            .filter_map(|module| {
                let id = module["id"].as_str()?;
                let expected = json!([
                    state["epoch"],
                    module["node"],
                    module["lease"],
                    module["boot"]
                ]);
                let current = |cache: &Value| {
                    cache["route"] == expected
                        && cache["configurationGeneration"] == module["configurationGeneration"]
                };
                let events = &state["moduleEvents"][id];
                let actions = &state["moduleActions"][id];
                let events_known =
                    current(events) && (events["ok"] == true || events["status"] == "unsupported");
                let actions_known = current(actions)
                    && matches!(actions["status"].as_str(), Some("ready" | "unsupported"));
                let commands = [
                    ("events_state", events_known),
                    ("action_describe", actions_known),
                ]
                .into_iter()
                .filter_map(|(command, known)| (!known).then_some(command))
                .collect::<Vec<_>>();
                (!commands.is_empty()).then(|| (module.clone(), commands))
            })
            .collect::<Vec<_>>();
        if modules.is_empty()
            || !self
                .refreshing
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(id.into())
        {
            return;
        }
        let panel = self.panel.clone();
        let pending = self.refreshing.clone();
        let gateway = id.to_owned();
        thread::spawn(move || {
            for (module, commands) in modules.into_iter().take(32) {
                for command in commands {
                    let _ = panel(
                        json!({"command":command,"gatewayId":gateway,"moduleId":module["id"],"boot":module["boot"],"clientId":"profile-resource-discovery"}),
                    );
                }
            }
            pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&gateway);
        });
    }
}
fn hex(value: &Option<String>) -> Option<u16> {
    value
        .as_deref()
        .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
}

#[cfg(test)]
#[path = "profile_resource_sources_tests.rs"]
mod tests;
