//! Portable debug controls share the canonical controller value domains.
use crate::{Direction, Port, PortContract, PortValue, Resource};
use osg_mapping::signal::ScalarSchema;

pub fn neutral(port: &Port) -> Result<PortValue, String> {
    let index = |prefix: &str, maximum: u16| {
        port.id.strip_prefix(prefix).and_then(|value| {
            value
                .parse::<u16>()
                .ok()
                .filter(|index| *index > 0 && *index <= maximum && index.to_string() == value)
        })
    };
    let PortContract::State {
        schema,
        fallback: Some(fallback),
        ..
    } = &port.contract
    else {
        return Err("Debug controls require scalar state contracts with neutral fallbacks.".into());
    };
    let expected = if index("button-", 128).is_some() && matches!(schema, ScalarSchema::Boolean {})
    {
        Some(PortValue::Boolean(false))
    } else if index("axis-", 16).is_some()
        && matches!(
            schema,
            ScalarSchema::Integer {
                min: -32768 | 0,
                max: 32767,
                unit: None
            }
        )
    {
        Some(PortValue::Number(0.0))
    } else if index("hat-", 4).is_some()
        && matches!(
            schema,
            ScalarSchema::Integer {
                min: -1,
                max: 7,
                unit: None
            }
        )
    {
        Some(PortValue::Number(-1.0))
    } else {
        None
    };
    if port.direction != Direction::Output || expected.as_ref() != Some(fallback) {
        return Err("Use canonical debug axis, button or hat output state ports and their neutral fallbacks.".into());
    }
    Ok(expected.unwrap())
}

pub fn validate(resource: &Resource) -> Result<(), String> {
    if resource.ports.is_empty() || resource.ports.len() > 128 {
        return Err("Debug controllers require one through 128 controls.".into());
    }
    for port in &resource.ports {
        neutral(port)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "debug_controller_tests.rs"]
mod tests;
