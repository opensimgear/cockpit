//! Bounded, ephemeral compiler previews. Inspection never submits effects and
//! cached plans are never an activation authority; Apply must capture again.
use osg_profiles::compiler_models::{CompilationReport, CompileSnapshot};
use osg_profiles::ProfileDocument;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

const MAX_PREVIEWS: usize = 4;
const MAX_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
const MAX_REPLY_BYTES: usize = 1_900_000;
const SECTIONS: [&str; 8] = [
    "diagnostics",
    "nodes",
    "order",
    "sources",
    "sinks",
    "branches",
    "controllers",
    "visibility",
];

#[derive(Default)]
pub(crate) struct Runtime {
    previews: Mutex<VecDeque<(String, Arc<CompilationReport>)>>,
}

impl Runtime {
    pub(crate) fn request(
        &self,
        request: &Value,
        capture: impl FnOnce(&ProfileDocument) -> Result<CompileSnapshot, String>,
    ) -> Result<Value, String> {
        let command = request["command"]
            .as_str()
            .ok_or("Missing compiler command.")?;
        let fields: &[&str] = match command {
            "profile_compile" => &["document"],
            "profile_compilation_inspect" => &["compilationId", "section", "offset", "limit"],
            _ => return Err("Unknown compiler operation.".into()),
        };
        if request
            .as_object()
            .ok_or("Compiler requests must be objects.")?
            .keys()
            .any(|key| key != "command" && !fields.contains(&key.as_str()))
        {
            return Err("Unknown compiler request field.".into());
        }
        if command == "profile_compile" {
            let document = ProfileDocument::parse_draft(
                &serde_json::to_vec(&request["document"]).map_err(|error| error.to_string())?,
            )?;
            let snapshot = capture(&document)?;
            let report = osg_profiles::compiler::compile(&document, &snapshot);
            serde_json::to_writer(SizeBudget(0), &report)
                .map_err(|_| "The compiled preview exceeds its 8 MiB inspection budget.")?;
            let id = uuid::Uuid::new_v4().to_string();
            let response = summary(&id, &report);
            let mut previews = self.previews.lock().unwrap_or_else(|p| p.into_inner());
            previews.push_back((id, Arc::new(report)));
            while previews.len() > MAX_PREVIEWS {
                previews.pop_front();
            }
            Ok(response)
        } else {
            let id = request["compilationId"]
                .as_str()
                .filter(|id| {
                    uuid::Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == *id)
                })
                .ok_or("Choose a current compilation preview.")?;
            let section = request["section"]
                .as_str()
                .filter(|section| SECTIONS.contains(section))
                .ok_or("Choose a compiler inspection section.")?;
            let offset = number(request, "offset", 0, 16_384)?;
            let limit = number(request, "limit", 32, 64)?;
            if limit == 0 {
                return Err("Choose a nonempty compiler page limit.".into());
            }
            let report = self
                .previews
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .find(|(stored, _)| stored == id)
                .map(|(_, report)| report.clone())
                .ok_or("The compilation preview expired; compile the current draft again.")?;
            let (total, items) = match (section, &report.plan) {
                ("diagnostics", _) => page(&report.diagnostics, offset, limit)?,
                ("nodes", Some(plan)) => page(&plan.trace, offset, limit)?,
                ("order", Some(plan)) => page(&plan.topological_order, offset, limit)?,
                ("sources", Some(plan)) => page(&plan.sources, offset, limit)?,
                ("sinks", Some(plan)) => page(&plan.sinks, offset, limit)?,
                ("branches", Some(plan)) => page(&plan.branches, offset, limit)?,
                ("controllers", Some(plan)) => page(&plan.controllers, offset, limit)?,
                ("visibility", Some(plan)) => page(&plan.visibility, offset, limit)?,
                _ => page::<Value>(&[], offset, limit)?,
            };
            let mut response = json!({"compilationId":id,"section":section,"offset":offset,"total":total,"items":items,
                "profileId":report.profile_id,"profileRevision":report.profile_revision,"programHash":report.program_hash,"captureFingerprint":report.capture_fingerprint});
            while serde_json::to_vec(&response)
                .map_err(|error| error.to_string())?
                .len()
                > MAX_REPLY_BYTES
            {
                let items = response["items"].as_array_mut().unwrap();
                if items.len() <= 1 {
                    return Err("A compiled item exceeds its inspection reply budget.".into());
                }
                items.pop();
            }
            Ok(response)
        }
    }
}

fn summary(id: &str, report: &CompilationReport) -> Value {
    let mut sections = BTreeMap::from([
        ("diagnostics", report.diagnostics.len()),
        ("nodes", 0),
        ("order", 0),
        ("sources", 0),
        ("sinks", 0),
        ("branches", 0),
        ("controllers", 0),
        ("visibility", 0),
    ]);
    if let Some(plan) = &report.plan {
        sections.extend([
            ("nodes", plan.trace.len()),
            ("order", plan.topological_order.len()),
            ("sources", plan.sources.len()),
            ("sinks", plan.sinks.len()),
            ("branches", plan.branches.len()),
            ("controllers", plan.controllers.len()),
            ("visibility", plan.visibility.len()),
        ]);
    }
    json!({"schemaVersion":report.schema_version,"compilationId":id,"profileId":report.profile_id,"profileRevision":report.profile_revision,
        "programHash":report.program_hash,"documentSemanticHash":report.document_semantic_hash,"captureFingerprint":report.capture_fingerprint,
        "readiness":report.readiness,"diagnostics":report.diagnostics.iter().take(32).collect::<Vec<_>>(),"sectionTotals":sections,
        "budgets":report.plan.as_ref().map(|plan| &plan.budgets)})
}
fn number(request: &Value, key: &str, default: usize, maximum: usize) -> Result<usize, String> {
    request.get(key).map_or(Ok(default), |value| {
        value
            .as_u64()
            .filter(|value| *value <= maximum as u64)
            .map(|value| value as usize)
            .ok_or_else(|| format!("Invalid compiler {key}."))
    })
}
fn page<T: Serialize>(
    items: &[T],
    offset: usize,
    limit: usize,
) -> Result<(usize, Vec<Value>), String> {
    if offset > items.len() {
        return Err("The compiler page starts after its section.".into());
    }
    Ok((
        items.len(),
        items
            .iter()
            .skip(offset)
            .take(limit)
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?,
    ))
}
struct SizeBudget(usize);
impl Write for SizeBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .0
            .checked_add(bytes.len())
            .filter(|total| *total <= MAX_PREVIEW_BYTES)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "compiled preview is too large")
            })?;
        self.0 = next;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "profile_compilation_tests.rs"]
mod tests;
