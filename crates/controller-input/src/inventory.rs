//! Read-only system visibility evidence, independent of input capture and profiles.
use crate::{registry::ResourceInventory, visibility::policy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub instance_id: String,
    pub controller_id: Option<String>,
    pub controller_ids: Vec<String>,
    pub interface_path: Option<String>,
    pub interface_paths: Vec<String>,
    pub present: Option<bool>,
    pub hidden: Option<bool>,
    pub configured: Option<bool>,
    pub inverse: Option<bool>,
    pub scope: String,
    pub parent_instance_id: Option<String>,
    pub container_id: Option<String>,
    pub ancestors: Vec<String>,
    pub diagnostics: Vec<String>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub hidden_devices: Vec<Device>,
    pub diagnostics: Vec<String>,
    pub policy_inspected: bool,
}

/// Returns configured hidden devices even when Raw Input cannot see them. Failed
/// policy or PnP reads remain unknown, rather than inventing offline/visible state.
pub fn current(sources: ResourceInventory) -> Inventory {
    #[cfg(windows)]
    {
        use crate::visibility::journal::Backend;
        let state = crate::visibility::windows::installed().and_then(|mut driver| driver.read());
        compose(sources, state, crate::visibility::windows::instance, windows::inspect)
    }
    #[cfg(not(windows))]
    {
        let _ = sources;
        Inventory { diagnostics: vec!["PnP and HidHide inspection is available on Windows.".into()], ..Default::default() }
    }
}

fn compose(
    sources: ResourceInventory,
    state: Result<policy::State, String>,
    resolve: impl Fn(&str) -> Result<String, String>,
    inspect: impl Fn(&str) -> Device,
) -> Inventory {
    let mut result = Inventory::default();
    let policy = match state {
        Ok(state) => Some(state),
        Err(error) => {
            result.diagnostics.push(format!("HidHide visibility is unknown: {error}"));
            None
        }
    };
    let mut devices = BTreeMap::<String, Device>::new();
    result.policy_inspected = policy.is_some();
    if let Some(state) = &policy {
        for instance in &state.hidden {
            devices.entry(policy::key(instance)).or_insert_with(|| inspect(instance));
        }
    }
    for source in sources.sources {
        let interface = &source.saved.identity.path;
        match resolve(interface) {
            Ok(instance) => {
                let device = devices.entry(policy::key(&instance)).or_insert_with(|| inspect(&instance));
                device.name = source.saved.name;
                device.controller_ids.push(source.saved.id);
                device.interface_paths.push(interface.clone());
            }
            Err(error) => {
                // A previously observed interface can disappear before this read.
                // Retain the controller inventory's own connection evidence.
                result.diagnostics.push(format!("{}: {error}", source.saved.name));
            }
        }
    }
    for (_, mut device) in devices {
        device.id = format!("pnp:{}", policy::key(&device.instance_id));
        device.controller_id = device.controller_ids.first().cloned();
        device.interface_path = device.interface_paths.first().cloned();
        device.scope = "system-inspection".into();
        if let Some(state) = &policy {
            let configured = policy::contains(&state.hidden, &device.instance_id);
            device.configured = Some(configured);
            device.hidden = Some(state.active && configured);
            device.inverse = Some(state.inverse);
        }
        result.hidden_devices.push(device);
    }
    result
}

#[cfg(windows)]
mod windows {
    use super::Device;
    use windows_sys::Win32::{Devices::{DeviceAndDriverInstallation::*, Properties::*}, Foundation::DEVPROPKEY};

    struct Api {
        locate: unsafe extern "system" fn(*mut u32,*const u16,u32)->u32,
        property: unsafe extern "system" fn(u32,*const DEVPROPKEY,*mut u32,*mut u8,*mut u32,u32)->u32,
        parent: unsafe extern "system" fn(*mut u32,u32,u32)->u32,
        id: unsafe extern "system" fn(u32,*mut u16,u32,u32)->u32,
    }
    impl Api {
        fn native()->Self { Self {locate:CM_Locate_DevNodeW,property:CM_Get_DevNode_PropertyW,parent:CM_Get_Parent,id:CM_Get_Device_IDW} }
    }

    fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() }
    fn text(api:&Api,node: u32, key: &DEVPROPKEY) -> Option<String> {
        let mut value = [0u16; 2048];
        let mut size = (value.len() * 2) as u32;
        let mut kind = 0;
        let code = unsafe { (api.property)(node, key, &mut kind, value.as_mut_ptr().cast(), &mut size, 0) };
        if code != CR_SUCCESS || kind != DEVPROP_TYPE_STRING || size < 2 || size as usize > value.len() * 2 || size % 2 != 0 { return None; }
        let used = size as usize / 2;
        if value[used - 1] != 0 { return None; }
        String::from_utf16(&value[..used - 1]).ok().filter(|v| !v.is_empty())
    }
    fn container(api:&Api,node: u32) -> Option<String> {
        let mut value = windows_sys::core::GUID::from_u128(0);
        let mut size = std::mem::size_of_val(&value) as u32;
        let mut kind = 0;
        let code = unsafe { (api.property)(node, &DEVPKEY_Device_ContainerId, &mut kind, (&mut value as *mut windows_sys::core::GUID).cast(), &mut size, 0) };
        if code != CR_SUCCESS || kind != DEVPROP_TYPE_GUID || size as usize != std::mem::size_of_val(&value) { return None; }
        let tail = value.data4;
        Some(format!("{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",value.data1,value.data2,value.data3,tail[0],tail[1],tail[2],tail[3],tail[4],tail[5],tail[6],tail[7]))
    }
    pub(super) fn inspect(instance: &str) -> Device {
        inspect_with(&Api::native(),instance)
    }
    fn inspect_with(api:&Api,instance:&str)->Device {
        let mut result = Device { instance_id: instance.into(), name: instance.into(), ..Default::default() };
        let mut node = 0;
        let code = unsafe { (api.locate)(&mut node, wide(instance).as_ptr(), CM_LOCATE_DEVNODE_NORMAL) };
        if code == CR_SUCCESS {
            result.present = Some(true);
        } else if code == CR_NO_SUCH_DEVNODE {
            result.present = Some(false);
            if unsafe { (api.locate)(&mut node, wide(instance).as_ptr(), CM_LOCATE_DEVNODE_PHANTOM) } != CR_SUCCESS { return result; }
        } else {
            result.diagnostics.push(format!("PnP presence query failed with code {code}; presence is unknown."));
            return result;
        }
        result.name = text(api,node, &DEVPKEY_Device_FriendlyName).or_else(||text(api,node, &DEVPKEY_Device_BusReportedDeviceDesc)).or_else(||text(api,node, &DEVPKEY_Device_DeviceDesc)).unwrap_or(result.name);
        result.container_id = container(api,node);
        result.parent_instance_id = text(api,node, &DEVPKEY_Device_Parent);
        for _ in 0..8 {
            let mut parent = 0;
            if unsafe { (api.parent)(&mut parent, node, 0) } != CR_SUCCESS { break; }
            let mut value = [0u16; 1024];
            if unsafe { (api.id)(parent, value.as_mut_ptr(), value.len() as u32, 0) } != CR_SUCCESS { break; }
            let used = value.iter().position(|v| *v == 0).unwrap_or(value.len());
            let Ok(id) = String::from_utf16(&value[..used]) else { break; };
            result.ancestors.push(id);
            node = parent;
        }
        result
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        unsafe extern "system" fn locate(node:*mut u32,id:*const u16,flags:u32)->u32 {
            let length=(0..1024).find(|index|*id.add(*index)==0).unwrap();
            let id=String::from_utf16_lossy(std::slice::from_raw_parts(id,length));
            match id.as_str() {
                "present"=>{*node=1;CR_SUCCESS},
                "phantom" if flags==CM_LOCATE_DEVNODE_PHANTOM=>{*node=2;CR_SUCCESS},
                "phantom"|"absent"=>CR_NO_SUCH_DEVNODE,
                _=>CR_ACCESS_DENIED,
            }
        }
        unsafe extern "system" fn property(node:u32,key:*const DEVPROPKEY,kind:*mut u32,buffer:*mut u8,size:*mut u32,_:u32)->u32 {
            if node==99 {return CR_NO_SUCH_VALUE;}
            if (*key).fmtid.data1==DEVPKEY_Device_ContainerId.fmtid.data1 && (*key).pid==DEVPKEY_Device_ContainerId.pid {
                *kind=DEVPROP_TYPE_GUID;
                *size=16;
                *buffer.cast::<windows_sys::core::GUID>()=windows_sys::core::GUID::from_u128(0x00112233_4455_6677_8899_aabbccddeeff);
            } else {
                *kind=DEVPROP_TYPE_STRING;
                let text=if (*key).pid==DEVPKEY_Device_Parent.pid && (*key).fmtid.data1==DEVPKEY_Device_Parent.fmtid.data1 {"USB\\VERIFIED\\SERIAL"}else{"System controller"};
                let value=wide(text);
                *size=(value.len()*2) as u32;
                std::ptr::copy_nonoverlapping(value.as_ptr(),buffer.cast(),value.len());
            }
            CR_SUCCESS
        }
        unsafe extern "system" fn parent(result:*mut u32,node:u32,_:u32)->u32 {
            if node==1 {*result=2;CR_SUCCESS}else{CR_NO_SUCH_DEVNODE}
        }
        unsafe extern "system" fn id(_:u32,buffer:*mut u16,_:u32,_:u32)->u32 {
            let value=wide("USB\\VERIFIED\\SERIAL");
            std::ptr::copy_nonoverlapping(value.as_ptr(),buffer,value.len());CR_SUCCESS
        }
        fn api()->Api {Api {locate,property,parent,id}}
        #[test]
        fn pnp_presence_metadata_and_ancestors_are_read_without_controller_capture() {
            let device=inspect_with(&api(),"present");
            assert_eq!(device.present,Some(true));
            assert_eq!(device.name,"System controller");
            assert_eq!(device.container_id.as_deref(),Some("00112233-4455-6677-8899-aabbccddeeff"));
            assert_eq!(device.parent_instance_id.as_deref(),Some("USB\\VERIFIED\\SERIAL"));
            assert_eq!(device.ancestors,vec!["USB\\VERIFIED\\SERIAL"]);
            assert_eq!(inspect_with(&api(),"phantom").present,Some(false));
            assert_eq!(inspect_with(&api(),"absent").present,Some(false));
            let unknown=inspect_with(&api(),"denied");
            assert_eq!(unknown.present,None);
            assert!(!unknown.diagnostics.is_empty());
            assert_eq!(text(&api(),99,&DEVPKEY_Device_FriendlyName),None);
            assert_eq!(container(&api(),99),None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{Identity, ResourceDescriptor, SavedSource};
    fn sources() -> ResourceInventory {
        ResourceInventory { session:"test".into(), backend_status:"ready".into(), diagnostic:String::new(), sources:vec![ResourceDescriptor {
            saved:SavedSource {id:"controller-a".into(),name:"Hidden throttle".into(),role:String::new(),identity:Identity {backend:"windows-raw-input".into(),vendor_id:1,product_id:2,serial:None,collection:String::new(),fingerprint:String::new(),path:"interface-a".into()}},
            online:false,generation:1,baseline_ready:false,controls:vec![],diagnostics:vec![],
        }] }
    }
    fn inspect(id:&str)->Device {Device {name:id.into(),instance_id:id.into(),present:Some(true),..Default::default()}}
    #[test]
    fn configured_hidden_present_controller_survives_raw_input_absence_and_exact_instance_deduplicates() {
        let policy=policy::State {active:true,inverse:true,applications:vec![],hidden:vec![r"HID\EXACT\A".into(),r"HID\EXACT\EXTERNAL".into()]};
        let result=compose(sources(),Ok(policy), |_|Ok(r"hid\exact\a".into()),inspect);
        assert_eq!(result.hidden_devices.len(),2);
        assert!(result.policy_inspected);
        let controller=result.hidden_devices.iter().find(|d|d.controller_id.is_some()).unwrap();
        assert_eq!(controller.present,Some(true));
        assert_eq!(controller.hidden,Some(true));
        assert_eq!(controller.inverse,Some(true));
        assert_eq!(controller.controller_ids,vec!["controller-a"]);
        assert_eq!(controller.scope,"system-inspection");
    }
    #[test]
    fn unavailable_policy_preserves_unknown_visibility_and_pnp_presence() {
        let result=compose(sources(),Err("driver unavailable".into()), |_|Ok(r"HID\EXACT\A".into()), |id|Device {name:id.into(),instance_id:id.into(),..Default::default()});
        assert_eq!(result.hidden_devices[0].hidden,None);
        assert!(!result.policy_inspected);
        assert_eq!(result.hidden_devices[0].present,None);
        assert!(result.diagnostics[0].contains("unknown"));
    }
    #[test]
    fn inactive_policy_is_configured_but_not_effectively_hidden() {
        let state=policy::State {active:false,inverse:false,applications:vec![],hidden:vec![r"HID\EXACT\A".into()]};
        let result=compose(sources(),Ok(state), |_|Err("interface gone".into()),inspect);
        assert_eq!(result.hidden_devices[0].configured,Some(true));
        assert_eq!(result.hidden_devices[0].hidden,Some(false));
        assert!(result.hidden_devices[0].controller_id.is_none());
    }
    #[test]
    fn current_system_inspection_remains_read_only_when_driver_or_interface_is_unavailable() {
        let result=current(sources());
        assert!(result.hidden_devices.iter().all(|device|device.scope=="system-inspection"));
        #[cfg(windows)]
        assert!(result.diagnostics.iter().any(|message|message.contains("Hidden throttle")));
    }
}
