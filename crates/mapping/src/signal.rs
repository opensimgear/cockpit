//! Canonical bounded signal contracts shared by providers, profiles and execution.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Input,
    Output,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PortValue {
    Boolean(bool),
    Number(f64),
    Text(String),
}

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
    /// Atomic ordered argument tuple produced by an explicit action-pack node.
    Invocation {
        parameters: Vec<ActionParameter>,
        semantics: ActionSemantics,
    },
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
                if min > max || *min < -(MAX_SAFE_INTEGER as i64) || *max > MAX_SAFE_INTEGER as i64
                {
                    return Err("Invalid integer signal range.".into());
                }
                unit
            }
            Self::Number { min, max, unit } => {
                if !min.is_finite()
                    || !max.is_finite()
                    || min > max
                    || min.abs() > MAX_SAFE_INTEGER as f64
                    || max.abs() > MAX_SAFE_INTEGER as f64
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
}

impl PortDescriptor {
    pub fn validate(&self) -> Result<(), String> {
        text(&self.id, 256)?;
        text(&self.name, 320)?;
        match &self.contract {
            PortContract::Invocation { .. } => {
                return Err("Provider ports cannot advertise graph invocations.".into())
            }
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

pub fn digest(value: &impl Serialize) -> Result<String, String> {
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

impl PortContract {
    /// Validates a graph contract without turning an invocation into a provider capability.
    pub fn validate_graph(&self, direction: Direction) -> Result<(), String> {
        if let Self::Invocation { parameters, .. } = self {
            if direction != Direction::Output {
                return Err("Invocations require a processing output.".into());
            }
            return validate_parameters(parameters);
        }
        PortDescriptor {
            id: "port".into(),
            name: "Port".into(),
            direction,
            contract: self.clone(),
        }
        .validate()
    }
    pub fn scalar_schema(&self) -> Option<&ScalarSchema> {
        match self {
            Self::State { schema, .. } | Self::Event { schema } => Some(schema),
            _ => None,
        }
    }
    /// A binding performs no conversion. Range/encoding containment is safe; units and delivery remain exact.
    pub fn can_bind_to(&self, target: &Self) -> bool {
        match (self, target) {
            (Self::State { schema: a, .. }, Self::State { schema: b, .. })
            | (Self::Event { schema: a }, Self::Event { schema: b }) => a.fits(b),
            (
                Self::Invocation {
                    parameters: a,
                    semantics: u,
                },
                Self::Action {
                    parameters: b,
                    semantics: v,
                    ..
                },
            ) => {
                u == v
                    && a.len() == b.len()
                    && a.iter()
                        .zip(b)
                        .all(|(a, b)| a.id == b.id && a.schema.fits(&b.schema))
            }
            _ => false,
        }
    }
}
fn validate_parameters(parameters: &[ActionParameter]) -> Result<(), String> {
    if parameters.len() > 4 {
        return Err("An action tuple has at most four arguments.".into());
    }
    let mut ids = BTreeSet::new();
    for p in parameters {
        text(&p.id, 80)?;
        text(&p.name, 320)?;
        p.schema.validate()?;
        if !ids.insert(&p.id) {
            return Err("Duplicate action argument.".into());
        }
    }
    Ok(())
}
impl ScalarSchema {
    pub fn fits(&self, target: &Self) -> bool {
        match (self, target) {
            (
                Self::Integer {
                    min: a,
                    max: b,
                    unit: u,
                },
                Self::Integer {
                    min: c,
                    max: d,
                    unit: v,
                },
            ) => u == v && a >= c && b <= d,
            (
                Self::Number {
                    min: a,
                    max: b,
                    unit: u,
                },
                Self::Number {
                    min: c,
                    max: d,
                    unit: v,
                },
            ) => u == v && a >= c && b <= d,
            (
                Self::Text {
                    max_bytes: a,
                    encoding: u,
                },
                Self::Text {
                    max_bytes: b,
                    encoding: v,
                },
            ) => a <= b && (u == v || *u == TextEncoding::Ascii),
            (Self::Enum { values: a }, Self::Enum { values: b }) => a.iter().all(|v| b.contains(v)),
            _ => self == target,
        }
    }
}

#[cfg(test)]
#[path = "signal_tests.rs"]
mod tests;
