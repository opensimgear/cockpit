//! Supplemental system observation. No profile, acquisition or mutation is needed.
use osg_virtual_controller::vjoy::{Slot, Status};
use serde_json::{json, Value};
use std::{sync::Mutex, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

pub(crate) struct Runtime {
    native: bool,
    snapshot: Mutex<Option<(Instant, Value)>>,
}
impl Runtime {
    pub(crate) fn new(native: bool) -> Self { Self { native, snapshot: Mutex::new(None) } }
    pub(crate) fn request(&self, request: &Value, controllers: &osg_controller_input::ControllerService) -> Result<Value, String> {
        if !request.is_object() || request.to_string().len() > 65_536
            || request.as_object().unwrap().keys().any(|key| !matches!(key.as_str(), "command" | "clientId")) {
            return Err("System inventory accepts only its command and Cockpit client identity.".into());
        }
        if !request["clientId"].as_str().is_some_and(|id|crate::protocol::identifier(id,16,80)) {
            return Err("Missing Cockpit client identity; reload the application.".into());
        }
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|p|p.into_inner());
        if let Some((observed, value)) = &*snapshot {
            if observed.elapsed() < Duration::from_secs(2) { return Ok(value.clone()); }
        }
        let observed = Instant::now();
        let value = if self.native {
            let visibility = osg_controller_input::inventory::current(controllers.resource_inventory());
            compose(visibility, osg_virtual_controller::inspection::current())
        } else {
            compose(Default::default(), Ok(vec![]))
        };
        *snapshot = Some((observed, value.clone()));
        Ok(value)
    }
}
fn compose(visibility: osg_controller_input::inventory::Inventory, slots: Result<Vec<Slot>, String>) -> Value {
    let mut diagnostics = visibility.diagnostics;
    let virtual_devices = match slots {
        Ok(slots) => slots.into_iter().filter(|slot|slot.status != Status::Missing).map(|slot| {
            let status = serde_json::to_value(slot.status).unwrap();
            json!({
                "id":format!("vjoy:{}",slot.id),"name":format!("vJoy Device {}",slot.id),"slot":slot.id,"backend":"vjoy",
                "available":if slot.status==Status::Unknown {None} else {Some(true)},"status":status,
                "createdByCockpit":null,"ownerPid":slot.owner_pid,"owner":slot.owner_pid.map(|pid|format!("Process {pid}")),
                "buttons":slot.buttons,"axes":slot.axes,"discreteHats":slot.discrete_hats,"continuousHats":slot.continuous_hats,
                "forceFeedback":slot.force_feedback,"diagnostics":[],
            })
        }).collect::<Vec<_>>(),
        Err(error) => {
            diagnostics.push(format!("Virtual device inventory is unavailable: {error}"));
            vec![]
        }
    };
    json!({"observedAt":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
        "virtualDevices":virtual_devices,"hiddenDevices":visibility.hidden_devices,"diagnostics":diagnostics})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn virtual_inventory_reports_external_ownership_without_claiming_creation() {
        let slot=Slot {id:3,status:Status::Busy,owner_pid:Some(1234),buttons:16,axes:vec![],discrete_hats:0,continuous_hats:1,force_feedback:false};
        let value=compose(Default::default(),Ok(vec![slot]));
        assert_eq!(value["virtualDevices"][0]["id"],"vjoy:3");
        assert_eq!(value["virtualDevices"][0]["status"],"busy");
        assert_eq!(value["virtualDevices"][0]["ownerPid"],1234);
        assert!(value["virtualDevices"][0]["createdByCockpit"].is_null());
    }
    #[test]
    fn inspection_failure_is_reported_as_unavailable_not_an_empty_successful_provider() {
        let value=compose(Default::default(),Err("driver unavailable".into()));
        assert_eq!(value["virtualDevices"],json!([]));
        assert!(value["diagnostics"][0].as_str().unwrap().contains("driver unavailable"));
    }
}
