//! Translate existing validated provider descriptors without inventing controls.
use osg_profiles::resources_ports::{
    ActionParameter, ActionSemantics, DisplayConstraints, OwnershipCapability, PortContract,
    PortDescriptor, ScalarSchema, TextEncoding,
};
use osg_profiles::{Direction, PortValue};
use serde_json::Value;

pub(crate) fn state(
    id: String,
    name: String,
    direction: Direction,
    schema: ScalarSchema,
    fallback: Option<PortValue>,
) -> PortDescriptor {
    PortDescriptor {
        id,
        name,
        direction,
        contract: PortContract::State {
            schema,
            freshness_ms: 1500,
            fallback,
        },
    }
}

pub(crate) fn controls(
    controls: &[osg_controller_input::registry::Control],
) -> Vec<PortDescriptor> {
    use osg_controller_input::registry::Kind;
    controls
        .iter()
        .filter_map(|control| {
            let (schema, fallback) = match control.kind {
                Kind::Button => (ScalarSchema::Boolean {}, Some(PortValue::Boolean(false))),
                Kind::Axis => (
                    ScalarSchema::Integer {
                        min: control.min,
                        max: control.max,
                        unit: None,
                    },
                    None,
                ),
                Kind::Hat => (
                    ScalarSchema::Integer {
                        min: -1,
                        max: 7,
                        unit: None,
                    },
                    Some(PortValue::Number(-1.0)),
                ),
                Kind::Diagnostic => return None,
            };
            Some(state(
                control.id.clone(),
                control.id.clone(),
                Direction::Output,
                schema,
                fallback,
            ))
        })
        .collect()
}

pub(crate) fn events(events: &Value) -> Result<Vec<PortDescriptor>, String> {
    crate::panel_events::validate(events, None)?;
    let mut result = Vec::new();
    // Events expose ordered button transitions and current axes, never hats.
    for index in 0..events["buttonCount"].as_u64().unwrap() {
        let id = format!("button:{index}");
        result.push(state(
            id.clone(),
            format!("Button {}", index + 1),
            Direction::Output,
            ScalarSchema::Boolean {},
            Some(PortValue::Boolean(false)),
        ));
        result.push(PortDescriptor {
            id: format!("button-event:{index}"),
            name: format!("Button {} transition", index + 1),
            direction: Direction::Output,
            contract: PortContract::Event {
                schema: ScalarSchema::Boolean {},
            },
        });
    }
    for index in 0..events["axisCount"].as_u64().unwrap_or(0) {
        result.push(state(
            format!("axis:{index}"),
            format!("Axis {}", index + 1),
            Direction::Output,
            ScalarSchema::Integer {
                min: 0,
                max: 65535,
                unit: None,
            },
            events["axisNeutral"][index as usize]
                .as_u64()
                .map(|v| PortValue::Number(v as f64)),
        ));
    }
    Ok(result)
}

pub(crate) fn actions(cache: &Value) -> Result<Vec<PortDescriptor>, String> {
    let mut ports = Vec::new();
    let ownership = cache
        .get("ownership")
        .filter(|value| !value.is_null())
        .map(crate::output_leases::capability)
        .transpose()?;
    for action in cache["actions"].as_array().into_iter().flatten() {
        crate::actions::descriptor(action)?;
        let display = action.get("display").map(|d| DisplayConstraints {
            columns: d["columns"].as_u64().unwrap() as u32,
            rows: d["rows"].as_u64().unwrap() as u32,
            encoding: if d["encoding"] == "ascii" {
                TextEncoding::Ascii
            } else {
                TextEncoding::Utf8
            },
        });
        let parameters = action["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let schema = match p["type"].as_str().unwrap() {
                    "boolean" => ScalarSchema::Boolean {},
                    "integer" => ScalarSchema::Integer {
                        min: p["minimum"].as_i64().unwrap(),
                        max: p["maximum"].as_i64().unwrap(),
                        unit: None,
                    },
                    "enum" => ScalarSchema::Enum {
                        values: p["options"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_str().unwrap().to_owned())
                            .collect(),
                    },
                    "rgb" => ScalarSchema::Rgb {},
                    _ => ScalarSchema::Text {
                        max_bytes: p["maxBytes"].as_u64().unwrap() as u32,
                        encoding: display.as_ref().map_or(TextEncoding::Utf8, |d| d.encoding),
                    },
                };
                ActionParameter {
                    id: p["id"].as_str().unwrap().into(),
                    name: p["label"].as_str().unwrap().into(),
                    schema,
                }
            })
            .collect();
        let port = PortDescriptor {
            id: format!("action:{}", action["id"].as_str().unwrap()),
            name: action["label"].as_str().unwrap().into(),
            direction: Direction::Input,
            contract: PortContract::Action {
                parameters,
                semantics: if action["semantics"] == "setter" {
                    ActionSemantics::Setter
                } else {
                    ActionSemantics::OneShot
                },
                endpoint: action["endpoint"].as_str().unwrap().into(),
                readable: action["readable"].as_bool().unwrap(),
                display,
                ownership: ownership
                    .as_ref()
                    .filter(|_| action["readable"] == true && action["semantics"] == "setter")
                    .map(|capability| OwnershipCapability {
                        scope: capability["scope"].as_str().unwrap().into(),
                        max_lease_ms: capability["maxLeaseMs"].as_u64().unwrap() as u32,
                    }),
            },
        };
        port.validate()?;
        ports.push(port);
    }
    Ok(ports)
}

pub(crate) fn integration(catalogue: &Value) -> Result<Vec<PortDescriptor>, String> {
    let mut result = Vec::new();
    if let Some(channels) = catalogue["channels"].as_array() {
        for channel in channels {
            let id = channel["id"]
                .as_str()
                .ok_or("Integration channel identity is missing.")?;
            let unit = channel["unit"]
                .as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_owned);
            let schema = match channel["type"].as_str() {
                Some("boolean") => ScalarSchema::Boolean {},
                Some("string") => ScalarSchema::Text {
                    max_bytes: 256,
                    encoding: TextEncoding::Utf8,
                },
                Some("integer") => ScalarSchema::Integer {
                    min: channel["minimum"]
                        .as_f64()
                        .filter(|v| v.is_finite() && v.fract() == 0.0)
                        .ok_or("Integration integer range is missing.")?
                        as i64,
                    max: channel["maximum"]
                        .as_f64()
                        .filter(|v| v.is_finite() && v.fract() == 0.0)
                        .ok_or("Integration integer range is missing.")?
                        as i64,
                    unit,
                },
                Some("number") => ScalarSchema::Number {
                    min: channel["minimum"]
                        .as_f64()
                        .ok_or("Integration numeric range is missing.")?,
                    max: channel["maximum"]
                        .as_f64()
                        .ok_or("Integration numeric range is missing.")?,
                    unit,
                },
                _ => return Err("Unsupported integration channel type.".into()),
            };
            result.push(state(
                format!("channel:{id}"),
                channel["label"].as_str().unwrap_or(id).into(),
                Direction::Output,
                schema,
                None,
            ));
        }
    }
    for control in catalogue["controls"].as_array().into_iter().flatten() {
        let id = control["id"]
            .as_str()
            .ok_or("DCS catalogue identity is missing.")?;
        let name = control["label"].as_str().unwrap_or(id);
        for output in control["outputs"].as_array().into_iter().flatten() {
            let schema = if output["type"] == "string" {
                ScalarSchema::Text {
                    max_bytes: output["maxLength"]
                        .as_u64()
                        .ok_or("Missing DCS string length.")? as u32,
                    encoding: TextEncoding::Utf8,
                }
            } else {
                ScalarSchema::Integer {
                    min: 0,
                    max: output["maxValue"]
                        .as_i64()
                        .ok_or("Missing DCS numeric range.")?,
                    unit: None,
                }
            };
            result.push(state(
                format!("channel:{id}:{}", output["index"]),
                name.into(),
                Direction::Output,
                schema,
                None,
            ));
        }
        for input in control["inputs"].as_array().into_iter().flatten() {
            let interface = input["interface"]
                .as_str()
                .ok_or("Missing DCS command interface.")?;
            let schema = match interface {
                "set_state" => ScalarSchema::Integer {
                    min: 0,
                    max: input["maxValue"]
                        .as_i64()
                        .ok_or("Missing DCS command range.")?,
                    unit: None,
                },
                "variable_step" => {
                    let max = input["maxValue"]
                        .as_i64()
                        .ok_or("Missing DCS variable-step range.")?;
                    ScalarSchema::Integer {
                        min: -max,
                        max,
                        unit: None,
                    }
                }
                "fixed_step" => ScalarSchema::Enum {
                    values: vec!["INC".into(), "DEC".into()],
                },
                _ => continue,
            };
            result.push(PortDescriptor {
                id: format!("command:{id}:{interface}"),
                name: format!("{name} ({interface})"),
                direction: Direction::Input,
                contract: PortContract::Action {
                    parameters: vec![ActionParameter {
                        id: "value".into(),
                        name: "Value".into(),
                        schema,
                    }],
                    semantics: if interface == "set_state" {
                        ActionSemantics::Setter
                    } else {
                        ActionSemantics::OneShot
                    },
                    endpoint: id.into(),
                    readable: false,
                    display: None,
                    ownership: None,
                },
            });
        }
    }
    for port in &result {
        port.validate()?;
    }
    Ok(result)
}

#[cfg(test)]
#[path = "profile_resource_ports_tests.rs"]
mod tests;
