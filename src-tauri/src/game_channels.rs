//! Shared telemetry vocabulary. Native fields remain available; canonical
//! channels are projections with explicit units, never guesses from a label.
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(crate) struct Projection {
    pub id: &'static str,
    pub native: &'static str,
    pub scale: f64,
    pub offset: f64,
    label: &'static str,
    unit: &'static str,
    category: &'static str,
}
fn rule(id: &'static str, native: &'static str, label: &'static str, unit: &'static str, category: &'static str, scale: f64) -> Projection {
    Projection { id, native, label, unit, category, scale, offset: 0. }
}
pub(crate) fn projections(game: &str) -> Vec<Projection> {
    if game.starts_with("assetto-corsa") {
        let mut result = vec![
            rule("engine.rpm", "rpm", "Engine RPM", "rpm", "engine", 1.),
            rule("engine.max_rpm", "max_rpm", "Engine maximum RPM", "rpm", "engine", 1.),
            rule("engine.rev_ratio", "rev_ratio", "Engine rev ratio", "ratio", "engine", 1.),
            rule("vehicle.speed_mps", "speed_kmh", "Vehicle ground speed", "m/s", "motion", 1. / 3.6),
            rule("transmission.gear", "gear", "Current gear (reverse −1, neutral 0)", "", "drivetrain", 1.),
            rule("controls.throttle", "physics.gas", "Throttle", "ratio", "controls", 1.),
            rule("controls.brake", "physics.brake", "Brake", "ratio", "controls", 1.),
            rule("controls.clutch", "physics.clutch", "Clutch", "ratio", "controls", 1.),
            rule("environment.air_temperature_c", "physics.airTemp", "Air temperature", "°C", "environment", 1.),
            rule("environment.track_temperature_c", "physics.roadTemp", "Track temperature", "°C", "environment", 1.),
        ];
        result.push(if game == "assetto-corsa-competizione" {
            rule("vehicle.fuel_kg", "physics.fuel", "Fuel mass", "kg", "engine", 1.)
        } else { rule("vehicle.fuel_l", "physics.fuel", "Fuel volume", "L", "engine", 1.) });
        result
    } else if game.starts_with("x-plane-") {
        vec![
            rule("engine.rpm", "engine_1_rpm", "Engine RPM", "rpm", "engine", 1.),
            rule("engine.2.rpm", "engine_2_rpm", "Engine 2 RPM", "rpm", "engine", 1.),
            rule("vehicle.speed_mps", "sim/flightmodel/position/groundspeed", "Vehicle ground speed", "m/s", "motion", 1.),
            rule("aircraft.indicated_airspeed_mps", "airspeed_kts", "Indicated airspeed", "m/s", "motion", 0.5144444444444445),
            rule("aircraft.true_airspeed_mps", "sim/flightmodel/position/true_airspeed", "True airspeed", "m/s", "motion", 1.),
            rule("aircraft.altitude_msl_m", "sim/flightmodel/position/elevation", "Altitude above sea level", "m", "motion", 1.),
            rule("aircraft.indicated_altitude_m", "altitude_ft", "Indicated altitude", "m", "motion", 0.3048),
            rule("aircraft.vertical_speed_mps", "vertical_speed_fpm", "Vertical speed", "m/s", "motion", 0.00508),
            rule("aircraft.heading_magnetic_rad", "heading_deg", "Magnetic heading", "rad", "motion", std::f64::consts::PI / 180.),
            rule("aircraft.pitch_rad", "pitch_deg", "Pitch attitude", "rad", "motion", std::f64::consts::PI / 180.),
            rule("aircraft.roll_rad", "roll_deg", "Roll attitude", "rad", "motion", std::f64::consts::PI / 180.),
        ]
    } else if game == "dcs" {
        vec![
            rule("engine.rpm_percent", "flight.engine.left.speedPercent", "Engine speed percentage", "%", "engine", 1.),
            rule("engine.2.rpm_percent", "flight.engine.right.speedPercent", "Engine 2 speed percentage", "%", "engine", 1.),
            rule("aircraft.true_airspeed_mps", "flight.airspeed.true", "True airspeed", "m/s", "motion", 1.),
            rule("aircraft.altitude_msl_m", "flight.altitude.msl", "Altitude above sea level", "m", "motion", 1.),
            rule("aircraft.pitch_rad", "flight.attitude.pitch", "Pitch attitude", "rad", "motion", 1.),
            rule("aircraft.roll_rad", "flight.attitude.bank", "Roll attitude", "rad", "motion", 1.),
            rule("vehicle.fuel.internal_kg", "flight.fuel.internal", "Internal fuel mass", "kg", "engine", 1.),
            rule("vehicle.fuel.external_kg", "flight.fuel.external", "External fuel mass", "kg", "engine", 1.),
        ]
    } else { vec![] }
}
pub(crate) fn projection(game: &str, channel: &str) -> Option<Projection> {
    projections(game).into_iter().find(|rule| rule.id == channel)
}
pub(crate) fn convert(rule: Projection, value: &Value) -> Option<Value> {
    let value = value.as_f64()? * rule.scale + rule.offset;
    value.is_finite().then(|| json!(value))
}
fn g_sources(game: &str) -> Option<[&'static str; 3]> {
    if game.starts_with("assetto-corsa") {
        Some(["physics.accG.0", "physics.accG.1", "physics.accG.2"])
    } else if game.starts_with("x-plane-") {
        Some(["sim/flightmodel/forces/g_side", "sim/flightmodel/forces/g_nrml", "sim/flightmodel/forces/g_axil"])
    } else if game == "dcs" { Some(["flight.acceleration.x","flight.acceleration.y","flight.acceleration.z"]) }
    else { None }
}
/// This quantity is independent of native vector axis order/sign. Signed
/// native axes stay available until the provider's frame is qualified.
pub(crate) fn magnitude_sources(game: &str, channel: &str) -> Option<[&'static str; 3]> {
    (channel == "vehicle.g_force.magnitude").then(|| g_sources(game)).flatten()
}
pub(crate) fn native_ids(game: &str, ids: &[String]) -> Vec<String> {
    let mut result = std::collections::BTreeSet::new();
    for id in ids {
        if let Some(sources) = magnitude_sources(game, id) { result.extend(sources.map(str::to_owned)); }
        else { result.insert(projection(game, id).map_or(id.as_str(), |rule|rule.native).to_owned()); }
    }
    result.into_iter().collect()
}
pub(crate) fn source_rows(game: &str, rows: &[Value]) -> Vec<Value> {
    rows.iter().flat_map(|row| {
        let id = row["id"].as_str().unwrap_or("");
        let channel = row["channel"].as_str().unwrap_or("");
        if let Some(sources) = magnitude_sources(game, channel) {
            sources.into_iter().enumerate().map(|(index,channel)|json!({"id":format!("{id}/g{index}"),"channel":channel})).collect()
        } else {
            let mut row = row.clone();
            if let Some(rule) = projection(game, channel) { row["channel"] = json!(rule.native); }
            vec![row]
        }
    }).collect()
}
pub(crate) fn project_rows(game: &str, rows: &[Value], values: &mut BTreeMap<String,Value>) {
    for row in rows {
        let (Some(id),Some(channel)) = (row["id"].as_str(),row["channel"].as_str()) else { continue; };
        if magnitude_sources(game, channel).is_some() {
            if let Some(vector) = (0..3).map(|i|values.get(&format!("{id}/g{i}"))?.as_f64()).collect::<Option<Vec<_>>>() {
                let magnitude = vector.iter().fold(0_f64, |sum,value|sum.hypot(*value));
                if magnitude.is_finite() { values.insert(id.into(),json!(magnitude)); }
            }
        } else if let Some(rule) = projection(game,channel) {
            if let Some(value) = values.get(id).and_then(|value|convert(rule,value)) { values.insert(id.into(),value); }
            else { values.remove(id); }
        }
    }
}
pub(crate) fn catalogue(game: &str, catalogue: &mut Value) {
    let Some(channels) = catalogue["channels"].as_array_mut() else { return; };
    // Idempotent, including catalogue caches and adapter-local decoration.
    for projection in projections(game) {
        if channels.iter().any(|channel| channel["id"] == projection.id) { continue; }
        let Some(source) = channels.iter().find(|channel| channel["id"] == projection.native).cloned() else { continue; };
        if source["supported"] == false { continue; }
        let mut channel = source.clone();
        channel["id"] = json!(projection.id);
        channel["label"] = json!(projection.label);
        channel["unit"] = json!(projection.unit);
        channel["type"] = json!("number");
        channel["category"] = json!(projection.category);
        channel["group"] = json!(projection.category);
        channel["nativeId"] = json!(projection.native);
        channel["canonical"] = json!(true);
        channel["normalization"] = json!({"sourceChannels":[projection.native],"scale":projection.scale,"offset":projection.offset});
        for key in ["minimum", "maximum"] {
            channel[key] = json!(source[key].as_f64().unwrap_or(if key == "minimum" { -1e30 } else { 1e30 }) * projection.scale + projection.offset);
        }
        channels.push(channel);
    }
    if let Some(sources) = g_sources(game) {
        if !channels.iter().any(|channel| channel["id"] == "vehicle.g_force.magnitude")
            && sources.iter().all(|id| channels.iter().any(|channel| channel["id"] == *id && channel["supported"] != false)) {
            channels.push(json!({"id":"vehicle.g_force.magnitude","label":"G-force magnitude","unit":"g","type":"number","minimum":0.,"maximum":1e30,"group":"motion","category":"motion","canonical":true,"derived":true,"nativeField":sources.join(", "),"normalization":{"sourceChannels":sources,"operation":"euclidean-magnitude"},"description":"Magnitude of the reported three-axis g vector. Includes the provider's gravity convention. Signed native axes retain their documented coordinate frame."}));
        }
    }
    catalogue["channelSchema"] = json!({"name":"opensimgear.telemetry","version":1,"signedAxes":"Native coordinate frames are retained; no unverified axis/sign conversion is applied."});
}
pub(crate) fn values(game: &str, values: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    let mut result = values.clone();
    for rule in projections(game) {
        if let Some(value) = values.get(rule.native).and_then(|value| convert(rule, value)) {
            result.insert(rule.id.into(), value);
        }
    }
    if let Some(sources) = g_sources(game) {
        if let Some(vector) = sources.iter().map(|id| values.get(*id)?.as_f64()).collect::<Option<Vec<_>>>() {
            let magnitude = vector.iter().fold(0_f64, |sum, value| sum.hypot(*value));
            if magnitude.is_finite() { result.insert("vehicle.g_force.magnitude".into(), json!(magnitude)); }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn common_quantities_share_names_and_units_without_changing_legacy_values() {
        let ac = values("assetto-corsa", &BTreeMap::from([("rpm".into(),json!(4500)),("speed_kmh".into(),json!(72))]));
        let xp = values("x-plane-12", &BTreeMap::from([("engine_1_rpm".into(),json!(4500)),("sim/flightmodel/position/groundspeed".into(),json!(20))]));
        assert_eq!(ac["engine.rpm"], xp["engine.rpm"]);
        assert_eq!(ac["vehicle.speed_mps"], xp["vehicle.speed_mps"]);
        assert_eq!(ac["speed_kmh"], 72);
        assert!(values("dcs", &BTreeMap::from([("RPM_PERCENT".into(),json!(90))])).get("engine.rpm").is_none());
    }
    #[test]
    fn g_magnitude_requires_all_axes_and_zero_stays_known() {
        let mut native = BTreeMap::from([("physics.accG.0".into(),json!(0)),("physics.accG.1".into(),json!(3)),("physics.accG.2".into(),json!(-4))]);
        assert_eq!(values("assetto-corsa-evo", &native)["vehicle.g_force.magnitude"], 5.);
        native.remove("physics.accG.2");
        assert!(!values("assetto-corsa-evo", &native).contains_key("vehicle.g_force.magnitude"));
    }
    #[test]
    fn documented_true_airspeed_and_msl_altitude_match_flight_integrations() {
        let xp = values("x-plane-12", &BTreeMap::from([("sim/flightmodel/position/true_airspeed".into(),json!(120)),("sim/flightmodel/position/elevation".into(),json!(3000))]));
        let dcs = values("dcs", &BTreeMap::from([("flight.airspeed.true".into(),json!(120)),("flight.altitude.msl".into(),json!(3000))]));
        for id in ["aircraft.true_airspeed_mps","aircraft.altitude_msl_m"] { assert_eq!(xp[id], dcs[id]); }
        // Indicated altitude and indicated airspeed keep separate channels.
        assert!(!xp.contains_key("aircraft.indicated_altitude_m"));
        assert!(!xp.contains_key("aircraft.indicated_airspeed_mps"));
    }
    #[test]
    fn selected_profile_rows_use_the_same_canonical_conversion_as_inspection() {
        let rows = vec![json!({"id":"source/speed","channel":"aircraft.indicated_airspeed_mps"}),json!({"id":"source/g","channel":"vehicle.g_force.magnitude"})];
        let native = source_rows("x-plane-12",&rows);
        assert_eq!(native.len(),4);
        assert_eq!(native[0]["channel"],"airspeed_kts");
        let mut values = BTreeMap::from([("source/speed".into(),json!(100)),("source/g/g0".into(),json!(0)),("source/g/g1".into(),json!(3)),("source/g/g2".into(),json!(4))]);
        project_rows("x-plane-12",&rows,&mut values);
        assert!((values["source/speed"].as_f64().unwrap()-51.44444444444445).abs()<1e-10);
        assert_eq!(values["source/g"],5.);
    }
}
