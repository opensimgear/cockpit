impl Sources {
    fn disconnected_can_batch(&self, scope: &str, revision: u64) -> ProviderBatch {
        let items = self
            .can_retained
            .get(scope)
            .into_iter()
            .flatten()
            .cloned()
            .map(|mut item| {
                item.route = None;
                item.ports = None;
                item.diagnostics.push(
                    "Gateway is disconnected; this module's settings and controls are inactive."
                        .into(),
                );
                item
            })
            .collect();
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
        let modules = state["modules"]
            .as_array()
            .ok_or("Gateway module inventory is unavailable; retained resources are preserved.")?;
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
                item.attributes.insert(
                    "controllerPublicationOverride".into(),
                    publication.to_string(),
                );
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
                item.attributes.insert(
                    osg_profiles::compiler::hid_policy::attribute(&route.id),
                    osg_profiles::compiler::hid_policy::evidence(
                        route,
                        hid_publication::can_status(gateway, state, module),
                    ),
                );
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