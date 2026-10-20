pub use osg_mapping::program::{ControllerLayout, Operation};
pub use osg_mapping::signal::{Direction, PortContract, PortValue};
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub desktop_outputs: Vec<DesktopOutput>,
    pub visibility: Vec<VisibilityIntent>,
    pub overrides: Vec<RuntimeOverride>,
    pub companion_assignments: Vec<CompanionAssignment>,
    pub editor: EditorLayout,
}
impl ProfileDocument {
    pub(crate) fn empty(id: String, name: String) -> Self {
        Self {
            schema_version: crate::PROFILE_SCHEMA_VERSION,
            id,
            name,
            revision: 1,
            eligibility: GameEligibility { game_ids: vec![] },
            resources: vec![],
            bindings: vec![],
            processing_nodes: vec![],
            virtual_outputs: vec![],
            desktop_outputs: vec![],
            visibility: vec![],
            overrides: vec![],
            companion_assignments: vec![],
            editor: EditorLayout {
                positions: vec![],
                groups: vec![],
                collapsed_port_groups: vec![],
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
    /// Parse a bounded compiler preview without requiring a valid routing graph.
    /// Missing ports, incompatible contracts and cycles belong in the compiler's
    /// traceable diagnostic report. Storage/import/export continue using `parse`.
    pub fn parse_draft(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > crate::MAX_PROFILE_BYTES {
            return Err("Profile exceeds 512 KiB.".into());
        }
        // All nested structs/enums retain deny_unknown_fields and required keys.
        let document: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("Invalid profile draft: {error}"))?;
        if document.schema_version != crate::PROFILE_SCHEMA_VERSION {
            return Err("Unsupported unified profile version.".into());
        }
        if uuid::Uuid::parse_str(&document.id)
            .ok()
            .map(|id| id.to_string())
            .as_deref()
            != Some(document.id.as_str())
        {
            return Err("Profile identity must be a canonical lowercase UUID.".into());
        }
        if document.revision == 0
            || document.revision > crate::MAX_REVISION
            || !crate::validation::valid_name(&document.name)
        {
            return Err("Profile drafts require a trimmed name and positive safe revision.".into());
        }
        let resource_ports = document
            .resources
            .iter()
            .map(|resource| resource.ports.len());
        let processing_ports = document
            .processing_nodes
            .iter()
            .map(|node| node.inputs.len() + node.outputs.len());
        let output_ports = document
            .virtual_outputs
            .iter()
            .map(|output| output.ports.len());
        let desktop_ports = document.desktop_outputs.iter().map(|output| output.ports.len());
        let ports = resource_ports
            .chain(processing_ports)
            .chain(output_ports)
            .chain(desktop_ports)
            .collect::<Vec<_>>();
        if document.resources.len() > crate::validation::MAX_RESOURCES
            || document.processing_nodes.len() > crate::validation::MAX_PROCESSING_NODES
            || document.virtual_outputs.len() > crate::validation::MAX_VIRTUAL_OUTPUTS
            || document.desktop_outputs.len() > 1
            || document.desktop_outputs.iter().any(|output| output.actions.len() > 128)
            || document.processing_nodes.len() + document.desktop_outputs.iter().map(|output| output.actions.len()).sum::<usize>() > 256
            || document.bindings.len() > crate::validation::MAX_BINDINGS
            || document.eligibility.game_ids.len() > 32
            || document.visibility.len() > 64
            || document.overrides.len() > 64
            || document.companion_assignments.len() > 64
            || document.editor.positions.len() > 336
            || document.editor.groups.len() > 32
            || document.editor.groups.iter().any(|group| group.members.len() > 336)
            || document.editor.collapsed_port_groups.len() > 1008
            || ports
                .iter()
                .any(|count| *count > crate::validation::MAX_PORTS_PER_NODE)
            || ports.iter().sum::<usize>() > crate::validation::MAX_PORTS
        {
            return Err("Profile draft exceeds its bounded graph collections.".into());
        }
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Port {
    pub id: String,
    pub name: String,
    /// Output produces a value into the graph; input consumes a value from it.
    pub direction: Direction,
    pub contract: PortContract,
}
impl Port {
    pub fn descriptor(&self) -> osg_mapping::signal::PortDescriptor {
        osg_mapping::signal::PortDescriptor {
            id: self.id.clone(),
            name: self.name.clone(),
            direction: self.direction,
            contract: self.contract.clone(),
        }
    }
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
    pub layout: ControllerLayout,
    pub id: String,
    pub name: String,
    pub required: bool,
    pub ports: Vec<Port>,
}
/// One logical desktop endpoint; its typed inputs never allocate a controller.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopOutput {
    pub id: String,
    pub name: String,
    pub required: bool,
    /// Missing scope retains application-only compatibility for older profiles.
    #[serde(default, deserialize_with = "present_desktop_scope", skip_serializing_if = "Option::is_none")]
    pub scope: Option<DesktopScope>,
    pub ports: Vec<Port>,
    pub actions: Vec<DesktopActionDefinition>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DesktopScope {
    SelectedGame,
    Application {
        #[serde(rename = "applicationReference", default, deserialize_with = "present_application_reference", skip_serializing_if = "Option::is_none")]
        application_reference: Option<String>,
    },
    DesktopWide,
}
fn present_desktop_scope<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<DesktopScope>, D::Error> {
    DesktopScope::deserialize(deserializer).map(Some)
}
fn present_application_reference<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopActionDefinition {
    pub port_id: String,
    pub action: osg_mapping::desktop::Action,
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
    pub enabled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompanionAssignment {
    pub resource_id: String,
    /// Stable saved panel identity. Legacy imports may only have an immutable
    /// publication hash; native panel resolution upgrades those references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_id: Option<String>,
    pub publication_hash: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorLayout {
    pub positions: Vec<NodePosition>,
    pub viewport: Viewport,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<EditorGroup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collapsed_port_groups: Vec<CollapsedPortGroup>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorGroup {
    pub id: String,
    pub name: String,
    pub members: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollapsedPortGroup {
    pub node_id: String,
    pub group: EditorPortGroup,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EditorPortGroup {
    Outputs,
    Inputs,
    Actions,
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

#[cfg(test)]
#[path = "compiler_draft_tests.rs"]
mod draft_tests;
