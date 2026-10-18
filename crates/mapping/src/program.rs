//! Immutable compiler output consumed by the mapping runtime. This is never an
//! independently saved authoring document; portable profile graphs own edits.
use crate::{
    profile::{AxisRange, OutputControl},
    signal::{PortContract, PortDescriptor, PortValue},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const MAX_SOURCES: usize = 16;
pub const MAX_TARGETS: usize = 8;
pub const MAX_PORTS: usize = 1024;
pub const MAX_NODES: usize = 256;
pub const MAX_BINDINGS: usize = 512;
pub const MAX_PROGRAM_BYTES: usize = 512 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Endpoint {
    pub node_id: String,
    pub port_id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControllerAxis {
    pub port_id: String,
    pub range: AxisRange,
    pub fallback: i32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControllerLayout {
    pub buttons: Vec<String>,
    pub axes: Vec<ControllerAxis>,
    pub hats: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VirtualController {
    pub id: String,
    pub layout: ControllerLayout,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rounding {
    None,
    Nearest,
    Floor,
    Ceil,
    Truncate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Comparison {
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Equal,
    NotEqual,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Edge {
    Rising,
    Falling,
    Change,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextOverflow {
    Reject,
    Truncate,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CurvePoint {
    pub input: f64,
    pub output: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Operation {
    Identity {},
    DeclareQuantity {
        quantity: String,
        unit: String,
    },
    Constant {
        value: PortValue,
    },
    Not {},
    Scale {
        factor: f64,
        offset: f64,
    },
    RangeMap {
        input_min: f64,
        input_max: f64,
        output_min: f64,
        output_max: f64,
        rounding: Rounding,
    },
    UnitConvert {
        from: String,
        to: String,
        rounding: Rounding,
    },
    Deadzone {
        center: f64,
        radius: f64,
        rounding: Rounding,
    },
    Invert {},
    Curve {
        points: Vec<CurvePoint>,
        rounding: Rounding,
    },
    Compare {
        comparison: Comparison,
        threshold: f64,
    },
    Hysteresis {
        press: f64,
        release: f64,
        comparison: Comparison,
    },
    HatDirection {
        direction: crate::desktop::HatDirection,
        diagonals: bool,
    },
    Gate {},
    Any {},
    All {},
    Priority {
        order: Vec<String>,
    },
    EventEdge {
        edge: Edge,
    },
    EventToState {},
    Select {
        when_true: PortValue,
        when_false: PortValue,
    },
    Format {
        decimals: u8,
        prefix: String,
        suffix: String,
        rounding: Rounding,
        overflow: TextOverflow,
    },
    Fallback {
        unknown: PortValue,
        stale: PortValue,
        unavailable: PortValue,
    },
    ActionPack {
        arguments: Vec<String>,
    },
    SetOnChange {
        arguments: Vec<String>,
    },
    /// Reuses the existing bounded timing/logic rule implementation. Its
    /// controller rules require exactly their declared native numeric range.
    Builtin {
        rule: crate::rules::Rule,
        inputs: Vec<String>,
    },
    Script {
        source: String,
        parameters: BTreeMap<String, PortValue>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourcePort {
    pub endpoint: Endpoint,
    pub contract: PortContract,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputBinding {
    pub port_id: String,
    pub from: Endpoint,
    pub contract: PortContract,
    pub edge_id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub inputs: Vec<InputBinding>,
    pub outputs: Vec<PortDescriptor>,
    pub operation: Operation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceAdapter {
    DeviceAction,
    SimulatorCommand,
    CompanionFeedback,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SinkKind {
    Desktop {
        action: crate::desktop::Action,
    },
    Virtual {
        controller_id: String,
        control: OutputControl,
    },
    Resource {
        adapter: ResourceAdapter,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sink {
    pub id: String,
    pub endpoint: Endpoint,
    pub from: Endpoint,
    pub contract: PortContract,
    pub kind: SinkKind,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Program {
    pub sources: Vec<SourcePort>,
    pub nodes: Vec<Node>,
    pub sinks: Vec<Sink>,
    pub controllers: Vec<VirtualController>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NamedArgument {
    pub id: String,
    pub value: PortValue,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionPack {
    pub arguments: Vec<NamedArgument>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum SignalValue {
    Scalar(PortValue),
    ActionPack(ActionPack),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    content = "value",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum SignalSample {
    Known(SignalValue),
    Unknown,
    Stale,
    Unavailable,
}
impl SignalSample {
    pub fn scalar(value: PortValue) -> Self {
        Self::Known(SignalValue::Scalar(value))
    }
}

#[path = "program_validation.rs"]
pub(crate) mod validation;
pub use validation::{compatible, validate_operation, virtual_port_contract, CompiledProgram};
