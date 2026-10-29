use super::*;
use osg_profiles::{
    compiler_models::GraphLocation, ControllerLayout, DesktopOutput, Operation, ProcessingNode,
    Resource, ResourceKind, VirtualOutput,
};

fn fixture() -> (ProfileDocument, CompilationReport) {
    let mut document = osg_profiles::new_document("Flight profile").unwrap();
    document.resources.push(Resource {
        id: "panel".into(),
        name: "Browser panel".into(),
        kind: ResourceKind::Companion,
        required: false,
        ports: vec![],
    });
    document.virtual_outputs.push(VirtualOutput {
        id: "virtual".into(),
        name: "Pedal controller".into(),
        required: false,
        layout: ControllerLayout {
            buttons: vec![],
            axes: vec![],
            hats: vec![],
        },
        ports: vec![],
    });
    document.desktop_outputs.push(DesktopOutput {
        id: "desktop".into(),
        name: "Desktop controls".into(),
        required: false,
        scope: None,
        ports: vec![],
        actions: vec![],
    });
    document.processing_nodes.push(ProcessingNode {
        id: "transform".into(),
        name: "Brake smoothing".into(),
        operation: Operation::Identity {},
        inputs: vec![],
        outputs: vec![],
        draft: None,
    });
    let report = CompilationReport {
        schema_version: 1,
        profile_id: document.id.clone(),
        profile_revision: 1,
        document_semantic_hash: None,
        program_hash: None,
        capture_fingerprint: "capture".into(),
        readiness: PlanReadiness::Blocked,
        diagnostics: vec![],
        plan: None,
    };
    (document, report)
}
fn diagnostic(
    code: &str,
    severity: DiagnosticSeverity,
    message: &str,
    resource: Option<&str>,
) -> CompileDiagnostic {
    CompileDiagnostic {
        code: code.into(),
        severity,
        message: message.into(),
        location: GraphLocation {
            resource_id: resource.map(str::to_owned),
            ..Default::default()
        },
    }
}
fn diagnostic_node(
    code: &str,
    severity: DiagnosticSeverity,
    message: &str,
    node: &str,
) -> CompileDiagnostic {
    CompileDiagnostic {
        code: code.into(),
        severity,
        message: message.into(),
        location: GraphLocation {
            node_id: Some(node.into()),
            ..Default::default()
        },
    }
}

#[test]
fn warning_only_blocked_plan_reports_the_actual_named_component() {
    let (document, mut report) = fixture();
    report.diagnostics = vec![
        diagnostic(
            "gameContextDormant",
            DiagnosticSeverity::Warning,
            "Waiting for game",
            None,
        ),
        diagnostic(
            "portContractChanged",
            DiagnosticSeverity::Warning,
            "Selected panel port no longer matches",
            Some("panel"),
        ),
    ];
    let result = blockers(&report, &document);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].message,
        "Browser panel: Selected panel port no longer matches"
    );
}

#[test]
fn blocker_names_cover_resources_virtual_desktop_processing_and_unknown_nodes() {
    let (document, mut report) = fixture();
    report.diagnostics = vec![
        diagnostic(
            "resource",
            DiagnosticSeverity::Error,
            "Resource is unavailable",
            Some("panel"),
        ),
        diagnostic_node(
            "virtual",
            DiagnosticSeverity::Error,
            "Virtual output is unavailable",
            "virtual",
        ),
        diagnostic_node(
            "desktop",
            DiagnosticSeverity::Error,
            "Desktop output is unavailable",
            "desktop",
        ),
        diagnostic_node(
            "processing",
            DiagnosticSeverity::Error,
            "Transform is invalid",
            "transform",
        ),
        diagnostic_node(
            "unknown",
            DiagnosticSeverity::Error,
            "Native resource is unavailable",
            "mystery",
        ),
    ];
    let messages = blockers(&report, &document)
        .into_iter()
        .map(|item| item.message)
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        vec![
            "Brake smoothing: Transform is invalid",
            "Browser panel: Resource is unavailable",
            "Desktop controls: Desktop output is unavailable",
            "Pedal controller: Virtual output is unavailable",
            "mystery: Native resource is unavailable",
        ]
    );
}

#[test]
fn structural_errors_keep_precedence_and_unknown_locations_remain_identifiable() {
    let (document, mut report) = fixture();
    report.readiness = PlanReadiness::Invalid;
    report.diagnostics = vec![
        diagnostic(
            "warning",
            DiagnosticSeverity::Warning,
            "Ignore secondary warning",
            None,
        ),
        diagnostic(
            "invalid",
            DiagnosticSeverity::Error,
            "Bad binding",
            Some("unknown"),
        ),
    ];
    let result = blockers(&report, &document);
    assert_eq!(result.len(), 1);
    assert!(matches!(result[0].kind, BlockerKind::Structural));
    assert_eq!(result[0].message, "unknown: Bad binding");
}

#[test]
fn healthy_plans_have_no_blocker_and_missing_diagnostics_report_an_internal_gap() {
    let (document, mut report) = fixture();
    assert!(blockers(&report, &document)[0]
        .message
        .contains("without a resource diagnostic"));
    report.readiness = PlanReadiness::Ready;
    assert!(blockers(&report, &document).is_empty());
}
