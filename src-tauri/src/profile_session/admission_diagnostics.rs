//! Activation errors identify the saved component instead of discarding warning
//! diagnostics when a native readiness gate blocks a compiled plan.
use crate::profile_session::{AdmissionBlocker, BlockerKind};
use osg_profiles::{compiler_models::{CompilationReport, CompileDiagnostic, DiagnosticSeverity, PlanReadiness}, ProfileDocument};

fn named_message(document: &ProfileDocument, diagnostic: &CompileDiagnostic) -> String {
    let id = diagnostic.location.resource_id.as_ref().or(diagnostic.location.node_id.as_ref());
    let name = id.and_then(|id| document.resources.iter().find(|item| &item.id == id).map(|item| item.name.as_str())
        .or_else(|| document.virtual_outputs.iter().find(|item| &item.id == id).map(|item| item.name.as_str()))
        .or_else(|| document.desktop_outputs.iter().find(|item| &item.id == id).map(|item| item.name.as_str()))
        .or_else(|| document.processing_nodes.iter().find(|item| &item.id == id).map(|item| item.name.as_str()))
        .or(Some(id.as_str())));
    match name {
        Some(name) => format!("{name}: {}", diagnostic.message),
        None => diagnostic.message.clone(),
    }
}

pub(super) fn blockers(report: &CompilationReport, document: &ProfileDocument) -> Vec<AdmissionBlocker> {
    if !matches!(report.readiness, PlanReadiness::Blocked | PlanReadiness::Invalid) { return vec![]; }
    let errors = report.diagnostics.iter().any(|item| item.severity == DiagnosticSeverity::Error);
    let mut messages = report.diagnostics.iter().filter(|item| if errors {
        item.severity == DiagnosticSeverity::Error
    } else {
        item.severity == DiagnosticSeverity::Warning && item.code != "gameContextDormant"
    }).map(|item| named_message(document, item)).collect::<Vec<_>>();
    messages.sort();
    messages.dedup();
    if messages.is_empty() {
        messages.push(format!("{}: Native activation was blocked without a resource diagnostic. Open Debug for the compilation report.", document.name));
    }
    messages.into_iter().take(32).map(|message| AdmissionBlocker {
        kind: if report.readiness == PlanReadiness::Invalid { BlockerKind::Structural } else { BlockerKind::RequiredResource },
        message: message.chars().take(2048).collect(),
    }).collect()
}

#[cfg(test)]
#[path = "admission_diagnostics_tests.rs"]
mod tests;
