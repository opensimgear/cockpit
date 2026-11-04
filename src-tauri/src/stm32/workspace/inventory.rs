use serde_json::Value;

/// Fields consumed by the CAN resource projection. Leave sample cursors,
/// values, command receipts and observation timestamps on their own paths.
pub(super) fn inventory_changed(before: &Value, after: &Value) -> bool {
    if fields_changed(before, after, &[
        "connected", "epoch", "gatewaySerial", "gatewayType", "protocolVersion",
        "canProtocol", "panelStreamSupported", "layoutPending", "publicationRevision",
        "controllerPublicationOverride",
    ]) || fields_changed(&before["publication"], &after["publication"], &[
        "phase", "outcome", "expectedReconnect",
    ]) {
        return true;
    }
    match (before["modules"].as_array(), after["modules"].as_array()) {
        (Some(before_modules), Some(after_modules)) => {
            before_modules.len() != after_modules.len()
                || before_modules.iter().zip(after_modules).any(|(old, new)| {
                    fields_changed(old, new, &[
                        "id", "name", "node", "sourceNode", "lease", "boot", "online",
                        "canConfigure", "configurationGeneration", "buttonCount", "axisCount",
                        "neutral", "profile", "directUsb", "published", "publicationSlot",
                        "publicationReportId", "configuredEnabled", "configuredDescriptorIndex",
                    ]) || new["id"].as_str().is_some_and(|id| {
                        fields_changed(&before["moduleEvents"][id], &after["moduleEvents"][id], &[
                            "ok", "status", "route", "configurationGeneration", "stream",
                            "buttonCount", "axisCount", "axisNeutral",
                        ]) || fields_changed(&before["moduleActions"][id], &after["moduleActions"][id], &[
                            "status", "route", "configurationGeneration", "catalogueRevision", "actions",
                            "ownership",
                        ])
                    })
                })
        }
        (None, None) => false,
        _ => true,
    }
}

fn fields_changed(before: &Value, after: &Value, fields: &[&str]) -> bool {
    fields.iter().any(|&field| before[field] != after[field])
}
