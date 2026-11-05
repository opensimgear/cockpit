//! Qualify capture-driven service only when every active adapter can wake or
//! has an explicit deadline. Other adapters keep their existing service bound.
use super::*;

fn document_supported(document: &osg_profiles::ProfileDocument) -> bool {
    document.resources.iter().all(|resource| matches!(resource.kind,
        osg_profiles::ResourceKind::DebugController | osg_profiles::ResourceKind::Device
            | osg_profiles::ResourceKind::Companion))
        && document.visibility.is_empty()
        && document.overrides.is_empty()
        && document.vehicle_tabs.is_empty() && document.eligibility.game_ids.is_empty()
}

pub(super) fn capture_driven(host: &NativeProfileHost, state: &NativeRun, run: &OwnedRun) -> bool {
    cfg!(windows) && host.native && host.native_wake.is_some() && host.input_wake.is_some()
        && state.owner == run.owner && !state.vehicle_paused
        && state.reducer.as_ref().is_some_and(|reducer| reducer.active() && reducer.fault().is_none())
        && document_supported(&state.source_document) && document_supported(&state.document)
        && state.inputs.as_ref().is_some_and(|inputs| inputs.notification_driven())
        && state.delivery.as_ref().is_some_and(|plan| {
            plan.sources.iter().all(|source| {
                state.document.resources.iter().any(|resource| resource.id == source.endpoint.node_id
                    && resource.kind == osg_profiles::ResourceKind::DebugController)
                    || source.readiness == PlanReadiness::Ready && source.token.is_some()
                        && source.route.as_ref().is_some_and(|route| route.transport == Transport::Hid
                            || state.companions.has_resource(&source.endpoint.node_id)
                                && matches!(route.transport, Transport::Network | Transport::Usb))
            }) && plan.sinks.iter().all(|sink| matches!(sink.adapter,
                SinkAdapter::VirtualController | SinkAdapter::DesktopInput)
                || sink.adapter == SinkAdapter::CompanionFeedback
                    && state.companions.has_resource(&sink.endpoint.node_id))
        })
        && run.owner.game_id.is_none() && state.game_preparation.is_none()
        && state.child_game_owner.is_none() && state.game_resume.is_none()
        && desktop_notifications(state) && state.actions.is_empty() && state.publication.is_empty()
        && state.visibility.is_empty() && state.companions.notification_driven()
}

fn desktop_notifications(state: &NativeRun) -> bool {
    if state.document.desktop_outputs.is_empty() && state.source_document.desktop_outputs.is_empty() {
        state.desktop.is_none()
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
