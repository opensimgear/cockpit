//! DCS direct-command admission checks current native publishers independently
//! of controller capture tokens: publication can change without an event gap.
use super::*;
use crate::profile_resource_sources::hid_publication;
use osg_profiles::compiler::CompileSnapshot;
use serde_json::{json, Value};

const LOST: &str = "An OSG input's direct or gateway HID publication changed or is unverified; the DCS command branch was withdrawn. Apply a fresh profile after disabling every publisher.";
const BUSY: &str = "DCS command admission is waiting for current HID publication proof. Commands during this interval are rejected and are not replayed.";
type Snapshots = (CompileSnapshot, Vec<(String, Value)>, Vec<crate::module_usb_fleet::SlotSnapshot>);

pub(super) fn append_diagnostics(
    retained: &mut std::collections::BTreeSet<String>,
    messages: impl IntoIterator<Item = String>,
) {
    retained.extend(messages);
}
pub(super) fn unique_diagnostics(messages: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut retained = std::collections::BTreeSet::new();
    append_diagnostics(&mut retained, messages);
    retained.into_iter().collect()
}

#[derive(Clone, Default)]
pub(super) struct Guard {
    // Collected even for dormant DCS branches, then enforced only when that
    // immutable branch is admitted. Current inventory cannot erase obligations.
    branches: Vec<(
        osg_profiles::Endpoint,
        BTreeMap<String, (String, Option<String>)>,
    )>,
    revoked: BTreeMap<(String, String), String>,
    pending: Vec<osg_profiles::Endpoint>,
    withdrawals: BTreeMap<(String, String), std::collections::BTreeSet<String>>,
    withdrawal_retry: Option<Instant>,
}
impl Guard {
    pub(super) fn capture(plan: &ExecutionPlan, snapshot: &CompileSnapshot) -> Self {
        let mut result = Self::default();
        for branch in &plan.branches {
            let capture = |logical: &str| {
                snapshot
                    .assignments
                    .iter()
                    .find(|a| a.resource_id == logical)
                    .and_then(|a| {
                        snapshot
                            .resources
                            .iter()
                            .find(|r| r.summary.id == a.source_id)
                    })
            };
            if !plan.sinks.iter().any(|sink| {
                sink.endpoint == branch.sink && sink.adapter == SinkAdapter::SimulatorCommand
            }) || !capture(&branch.sink.node_id)
                .is_some_and(|r| r.game_id.as_deref() == Some("dcs"))
            {
                continue;
            }
            let sources = branch
                .source_ports
                .iter()
                .filter_map(|endpoint| {
                    let resource = capture(&endpoint.node_id).filter(|r| {
                        r.summary.resource_type == osg_profiles::resources::ResourceType::OsgModule
                    })?;
                    Some((
                        endpoint.node_id.clone(),
                        (
                            resource.summary.id.clone(),
                            resource.summary.attributes.get("moduleId").cloned(),
                        ),
                    ))
                })
                .collect();
            result.branches.push((branch.sink.clone(), sources));
        }
        result
    }
    fn required(&self, plan: &ExecutionPlan) -> BTreeMap<String, (String, Option<String>)> {
        self.branches
            .iter()
            .filter(|(sink, _)| {
                plan.branches
                    .iter()
                    .any(|b| b.sink == *sink && b.readiness == PlanReadiness::Ready)
                    && plan
                        .sinks
                        .iter()
                        .any(|s| s.endpoint == *sink && s.readiness == PlanReadiness::Ready)
            })
            .flat_map(|(_, sources)| sources.clone())
            .collect()
    }
    fn needs_observation(&self, plan: &ExecutionPlan) -> bool {
        self.branches.iter().any(|(sink, sources)| {
            !sources.is_empty()
                && !self
                    .revoked
                    .contains_key(&(sink.node_id.clone(), sink.port_id.clone()))
                && plan
                    .branches
                    .iter()
                    .any(|b| b.sink == *sink && b.readiness == PlanReadiness::Ready)
                && plan
                    .sinks
                    .iter()
                    .any(|s| s.endpoint == *sink && s.readiness == PlanReadiness::Ready)
        })
    }
    pub(super) fn withdrawal_deadline(&self) -> Option<Instant> {
        self.withdrawal_retry
    }
    fn flush_withdrawals(&mut self, resources: &crate::profile_resources::Runtime) -> Vec<String> {
        let mut diagnostics = self.diagnostics();
        let mut busy = false;
        self.withdrawals.retain(|(game, reader), ports| {
            let ports = ports.iter().cloned().collect::<Vec<_>>();
            match resources.try_withdraw_profile_game_commands(game, reader, &ports) {
                Ok(true) => false,
                Ok(false) => {
                    busy = true;
                    true
                }
                Err(error) => {
                    // The private latch is already closed. Never modify a replacement reader.
                    diagnostics.push(error);
                    false
                }
            }
        });
        self.withdrawal_retry = busy.then(|| Instant::now() + std::time::Duration::from_millis(5));
        diagnostics
    }
    fn diagnostics(&self) -> Vec<String> {
        unique_diagnostics(self.revoked.values().cloned().chain(
            (!self.pending.is_empty()).then(|| BUSY.to_owned()),
        ))
    }
    fn observe(
        &mut self,
        plan: &ExecutionPlan,
        current: &Result<Option<Snapshots>, String>,
        mut owned: impl FnMut(&str, &str, &Value) -> Result<Option<()>, String>,
    ) -> Vec<osg_profiles::Endpoint> {
        let active = self
            .branches
            .iter()
            .filter(|(sink, sources)| {
                !sources.is_empty()
                    && plan
                        .branches
                        .iter()
                        .any(|b| b.sink == *sink && b.readiness == PlanReadiness::Ready)
                    && plan
                        .sinks
                        .iter()
                        .any(|s| s.endpoint == *sink && s.readiness == PlanReadiness::Ready)
            })
            .cloned()
            .collect::<Vec<_>>();
        self.pending.clear();
        let mut newly_lost = vec![];
        for branch in active {
            let key = (branch.0.node_id.clone(), branch.0.port_id.clone());
            if self.revoked.contains_key(&key) {
                continue;
            }
            let single = Self {
                branches: vec![branch.clone()],
                ..Default::default()
            };
            let mut pending = false;
            let result = match current {
                Ok(Some((snapshot, gateways, usb))) => {
                    validate_snapshots(&single, plan, snapshot, gateways, usb, |g, m, state| {
                        pending |= owned(g, m, state)?.is_none();
                        Ok(())
                    })
                }
                Ok(None) => { pending = true; Ok(()) }
                Err(error) => Err(error.clone()),
            };
            if let Err(error) = result {
                self.revoked.insert(key, error);
                newly_lost.push(branch.0);
            } else if pending {
                self.pending.push(branch.0);
            }
        }
        newly_lost
    }
    pub(super) fn blocked(&self, endpoint: &osg_mapping::program::Endpoint) -> bool {
        self.revoked
            .contains_key(&(endpoint.node_id.clone(), endpoint.port_id.clone()))
            || self.pending.iter().any(|pending| {
                pending.node_id == endpoint.node_id && pending.port_id == endpoint.port_id
            })
    }
}

pub(super) fn validate(
    state: &NativeRun,
    resources: &crate::profile_resources::Runtime,
    panels: &crate::gateways::ProfilePanelIo,
) -> Result<(), String> {
    let Some(plan) = &state.delivery else {
        return Ok(());
    };
    if state.dcs_hid.required(plan).is_empty() {
        return Ok(());
    }
    if let Some(error) = state.dcs_hid.revoked.values().next() {
        return Err(error.clone());
    }
    let snapshot = resources.snapshot_for_compile(state.assignments.clone())?;
    validate_snapshots(
        &state.dcs_hid,
        plan,
        &snapshot,
        &panels.publication_snapshots()?,
        &panels.usb_snapshots(),
        |gateway, module, _| {
            let proof = state.publication.verified(false)?;
            proof.validate()?;
            proof
                .targets()
                .iter()
                .any(|t| t.gateway == gateway && t.module == module && !t.enabled)
                .then_some(())
                .ok_or_else(|| LOST.to_owned())
        },
    )
}

/// Permanently withdraw affected command sinks without stopping their game
/// reader, disarming telemetry sources, or changing unrelated controller work.
pub(super) fn refresh(
    state: &mut NativeRun,
    resources: &crate::profile_resources::Runtime,
    panels: &crate::gateways::ProfilePanelIo,
) -> Vec<String> {
    let Some(plan) = state.delivery.as_ref() else {
        state.dcs_hid.pending.clear();
        return state.dcs_hid.flush_withdrawals(resources);
    };
    if !state.dcs_hid.needs_observation(plan) {
        state.dcs_hid.pending.clear();
        return state.dcs_hid.flush_withdrawals(resources);
    }
    let current = (|| {
        let Some(snapshot) = resources.try_snapshot_for_compile_with_companions(state.assignments.clone(), &[])? else {
            return Ok(None);
        };
        let Some(gateways) = panels.try_publication_snapshots()? else { return Ok(None) };
        let Some(usb) = panels.try_usb_snapshots() else { return Ok(None) };
        Ok(Some((snapshot, gateways, usb)))
    })();
    let publication = &state.publication;
    let newly_lost = state.dcs_hid.observe(plan, &current, |gateway, module, current| {
        publication.try_verify_disabled(gateway, module, current, panels)
    });
    for endpoint in &state.dcs_hid.pending {
        state.game.cancel_pending_command(endpoint);
    }
    if !newly_lost.is_empty() {
        state.game.revoke_commands(&newly_lost);
        let ports = plan
            .sinks
            .iter()
            .filter(|s| newly_lost.contains(&s.endpoint))
            .filter_map(|s| s.observed_port_id.clone())
            .collect::<Vec<_>>();
        if let Some((game, reader)) = state.game.provider_identity() {
            state.dcs_hid.withdrawals.entry((game.into(), reader.into())).or_default().extend(ports);
        }
    }
    state.dcs_hid.flush_withdrawals(resources)
}

fn validate_snapshots(
    guard: &Guard,
    plan: &ExecutionPlan,
    snapshot: &CompileSnapshot,
    gateways: &[(String, Value)],
    usb: &[crate::module_usb_fleet::SlotSnapshot],
    mut owned: impl FnMut(&str, &str, &Value) -> Result<(), String>,
) -> Result<(), String> {
    for (logical, (inventory, captured_module)) in guard.required(plan) {
        let resource = snapshot
            .assignments
            .iter()
            .find(|a| a.resource_id == logical)
            .and_then(|a| {
                snapshot
                    .resources
                    .iter()
                    .find(|r| r.summary.id == a.source_id)
            })
            .ok_or(LOST)?;
        let module = resource.summary.attributes.get("moduleId").ok_or(LOST)?;
        if resource.summary.id != inventory
            || resource.summary.resource_type != osg_profiles::resources::ResourceType::OsgModule
            || captured_module.as_ref() != Some(module)
        {
            return Err(LOST.into());
        }
        let selected = plan
            .sources
            .iter()
            .filter(|source| source.endpoint.node_id == logical)
            .collect::<Vec<_>>();
        if selected.is_empty()
            || selected.iter().any(|source| {
                source.inventory_resource_id.as_deref() != Some(inventory.as_str())
                    || source.route.as_ref().is_none_or(|route| {
                        !resource
                            .summary
                            .connections
                            .iter()
                            .any(|c| c.route == *route)
                    })
            })
        {
            return Err(LOST.into());
        }
        if resource.summary.connections.is_empty() {
            return Err(LOST.into());
        }
        for connection in &resource.summary.connections {
            let route = &connection.route;
            match route.transport {
                Transport::Can if connection.scope.starts_with("gateway:") => {
                    let gateway = route.attributes.get("gatewayId").ok_or(LOST)?;
                    let current = gateways
                        .iter()
                        .find(|(id, _)| id == gateway)
                        .map(|(_, s)| s)
                        .ok_or(LOST)?;
                    let m = current["modules"]
                        .as_array()
                        .and_then(|items| items.iter().find(|m| m["id"] == *module))
                        .ok_or(LOST)?;
                    if route.id != format!("can:{gateway}:{module}")
                        || route.attributes.get("moduleId") != Some(module)
                        || route.provider_generation
                            != json!([
                                current["epoch"],
                                m["boot"],
                                m["lease"],
                                m["configurationGeneration"],
                                crate::profile_resource_ports::can_stream_generation(current, m)
                            ])
                            .to_string()
                    {
                        return Err(LOST.into());
                    }
                    match hid_publication::can_status(gateway, current, m) {
                        "disabled" => {}
                        "lease-disabled" => owned(gateway, module, current)?,
                        _ => return Err(LOST.into()),
                    }
                }
                Transport::Usb if connection.scope == "module-usb" => {
                    let slot = usb
                        .iter()
                        .find(|slot| route.id == format!("usb:{}", slot.key))
                        .ok_or(LOST)?;
                    let token = slot.snapshot.route.as_ref().ok_or(LOST)?;
                    if token.module_id != *module
                        || hid_publication::usb_status(slot) != "disabled"
                        || route.provider_generation
                            != format!(
                                "{}:{}:{}",
                                token.connection_generation,
                                token.boot,
                                token.configuration_generation
                            )
                    {
                        return Err(LOST.into());
                    }
                }
                _ => return Err(LOST.into()),
            }
        }
        // A newly attached publisher must not hide behind unchanged selected
        // input tokens or a still-unrefreshed aggregate inventory connection.
        for (gateway, current) in gateways {
            for m in current["modules"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|m| {
                    current["connected"] == true && m["online"] == true && m["id"] == *module
                })
            {
                if !resource
                    .summary
                    .connections
                    .iter()
                    .any(|c| c.route.id == format!("can:{gateway}:{module}"))
                    || !matches!(
                        hid_publication::can_status(gateway, current, m),
                        "disabled" | "lease-disabled"
                    )
                {
                    return Err(LOST.into());
                }
            }
        }
        if usb
            .iter()
            .filter(|s| {
                s.present
                    && s.snapshot.connected
                    && s.snapshot
                        .identity
                        .as_ref()
                        .is_some_and(|i| i.module_id == *module)
            })
            .any(|s| {
                !resource
                    .summary
                    .connections
                    .iter()
                    .any(|c| c.route.id == format!("usb:{}", s.key))
                    || hid_publication::usb_status(s) != "disabled"
            })
        {
            return Err(LOST.into());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "profile_session_dcs_hid_tests.rs"]
pub(super) mod tests;
