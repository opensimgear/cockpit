//! Fixed companion operator tools; the native host owns validation and effects.
use super::*;
use base64::{engine::general_purpose::STANDARD,Engine};
pub(super) fn handles(name:&str)->bool {
    matches!(name,"companion_inventory"|"companion_certificate_get"|"companion_pair_offer"|"companion_pair_cancel"|"companion_approve"|"companion_update"|"companion_revoke"|"companion_assign"|"companion_unassign"|"companion_takeover"|"companion_disconnect"|"companion_prune"|"companion_publish_begin"|"companion_publish_chunk"|"companion_publish_commit"|"companion_publish_abort"|"companion_firmware_check"|"companion_usb_ports"|"companion_usb_status"|"companion_usb_connect"|"companion_usb_disconnect"|"companion_usb_scan"|"companion_usb_provision"|"companion_usb_reconnect"|"companion_usb_arm"|"companion_usb_disarm"|"companion_usb_forget"|"companion_usb_update_prepare"|"companion_usb_flash"|"companion_usb_flash_cancel")
}
pub(super) fn mutation(name:&str)->bool {
    handles(name)&&!matches!(name,"companion_inventory"|"companion_usb_status")
}
impl Tools {
    pub(super) fn companion_tool(&self,name:&str,args:&Value,client:&str)->Result<Value,Value>{
        let command=if name=="companion_certificate_get"{"companion_browser_certificate"}else{name};
        let mut payload=args.clone();payload.as_object_mut().ok_or_else(||failure("invalid_arguments","Expected companion arguments.",false))?.remove("operationId");
        if name=="companion_publish_chunk" {
            let bytes=STANDARD.decode(payload["base64"].as_str().unwrap_or("")).map_err(|_|failure("invalid_arguments","Invalid panel chunk base64.",false))?;
            if bytes.is_empty()||bytes.len()>8192 {return Err(failure("invalid_arguments","Panel chunks contain 1–8192 bytes.",false));}
            payload.as_object_mut().unwrap().remove("base64");payload["bytes"]=json!(bytes);
        }
        if name=="companion_usb_status" {
            let state=self.native("companion_inventory",json!({}),client,Some("companions"))?;
            return Ok(json!({"usb":state["usb"]}));
        }
        self.native(command,json!({"companion":payload}),client,Some("companions"))
    }
}
#[cfg(test)]
#[path="companion_controls/tests.rs"]
mod tests;
