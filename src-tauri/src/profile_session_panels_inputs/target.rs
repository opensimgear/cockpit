use super::*;

impl Target {
    pub(super) fn from_route(route: &Route, io: &dyn Io) -> Result<Self, String> {
        match route.transport {
            Transport::Usb => {
                let key = route
                    .id
                    .strip_prefix("usb:")
                    .ok_or("The USB route has no exact native endpoint.")?;
                let slots = io.snapshots();
                let slot = slots
                    .iter()
                    .find(|slot| {
                        slot.key == key
                            && slot.present
                            && slot.admissible
                            && slot.snapshot.connected
                    })
                    .ok_or("The selected USB endpoint is no longer admissible.")?;
                let token = slot
                    .snapshot
                    .route
                    .as_ref()
                    .ok_or("The USB endpoint has no current native route.")?;
                if route.provider_generation
                    != format!(
                        "{}:{}:{}",
                        token.connection_generation, token.boot, token.configuration_generation
                    )
                {
                    return Err("The USB route changed during preparation.".into());
                }
                Ok(Self::Usb {
                    key: key.into(),
                    route: token.clone(),
                })
            }
            Transport::Can => {
                let values: Vec<Value> = serde_json::from_str(&route.provider_generation)
                    .map_err(|_| "The CAN route has no exact native generation.")?;
                if values.len() != 5 {
                    return Err("The CAN route generation is incomplete.".into());
                }
                let field = |key: &str| {
                    route
                        .attributes
                        .get(key)
                        .cloned()
                        .ok_or_else(|| format!("Missing native CAN {key}."))
                };
                let gateway = field("gatewayId")?;
                let module = field("moduleId")?;
                let node: Value = serde_json::from_str(&field("node")?)
                    .map_err(|_| "Invalid native CAN node.")?;
                let boot: Value = serde_json::from_str(&field("boot")?)
                    .map_err(|_| "Invalid native CAN boot.")?;
                if boot != values[1]
                    || boot.as_u64().is_none_or(|n| n == 0)
                    || node.as_u64().is_none()
                {
                    return Err("The CAN route boot/node changed.".into());
                }
                let target = Self::Can {
                    gateway: gateway.clone(),
                    module,
                    epoch: values[0].clone(),
                    node,
                    lease: values[2].clone(),
                    boot,
                    configuration: values[3].clone(),
                    stream: values[4].clone(),
                };
                target.check_can(&io.can_cached(&gateway)?)?;
                Ok(target)
            }
            _ => Err("Only native USB/CAN panel routes belong to this reader.".into()),
        }
    }
    pub(super) fn check_can(&self, state: &Value) -> Result<(), String> {
        let Self::Can {
            module,
            epoch,
            node,
            lease,
            boot,
            configuration,
            stream,
            ..
        } = self
        else {
            return Ok(());
        };
        let current = state["modules"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|m| m["id"] == *module && m["online"] == true)
            })
            .ok_or("The native CAN module is offline.")?;
        if state["connected"] != true
            || state["epoch"] != *epoch
            || current["node"] != *node
            || current["lease"] != *lease
            || current["boot"] != *boot
            || current["configurationGeneration"] != *configuration
        {
            return Err("CAN epoch, node, lease, boot or configuration changed.".into());
        }
        let events = &state["moduleEvents"][module];
        if events["route"] != json!([epoch, node, lease, boot])
            || events["configurationGeneration"] != *configuration
            || events["stream"] != *stream
        {
            return Err("The CAN input stream does not match its selected route.".into());
        }
        Ok(())
    }
    pub(super) fn events(&self, io: &dyn Io, cursor: Option<(u32, u32)>) -> Result<Value, String> {
        let events = match self {
            Self::Usb { .. } => return Err("USB panel inputs require native Capture subscriptions.".into()),
            Self::Can {
                gateway,
                module,
                boot,
                ..
            } => {
                let mut request = json!({"command":if cursor.is_some(){"events_read"}else{"events_state"},"gatewayId":gateway,"moduleId":module,"boot":boot,"clientId":"profile-panel-reader"});
                if let Some((stream, after)) = cursor {
                    request["stream"] = json!(stream);
                    request["after"] = json!(after);
                }
                let reply = io.can(request);
                if reply["ok"] != true {
                    return Err(reply["error"]
                        .as_str()
                        .unwrap_or("The CAN input read failed.")
                        .into());
                }
                let state = &reply["state"];
                self.check_can(state)?;
                let events = &state["moduleEvents"][module];
                events.clone()
            }
        };
        crate::panel_events::validate(&events, cursor)?;
        Ok(events)
    }
}
