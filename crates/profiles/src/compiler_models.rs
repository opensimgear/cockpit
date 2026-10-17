//! Captured compiler inputs and traceable, read-only preflight results.
//!
//! Captures are assembled by the native owner under its inventory lock. They are
//! data, not a discovery callback, an ownership lease, or permission to submit.
use crate::resources::{ObservedPort, ResourceSummary, ResourceToken, Route};
use crate::resources_ports::PortContract;
use crate::store::LocalAssignment;
use crate::{CompanionAssignment, Endpoint, RuntimeOverride, VisibilityMode};
use serde::Serialize;
use std::collections::BTreeMap;

pub const MAX_USED_SOURCES: usize = 16;
pub const MAX_VIRTUAL_TARGETS: usize = 8;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileSnapshot {
    pub inventory_session: String,
    pub inventory_revision: String,
    pub assignments: Vec<LocalAssignment>,
    pub resources: Vec<CapturedResource>,
    pub game_context: Option<VerifiedGameContext>,
    pub backend: BackendCapabilities,
    pub inventory_diagnostics: Vec<String>,
    pub read_only: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedResource {
    pub summary: ResourceSummary,
    /// Only exact port variants used by the captured local assignment are needed.
    pub ports: Vec<ObservedPort>,
    /// Route-specific descriptors; these do not acquire output ownership.
    pub tokens: Vec<ResourceToken>,
    pub provider_setup_revision: Option<String>,
    pub game_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedGameContext {
    pub game_id: String,
    pub session: String,
    /// Inventory resource identity -> captured game-details provider revision.
    pub provider_setup_revisions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendCapabilities {
    pub name: String,
    pub available: bool,
    pub qualified: bool,
    pub visibility_available: bool,
    pub visibility_qualified: bool,
    pub max_targets: usize,
    pub max_buttons: u16,
    pub max_axes: u8,
    pub max_hats: u8,
}

impl Default for BackendCapabilities {
    fn default() -> Self {
        Self {
            name: "unavailable".into(),
            available: false,
            qualified: false,
            visibility_available: false,
            visibility_qualified: false,
            max_targets: MAX_VIRTUAL_TARGETS,
            max_buttons: 128,
            max_axes: 16,
            max_hats: 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanReadiness {
    Ready,
    Blocked,
    Dormant,
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphLocation {
    pub resource_id: Option<String>,
    pub port_id: Option<String>,
    pub node_id: Option<String>,
    pub edge_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileDiagnostic {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub location: GraphLocation,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompilationReport {
    pub schema_version: u32,
    pub profile_id: String,
    pub profile_revision: u64,
    pub document_semantic_hash: Option<String>,
    /// Portable program identity, independent of route generations and sessions.
    pub program_hash: Option<String>,
    /// Captured native readiness/identity, independent of the portable program hash.
    pub capture_fingerprint: String,
    pub readiness: PlanReadiness,
    pub diagnostics: Vec<CompileDiagnostic>,
    pub plan: Option<ExecutionPlan>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPlan {
    pub program: osg_mapping::program::Program,
    pub topological_order: Vec<String>,
    pub trace: Vec<InstructionTrace>,
    pub sources: Vec<SourcePlan>,
    pub sinks: Vec<SinkPlan>,
    pub branches: Vec<BranchPlan>,
    pub controllers: Vec<ControllerSummary>,
    pub visibility: Vec<VisibilitySummary>,
    pub runtime_overrides: Vec<RuntimeOverride>,
    pub companion_assignments: Vec<CompanionAssignment>,
    pub budgets: PlanBudgets,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionTrace {
    pub node_id: String,
    pub input_ports: Vec<String>,
    pub output_ports: Vec<String>,
    pub incoming_edge_ids: Vec<String>,
    pub outgoing_edge_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePlan {
    pub endpoint: Endpoint,
    pub inventory_resource_id: Option<String>,
    pub observed_port_id: Option<String>,
    pub contract: PortContract,
    pub token: Option<ResourceToken>,
    pub route: Option<Route>,
    pub readiness: PlanReadiness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SinkAdapter {
    VirtualController,
    PhysicalState,
    PhysicalAction,
    SimulatorCommand,
    CompanionFeedback,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SinkPlan {
    pub endpoint: Endpoint,
    pub adapter: SinkAdapter,
    pub inventory_resource_id: Option<String>,
    pub observed_port_id: Option<String>,
    pub contract: PortContract,
    pub token: Option<ResourceToken>,
    pub route: Option<Route>,
    pub readiness: PlanReadiness,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchPlan {
    pub sink: Endpoint,
    pub source_ports: Vec<Endpoint>,
    pub processing_nodes: Vec<String>,
    pub edge_ids: Vec<String>,
    pub required: bool,
    pub readiness: PlanReadiness,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerSummary {
    pub id: String,
    pub name: String,
    pub buttons: usize,
    pub axes: usize,
    pub hats: usize,
    pub connected_ports: Vec<String>,
    pub readiness: PlanReadiness,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibilitySummary {
    pub resource_id: String,
    pub inventory_resource_id: Option<String>,
    pub mode: VisibilityMode,
    pub readiness: PlanReadiness,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanBudgets {
    pub used_sources: usize,
    pub virtual_targets: usize,
    pub source_ports: usize,
    pub processing_nodes: usize,
    pub sink_ports: usize,
    pub bindings: usize,
    pub state_slots: usize,
    pub event_queue_capacity: usize,
    pub action_queue_capacity: usize,
    pub timer_capacity: usize,
}
