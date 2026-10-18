//! Exact hardware publication associations. Names and VID/PID alone never join devices.
use crate::module_usb_fleet::SlotSnapshot;
use osg_controller_input::registry::ResourceInventory;
use serde_json::{json,Value};
use std::collections::BTreeSet;

fn hex(value:Option<&str>)->Option<u16> {
    let value=value?;
    u16::from_str_radix(value.strip_prefix("0x").or_else(||value.strip_prefix("0X")).unwrap_or(value),16).ok()
}
fn interface(path:&str)->Option<u64> {
    let hardware=path.split('#').nth(1)?.to_ascii_lowercase();
    hardware.split('&').find_map(|part|part.strip_prefix("mi_").filter(|value|value.len()==2).and_then(|value|u64::from_str_radix(value,16).ok()))
}
pub(crate) fn collect(sources:&ResourceInventory,gateways:&[(String,Value)],ports:&[Value],usb:&[SlotSnapshot])->Vec<Value> {
    let mut result=vec![];
    for source in &sources.sources {
        let identity=&source.saved.identity;
        let Some(serial)=identity.serial.as_deref().filter(|serial|!serial.is_empty()) else {continue;};
        let mut matches=vec![];
        for (id,state) in gateways {
            let Some((backend,uid))=id.split_once(':') else {continue;};
            if state["connected"]!=true || crate::gateways::canonical_id(backend,uid).as_deref()!=Some(id)
                || !state["gatewaySerial"].as_str().is_some_and(|value|value.eq_ignore_ascii_case(uid))
                || !serial.eq_ignore_ascii_case(uid) {continue;}
            let exact_ports=ports.iter().filter(|port|port["gatewayId"]==*id
                && port["serialNumber"].as_str().is_some_and(|value|value.eq_ignore_ascii_case(serial))
                && hex(port["vendorId"].as_str())==Some(identity.vendor_id)
                && hex(port["productId"].as_str())==Some(identity.product_id)).collect::<Vec<_>>();
            // Duplicate endpoints with the same claimed serial are ambiguous.
            if exact_ports.len()!=1 {continue;}
            let reports=source.controls.iter().filter(|control|control.kind!=osg_controller_input::registry::Kind::Diagnostic).map(|control|u64::from(control.report_id)).collect::<BTreeSet<_>>();
            let modules=state["modules"].as_array().into_iter().flatten().filter(|module| {
                if backend=="pico" {module["registered"]==true && interface(&identity.path).is_some_and(|interface|module["hidInterface"].as_u64()==Some(interface))}
                else {source.online && state["canProtocol"]==3 && state["layoutPending"]==false && reports.len()==1 && reports.first().is_some_and(|report|*report>0 && module["reportId"].as_u64()==Some(*report))}
            }).collect::<Vec<_>>();
            let module=if modules.len()==1 {modules[0]["id"].as_str()}else{None};
            matches.push(json!({"controllerId":source.saved.id,"gatewayId":id,"moduleId":module,
                "evidence":["verified-gateway-serial-and-usb-identity",if module.is_some(){if backend=="pico"{"exact-usb-interface"}else{"exact-hid-report-id"}}else{"physical-gateway"}]}));
        }
        for slot in usb {
            let snapshot=&slot.snapshot;
            let (Some(module),Some(route))=(&snapshot.identity,&snapshot.route) else {continue;};
            if !slot.present || !slot.admissible || !snapshot.connected || snapshot.error.is_some()
                || module.module_id!=route.module_id || module.serial!=route.serial || route.boot==0
                || !serial.eq_ignore_ascii_case(&module.serial)
                || !slot.port.serial_number.as_deref().is_some_and(|value|value.eq_ignore_ascii_case(serial))
                || hex(slot.port.vendor_id.as_deref())!=Some(identity.vendor_id)
                || hex(slot.port.product_id.as_deref())!=Some(identity.product_id) {continue;}
            matches.push(json!({"controllerId":source.saved.id,"usbKey":slot.key,"moduleId":module.module_id,
                "evidence":["verified-module-handshake-and-usb-identity"]}));
        }
        // A same-serial collision cannot authorize grouping across providers.
        if matches.len()==1 {result.push(matches.remove(0));}
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use osg_controller_input::registry::{Control,Identity,ResourceDescriptor,SavedSource};
    fn sources()->ResourceInventory {
        let mut control=Control::new(9,1,17,1,1);control.report_id=17;
        ResourceInventory {session:"test".into(),backend_status:"ready".into(),diagnostic:String::new(),sources:vec![ResourceDescriptor {
            saved:SavedSource {id:"controller-1".into(),name:"A matching name is not identity".into(),role:String::new(),identity:Identity {backend:"windows-raw-input".into(),vendor_id:0xcafe,product_id:0x4010,serial:Some("00112233445566778899aabb".into()),collection:String::new(),fingerprint:String::new(),path:r"\\?\HID#VID_CAFE&PID_4010&MI_02&COL01#instance".into()}},
            online:true,generation:1,baseline_ready:true,controls:vec![control],diagnostics:vec![],
        }]}
    }
    fn gateway()->(String,Value) {("stm32:00112233445566778899aabb".into(),json!({"connected":true,"gatewaySerial":"00112233445566778899aabb","canProtocol":3,"layoutPending":false,"modules":[{"id":"module-1","reportId":17}]}))}
    fn port()->Value {json!({"gatewayId":"stm32:00112233445566778899aabb","serialNumber":"00112233445566778899aabb","vendorId":"cafe","productId":"4010"})}
    #[test]
    fn matches_native_report_to_exact_verified_gateway_and_rejects_ambiguous_or_incomplete_evidence() {
        let sources=sources();
        let result=collect(&sources,&[gateway()],&[port()],&[]);
        assert_eq!(result[0]["moduleId"],"module-1");
        assert_eq!(result[0]["controllerId"],"controller-1");
        assert!(collect(&sources,&[gateway()],&[port(),port()],&[]).is_empty());
        let mut wrong=port();wrong["vendorId"]=json!("ffff");
        assert!(collect(&sources,&[gateway()],&[wrong],&[]).is_empty());
        let mut no_serial=sources.clone();no_serial.sources[0].saved.identity.serial=None;
        assert!(collect(&no_serial,&[gateway()],&[port()],&[]).is_empty());
    }
    #[test]
    fn stale_descriptor_only_proves_gateway_and_pico_uses_actual_usb_interface() {
        let mut sources=sources();sources.sources[0].online=false;
        let result=collect(&sources,&[gateway()],&[port()],&[]);
        assert!(result[0]["moduleId"].is_null());
        sources.sources[0].saved.identity.serial=Some("OSG-PICO-0123456789ABCDEF".into());
        let id="pico:OSG-PICO-0123456789ABCDEF";
        let gateway=(id.into(),json!({"connected":true,"gatewaySerial":"OSG-PICO-0123456789ABCDEF","modules":[{"id":"pico-module","registered":true,"hidInterface":2}]}));
        let port=json!({"gatewayId":id,"serialNumber":"OSG-PICO-0123456789ABCDEF","vendorId":"cafe","productId":"4010"});
        let result=collect(&sources,&[gateway],&[port],&[]);
        assert_eq!(result[0]["moduleId"],"pico-module");
    }
    #[test]
    fn standalone_module_requires_protocol_uid_route_and_matching_usb_identity() {
        let sources=sources();
        let identity=crate::module_console::UsbIdentity {model:"OSG".into(),board:"board".into(),profile:"Panel".into(),module_id:"module-1".into(),serial:"00112233445566778899aabb".into(),protocol:3,button_count:1,axis_count:0,hat_count:0};
        let slot=SlotSnapshot {key:"usb-key".into(),port:crate::transport::PortInfo {path:"COM4".into(),serial_number:Some(identity.serial.clone()),vendor_id:Some("cafe".into()),product_id:Some("4010".into()),manufacturer:None,friendly_name:None},present:true,automatic:true,admissible:true,
            snapshot:crate::module_usb::UsbSnapshot {connected:true,identity:Some(identity.clone()),route:Some(crate::module_usb::UsbRouteToken {module_id:identity.module_id,serial:identity.serial,connection_generation:1,boot:1,configuration_generation:1}),..Default::default()},diagnostics:vec![]};
        let result=collect(&sources,&[],&[],std::slice::from_ref(&slot));
        assert_eq!(result[0]["usbKey"],"usb-key");
        let mut mismatched=slot.clone();mismatched.snapshot.route.as_mut().unwrap().serial="different".into();
        assert!(collect(&sources,&[],&[],&[mismatched]).is_empty());
        // More than one agreeing transport endpoint remains an identity collision.
        assert!(collect(&sources,&[],&[],&[slot.clone(),slot]).is_empty());
    }
}
