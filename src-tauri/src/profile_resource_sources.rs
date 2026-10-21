//! Read-only provider composition. Transport topology remains resource metadata.
#[path = "profile_hid_publication.rs"]
pub(crate) mod hid_publication;
use crate::{
    profile_resource_ports as ports,
    profile_resources::{attributes, batch, Collect},
};
use osg_profiles::resources::{
    EvidenceKind, IdentityEvidence, Observation, ProviderBatch, ResourceType, Route, Transport,
};
use osg_profiles::resources_ports::PortUpdate;
use osg_profiles::Direction;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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
    aircraft_module: Option<String>,
    vehicle_aliases: Option<String>,
}
impl GameCatalogue {
    fn parse(catalogue: Value) -> Result<Self, String> {
        let revision = |field: &str| catalogue[field].as_str().filter(|value| !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control))
            .map(str::to_owned).ok_or_else(||format!("The game-owned {field} is missing."));
        Ok(Self {
            setup_revision: revision("providerSetupRevision")?,
            catalogue_revision: revision("catalogueRevision")?,
            ports: PortUpdate::complete(ports::integration(&catalogue)?)?,
            aircraft_module: catalogue["aircraftModule"].as_str().map(str::to_owned),
            vehicle_aliases: catalogue["vehicleAliases"].as_array().map(|aliases|json!(aliases).to_string()),
        })
    }
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
    can_retained: BTreeMap<String, Vec<Observation>>,
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
        can_retained: BTreeMap::new(),
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
        let host = self.controllers.authoritative_resource_inventory().map(|controllers| {
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
        let companions=self.companion_observations();
        let mut batches = vec![
            (
                "host-controllers".into(),
                host,
            ),
            (
                "standalone-usb".into(),
                usb_result.map(|items| batch("standalone-usb", &self.session, revision, items)),
            ),
            (
                "companions".into(),
                companions.clone().map(|items| batch("companions", &self.session, revision, items.into_iter()
                    .filter(|item|item.attributes.get("componentKind").is_none_or(|kind|kind!="paired-companion")).collect())),
            ),
            (
                "paired-companions".into(),
                companions.map(|items| batch("paired-companions", &self.session, revision, items.into_iter()
                    .filter(|item|item.attributes.get("componentKind").is_some_and(|kind|kind=="paired-companion")).collect())),
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
                ("processVerified", game["integrationContext"]["processVerified"].as_bool().unwrap_or(false).to_string()),
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
                attributes: resource.attributes.iter().filter(|(key,_)|["gameId", "aircraftModule", "providerSetupRevision", "catalogueRevision"].contains(&key.as_str())).map(|(key,value)|(key.clone(),value.clone())).collect(),
                baseline_ready: false,
                writable: false,
                priority: 0,
            });
            let base = resource.clone();
            batches.push((
                scope.clone(),
                Ok(batch(&scope, &self.session, revision, vec![resource])),
            ));
            if id == "dcs" {
                let mut modules = self.games.dcs_vehicle_catalogues().into_iter().collect::<BTreeSet<_>>();
                let mut active_module = None;
                // The native aircraft is exposed even when no editor has ever
                // registered its tab in this process.
                if let Some(vehicle) = self.games.current_vehicle_id("dcs") {
                    if let Ok(module) = self.games.register_dcs_vehicle_catalogue(&vehicle) { modules.insert(module.clone()); active_module = Some(module); }
                }
                for module in modules {
                    let key = format!("dcs:{module}");
                    let scope = format!("game:{key}");
                    if self.catalogues.get(&key).is_none_or(|(at, known, _)|known != setup_revision || at.elapsed() >= Duration::from_secs(5)) {
                        let capture = self.games.resource_dcs_vehicle_catalogue(&module).and_then(GameCatalogue::parse);
                        self.catalogues.insert(key.clone(), (Instant::now(), setup_revision.into(), capture));
                    }
                    let mut item = base.clone();
                    item.key = key.clone();
                    item.name = format!("DCS · {module}");
                    item.evidence = vec![evidence(EvidenceKind::GameScope, key)];
                    item.attributes.insert("aircraftModule".into(), module.clone());
                    item.attributes.insert("vehicleId".into(), module.clone());
                    if active_module.as_ref() != Some(&module) {
                        item.attributes.insert("processVerified".into(), "false".into());
                        item.attributes.insert("gameContextStatus".into(), "waiting-vehicle".into());
                    }
                    item.ports = None;
                    match self.catalogues[&item.key].2.clone() {
                        Ok(catalogue) => {
                            item.attributes.insert("providerSetupRevision".into(), catalogue.setup_revision);
                            item.attributes.insert("catalogueRevision".into(), catalogue.catalogue_revision);
                            if let Some(aliases) = catalogue.vehicle_aliases { item.attributes.insert("vehicleAliases".into(), aliases); }
                            item.ports = Some(catalogue.ports);
                        }
                        Err(error) => item.diagnostics.push(error),
                    }
                    item.route.as_mut().unwrap().id = scope.clone();
                    item.route.as_mut().unwrap().provider_generation = format!("{}:{}", item.attributes.get("providerSetupRevision").map(String::as_str).unwrap_or("unconfigured"), item.attributes.get("catalogueRevision").map(String::as_str).unwrap_or("unavailable"));
                    item.route.as_mut().unwrap().attributes = item.attributes.iter().filter(|(key,_)|["gameId", "aircraftModule", "providerSetupRevision", "catalogueRevision"].contains(&key.as_str())).map(|(key,value)|(key.clone(),value.clone())).collect();
                    batches.push((scope.clone(), Ok(batch(&scope, &self.session, revision, vec![item]))));
                }
            }
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
                item.attributes.insert(osg_profiles::compiler::hid_policy::attribute(&route.id),
                    osg_profiles::compiler::hid_policy::evidence(route, publication));
            }
        }
        Ok(Some(item))
    }
    fn companion_observations(&self) -> Result<Vec<Observation>, String> {
        let paired = self.host.resource_companions()?;
        let mut observations = self.companions
            .resource_inventory()
            .into_iter()
            .filter(|source| paired.iter().any(|companion|companion.instances.contains(&source.definition.source_id)))
            .map(|source| {
                let publication = self.host.resource_publication(&source.definition.source_id);
                let transport = self.host.resource_transport(&source.definition.source_id);
                let companion=paired.iter().find(|companion|companion.instances.contains(&source.definition.source_id)).unwrap();
                let mut item=Self::companion_observation(source, publication, transport)?;
                item.attributes.insert("companionId".into(),companion.companion_id.clone());
                Ok(item)
            })
            .collect::<Result<Vec<_>,String>>()?;
        let mut components=vec![];
        for companion in paired {
            let mut item=observations.iter().find(|item|item.key==companion.instance_id).cloned()
                .unwrap_or_else(||observation(String::new(),companion.name.clone(),ResourceType::Companion));
            item.key=format!("paired:{}",companion.companion_id);
            item.name=companion.name;
            item.evidence=vec![evidence(EvidenceKind::CompanionInstance,companion.instance_id.clone())];
            item.attributes.insert("componentKind".into(),"paired-companion".into());
            item.attributes.insert("companionId".into(),companion.companion_id);
            item.attributes.insert("companionKind".into(),companion.kind);
            item.attributes.insert("pairedIdentityFingerprint".into(),companion.identity_fingerprint.clone());
            if item.route.is_none() && companion.connected {
                item.route=Some(Route {id:format!("companion:{}",companion.instance_id),transport:Transport::Integration,
                    provider_generation:companion.identity_fingerprint,attributes:attributes([("preparationOnly","paired-companion".into())]),
                    baseline_ready:false,writable:false,priority:0});
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
                let descriptors=ports::companion(&definition,&publication.feedback)?;
                item.attributes.insert("capabilityFingerprint".into(),publication.capability_fingerprint);
                publication_verified=true;
                Some(PortUpdate::complete(descriptors)?)
            },
            Ok(_) => { item.diagnostics.push("Companion publication changed during inspection; capture its complete replacement descriptors.".into()); None },
            Err(error) => { item.diagnostics.push(error); None },
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
    fn disconnected_can_batch(&self, scope: &str, revision: u64) -> ProviderBatch {
        let items = self.can_retained.get(scope).into_iter().flatten().cloned().map(|mut item| {
            item.route = None;
            item.ports = None;
            item.diagnostics.push("Gateway is disconnected; this module's settings and controls are inactive.".into());
            item
        }).collect();
        let mut observed = batch(scope, &self.session, revision, items);
        // Absence revokes the last successful native routes while retaining
        // their identity and port contracts. It never removes remembered
        // modules, including resources restored before this collector started.
        observed.complete = false;
        observed
    }
    fn can_observations(
        &self,
        gateway: &str,
        state: &Value,
        verified: &BTreeMap<String, (String, u32)>,
    ) -> Result<Vec<Observation>, String> {
        let mut observed = Vec::new();
        let modules = state["modules"].as_array().ok_or("Gateway module inventory is unavailable; retained resources are preserved.")?;
        for module in modules {
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
            if let Some(publication) = crate::profile_session_overrides::advertised(state, module) {
                item.attributes.insert("controllerPublicationOverride".into(), publication.to_string());
            }
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
                let mut route_attributes = attributes([
                    ("gatewayId", gateway.into()),
                    ("moduleId", id.into()),
                    ("boot", module["boot"].to_string()),
                    ("node", module["node"].to_string()),
                ]);
                if actions_current {
                    // Publication may renew a CAN delivery lease, but it cannot
                    // authorize a different action catalogue with equal ports.
                    let catalogue = serde_json::to_vec(&json!({
                        "revision": actions["catalogueRevision"],
                        "actions": actions["actions"],
                        "ownership": actions["ownership"],
                    }))
                    .map_err(|error| error.to_string())?;
                    route_attributes.insert(
                        "actionCatalogueHash".into(),
                        format!("{:x}", Sha256::digest(catalogue)),
                    );
                }
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
                    attributes: route_attributes,
                    baseline_ready: events_current,
                    writable: !identity_conflict
                        && module["canConfigure"] == true
                        && actions["status"] != "uncertain",
                    priority: 50,
                });
                let route = item.route.as_ref().unwrap();
                item.attributes.insert(osg_profiles::compiler::hid_policy::attribute(&route.id),
                    osg_profiles::compiler::hid_policy::evidence(route,
                        hid_publication::can_status(gateway, state, module)));
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

// The ignored browser proof uses the production projection, not a fabricated
// registry descriptor or token. This exposes nothing in the production build.
#[cfg(test)]
pub(crate) fn companion_observation(
    source: osg_mapping_host::companions::ResourceDescriptor,
    publication: Result<osg_companion_service::Publication, String>,
    transport: Option<osg_companion_service::SourceTransport>,
) -> Result<Observation, String> {
    Sources::companion_observation(source, publication, transport)
}
