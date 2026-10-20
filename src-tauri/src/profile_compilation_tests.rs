use super::*;
use osg_profiles::compiler_models::{
    BackendCapabilities, CompileDiagnostic, DiagnosticSeverity, GraphLocation,
};

fn snapshot() -> CompileSnapshot {
    CompileSnapshot {
        inventory_session: uuid::Uuid::new_v4().to_string(),
        inventory_revision: "1".into(),
        assignments: vec![],
        resources: vec![],
        game_context: None,
        backend: BackendCapabilities::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    }
}

#[test]
fn debug_output_capture_observes_readiness_without_ownership_or_unused_driver_io() {
    let mut document = osg_profiles::default_document();
    let mut backend = BackendCapabilities::default();
    capture_output_capabilities(&document, &mut backend, true,
        || panic!("A profile without virtual output must not inspect its driver."),
        || panic!("A profile without hiding must not inspect visibility."));
    assert!(!backend.available);
    document.virtual_outputs = osg_profiles::ProfileDocument::parse(include_bytes!("../../tests/fixtures/profiles/mixed.json")).unwrap().virtual_outputs;
    assert!(!document.virtual_outputs.is_empty());
    document.visibility = vec![osg_profiles::VisibilityIntent { resource_id: "physical".into(), mode: osg_profiles::VisibilityMode::HiddenWhileActive }];
    let observations = std::cell::Cell::new(0);
    capture_output_capabilities(&document, &mut backend, true,
        || { observations.set(observations.get() + 1); Ok(vec![]) },
        || { observations.set(observations.get() + 1); Ok(json!({"visibility":{"qualified":true,"modes":[{"mode":"hide-all","supported":true}]}})) });
    assert_eq!(observations.get(), 2);
    assert!(backend.available);
    assert_eq!(backend.qualified, osg_mapping_host::service::LIVE_OUTPUT_QUALIFIED);
    assert_eq!(backend.visibility_available, cfg!(windows));
    assert!(backend.visibility_qualified);
    capture_output_capabilities(&document, &mut backend, true,
        || Err("Driver is unavailable.".into()), || Err("Visibility status is unavailable.".into()));
    assert!(!backend.available && !backend.visibility_available && !backend.visibility_qualified);
    capture_output_capabilities(&document, &mut backend, false,
        || panic!("Browser previews must not inspect native output."),
        || panic!("Browser previews must not inspect native hiding."));
    assert!(!backend.available && !backend.qualified && !backend.visibility_available && !backend.visibility_qualified);
}
#[test]
fn native_summary_matches_the_shared_frontend_wire_fixture() {
    let mut snapshot = snapshot();
    snapshot.inventory_session = "inventory-fixture".into();
    let report = osg_profiles::compiler::compile(&osg_profiles::default_document(), &snapshot);
    let response = summary("00000000-0000-4000-8000-000000000002", &report);
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/profiles/compilation-summary.json"
    ))
    .unwrap();
    assert_eq!(response, fixture);
}
#[test]
fn draft_pins_are_explicit_validated_and_never_saved_or_used_as_authority() {
    let runtime=Runtime::default();
    let document=ProfileDocument::parse(include_bytes!("../../tests/fixtures/profiles/mixed.json")).unwrap();
    let assignments=document.resources.iter().map(|resource|osg_profiles::store::LocalAssignment {
        resource_id:resource.id.clone(),source_id:format!("fixture:{}",resource.id),
        ports:resource.ports.iter().map(|port|osg_profiles::resources::PortExpectation {
            profile_port_id:port.id.clone(),observed_port_id:port.id.clone(),
            expected_contract_hash:port.descriptor().contract_hash().unwrap(),
        }).collect(),
    }).collect::<Vec<_>>();
    let request=json!({"command":"profile_compile","document":document,"assignments":assignments});
    let preview=runtime.request(&request,|draft,pins| {
        assert_eq!(draft.id,document.id);
        assert_eq!(serde_json::to_value(pins.unwrap()).unwrap(),request["assignments"]);
        Ok(snapshot())
    }).unwrap();
    assert!(!preview["compilationId"].is_null());
    runtime.request(&json!({"command":"profile_compile","document":document,"assignments":[]}),|_,pins| {
        assert!(pins.unwrap().is_empty());Ok(snapshot())
    }).unwrap();
    runtime.request(&json!({"command":"profile_compile","document":document}),|_,pins| {
        assert!(pins.is_none());Ok(snapshot())
    }).unwrap();
    for invalid in [Value::Null,json!({}),json!([{"resourceId":"unknown","sourceId":"device","ports":[]}]),{
        let mut missing=request["assignments"].clone();missing[0]["ports"]=json!([]);missing
    },{
        let mut extra=request["assignments"].clone();extra[0]["token"]=json!({"access":"invoke"});extra
    }] {
        let mut invalid_request=request.clone();invalid_request["assignments"]=invalid;
        assert!(runtime.request(&invalid_request,|_,_|panic!("invalid pins cannot capture")).is_err());
    }
}
#[test]
fn semantic_errors_are_inspectable_without_saving_or_executing_a_draft() {
    let runtime = Runtime::default();
    let mut document = serde_json::to_value(osg_profiles::default_document()).unwrap();
    document["bindings"] = json!([{"id":"missing-binding","from":{"nodeId":"missing-source","portId":"value"},"to":{"nodeId":"missing-sink","portId":"value"}}]);
    assert!(ProfileDocument::parse(&serde_json::to_vec(&document).unwrap()).is_err());
    let result = runtime
        .request(
            &json!({"command":"profile_compile","document":document}),
            |_, _| Ok(snapshot()),
        )
        .unwrap();
    assert_eq!(result["readiness"], "invalid");
    assert_eq!(result["programHash"], Value::Null);
    assert_eq!(result["budgets"], Value::Null);
    let page = inspect(
        &runtime,
        result["compilationId"].as_str().unwrap(),
        "diagnostics",
        0,
        32,
    )
    .unwrap();
    assert!(page["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["location"]["edgeIds"] == json!(["missing-binding"])));
    assert_eq!(
        inspect(
            &runtime,
            result["compilationId"].as_str().unwrap(),
            "order",
            0,
            32
        )
        .unwrap()["items"],
        json!([])
    );
}
fn preview(runtime: &Runtime) -> Value {
    runtime
        .request(
            &json!({"command":"profile_compile","document":osg_profiles::default_document()}),
            |_, _| Ok(snapshot()),
        )
        .unwrap()
}
fn inspect(
    runtime: &Runtime,
    id: &str,
    section: &str,
    offset: usize,
    limit: usize,
) -> Result<Value, String> {
    runtime.request(&json!({"command":"profile_compilation_inspect","compilationId":id,"section":section,"offset":offset,"limit":limit}), |_, _| panic!("inspection cannot capture providers"))
}
#[test]
fn previews_are_read_only_immutable_paged_and_separate_from_selection() {
    let runtime = Runtime::default();
    let response = preview(&runtime);
    assert_eq!(response["profileId"], osg_profiles::DEFAULT_PROFILE_ID);
    assert_eq!(response["profileRevision"], 1);
    assert_eq!(response["schemaVersion"], 1);
    assert!(response["programHash"].as_str().is_some());
    let id = response["compilationId"].as_str().unwrap();
    for section in SECTIONS {
        let page = inspect(&runtime, id, section, 0, 32).unwrap();
        assert_eq!(page["compilationId"], id);
        assert_eq!(page["programHash"], response["programHash"]);
        assert_eq!(page["captureFingerprint"], response["captureFingerprint"]);
        assert_eq!(page["total"], response["sectionTotals"][section]);
        assert_eq!(page["items"], json!([]));
    }
    assert_eq!(runtime.previews.lock().unwrap().len(), 1);
}
#[test]
fn strict_requests_reject_forged_capture_fields_and_stale_preview_ids() {
    let runtime = Runtime::default();
    for request in [
        json!([]),
        json!({}),
        json!({"command":"wrong"}),
        json!({"command":"profile_compile","document":osg_profiles::default_document(),"snapshot":snapshot()}),
        json!({"command":"profile_compile","document":{"schemaVersion":1}}),
    ] {
        assert!(runtime
            .request(&request, |_, _| panic!(
                "invalid requests cannot capture providers"
            ))
            .is_err());
    }
    let response = preview(&runtime);
    let id = response["compilationId"].as_str().unwrap();
    assert!(inspect(&runtime, "not-an-id", "nodes", 0, 1).is_err());
    assert!(inspect(&runtime, &uuid::Uuid::new_v4().to_string(), "nodes", 0, 1).is_err());
    for (section, offset, limit) in [
        ("unknown", 0, 1),
        ("nodes", 0, 0),
        ("nodes", 0, 65),
        ("nodes", 1, 1),
        ("nodes", 16_385, 1),
    ] {
        assert!(inspect(&runtime, id, section, offset, limit).is_err());
    }
    assert!(runtime.request(&json!({"command":"profile_compilation_inspect","compilationId":id,"section":"nodes","offset":-1}), |_, _| panic!()).is_err());
    assert!(runtime
        .request(
            &json!({"command":"profile_compile","document":osg_profiles::default_document()}),
            |_, _| Err("capture unavailable".into())
        )
        .unwrap_err()
        .contains("capture unavailable"));
    assert_eq!(runtime.previews.lock().unwrap().len(), 1);
}
#[test]
fn previews_expire_after_bounded_replacement_without_changing_a_surviving_capture() {
    let runtime = Runtime::default();
    let first = preview(&runtime);
    let mut last = first.clone();
    for _ in 0..MAX_PREVIEWS {
        last = preview(&runtime);
    }
    assert_eq!(runtime.previews.lock().unwrap().len(), MAX_PREVIEWS);
    assert!(inspect(
        &runtime,
        first["compilationId"].as_str().unwrap(),
        "nodes",
        0,
        1
    )
    .unwrap_err()
    .contains("expired"));
    let result = inspect(
        &runtime,
        last["compilationId"].as_str().unwrap(),
        "order",
        0,
        1,
    )
    .unwrap();
    assert_eq!(result["captureFingerprint"], last["captureFingerprint"]);
}
#[test]
fn large_diagnostics_use_complete_short_pages_and_preserve_progress() {
    let runtime = Runtime::default();
    let mut report =
        osg_profiles::compiler::compile(&osg_profiles::default_document(), &snapshot());
    report.diagnostics = (0..64)
        .map(|_| CompileDiagnostic {
            code: "boundedDiagnostic".into(),
            severity: DiagnosticSeverity::Warning,
            message: "detail".repeat(700),
            location: GraphLocation {
                edge_ids: (0..512)
                    .map(|index| format!("{index:03}{}", "e".repeat(76)))
                    .collect(),
                ..Default::default()
            },
        })
        .collect();
    let id = uuid::Uuid::new_v4().to_string();
    serde_json::to_writer(SizeBudget(0), &report).unwrap();
    let header = summary(&id, &report);
    assert_eq!(header["diagnostics"].as_array().unwrap().len(), 32);
    assert_eq!(header["sectionTotals"]["diagnostics"], 64);
    runtime
        .previews
        .lock()
        .unwrap()
        .push_back((id.clone(), Arc::new(report)));
    let first = inspect(&runtime, &id, "diagnostics", 0, 64).unwrap();
    let count = first["items"].as_array().unwrap().len();
    assert!(count > 0 && count < 64);
    assert!(serde_json::to_vec(&first).unwrap().len() <= MAX_REPLY_BYTES);
    let next = inspect(&runtime, &id, "diagnostics", count, 64).unwrap();
    assert_eq!(count + next["items"].as_array().unwrap().len(), 64);
    assert_eq!(next["captureFingerprint"], first["captureFingerprint"]);
}
#[test]
fn counting_budget_enforces_exact_capacity_without_allocating_a_full_report() {
    let mut budget = SizeBudget(MAX_PREVIEW_BYTES - 2);
    assert_eq!(budget.write(&[1, 2]).unwrap(), 2);
    assert!(budget.write(&[3]).is_err());
    budget.flush().unwrap();
}
