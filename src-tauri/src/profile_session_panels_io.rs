//! Native profile transport access shares the same Manager writer gate as UI
//! setup. This is not IPC and cannot create sessions or bypass worker fencing.
use super::*;
use crate::module_usb::{ActionRequest, UsbRequest, UsbSnapshot};

pub(crate) struct ProfilePanelIo {
    sessions: Arc<Mutex<BTreeMap<String, Arc<dyn Adapter>>>>,
    usb: Arc<crate::module_usb_fleet::Fleet>,
    gate: Arc<Mutex<()>>,
    closed: Arc<AtomicBool>,
}
impl ProfilePanelIo {
    pub(super) fn new(
        sessions: Arc<Mutex<BTreeMap<String, Arc<dyn Adapter>>>>,
        usb: Arc<crate::module_usb_fleet::Fleet>,
        gate: Arc<Mutex<()>>,
        closed: Arc<AtomicBool>,
    ) -> Self {
        Self {
            sessions,
            usb,
            gate,
            closed,
        }
    }
    /// Close only after the coordinator has released/restored owned effects.
    pub(crate) fn close(&self) {
        // Shutdown must not wait for a discovery/configuration operation that
        // itself needs adapter cancellation. Pending operations recheck this
        // fence immediately after acquiring the shared gate.
        self.closed.store(true, Ordering::Release);
    }
    fn adapters(&self) -> Vec<(String, Arc<dyn Adapter>)> {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|(id, adapter)| (id.clone(), adapter.clone()))
            .collect()
    }
    fn states(&self) -> Vec<(String, Value)> {
        self.adapters()
            .into_iter()
            .map(|(id, a)| (id, a.cached_state()))
            .collect()
    }
    fn claims(&self) -> Vec<(String, crate::module_configuration_routes::WriterPin)> {
        self.adapters()
            .into_iter()
            .flat_map(|(id, a)| {
                a.configuration_claims()
                    .into_iter()
                    .map(move |pin| (id.clone(), pin))
            })
            .collect()
    }
    pub(crate) fn can_snapshot(&self, gateway: &str) -> Result<Value, String> {
        if self.closed.load(Ordering::Acquire) {
            return Err("Cockpit gateway manager has closed.".into());
        }
        let adapter = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(gateway)
            .cloned()
            .ok_or("Connect the bound gateway first.")?;
        Ok(adapter.cached_state())
    }
    pub(crate) fn usb_snapshots(&self) -> Vec<crate::module_usb_fleet::SlotSnapshot> {
        self.usb.snapshots()
    }
    /// No raw worker ownership token or request is logged or returned here.
    pub(crate) fn usb(&self, key: &str, operation: UsbRequest) -> Result<UsbSnapshot, String> {
        let mutating = matches!(
            &operation,
            UsbRequest::Claim { .. }
                | UsbRequest::Release { .. }
                | UsbRequest::ConfigChange { .. }
                | UsbRequest::Action {
                    action: ActionRequest::Claim { .. } | ActionRequest::ClaimCurrent { .. }
                        | ActionRequest::Renew { .. }
                        | ActionRequest::Release { .. }
                        | ActionRequest::Invoke { .. }
                        | ActionRequest::InvokeOwned { .. },
                    ..
                }
        );
        let _gate = mutating.then(|| self.gate.lock().unwrap_or_else(|p| p.into_inner()));
        if self.closed.load(Ordering::Acquire) {
            return Err("Cockpit gateway manager has closed.".into());
        }
        if mutating {
            let route = match &operation {
                UsbRequest::Claim { route, .. }
                | UsbRequest::Release { route, .. }
                | UsbRequest::ConfigChange { route, .. }
                | UsbRequest::Action { route, .. } => route,
                _ => unreachable!(),
            };
            if !matches!(
                &operation,
                UsbRequest::Release { .. }
                    | UsbRequest::Action {
                        action: ActionRequest::Release { .. },
                        ..
                    }
            ) {
                crate::module_configuration_routes::evaluate(
                    &self.usb.snapshots(),
                    &self.states(),
                    &route.module_id,
                )?;
            }
            if matches!(&operation, UsbRequest::Claim { .. }) {
                crate::module_configuration_routes::guard_usb_writer(
                    &route.module_id,
                    &self.claims(),
                )?;
            }
        }
        self.usb.request_native(key, operation)
    }
    pub(crate) fn can(&self, request: Value) -> Value {
        self.can_result(request)
            .unwrap_or_else(|error| json!({"ok":false,"error":error}))
    }
    fn can_result(&self, mut request: Value) -> Result<Value, String> {
        if !request.is_object()
            || serde_json::to_vec(&request)
                .map_err(|e| e.to_string())?
                .len()
                > 65_536
        {
            return Err("Invalid bounded native panel request.".into());
        }
        let command = request["command"]
            .as_str()
            .ok_or("Missing native panel command.")?
            .to_owned();
        if !matches!(
            command.as_str(),
            "status"
                | "config_read"
                | "events_state"
                | "events_read"
                | "action_describe"
                | "action_state"
                | "action_ownership"
                | "action_claim" | "action_claim_current"
                | "action_renew"
                | "action_release"
                | "action_invoke_owned"
                | "action_invoke"
                | "claim"
                | "release"
                | "config_apply"
                | "enable"
                | "disable"
        ) {
            return Err("This operation is not part of native profile panel execution.".into());
        }
        let mutating = matches!(
            command.as_str(),
            "action_claim" | "action_claim_current"
                | "action_renew"
                | "action_release"
                | "action_invoke_owned"
                | "action_invoke"
                | "claim"
                | "release"
                | "config_apply"
                | "enable"
                | "disable"
        );
        let _gate = mutating.then(|| self.gate.lock().unwrap_or_else(|p| p.into_inner()));
        if self.closed.load(Ordering::Acquire) {
            return Err("Cockpit gateway manager has closed.".into());
        }
        let gateway = request["gatewayId"]
            .as_str()
            .ok_or("Select the exact native gateway identity.")?
            .to_owned();
        let adapter = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&gateway)
            .cloned()
            .ok_or("Connect the bound gateway first.")?;
        request
            .as_object_mut()
            .unwrap()
            .remove("_directUsbPreferred");
        if mutating {
            let module = request["moduleId"]
                .as_str()
                .ok_or("Select the exact native module identity.")?
                .to_owned();
            let state = adapter.cached_state();
            let current = state["modules"]
                .as_array()
                .and_then(|modules| {
                    modules
                        .iter()
                        .find(|m| m["id"] == module && m["online"] == true)
                })
                .ok_or("The bound native module is offline.")?;
            if state["connected"] != true
                || request["boot"]
                    .as_u64()
                    .is_none_or(|boot| boot == 0 || Some(boot) != current["boot"].as_u64())
            {
                return Err(
                    "The native module boot changed before dispatch; capture a fresh route.".into(),
                );
            }
            if !matches!(command.as_str(), "release" | "action_release") {
                let usb = self.usb.snapshots();
                let preference =
                    crate::module_configuration_routes::evaluate(&usb, &self.states(), &module)?;
                if command == "claim" {
                    crate::module_configuration_routes::guard_can_writer(
                        &module,
                        &self.usb.configuration_claims(),
                    )?;
                    request["_directUsbPreferred"] = json!(preference.direct_usb_preferred);
                }
            }
        }
        Ok(adapter.request(request))
    }
}
#[cfg(test)]
#[path = "profile_session_panels_io_tests.rs"]
mod tests;
