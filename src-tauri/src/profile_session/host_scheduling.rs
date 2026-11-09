//! Qualify capture-driven service only when every active adapter can wake or
//! has an explicit deadline. Other adapters keep their existing service bound.
use super::*;

pub(super) fn hold_uses_notifications(host: &NativeProfileHost, state: &NativeRun) -> bool {
    // Native HID reports and catalogue revocation wake the host. A neutral
    // virtual feeder wakes on failure and renews at its retained deadline.
    // Desktop registration starts only after the reducer is installed.
    cfg!(windows) && host.native && host.native_wake.is_some() && host.input_wake.is_some()
        && state.reducer.is_none() && !state.launch_context_installed
        && state.inputs.as_ref().is_some_and(|inputs| inputs.notification_driven())
        && state.panels.notification_driven() && state.companions.notification_driven()
        && state.publication.notification_driven()
        && state.output.as_ref().is_none_or(|output| output.renewal_deadline().is_some())
}

fn document_supported(document: &osg_profiles::ProfileDocument, watched_game: bool) -> bool {
    document.resources.iter().all(|resource| matches!(resource.kind,
        osg_profiles::ResourceKind::DebugController | osg_profiles::ResourceKind::Device
            | osg_profiles::ResourceKind::Companion)
        || watched_game && resource.kind == osg_profiles::ResourceKind::Game)
        && (watched_game || document.vehicle_tabs.is_empty())
        && (watched_game || document.eligibility.game_ids.is_empty())
}

pub(super) fn capture_driven(host: &NativeProfileHost, state: &NativeRun, run: &OwnedRun) -> bool {
    let game = match state.child_game_owner.as_ref() {
        Some(child) if state.launch_context_installed && run.owner.game_id.is_none()
            && game_child_matches_run(child, &run.owner) => child.game_id.as_deref(),
        Some(_) => return false,
        None => run.owner.game_id.as_deref(),
    };
    let watched_game = game.is_some() && state.game.watched_game_source()
        && state.game.provider_identity().is_some_and(|(provider, _)| Some(provider) == game)
        && state.game_preparation.as_ref().is_some_and(|prepared| Some(prepared.proof.game_id.as_str()) == game)
        && !state.game_publication.deferred;
    // DCS command completion and failure wake the same host as telemetry.
    let watched_dcs = watched_game && state.game_preparation.as_ref()
        .is_some_and(|prepared| prepared.integration_key == "dcs");
    cfg!(windows) && host.native && host.native_wake.is_some() && host.input_wake.is_some()
        && state.owner == run.owner && !state.vehicle_paused
        && state.reducer.as_ref().is_some_and(|reducer| reducer.active() && reducer.fault().is_none())
        && document_supported(&state.source_document, watched_game) && document_supported(&state.document, watched_game)
        && state.inputs.as_ref().is_some_and(|inputs| inputs.notification_driven())
        && state.panels.notification_driven()
        && state.delivery.as_ref().is_some_and(|plan| {
            plan.sources.iter().all(|source| {
                state.document.resources.iter().any(|resource| resource.id == source.endpoint.node_id
                    && resource.kind == osg_profiles::ResourceKind::DebugController)
                    || source.readiness == PlanReadiness::Ready && source.token.is_some()
                        && source.route.as_ref().is_some_and(|route| route.transport == Transport::Hid
                            || watched_game && route.transport == Transport::Integration
                                && route.attributes.get("gameId").map(String::as_str)
                                    .or_else(|| route.id.strip_prefix("game:")) == game
                            || state.companions.has_resource(&source.endpoint.node_id)
                                && matches!(route.transport, Transport::Network | Transport::Usb)
                            || matches!(route.transport, Transport::Usb | Transport::Can)
                                && state.panels.owns_active_source(source))
            }) && plan.sinks.iter().all(|sink| matches!(sink.adapter,
                SinkAdapter::VirtualController | SinkAdapter::DesktopInput)
                || sink.adapter == SinkAdapter::SimulatorCommand && watched_dcs
                || sink.adapter == SinkAdapter::CompanionFeedback
                    && state.companions.has_resource(&sink.endpoint.node_id)
                || sink.adapter == SinkAdapter::PhysicalAction
                    && state.actions.owns_endpoint(&sink.endpoint))
        })
        && (watched_game || game.is_none() && state.game_preparation.is_none())
        && state.game_resume.is_none()
        && desktop_notifications(state) && state.actions.notification_driven() && state.publication.notification_driven()
        && state.companions.notification_driven()
}

pub(super) fn game_child_matches_run(child: &crate::game_context::ProfileOwner, owner: &RunOwner) -> bool {
    child.session == owner.session
        && child.profile_id == owner.profile_id
        && child.applied_revision == owner.profile_revision
        && child.run_generation == owner.generation
        && child.game_id.is_some()
}

fn desktop_notifications(state: &NativeRun) -> bool {
    if state.document.desktop_outputs.is_empty() && state.source_document.desktop_outputs.is_empty() {
        state.desktop.is_none()
    } else if NativeProfileHost::context_desktop(&state.document)
        || NativeProfileHost::context_desktop(&state.source_document) {
        state.desktop.as_ref().is_some_and(|desktop| desktop.context_notification_driven())
    } else {
        state.desktop.as_ref().is_some_and(|desktop| desktop.notification_driven())
    }
}

/// A bounded output drain must schedule its remaining ordered work immediately.
/// Lease renewal is independent and remains anchored to its last send time.
pub(super) fn output_deadline(cursor: Counter, recorded: Counter, renewal: Option<Instant>) -> Option<Instant> {
    if cursor.0 < recorded.0 { Some(Instant::now()) } else { renewal }
}

#[cfg(test)]
#[path = "host_scheduling/tests.rs"]
mod tests;
