//! Lossless provider port contracts. These describe capabilities, not execution.
use crate::{Direction, Port, PortValue, ValueType};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_CATALOGUE_PORTS: usize = 20_000;
pub const MAX_PORT_PAGE: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextEncoding {
    Utf8,
    Ascii,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ScalarSchema {
    Boolean {},
    Integer {
        min: i64,
        max: i64,
        #[serde(deserialize_with = "required_option")]
        unit: Option<String>,
    },
    Number {
        min: f64,
        max: f64,
        #[serde(deserialize_with = "required_option")]
        unit: Option<String>,
    },
    Text {
        #[serde(rename = "maxBytes")]
        max_bytes: u32,
        encoding: TextEncoding,
    },
    Enum {
        values: Vec<String>,
    },
    Rgb {},
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionParameter {
    pub id: String,
    pub name: String,
    pub schema: ScalarSchema,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionSemantics {
    Setter,
    OneShot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayConstraints {
    pub columns: u32,
    pub rows: u32,
    pub encoding: TextEncoding,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnershipCapability {
    pub scope: String,
    pub max_lease_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "delivery", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PortContract {
    State {
        schema: ScalarSchema,
        #[serde(rename = "freshnessMs")]
        freshness_ms: u32,
        #[serde(deserialize_with = "required_option")]
        fallback: Option<PortValue>,
    },
    Event {
        schema: ScalarSchema,
    },
    Action {
        parameters: Vec<ActionParameter>,
        semantics: ActionSemantics,
        endpoint: String,
        readable: bool,
        #[serde(deserialize_with = "required_option")]
        display: Option<DisplayConstraints>,
        #[serde(deserialize_with = "required_option")]
        ownership: Option<OwnershipCapability>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortDescriptor {
    pub id: String,
    pub name: String,
    pub direction: Direction,
    pub contract: PortContract,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortUpdate {
    pub revision: String,
    pub total: u32,
    pub offset: u32,
    pub complete: bool,
    pub ports: Vec<PortDescriptor>,
}

impl PortUpdate {
    pub fn complete(ports: Vec<PortDescriptor>) -> Result<Self, String> {
        let update = Self {
            revision: catalogue_hash(&ports)?,
            total: ports.len() as u32,
            offset: 0,
            complete: true,
            ports,
        };
        update.validate()?;
        Ok(update)
    }
    pub fn validate(&self) -> Result<(), String> {
        text(&self.revision, 128)?;
        if self.total as usize > MAX_CATALOGUE_PORTS
            || self.offset > self.total
            || self.ports.len() > self.total.saturating_sub(self.offset) as usize
            || (self.complete && (self.offset != 0 || self.ports.len() != self.total as usize))
            || (!self.complete && self.ports.len() > MAX_PORT_PAGE)
        {
            return Err("Invalid bounded resource port page.".into());
        }
        validate_ports(&self.ports)
    }
}

pub fn text(value: &str, max: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err("Invalid bounded resource text.".into());
    }
    Ok(())
}

impl ScalarSchema {
    pub fn validate(&self) -> Result<(), String> {
        let unit = match self {
            Self::Integer { min, max, unit } => {
                if min > max
                    || *min < -(crate::MAX_REVISION as i64)
                    || *max > crate::MAX_REVISION as i64
                {
                    return Err("Invalid integer signal range.".into());
                }
                unit
            }
            Self::Number { min, max, unit } => {
                if !min.is_finite()
                    || !max.is_finite()
                    || min > max
                    || min.abs() > crate::MAX_REVISION as f64
                    || max.abs() > crate::MAX_REVISION as f64
                {
                    return Err("Invalid numeric signal range.".into());
                }
                unit
            }
            Self::Text { max_bytes, .. } => {
                if !(1..=4096).contains(max_bytes) {
                    return Err("Invalid text signal byte limit.".into());
                }
                return Ok(());
            }
            Self::Enum { values } => {
                if values.is_empty() || values.len() > 64 {
                    return Err("Invalid enum signal limit.".into());
                }
                let mut seen = BTreeSet::new();
                for value in values {
                    text(value, 256)?;
                    if !seen.insert(value) {
                        return Err("Duplicate signal enum value.".into());
                    }
                }
                return Ok(());
            }
            Self::Boolean {} | Self::Rgb {} => return Ok(()),
        };
        if let Some(unit) = unit {
            text(unit, 128)?;
        }
        Ok(())
    }
    pub fn accepts(&self, value: &PortValue) -> bool {
        match (self, value) {
            (Self::Boolean {}, PortValue::Boolean(_)) => true,
            (Self::Integer { min, max, .. }, PortValue::Number(value)) => {
                value.is_finite()
                    && value.fract() == 0.0
                    && *value >= *min as f64
                    && *value <= *max as f64
            }
            (Self::Number { min, max, .. }, PortValue::Number(value)) => {
                value.is_finite() && value >= min && value <= max
            }
            (
                Self::Text {
                    max_bytes,
                    encoding,
                },
                PortValue::Text(value),
            ) => {
                value.len() <= *max_bytes as usize
                    && !value.chars().any(char::is_control)
                    && (*encoding != TextEncoding::Ascii || value.is_ascii())
            }
            (Self::Enum { values }, PortValue::Text(value)) => values.contains(value),
            (Self::Rgb {}, PortValue::Text(value)) => {
                value.len() == 6 && value.bytes().all(|b| b.is_ascii_hexdigit())
            }
            _ => false,
        }
    }
    fn matches_graph(&self, port: &Port) -> bool {
        match self {
            Self::Boolean {} => {
                port.value_type == ValueType::Boolean
                    && port.unit.is_none()
                    && port.range.is_none()
                    && port.enum_values.is_empty()
            }
            Self::Integer { min, max, unit } => {
                port.value_type == ValueType::Integer
                    && port.unit == *unit
                    && port
                        .range
                        .as_ref()
                        .is_some_and(|r| r.min == *min as f64 && r.max == *max as f64)
            }
            Self::Number { min, max, unit } => {
                port.value_type == ValueType::Number
                    && port.unit == *unit
                    && port
                        .range
                        .as_ref()
                        .is_some_and(|r| r.min == *min && r.max == *max)
            }
            Self::Enum { values } => {
                port.value_type == ValueType::Enum
                    && port.enum_values.iter().collect::<BTreeSet<_>>()
                        == values.iter().collect::<BTreeSet<_>>()
            }
            // P0's text scalar has no byte/encoding/RGB constraints. Do not
            // erase those constraints while creating an apparently exact bind.
            Self::Text { .. } | Self::Rgb {} => false,
        }
    }
}

impl PortDescriptor {
    pub fn validate(&self) -> Result<(), String> {
        text(&self.id, 256)?;
        text(&self.name, 320)?;
        match &self.contract {
            PortContract::State {
                schema,
                freshness_ms,
                fallback,
            } => {
                schema.validate()?;
                if !(1..=600_000).contains(freshness_ms)
                    || fallback.as_ref().is_some_and(|v| !schema.accepts(v))
                {
                    return Err("Invalid state freshness or fallback.".into());
                }
            }
            PortContract::Event { schema } => schema.validate()?,
            PortContract::Action {
                parameters,
                endpoint,
                semantics,
                readable,
                display,
                ownership,
            } => {
                if self.direction != Direction::Input || parameters.len() > 4 {
                    return Err("Actions require an input port and at most four arguments.".into());
                }
                text(endpoint, 256)?;
                let mut ids = BTreeSet::new();
                for parameter in parameters {
                    text(&parameter.id, 80)?;
                    text(&parameter.name, 320)?;
                    parameter.schema.validate()?;
                    if !ids.insert(&parameter.id) {
                        return Err("Duplicate action argument.".into());
                    }
                }
                if let Some(display) = display {
                    if !(1..=63).contains(&display.columns)
                        || !(1..=4).contains(&display.rows)
                        || display.columns * display.rows > 63
                        || *semantics != ActionSemantics::Setter
                        || !readable
                        || parameters.len() != 1
                    {
                        return Err("Invalid display action constraints.".into());
                    }
                    if !matches!(&parameters[0].schema, ScalarSchema::Text { max_bytes, encoding } if *max_bytes >= display.columns * display.rows && *encoding == display.encoding)
                    {
                        return Err("Display constraints do not match its text argument.".into());
                    }
                }
                if let Some(ownership) = ownership {
                    if ownership.scope != "endpoint"
                        || ownership.max_lease_ms == 0
                        || ownership.max_lease_ms > 60_000
                    {
                        return Err("Invalid advertised output ownership.".into());
                    }
                }
            }
        }
        Ok(())
    }
    pub fn contract_hash(&self) -> Result<String, String> {
        self.validate()?;
        // Parameter order is significant. Enum order is retained conservatively.
        digest(&(self.direction, &self.contract))
    }
    pub fn graph_compatible(&self, port: &Port) -> bool {
        if self.direction != port.direction {
            return false;
        }
        match &self.contract {
            // The profile's explicit freshness/fallback is local graph policy;
            // the expectation hash still pins the provider's complete contract.
            PortContract::State { schema, .. } => {
                port.delivery == crate::Delivery::State && schema.matches_graph(port)
            }
            PortContract::Event { schema } => {
                port.delivery == crate::Delivery::Event && schema.matches_graph(port)
            }
            PortContract::Action { parameters, .. } => {
                port.delivery == crate::Delivery::Action
                    && parameters.len() == 1
                    && parameters[0].schema.matches_graph(port)
            }
        }
    }
    pub fn validate_arguments(&self, values: &[PortValue]) -> Result<(), String> {
        self.validate()?;
        let PortContract::Action {
            parameters,
            display,
            ..
        } = &self.contract
        else {
            return Err("This resource port does not accept commands.".into());
        };
        if parameters.len() != values.len() {
            return Err("Action argument count does not match its advertised contract.".into());
        }
        for (parameter, value) in parameters.iter().zip(values) {
            if !parameter.schema.accepts(value) {
                return Err(format!(
                    "Invalid value for action argument {}.",
                    parameter.id
                ));
            }
        }
        if let Some(display) = display {
            if let PortValue::Text(value) = &values[0] {
                if value.chars().count() > (display.columns * display.rows) as usize {
                    return Err("Text exceeds the advertised display dimensions.".into());
                }
            }
        }
        Ok(())
    }
}

pub fn validate_ports(ports: &[PortDescriptor]) -> Result<(), String> {
    if ports.len() > MAX_CATALOGUE_PORTS {
        return Err("Resource catalogue exceeds 20000 ports.".into());
    }
    let mut ids = BTreeSet::new();
    for port in ports {
        port.validate()?;
        if !ids.insert(&port.id) {
            return Err("Duplicate resource port identity.".into());
        }
    }
    Ok(())
}

pub fn catalogue_hash(ports: &[PortDescriptor]) -> Result<String, String> {
    validate_ports(ports)?;
    let sorted = ports
        .iter()
        .map(|p| Ok((&p.id, p.contract_hash()?)))
        .collect::<Result<std::collections::BTreeMap<_, _>, String>>()?;
    digest(&sorted)
}

pub(crate) fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|e| e.to_string())?)
    ))
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[cfg(test)]
#[path = "resources_ports_tests.rs"]
mod tests;
