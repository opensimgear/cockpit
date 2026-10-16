use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileDocument {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub eligibility: GameEligibility,
    pub resources: Vec<Resource>,
    pub bindings: Vec<Binding>,
    pub processing_nodes: Vec<ProcessingNode>,
    pub virtual_outputs: Vec<VirtualOutput>,
    pub visibility: Vec<VisibilityIntent>,
    pub overrides: Vec<RuntimeOverride>,
    pub companion_assignments: Vec<CompanionAssignment>,
    pub editor: EditorLayout,
}
impl ProfileDocument {
    pub(crate) fn empty(id: String, name: String) -> Self {
        Self {
            schema_version: 1,
            id,
            name,
            revision: 1,
            eligibility: GameEligibility { game_ids: vec![] },
            resources: vec![],
            bindings: vec![],
            processing_nodes: vec![],
            virtual_outputs: vec![],
            visibility: vec![],
            overrides: vec![],
            companion_assignments: vec![],
            editor: EditorLayout {
                positions: vec![],
                viewport: Viewport {
                    x: 0.0,
                    y: 0.0,
                    zoom: 1.0,
                },
            },
        }
    }
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > crate::MAX_PROFILE_BYTES {
            return Err("Profile exceeds 512 KiB.".into());
        }
        let document: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("Invalid profile document: {error}"))?;
        crate::validate_document(&document)?;
        Ok(document)
    }
    pub fn export(&self) -> Result<Vec<u8>, String> {
        crate::validate_document(self)?;
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        if bytes.len() > crate::MAX_PROFILE_BYTES {
            return Err("Profile exceeds 512 KiB.".into());
        }
        Ok(bytes)
    }
    pub fn semantic_hash(&self) -> Result<String, String> {
        crate::semantic_hash(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameEligibility {
    /// Empty means eligible for every game; these are catalogue keys, never paths or processes.
    pub game_ids: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceKind {
    Device,
    Companion,
    Game,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Resource {
    pub id: String,
    pub name: String,
    pub kind: ResourceKind,
    pub required: bool,
    pub ports: Vec<Port>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Input,
    Output,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Delivery {
    State,
    Event,
    Action,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ValueType {
    Boolean,
    Integer,
    Number,
    Text,
    Enum,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NumericRange {
    pub min: f64,
    pub max: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PortValue {
    Boolean(bool),
    Number(f64),
    Text(String),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Port {
    pub id: String,
    pub name: String,
    /// Output produces a value into the graph; input consumes a value from it.
    pub direction: Direction,
    pub delivery: Delivery,
    pub value_type: ValueType,
    #[serde(deserialize_with = "required_option")]
    pub unit: Option<String>,
    #[serde(deserialize_with = "required_option")]
    pub range: Option<NumericRange>,
    pub enum_values: Vec<String>,
    #[serde(deserialize_with = "required_option")]
    pub freshness_ms: Option<u32>,
    #[serde(deserialize_with = "required_option")]
    pub fallback: Option<PortValue>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Endpoint {
    pub node_id: String,
    pub port_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub id: String,
    pub from: Endpoint,
    pub to: Endpoint,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Operation {
    Identity {},
    Not {},
    Scale { factor: f64, offset: f64 },
    Threshold { threshold: f64 },
    Toggle {},
    Trigger {},
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessingNode {
    pub id: String,
    pub name: String,
    pub operation: Operation,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VirtualOutput {
    pub id: String,
    pub name: String,
    pub required: bool,
    pub ports: Vec<Port>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisibilityMode {
    Visible,
    HiddenWhileActive,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisibilityIntent {
    pub resource_id: String,
    pub mode: VisibilityMode,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeOverride {
    pub resource_id: String,
    #[serde(deserialize_with = "required_option")]
    pub enabled: Option<bool>,
    #[serde(deserialize_with = "required_option")]
    pub debounce_ms: Option<u32>,
    pub invert_ports: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompanionAssignment {
    pub resource_id: String,
    pub publication_hash: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorLayout {
    pub positions: Vec<NodePosition>,
    pub viewport: Viewport,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodePosition {
    pub node_id: String,
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
