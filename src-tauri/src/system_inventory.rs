//! Supplemental system observation. No profile, acquisition or mutation is needed.
use osg_virtual_controller::vjoy::{Slot, Status};
use serde_json::{json, Value};
use std::{sync::Mutex, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
#[path = "system_inventory_associations.rs"]
mod associations;
pub(crate) use associations::collect as controller_associations;

pub(crate) struct Runtime {
    native: bool,
    snapshot: Mutex<Option<(Instant, Value)>>,
}
impl Runtime {
    pub(crate) fn new(native: bool) -> Self { Self { native, snapshot: Mutex::new(None) } }
    pub(crate) fn request(&self, request: &Value, controllers: &osg_controller_input::ControllerService, associations:impl FnOnce(&osg_controller_input::registry::ResourceInventory)->Vec<Value>) -> Result<Value, String> {
        if !request.is_object() || request.to_string().len() > 65_536
            || request.as_object().unwrap().keys().any(|key| !matches!(key.as_str(), "command" | "clientId" | "offset" | "limit" | "observedAt")) {
            return Err("System inventory accepts only its command, page and Cockpit client identity.".into());
        }
        if !request["clientId"].as_str().is_some_and(|id|crate::protocol::identifier(id,16,80)) {
            return Err("Missing Cockpit client identity; reload the application.".into());
        }
        let offset=request.get("offset").map_or(Ok(0),|value|value.as_u64().filter(|offset|*offset<=4096).map(|offset|offset as usize).ok_or("Invalid system inventory page."))?;
        let limit=request.get("limit").map_or(Ok(32),|value|value.as_u64().filter(|limit|*limit>0 && *limit<=32).map(|limit|limit as usize).ok_or("System inventory page limit must be 1–32."))?;
        let expected=request.get("observedAt").map(|value|value.as_u64().filter(|at|*at>0).ok_or("Invalid system inventory observation token.")).transpose()?;
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|p|p.into_inner());
        if let Some(expected)=expected {
            let (_,value)=snapshot.as_ref().filter(|(_,value)|value["observedAt"].as_u64()==Some(expected)).ok_or("System inventory snapshot expired; restart from its first page.")?;
            return page(value,offset,limit);
        }
        if let Some((observed, value)) = &*snapshot {
            if observed.elapsed() < Duration::from_secs(2) { return page(value,offset,limit); }
        }
        let observed = Instant::now();
        let mut value = if self.native {
            let sources=controllers.resource_inventory();
            let associations=associations(&sources);
            let visibility = osg_controller_input::inventory::current(sources);
            let mut value=compose(visibility, osg_virtual_controller::inspection::current(), configured());
            value["controllerAssociations"]=json!(associations);
            value
        } else {
            compose(Default::default(), Ok(vec![]), Ok((vec![],vec![])))
        };
        if value["controllerAssociations"].is_null() {value["controllerAssociations"]=json!([]);}
        *snapshot = Some((observed, value.clone()));
        page(&value,offset,limit)
    }
}
fn page(value:&Value,offset:usize,limit:usize)->Result<Value,String> {
    let devices=value["hiddenDevices"].as_array().ok_or("Invalid native system inventory snapshot.")?;
    let associations=value["controllerAssociations"].as_array().cloned().unwrap_or_default();
    let items=devices.iter().map(|value|(false,value.clone())).chain(associations.iter().map(|value|(true,value.clone()))).collect::<Vec<_>>();
    let mut selected=items.iter().skip(offset).take(limit).cloned().collect::<Vec<_>>();
    loop {
        let next=offset+selected.len();
        let hidden=selected.iter().filter(|(association,_)|!*association).map(|(_,value)|value).collect::<Vec<_>>();
        let related=selected.iter().filter(|(association,_)|*association).map(|(_,value)|value).collect::<Vec<_>>();
        let response=json!({"observedAt":value["observedAt"],"virtualDevices":value["virtualDevices"],
            "diagnostics":value["diagnostics"],"visibilityInspected":value["visibilityInspected"],"hiddenDevices":hidden,"controllerAssociations":related,"offset":offset,"total":items.len(),
            "hiddenTotal":devices.len(),"associationTotal":associations.len(),"nextOffset":if next<items.len(){Some(next)}else{None}});
        if serde_json::to_vec(&response).map_err(|e|e.to_string())?.len()<=60_000 {return Ok(response);}
        if selected.len()<=1 {return Err("System inventory metadata exceeds the native reply budget.".into());}
        selected.pop();
    }
}
fn configured() -> Result<(Vec<Value>,Vec<String>),String> {
    #[cfg(windows)]
    {
        let inventory=osg_virtual_controller::windows_provision::NativeBackend::native(&uuid::Uuid::new_v4().to_string())?.configured_inventory()?;
        let slots=inventory.slots.into_iter().map(|slot|json!({
            "id":format!("vjoy:{}",slot.id),"name":format!("vJoy Device {}",slot.id),"slot":slot.id,"backend":"vjoy",
            "available":null,"status":"unknown","configured":true,"createdByCockpit":slot.created_by_cockpit,
            "creationOperation":slot.creation_operation,"layout":slot.layout,"diagnostics":slot.diagnostics,
            "ownerPid":null,"owner":null,
        })).collect();
        Ok((slots,inventory.diagnostics))
    }
    #[cfg(not(windows))]
    { Ok((vec![],vec![])) }
}
fn compose(visibility: osg_controller_input::inventory::Inventory, slots: Result<Vec<Slot>, String>, configured:Result<(Vec<Value>,Vec<String>),String>) -> Value {
    let mut diagnostics = visibility.diagnostics;
    // Successful SDK inspection queries every slot. Its inventory omits Missing
    // slots; only a separately known configured target can turn that absence
    // into a device row. A failed inspection leaves availability unknown.
    let inspected=slots.is_ok();
    let mut virtual_devices = match slots {
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
    match configured {
        Ok((configured,issues)) => {
            diagnostics.extend(issues);
            for mut configuration in configured {
                if let Some(device)=virtual_devices.iter_mut().find(|device|device["id"]==configuration["id"]) {
                    device["configured"]=json!(true);
                    device["createdByCockpit"]=configuration["createdByCockpit"].clone();
                    device["creationOperation"]=configuration["creationOperation"].clone();
                } else {
                    if inspected {configuration["available"]=json!(false);configuration["status"]=json!("missing");}
                    virtual_devices.push(configuration);
                }
            }
        }
        Err(error)=>diagnostics.push(format!("Configured virtual device inventory is unavailable: {error}")),
    }
    json!({"observedAt":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
        "virtualDevices":virtual_devices,"visibilityInspected":visibility.policy_inspected,"hiddenDevices":visibility.hidden_devices,"diagnostics":diagnostics})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn virtual_inventory_reports_external_ownership_without_claiming_creation() {
        let slot=Slot {id:3,status:Status::Busy,owner_pid:Some(1234),buttons:16,axes:vec![],discrete_hats:0,continuous_hats:1,force_feedback:false};
        let value=compose(Default::default(),Ok(vec![slot]),Ok((vec![],vec![])));
        assert_eq!(value["virtualDevices"][0]["id"],"vjoy:3");
        assert_eq!(value["virtualDevices"][0]["status"],"busy");
        assert_eq!(value["virtualDevices"][0]["ownerPid"],1234);
        assert!(value["virtualDevices"][0]["createdByCockpit"].is_null());
    }
    #[test]
    fn inspection_failure_is_reported_as_unavailable_not_an_empty_successful_provider() {
        let value=compose(Default::default(),Err("driver unavailable".into()),Ok((vec![],vec![])));
        assert_eq!(value["virtualDevices"],json!([]));
        assert!(value["diagnostics"][0].as_str().unwrap().contains("driver unavailable"));
    }
    #[test]
    fn configured_virtual_slots_remain_visible_when_optional_sdk_is_unavailable() {
        let configured=json!({"id":"vjoy:4","name":"vJoy Device 4","status":"unknown","available":null,"configured":true,"createdByCockpit":true,"creationOperation":"verified-operation"});
        let value=compose(Default::default(),Err("driver unavailable".into()),Ok((vec![configured],vec![])));
        assert_eq!(value["virtualDevices"][0]["id"],"vjoy:4");
        assert_eq!(value["virtualDevices"][0]["createdByCockpit"],true);
        assert!(value["virtualDevices"][0]["available"].is_null());
    }
    #[test]
    fn configured_virtual_targets_preserve_sdk_missing_without_adding_unused_slots() {
        let configured=json!({"id":"vjoy:4","name":"vJoy Device 4","status":"unknown","available":null,"configured":true,"createdByCockpit":true});
        let missing=(1..=16).map(|id|Slot {id,status:Status::Missing,owner_pid:None,buttons:0,axes:vec![],discrete_hats:0,continuous_hats:0,force_feedback:false}).collect::<Vec<_>>();
        let value=compose(Default::default(),Ok(missing.clone()),Ok((vec![configured.clone()],vec![])));
        assert_eq!(value["virtualDevices"].as_array().unwrap().len(),1);
        assert_eq!(value["virtualDevices"][0]["id"],"vjoy:4");
        assert_eq!(value["virtualDevices"][0]["status"],"missing");
        assert_eq!(value["virtualDevices"][0]["available"],false);
        assert_eq!(value["virtualDevices"][0]["createdByCockpit"],true);
        // The production inspector omits all Missing slots after querying them.
        let omitted=compose(Default::default(),Ok(vec![]),Ok((vec![configured.clone()],vec![])));
        assert_eq!(omitted["virtualDevices"][0]["status"],"missing");
        let unavailable=compose(Default::default(),Err("SDK unavailable".into()),Ok((vec![configured.clone()],vec![])));
        assert_eq!(unavailable["virtualDevices"][0]["status"],"unknown");
        assert!(unavailable["virtualDevices"][0]["available"].is_null());
        let mut unknown=missing[3].clone();unknown.status=Status::Unknown;
        let uncertain=compose(Default::default(),Ok(vec![unknown]),Ok((vec![configured],vec![])));
        assert_eq!(uncertain["virtualDevices"][0]["status"],"unknown");
        assert!(uncertain["virtualDevices"][0]["available"].is_null());
    }
    #[test]
    fn hidden_inventory_pages_preserve_all_devices_within_native_transport_budget() {
        let devices=(0..100).map(|id|json!({"id":id,"ancestors":["a".repeat(4000)]})).collect::<Vec<_>>();
        let inventory=json!({"observedAt":1,"virtualDevices":[],"hiddenDevices":devices,"diagnostics":[]});
        let mut offset=0;
        let mut count=0;
        loop {
            let response=page(&inventory,offset,32).unwrap();
            assert!(response.to_string().len()<=60_000);
            assert_eq!(response["total"],100);
            count+=response["hiddenDevices"].as_array().unwrap().len();
            let Some(next)=response["nextOffset"].as_u64() else {break;};
            assert!(next as usize>offset);
            offset=next as usize;
        }
        assert_eq!(count,100);
    }
    #[test]
    fn association_only_pages_advance_the_combined_cursor_and_tokens_cannot_mix_snapshots() {
        let value=json!({"observedAt":1,"virtualDevices":[],"hiddenDevices":[{"id":"hidden"}],"controllerAssociations":[{"controllerId":"one"},{"controllerId":"two"}],"diagnostics":[]});
        let response=page(&value,1,1).unwrap();
        assert_eq!(response["hiddenDevices"],json!([]));
        assert_eq!(response["controllerAssociations"][0]["controllerId"],"one");
        assert_eq!(response["total"],3);
        assert_eq!(response["nextOffset"],2);
        let runtime=Runtime::new(false);
        *runtime.snapshot.lock().unwrap()=Some((Instant::now()-Duration::from_secs(5),value));
        let controllers=osg_controller_input::ControllerService::new(None,false);
        let request=json!({"command":"system_inventory","clientId":"native-inventory-test-client","observedAt":1,"offset":1,"limit":1});
        assert_eq!(runtime.request(&request,&controllers,|_|panic!("Pinned pages cannot observe a new snapshot")).unwrap()["controllerAssociations"][0]["controllerId"],"one");
        let mut wrong=request;wrong["observedAt"]=json!(2);
        assert!(runtime.request(&wrong,&controllers,|_|vec![]).unwrap_err().contains("expired"));
    }
    #[test]
    fn native_snapshot_inspects_available_optional_services_without_acquiring_devices() {
        let runtime=Runtime::new(true);
        let controllers=osg_controller_input::ControllerService::new(None,false);
        let request=json!({"command":"system_inventory","clientId":"native-inventory-test-client"});
        let value=runtime.request(&request,&controllers,|_|vec![json!({"controllerId":"test","gatewayId":"exact-test-evidence"})]).unwrap();
        assert!(value["observedAt"].as_u64().is_some());
        assert!(value["virtualDevices"].is_array());
        assert!(value["hiddenDevices"].is_array());
        assert!(value["controllerAssociations"].is_array());
        assert_eq!(value,runtime.request(&request,&controllers,|_|panic!("Cached inspection must not observe providers again")).unwrap());
        for fields in [json!({"offset":4097}),json!({"offset":-1}),json!({"limit":0}),json!({"limit":33}),json!({"observedAt":0})] {
            let mut invalid=request.clone();for (key,value) in fields.as_object().unwrap(){invalid[key]=value.clone();}
            assert!(runtime.request(&invalid,&controllers,|_|vec![]).is_err());
        }
    }
}
