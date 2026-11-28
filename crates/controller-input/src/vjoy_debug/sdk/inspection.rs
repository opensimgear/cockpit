use super::native::{Api, DLL_HASH, DRIVER_HASH};
use serde_json::{json, Value};

#[repr(C)]
struct PositionV3 {
    device: u8,
    words: [i32; 30],
}
const _: () = {
    assert!(std::mem::size_of::<PositionV3>() == 124);
    assert!(std::mem::align_of::<PositionV3>() == 4);
    assert!(std::mem::offset_of!(PositionV3, device) == 0);
    assert!(std::mem::offset_of!(PositionV3, words) == 4);
};

// Every axis exposed by the pinned SDK (including simulation-page usages).
pub(super) const AXES: [(u32, &str, u16); 16] = [
    (0x30, "X", 1),
    (0x31, "Y", 1),
    (0x32, "Z", 1),
    (0x33, "Rx", 1),
    (0x34, "Ry", 1),
    (0x35, "Rz", 1),
    (0x36, "Slider", 1),
    (0x37, "Dial", 1),
    (0x38, "Wheel", 1),
    (0xc4, "Accelerator", 2),
    (0xc5, "Brake", 2),
    (0xc6, "Clutch", 2),
    (0xc8, "Steering", 2),
    (0xb0, "Aileron", 2),
    (0xba, "Rudder", 2),
    (0xbb, "Throttle", 2),
];
pub(super) const EFFECTS: [(u32, &str); 12] = [
    (0x26, "constant"),
    (0x27, "ramp"),
    (0x30, "square"),
    (0x31, "sine"),
    (0x32, "triangle"),
    (0x33, "sawtoothUp"),
    (0x34, "sawtoothDown"),
    (0x40, "spring"),
    (0x41, "damper"),
    (0x42, "inertia"),
    (0x43, "friction"),
    (0x28, "custom"),
];

fn boolean(value: i32, name: &str) -> Result<bool, String> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        n => Err(format!("vJoy {name} query failed ({n}).")),
    }
}
fn bounded(value: i32, max: i32, name: &str) -> Result<i32, String> {
    if (0..=max).contains(&value) {
        Ok(value)
    } else {
        Err(format!("vJoy {name} query failed ({value})."))
    }
}
unsafe fn output(api: super::native::Out, max: i32, name: &str) -> Result<i32, String> {
    let mut value = -1;
    if api(&mut value) != 1 {
        return Err(format!("vJoy {name} query failed."));
    }
    bounded(value, max, name)
}

pub(super) fn inspect(api: &Api, slot: u8) -> Result<Value, String> {
    super::validate_slot(slot)?;
    api.check()?;
    unsafe {
        let id = u32::from(slot);
        let status_code = (api.status)(id);
        let status = match status_code {
            0 => "owned",
            1 => "free",
            2 => "busy",
            3 => "missing",
            _ => "unknown",
        };
        let exists = boolean((api.exists)(id), "slot existence")?;
        let mut result = json!({
            "slotId":slot, "status":status, "statusCode":status_code, "exists":exists,
            "sdkVersion":"2.2.2.0", "dllVersion":0x222, "driverVersion":0x222,
            "sdkSource":"bd268d1fa5934a900a79e76e519d4aa804e0ee3d",
            "dllSha256":DLL_HASH, "driverSha256":DRIVER_HASH,
            "driverForceFeedback":output(api.driver_ffb, 1, "driver FFB")? == 1,
            "maximumDevices":output(api.maximum_devices, 16, "maximum devices")?,
            "existingDevices":output(api.existing_devices, 16, "existing devices")?,
            "ownerPid":null, "captureAvailable":false, "forceFeedback":null,
            "buttons":null, "discreteHats":null, "continuousHats":null,
            "axes":[], "effects":[], "reportLayout":layout(),
            "configuredDescriptor":super::configured_descriptor::inspect(slot),
            "receiveLimitations":["One isolated SDK owner per capture; only FREE FFB slots can be acquired.",
                "FFB report device IDs use four bits; receive supports slots 1–15.",
                "The SDK exposes write/set-feature FFB requests, not every HID response or physical force."]
        });
        if !exists || status == "missing" || status == "unknown" {
            return Ok(result);
        }
        if matches!(status_code, 0 | 2) {
            let owner = (api.owner)(id);
            if owner <= 0 {
                return Err(format!("vJoy owner PID query failed ({owner})."));
            }
            result["ownerPid"] = json!(owner);
        }
        result["buttons"] = json!(bounded((api.buttons)(id), 128, "buttons")?);
        result["discreteHats"] = json!(bounded((api.discrete)(id), 4, "discrete hats")?);
        result["continuousHats"] = json!(bounded((api.continuous)(id), 4, "continuous hats")?);
        let ffb = boolean((api.ffb)(id), "FFB")?;
        result["forceFeedback"] = json!(ffb);
        result["captureAvailable"] = json!(status_code == 1 && ffb && slot <= 15);
        let mut axes = Vec::new();
        for (usage, name, usage_page) in AXES {
            match (api.axis)(id, usage) {
                1 => {
                    let (mut min, mut max) = (0, 0);
                    if (api.minimum)(id, usage, &mut min) != 1
                        || (api.maximum)(id, usage, &mut max) != 1
                        || min > max
                    {
                        return Err(format!("Cannot inspect vJoy axis {name} range."));
                    }
                    axes.push(json!({"usage":usage,"usagePage":usage_page,"name":name,"minimum":min,"maximum":max}));
                }
                0 | -10 => {}
                code => return Err(format!("vJoy axis {name} query failed ({code}).")),
            }
        }
        result["axes"] = json!(axes);
        let mut effects = Vec::new();
        for (usage, name) in EFFECTS {
            effects.push(json!({"usagePage":15,"usage":usage,"name":name,"supported":boolean((api.effect)(id, usage), "effect")?}));
        }
        result["effects"] = json!(effects);
        Ok(result)
    }
}

pub(super) fn layout() -> Value {
    let names = [
        "wThrottle",
        "wRudder",
        "wAileron",
        "wAxisX",
        "wAxisY",
        "wAxisZ",
        "wAxisXRot",
        "wAxisYRot",
        "wAxisZRot",
        "wSlider",
        "wDial",
        "wWheel",
        "wAccelerator",
        "wBrake",
        "wClutch",
        "wSteering",
        "wAxisVX",
        "wAxisVY",
        "lButtons",
        "bHats",
        "bHatsEx1",
        "bHatsEx2",
        "bHatsEx3",
        "lButtonsEx1",
        "lButtonsEx2",
        "lButtonsEx3",
        "wAxisVZ",
        "wAxisVBRX",
        "wAxisVBRY",
        "wAxisVBRZ",
    ];
    let fields: Vec<_> = names.iter().enumerate().map(|(index, name)| json!({"name":name,"offset":4+index*4,"bytes":4,"type":if name.starts_with("bHats") {"DWORD"} else {"LONG"}})).collect();
    json!({"name":"JOYSTICK_POSITION_V3","bytes":124,"alignment":4,"deviceIdOffset":0,"deviceIdBytes":1,"paddingBytes":3,"fields":fields,
        "ffbCallback":{"name":"FFB_DATA","bytes":16,"alignment":8,"sizeOffset":0,"commandOffset":4,"dataPointerOffset":8,"wireHeaderBytes":8,"maximumWireBytes":256,"callingConvention":"CALLBACK (Win64)","exportCallingConvention":"__cdecl"}})
}
