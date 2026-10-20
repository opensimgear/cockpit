//! Native-only companion preparation and compare/restore journal policy.
use super::*;
use crate::profile_resources::CompanionCatalogue;
use crate::profile_session_companions::{CompanionPoll, CompanionSources, PreparedCompanion};
use osg_companion_service::{profile::AssignmentReceipt, Hub};
use osg_profiles::compiler_models::CompileSnapshot;
use serde_json::Value;
use std::collections::BTreeSet;

const ASSIGNMENT: &str = "companion-assignment";
const RELEASE: &str = "companion-release";
// USB companions may need to transfer panel images, persist them, render the
// replacement, and confirm released touch before a neutral baseline exists.
// Keep this bounded while renewing neutral outputs and checking cancellation.
const PANEL_LOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
const PANEL_LOAD_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const PANEL_LOAD_LEGACY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

fn panel_operation_failure(operation: &Value) -> Option<String> {
    if operation["phase"] != "failed" { return None; }
    let code = operation["errorCode"].as_str().unwrap_or("panel-load-failed");
    let message = operation["message"].as_str().unwrap_or("The screen could not finish loading the assigned panel");
    let action = if operation["retryable"] == true { "Recover outputs, then retry activation." }
        else { "Check the panel and companion before retrying activation." };
    Some(format!("Companion panel failed: {message} ({code}). {action}"))
}
fn panel_operation_progress(operation: &Value) -> Value {
    json!([operation["sessionId"], operation["attemptId"], operation["phase"], operation["completedBytes"], operation["totalBytes"]])
}
fn panel_operation_status(operation: &Value) -> (&'static str, String) {
    let (step, message) = match operation["phase"].as_str() {
        Some("opening") => ("companion-opening", "Companion opening panel"),
        Some("checking-cache") => ("companion-checking-cache", "Companion checking cached assets"),
        Some("downloading") => ("companion-downloading-assets", "Companion downloading assets"),
        Some("rendering") => ("companion-rendering", "Companion rendering panel"),
        Some("awaiting-neutral") => ("companion-awaiting-neutral", "Waiting for companion touch controls to be released"),
        _ => ("companion-baseline", "Checking companion input readiness"),
    };
    let total = operation["totalBytes"].as_u64().unwrap_or(0);
    let completed = operation["completedBytes"].as_u64().unwrap_or(0);
    let message = if operation["phase"] == "downloading" && total > 0 {
        format!("{message} · {} / {} KiB", completed / 1024, total.div_ceil(1024))
    } else { message.into() };
    (step, message)
}
fn diagnostic_failure(report: &crate::companion_host::ProfilePanelOperation, initial: Option<&Value>) -> Option<String> {
    // A retained failure is useful context, but cannot reject an explicit
    // retry before the device has reported a new attempt or state transition.
    if !report.fresh || (!report.live_session && initial.is_none_or(|initial|initial == &report.operation)) { return None; }
    panel_operation_failure(&report.operation)
}
fn panel_load_timeout(operation: Option<&Value>, hard_limit: bool) -> String {
    let reason = if hard_limit { "exceeded the five-minute loading limit" } else { "stopped making progress" };
    match operation {
        Some(operation) => format!("The companion {reason} during {} ({} of {} image bytes). {}",
            operation["phase"].as_str().unwrap_or("panel loading"),
            operation["completedBytes"].as_u64().unwrap_or(0), operation["totalBytes"].as_u64().unwrap_or(0),
            if operation["phase"] == "awaiting-neutral" { "Release the screen's touch controls, recover outputs, and retry." }
            else { "Check its connection, recover outputs, and retry activation." }),
        None => "The assigned companion did not finish loading its panel and supply its actual publication, capabilities and fresh neutral baseline within 60 seconds. Check its connection and firmware, recover outputs, and retry activation.".into(),
    }
}

pub(super) fn prepare_catalogues(
    hub: &Hub,
    snapshot: &CompileSnapshot,
    candidate: &Candidate,
) -> Result<(Vec<CompanionCatalogue>, Vec<PreparedCompanion>), String> {
    let mut catalogues = vec![];
    let mut bindings = vec![];
    let mut selected = BTreeSet::new();
    for desired in &candidate.document.companion_assignments {
        let assignment = snapshot
            .assignments
            .iter()
            .find(|a| a.resource_id == desired.resource_id)
            .ok_or("Assign the companion resource to its actual paired native instance first.")?;
        let resource = snapshot
            .resources
            .iter()
            .find(|r| {
                r.summary.id == assignment.source_id
                    && r.summary.resource_type == osg_profiles::resources::ResourceType::Companion
            })
            .ok_or("The selected native companion is unavailable.")?;
        let instances = resource
            .summary
            .connections
            .iter()
            .filter(|c| matches!(c.route.transport, Transport::Usb | Transport::Network)
                || (c.route.transport==Transport::Integration && resource.summary.attributes.get("componentKind").is_some_and(|kind|kind=="paired-companion")
                    && c.route.attributes.get("preparationOnly").is_some_and(|kind|kind=="paired-companion")))
            .filter_map(|c| c.route.id.strip_prefix("companion:"))
            .collect::<BTreeSet<_>>();
        if instances.len() != 1 {
            return Err("Connect the exact paired companion through one verified native route before applying its panel.".into());
        }
        let instance = *instances.iter().next().unwrap();
        if !selected.insert(instance.to_owned()) {
            return Err("A profile cannot assign the same companion instance twice.".into());
        }
        let proof = hub.prepare_profile_assignment(instance, &desired.publication_hash)?;
        if desired.panel_id.as_ref().is_some_and(|panel_id|!hub.publication_summaries().iter().any(|panel|
            panel["publicationHash"]==proof.publication_hash()&&panel["uiId"]==*panel_id)) {
            return Err("The selected companion panel identity does not match its saved view. Select the panel again before activating this profile.".into());
        }
        if resource.summary.attributes.get("companionId").is_some_and(|companion|companion!=proof.companion_id()) {
            return Err("The paired companion changed its native installation identity.".into());
        }
        let definition = osg_mapping_host::companions::native_definition(proof.definition())?;
        catalogues.push(CompanionCatalogue {
            logical_resource_id: desired.resource_id.clone(),
            inventory_resource_id: assignment.source_id.clone(),
            instance_id: instance.into(),
            publication_hash: proof.publication_hash().into(),
            capability_fingerprint: proof.capability_fingerprint().into(),
            ports: crate::profile_resource_ports::companion(&definition, proof.feedback())?
                .into_iter()
                .filter(|port| {
                    assignment.ports.iter().any(|expected| {
                        port.id == expected.observed_port_id
                            && port.contract_hash().ok().as_ref()
                                == Some(&expected.expected_contract_hash)
                    })
                })
                .collect(),
        });
        bindings.push(PreparedCompanion {
            resource_id: desired.resource_id.clone(),
            proof,
        });
    }
    Ok((catalogues, bindings))
}

fn decode(receipt: &Receipt) -> Result<Option<(bool, AssignmentReceipt)>, String> {
    if !matches!(receipt.kind.as_str(), ASSIGNMENT | RELEASE) {
        return Ok(None);
    }
    let captured: AssignmentReceipt = serde_json::from_value(receipt.state.clone())
        .map_err(|e| format!("The companion recovery receipt is malformed: {e}"))?;
    if captured.instance_id() != receipt.resource_id {
        return Err("The companion recovery receipt changed its instance identity.".into());
    }
    Ok(Some((receipt.kind == RELEASE, captured)))
}
pub(super) fn receipts(receipts: &[Receipt]) -> Result<Vec<(bool, AssignmentReceipt)>, String> {
    let mut result = vec![];
    let mut instances = BTreeSet::new();
    for receipt in receipts {
        if let Some(row) = decode(receipt)? {
            if !instances.insert(row.1.instance_id().to_owned()) {
                return Err(
                    "The native companion journal contains duplicate instance ownership.".into(),
                );
            }
            result.push(row);
        }
    }
    Ok(result)
}
pub(super) fn has_receipts(receipts: &[Receipt]) -> bool {
    receipts
        .iter()
        .any(|r| matches!(r.kind.as_str(), ASSIGNMENT | RELEASE))
}
pub(super) fn capture(
    hub: &Hub,
    sources: &CompanionSources,
    previous: &[Receipt],
) -> Result<Vec<Receipt>, String> {
    let previous = receipts(previous)?
        .into_iter()
        .filter_map(|(release, receipt)| {
            (!release && hub.validate_profile_assignment_owner(&receipt).is_ok()).then_some(receipt)
        })
        .collect::<Vec<_>>();
    let selected = sources.capture(hub, &previous)?;
    let mut result = selected
        .iter()
        .map(|receipt| {
            Ok(Receipt {
                kind: ASSIGNMENT.into(),
                resource_id: receipt.instance_id().into(),
                state: serde_json::to_value(receipt).map_err(|e| e.to_string())?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    for receipt in previous {
        if !selected
            .iter()
            .any(|r| r.instance_id() == receipt.instance_id())
        {
            result.push(Receipt {
                kind: RELEASE.into(),
                resource_id: receipt.instance_id().into(),
                state: serde_json::to_value(receipt).map_err(|e| e.to_string())?,
            });
        }
    }
    Ok(result)
}
#[cfg(test)]
pub(super) fn restore(
    hub: &Hub,
    captured: &[(bool, AssignmentReceipt)],
    release_only: bool,
) -> Result<(), String> {
    restore_with_heads(hub,captured,release_only,&Default::default())
}
pub(super) fn restore_latest(
    runtime:&crate::companion_host::Runtime,
    captured:&[(bool,AssignmentReceipt)],
    release_only:bool,
) -> Result<(),String> {
    let heads=runtime.saved_panel_heads()?;
    restore_with_heads(&runtime.profile_metadata()?,captured,release_only,&heads)
}
fn restore_with_heads(
    hub:&Hub,
    captured:&[(bool,AssignmentReceipt)],
    release_only:bool,
    heads:&std::collections::BTreeMap<String,String>,
) -> Result<(),String> {
    for (release, receipt) in captured {
        if !release_only || *release {
            // Exact protected owner comparison preserves operator edits and a
            // newer run before choosing the original panel's latest saved view.
            hub.restore_profile_assignment_latest(receipt,heads)?;
        }
    }
    Ok(())
}
fn admitted(snapshot: &CompileSnapshot, catalogue: &CompanionCatalogue) -> bool {
    snapshot.resources.iter().any(|r| {
        r.summary.id == catalogue.inventory_resource_id
            && r.summary.attributes.get("publicationHash") == Some(&catalogue.publication_hash)
            && r.summary.attributes.get("capabilityFingerprint")
                == Some(&catalogue.capability_fingerprint)
            && r.summary.connections.iter().any(|c| {
                c.route.id == format!("companion:{}", catalogue.instance_id)
                    && c.route.baseline_ready
            })
    })
}
fn admitted_current(
    snapshot: &CompileSnapshot,
    catalogue: &CompanionCatalogue,
    native: &[osg_mapping_host::companions::ResourceDescriptor],
) -> bool {
    let Some(actual) = native.iter().find(|source| {
        source.definition.source_id == catalogue.instance_id
            && source.definition.publication_hash == catalogue.publication_hash
            && source.online
            && source.baseline_ready
            && source.generation > 0
    }) else {
        return false;
    };
    admitted(snapshot, catalogue)
        && snapshot.resources.iter().any(|resource| {
            resource.summary.id == catalogue.inventory_resource_id
                && resource.summary.connections.iter().any(|connection| {
                    connection.route.id == format!("companion:{}", catalogue.instance_id)
                        && connection.route.provider_generation == actual.generation.to_string()
                })
        })
}

impl NativeProfileHost {
    pub(super) fn companion_baseline(
        &mut self,
        owner: &RunOwner,
        prepared: &ExecutionPlan,
    ) -> Result<(ExecutionPlan, Vec<osg_mapping::engine::typed::Frame>), String> {
        if self.run(owner)?.companions.is_empty() {
            return Ok((self.resources.bind_delivery_reads(prepared)?, vec![]));
        }
        self.report_progress(owner, "companion-baseline", "Waiting for companion panel readiness");
        let callback = self.progress.clone();
        let mut displayed_progress = None;
        let hub = self.companions.profile_metadata()?;
        let runtime = self.companions.clone();
        let provider = self.companions.profile_provider();
        let resources = self.resources.clone();
        let deadline = Instant::now() + PANEL_LOAD_TIMEOUT;
        let legacy_deadline = Instant::now() + PANEL_LOAD_LEGACY_TIMEOUT;
        let mut initial_operations = self.run(owner)?.companion_initial_operations.clone();
        let mut progress = std::collections::BTreeMap::<String, (Value, Instant, Value)>::new();
        let mut live_seen = BTreeSet::new();
        let mut latest_operation = None;
        loop {
            self.check_transition(owner)?;
            if let Some(output) = self.run(owner)?.output.as_mut() {
                output.keep_alive()?;
            }
            if let Some(desktop) = self.run(owner)?.desktop.as_mut() { desktop.keep_alive()?; }
            // The independent inventory worker publishes the replacement
            // browser's actual generation. Wait for that cached observation:
            // synchronous discovery here can starve the acquired output lease.
            // A prior generation still cannot authorize the replacement writer.
            let snapshot = resources.snapshot_for_compile(self.run(owner)?.assignments.clone())?;
            let native = provider.resource_inventory();
            let now = self.controllers.stream_page("", 0).captured_us;
            let state = self.run(owner)?;
            let mut current_operations = vec![];
            for catalogue in &state.companion_catalogues {
                let report = runtime.profile_panel_operation(&catalogue.instance_id, &catalogue.publication_hash);
                if let Some(report) = &report {
                    if !report.live_session {
                        initial_operations.entry(catalogue.instance_id.clone()).or_insert_with(||report.operation.clone());
                    }
                    if let Some(error) = diagnostic_failure(report,initial_operations.get(&catalogue.instance_id)) { return Err(error); }
                    if report.live_session { live_seen.insert(catalogue.instance_id.clone()); }
                    if report.fresh && report.operation["phase"] != "failed" {
                        if report.live_session || initial_operations.get(&catalogue.instance_id).is_none_or(|initial| initial != &report.operation) {
                            current_operations.push(panel_operation_status(&report.operation));
                        }
                        let signature = panel_operation_progress(&report.operation);
                        if progress.get(&catalogue.instance_id).is_none_or(|(previous,_,_)|previous != &signature) {
                            progress.insert(catalogue.instance_id.clone(), (signature, Instant::now() + PANEL_LOAD_IDLE_TIMEOUT,report.operation.clone()));
                        }
                        latest_operation = Some(report.operation.clone());
                    }
                }
                if live_seen.contains(&catalogue.instance_id) && report.as_ref().is_none_or(|report|!report.live_session) {
                    return Err("The companion disconnected while loading the assigned panel. Check its connection, recover outputs, and retry activation.".into());
                }
                if let Some((_,until,operation)) = progress.get(&catalogue.instance_id) {
                    if Instant::now() >= *until { return Err(panel_load_timeout(Some(operation),false)); }
                } else if Instant::now() >= legacy_deadline {
                    return Err(panel_load_timeout(None, false));
                }
            }
            // Multiple companions may prepare concurrently. Keep every current
            // operation visible instead of cycling messages on each 5 ms poll.
            if !current_operations.is_empty() {
                let step = current_operations.iter().find(|(step, _)| *step != "companion-baseline")
                    .map_or("companion-baseline", |(step, _)| *step);
                let message = current_operations.iter().map(|(_, message)| message.as_str()).collect::<Vec<_>>().join("; ");
                let next = (step, message);
                if displayed_progress.as_ref() != Some(&next) {
                    Self::emit_progress(callback.as_ref(), state.admission_epoch, next.0, &next.1);
                    displayed_progress = Some(next);
                }
            }
            if state.companions.baseline_ready(&hub)?
                && state
                    .companion_catalogues
                    .iter()
                    .all(|c| admitted_current(&snapshot, c, &native))
            {
                if let Ok(delivery) = resources.bind_delivery_reads(prepared) {
                    state.companions.bind_delivery_plan(&delivery)?;
                    if let Some(frames) = state.companions.baseline(
                        &hub,
                        |token| Self::validate_token(&resources, token),
                        now,
                    ).map_err(|error| format!("The acquired companion baseline rejected its native bindings: {error}"))? {
                        self.check_transition(owner)?;
                        let native = provider.resource_inventory();
                        if self.run(owner)?.companion_catalogues.iter()
                            .all(|catalogue| admitted_current(&snapshot, catalogue, &native)) {
                            return Ok((delivery, frames));
                        }
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(panel_load_timeout(latest_operation.as_ref(), true));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    pub(super) fn companion_poll(
        runtime: &crate::companion_host::Runtime,
        resources: &crate::profile_resources::Runtime,
        state: &mut NativeRun,
        now: u64,
    ) -> CompanionPoll {
        if state.companions.is_empty() {
            return CompanionPoll::default();
        }
        let hub = match runtime.profile_metadata() {
            Ok(hub) => hub,
            Err(error) => return state.companions.fail(now, error),
        };
        let mut poll =
            state
                .companions
                .poll(&hub, |token| Self::validate_token(resources, token), now);
        if state
            .companions
            .needs_rebind(|token| Self::validate_token(resources, token))
            && state
                .companion_retry
                .is_none_or(|last| now.saturating_sub(last) >= 100_000)
        {
            state.companion_retry = Some(now);
            let capture = resources
                .snapshot_for_compile(state.assignments.clone())
                .and_then(|mut snapshot| {
                    let allowed = state
                        .companion_catalogues
                        .iter()
                        .filter(|c| admitted(&snapshot, c))
                        .map(|c| c.logical_resource_id.clone())
                        .collect::<BTreeSet<_>>();
                    snapshot.backend = state.backend.clone();
                    Self::game_capture(&mut snapshot, state.game_preparation.as_ref());
                    let report = osg_profiles::compiler::compile(&state.document, &snapshot);
                    if report.program_hash != state.report.program_hash {
                        return Err(
                            "The companion delivery graph changed; prepare a fresh profile.".into(),
                        );
                    }
                    let delivery = report
                        .plan
                        .ok_or("The companion delivery graph is unavailable.")?;
                    Ok((delivery, allowed))
                });
            match capture {
                Ok((delivery, allowed)) => {
                    let rebound = state.companions.rebind(
                        &hub,
                        &delivery,
                        |logical| allowed.contains(logical),
                        |token| Self::validate_token(resources, token),
                        now,
                    );
                    let replaced=rebound.frames.iter().map(|frame|frame.source.as_str()).collect::<BTreeSet<_>>();
                    poll.liveness.retain(|(source,_)|!replaced.contains(source.as_str()));
                    poll.frames.extend(rebound.frames);
                    poll.diagnostics.extend(rebound.diagnostics);
                }
                Err(error) => poll.diagnostics.push(error),
            }
        }
        poll
    }
}

#[cfg(test)]
#[path = "profile_session_companion_host_tests.rs"]
pub(super) mod tests;
