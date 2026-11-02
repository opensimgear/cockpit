struct Channel {
    id: &'static str,
    label: &'static str,
    dataref: &'static str,
    unit: &'static str,
    kind: &'static str,
    min: f64,
    max: f64,
}
const CHANNELS: &[Channel] = &[
    Channel {
        id: "airspeed_kts",
        label: "Indicated airspeed",
        dataref: "sim/flightmodel/position/indicated_airspeed",
        unit: "kt",
        kind: "number",
        min: 0.,
        max: 3000.,
    },
    Channel {
        id: "altitude_ft",
        label: "Indicated altitude",
        dataref: "sim/cockpit2/gauges/indicators/altitude_ft_pilot",
        unit: "ft",
        kind: "number",
        min: -3000.,
        max: 300000.,
    },
    Channel {
        id: "heading_deg",
        label: "Magnetic heading",
        dataref: "sim/flightmodel/position/mag_psi",
        unit: "deg",
        kind: "number",
        min: 0.,
        max: 360.,
    },
    Channel {
        id: "pitch_deg",
        label: "Pitch",
        dataref: "sim/flightmodel/position/theta",
        unit: "deg",
        kind: "number",
        min: -180.,
        max: 180.,
    },
    Channel {
        id: "roll_deg",
        label: "Roll",
        dataref: "sim/flightmodel/position/phi",
        unit: "deg",
        kind: "number",
        min: -180.,
        max: 180.,
    },
    Channel {
        id: "vertical_speed_fpm",
        label: "Vertical speed",
        dataref: "sim/cockpit2/gauges/indicators/vvi_fpm_pilot",
        unit: "ft/min",
        kind: "number",
        min: -100000.,
        max: 100000.,
    },
    Channel {
        id: "engine_1_rpm",
        label: "Engine 1 RPM",
        dataref: "sim/cockpit2/engine/indicators/engine_speed_rpm[0]",
        unit: "rpm",
        kind: "number",
        min: 0.,
        max: 100000.,
    },
    Channel {
        id: "engine_2_rpm",
        label: "Engine 2 RPM",
        dataref: "sim/cockpit2/engine/indicators/engine_speed_rpm[1]",
        unit: "rpm",
        kind: "number",
        min: 0.,
        max: 100000.,
    },
    Channel {
        id: "gear_ratio",
        label: "Gear deployment (first gear)",
        dataref: "sim/flightmodel2/gear/deploy_ratio[0]",
        unit: "ratio",
        kind: "number",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "flaps_ratio",
        label: "Flap handle",
        dataref: "sim/cockpit2/controls/flap_ratio",
        unit: "ratio",
        kind: "number",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "gear_unsafe",
        label: "Gear unsafe",
        dataref: "sim/cockpit2/annunciators/gear_unsafe",
        unit: "",
        kind: "boolean",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "stall_warning",
        label: "Stall warning",
        dataref: "sim/cockpit2/annunciators/stall_warning",
        unit: "",
        kind: "boolean",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "low_fuel",
        label: "Low fuel",
        dataref: "sim/cockpit2/annunciators/fuel_quantity",
        unit: "",
        kind: "boolean",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "landing_lights",
        label: "Landing lights switch",
        dataref: "sim/cockpit/electrical/landing_lights_on",
        unit: "",
        kind: "boolean",
        min: 0.,
        max: 1.,
    },
    Channel {
        id: "beacon_lights",
        label: "Beacon switch",
        dataref: "sim/cockpit/electrical/beacon_lights_on",
        unit: "",
        kind: "boolean",
        min: 0.,
        max: 1.,
    },
];
const GUARDS: &[&str] = &[
    "sim/version/xplane_internal_version",
    "sim/time/paused",
    "sim/time/is_in_replay",
    "sim/time/total_running_time_sec",
    "sim/aircraft/engine/acf_num_engines",
];

pub(crate) fn catalogue() -> Value {
    json!({"provider":PROVIDER,"revision":2,"commands":[],"channels":legacy_channels().iter().map(datarefs::Channel::value).collect::<Vec<_>>()})
}
fn legacy_channels() -> Vec<datarefs::Channel> {
    CHANNELS.iter().map(|c| datarefs::Channel {
        id:c.id.into(), label:c.label.into(), dataref:c.dataref.into(), unit:c.unit.into(), kind:c.kind.into(),
        minimum:Some(c.min), maximum:Some(c.max), integer:false,
        metadata:json!({"nativeField":c.dataref,"group":c.dataref.rsplit_once('/').map(|(group,_)|group).unwrap_or("sim"),"supported":true,"readOnly":true,"legacyAlias":true,"source":"https://developer.x-plane.com/sdk/XPLMDataAccess/"})
    }).collect()
}
/// Complete numeric metadata comes from the selected installation; no UDP
/// subscription is created while browsing its catalogue.
pub(crate) fn catalogue_for_installation(executable: &std::path::Path) -> Result<Value, String> {
    let native = datarefs::load(executable)?;
    let available = native.iter().filter(|c| c.metadata["supported"] == true)
        .map(|c| c.dataref.as_str()).collect::<BTreeSet<_>>();
    let mut channels = legacy_channels();
    for channel in &mut channels {
        channel.metadata["supported"] = json!(available.contains(channel.dataref.as_str()));
    }
    channels.extend(native);
    Ok(json!({"provider":PROVIDER,"revision":2,"commands":[],"channels":channels.iter().map(datarefs::Channel::value).collect::<Vec<_>>(),"catalogueSource":"Resources/plugins/DataRefs.txt","transport":"RREF","transportPrecision":"All RREF responses are IEEE-754 float32, including integer and double datarefs."}))
}
pub(crate) fn defaults(version: u64) -> Value {
    json!({"source":"rref","host":"127.0.0.1","port":49000,"listenPort":0,"updateHz":10,"freshnessMs":1500,"version":version})
}
pub(crate) fn validate_connection(v: &Value) -> Result<(), String> {
    if v["source"] != "rref" || !matches!(v["version"].as_u64(), Some(11 | 12)) {
        return Err("Choose X-Plane 11 or 12 and the RREF UDP source.".into());
    }
    // Local process monitoring is required; remote UDP cannot prove its producer.
    if v["host"]
        .as_str()
        .and_then(|s| s.parse::<Ipv4Addr>().ok())
        .is_none_or(|ip| !ip.is_loopback())
    {
        return Err(
            "Use a local IPv4 loopback address for the monitored X-Plane installation.".into(),
        );
    }
    for (key, min, max) in [
        ("port", 1, 65535),
        ("listenPort", 0, 65535),
        ("updateHz", 1, 20),
        ("freshnessMs", 100, 10000),
    ] {
        if v[key].as_u64().is_none_or(|n| n < min || n > max) {
            return Err(format!("Invalid X-Plane {key} ({min}–{max})."));
        }
    }
    if v["freshnessMs"].as_u64().unwrap() * v["updateHz"].as_u64().unwrap() < 2000 {
        return Err("Freshness must allow at least two requested updates.".into());
    }
    Ok(())
}
