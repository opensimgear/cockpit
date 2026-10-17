//! Configuration route preference uses verified module identity and boot proof.
//! It never moves an existing draft or opens/releases transport ownership.
use crate::module_usb_fleet::SlotSnapshot;
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) const PREFER_USB: &str = "Use Direct USB configuration to start a new setup session for this device. Its current CAN draft was not moved or released.";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RoutePreference {
    pub direct_usb_preferred: bool,
}
/// Process-local ownership metadata. Deliberately has no serialization: public
/// inventory and active configuration snapshots do not grant writer authority.
#[derive(Clone)]
pub(crate) struct WriterPin {
    pub module_id: String,
    pub client_id: String,
    pub boot: u32,
}
pub(crate) fn guard_usb_writer(module_id: &str, can: &[(String, WriterPin)]) -> Result<(), String> {
    if let Some((_, pin)) = can
        .iter()
        .find(|(_, pin)| pin.module_id.eq_ignore_ascii_case(module_id))
    {
        if pin.boot == 0 || pin.client_id.is_empty() {
            return Err("Resolve the existing uncertain CAN configuration writer before starting another setup session.".into());
        }
        return Err("Finish and explicitly release the current CAN configuration session before starting Direct USB configuration. Its draft stays on CAN.".into());
    }
    Ok(())
}
pub(crate) fn guard_can_writer(module_id: &str, usb: &[WriterPin]) -> Result<(), String> {
    if let Some(pin) = usb
        .iter()
        .find(|pin| pin.module_id.eq_ignore_ascii_case(module_id))
    {
        if pin.boot == 0 || pin.client_id.is_empty() {
            return Err("Resolve the existing uncertain USB configuration writer before starting another setup session.".into());
        }
        return Err("Finish and explicitly release the current Direct USB configuration session before starting CAN configuration. Its draft stays on USB.".into());
    }
    Ok(())
}

/// Cached CAN module IDs alone never establish a physical join. A USB module
/// must previously have passed the existing role + factory-derived UID check.
pub(crate) fn evaluate(
    usb: &[SlotSnapshot],
    can: &[(String, Value)],
    module_id: &str,
) -> Result<RoutePreference, String> {
    if module_id.len() != 32 || !module_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Choose a verified module identity before starting configuration.".into());
    }
    let id = module_id.to_ascii_lowercase();
    let proof = usb
        .iter()
        .filter(|slot| verified(slot, &id))
        .collect::<Vec<_>>();
    if proof.is_empty() {
        return Ok(RoutePreference::default());
    }
    if proof.iter().any(|slot| slot.present && !slot.admissible) {
        return Err("Conflicting or uncertain direct USB endpoints block configuration claims. Reconcile this device's physical USB identity first.".into());
    }
    let live_usb = proof
        .iter()
        .filter_map(|slot| {
            let snapshot = &slot.snapshot;
            let route = snapshot.route.as_ref()?;
            (slot.present
                && slot.admissible
                && snapshot.connected
                && route.boot > 0
                && route.connection_generation > 0
                && route.module_id.eq_ignore_ascii_case(&id)
                && route
                    .serial
                    .eq_ignore_ascii_case(&snapshot.identity.as_ref()?.serial))
            .then_some((*slot, route))
        })
        .collect::<Vec<_>>();
    if live_usb.len() > 1 {
        return Err("Several live direct USB routes claim this physical device. Disconnect the duplicate endpoints before configuring it.".into());
    }
    let live_can = can
        .iter()
        .filter(|(_, state)| state["connected"] == true)
        .flat_map(|(gateway, state)| {
            state["modules"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|module| {
                    module["online"] == true
                        && module["id"]
                            .as_str()
                            .is_some_and(|module| module.eq_ignore_ascii_case(&id))
                })
                .map(move |module| (gateway.as_str(), module))
        })
        .collect::<Vec<_>>();
    if live_can
        .iter()
        .map(|(gateway, _)| *gateway)
        .collect::<BTreeSet<_>>()
        .len()
        > 1
    {
        return Err("Several live CAN gateways claim this verified physical device. Configuration is blocked on USB and CAN until its physical routes are reconciled.".into());
    }
    let boots = live_can
        .iter()
        .filter_map(|(_, module)| {
            module["boot"]
                .as_u64()
                .filter(|boot| *boot > 0 && *boot <= u32::MAX as u64)
        })
        .chain(live_usb.iter().map(|(_, route)| u64::from(route.boot)))
        .collect::<BTreeSet<_>>();
    if boots.len() > 1 {
        return Err("The verified USB and CAN routes report conflicting live boot sessions. Configuration is blocked on both until this device's identity is reconciled.".into());
    }
    if !live_usb.is_empty()
        && live_can.iter().any(|(_, module)| {
            module["boot"]
                .as_u64()
                .is_none_or(|boot| boot == 0 || boot > u32::MAX as u64)
        })
    {
        return Err("Wait for a valid current CAN boot session before configuring this device through either route.".into());
    }
    Ok(RoutePreference {
        direct_usb_preferred: live_usb.first().is_some_and(|(slot, route)| {
            let Some(config) = &slot.snapshot.config else {
                return false;
            };
            let identity = slot.snapshot.identity.as_ref().unwrap();
            slot.snapshot.error.is_none()
                && config["view"] == "active"
                && config["valid"] == true
                && config["pendingApply"] == false
                && config["generation"].as_u64() == Some(u64::from(route.configuration_generation))
                && crate::module_console::validate_config(config).is_ok()
                && config["layout"]["buttons"].as_u64() == Some(u64::from(identity.button_count))
                && config["layout"]["axes"].as_u64() == Some(u64::from(identity.axis_count))
                && config["layout"]["hats"].as_u64() == Some(u64::from(identity.hat_count))
        }),
    })
}

fn verified(slot: &SlotSnapshot, id: &str) -> bool {
    slot.snapshot.identity.as_ref().is_some_and(|identity| {
        crate::module_console::serial(&identity.serial).is_some_and(|serial| {
            identity.model == "device-template"
                && identity.protocol == 3
                && identity.module_id.eq_ignore_ascii_case(id)
                && format!("4f534744{serial}") == id
                && slot
                    .port
                    .serial_number
                    .as_deref()
                    .is_some_and(|port| port.eq_ignore_ascii_case(&serial))
        })
    })
}

#[cfg(test)]
#[path = "module_configuration_routes_tests.rs"]
mod tests;
