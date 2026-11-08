use super::{capture, Device, Mac, NativeDevice, NativeHandle, Provider, Wake};
use crate::{
    capture::{macos::Topology, Shared},
    visibility::suppression::Control,
};
use iohidmanager::hid::{
    DeviceMatch, HidDevice, HidElementType, HidManager, HidManagerOptions, HidUsage,
};
use std::sync::{atomic::AtomicBool, Arc};

fn manager() -> Result<HidManager, String> {
    let manager = HidManager::with_options(
        HidManagerOptions::DO_NOT_LOAD_PROPERTIES | HidManagerOptions::DO_NOT_SAVE_PROPERTIES,
    )
    .map_err(|error| capture::permission_error("macos", error))?;
    manager
        .set_device_matching_multiple(&[
            DeviceMatch::usage(HidUsage::Joystick),
            DeviceMatch::usage(HidUsage::GamePad),
            DeviceMatch::usage(HidUsage::Custom(1, 8)),
        ])
        .map_err(|error| error.to_string())?;
    Ok(manager)
}

pub(super) struct NativeProvider(pub(super) Arc<Wake>);

impl Provider for NativeProvider {
    fn scan(&mut self) -> Result<Vec<(String, Box<dyn Device>)>, String> {
        let manager = manager()?;
        let mut devices: Vec<(String, Box<dyn Device>)> = Vec::new();
        for device in manager.live_devices() {
            if devices.len() >= 1024 {
                return Err("Too many IOHID interfaces.".into());
            }
            let id = device.id().map_err(|error| error.to_string())?;
            // Own a separate handle that survives the enumeration manager.
            let device = HidDevice::from_id(id).map_err(|error| error.to_string())?;
            let native = device
                .elements()
                .into_iter()
                .filter(|element| {
                    matches!(
                        element.element_kind(),
                        HidElementType::InputButton
                            | HidElementType::InputAxis
                            | HidElementType::InputMisc
                    )
                })
                .collect();
            devices.push((
                id.0.to_string(),
                Box::new(NativeDevice {
                    handle: NativeHandle {
                        device,
                        native,
                        ready: Arc::default(),
                        wake: self.0.clone(),
                    },
                    opened: false,
                }),
            ));
        }
        Ok(devices)
    }
}

pub(crate) fn run(
    shared: Shared,
    stop: Arc<AtomicBool>,
    control: Control,
    wake: Arc<Wake>,
) -> Result<(), String> {
    let topology = Arc::new(Topology::new(wake.clone()));
    let manager = manager()?;
    // Start removal first, then matching, before enumerating any live handles.
    let removal = topology.clone();
    let _removal = manager
        .on_device_removal(move |device| {
            removal.removed(
                device
                    .id()
                    .map(|id| id.0.to_string())
                    .map_err(|error| format!("Cannot identify a removed IOHID device: {error}")),
            );
        })
        .map_err(|error| capture::permission_error("macos", error))?;
    let arrival = topology.clone();
    let _arrival = manager
        .on_device_matching(move |_| arrival.arrived())
        .map_err(|error| capture::permission_error("macos", error))?;
    capture::macos::run(
        Mac::with_wake(wake.clone()),
        shared,
        &stop,
        control,
        &wake,
        &topology,
    )
}
