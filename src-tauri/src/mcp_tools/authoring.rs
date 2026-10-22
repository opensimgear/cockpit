use super::*;
use osg_profiles::{
    Binding, CompanionAssignment, DesktopOutput, EditorLayout, GameEligibility, ProcessingNode,
    ProfileDocument, Resource, RuntimeOverride, VehicleTab, VirtualOutput, VisibilityIntent,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Edit {
    SetName {
        name: String,
    },
    SetEligibility {
        eligibility: GameEligibility,
    },
    UpsertResource {
        resource: Resource,
    },
    RemoveResource {
        id: String,
    },
    UpsertBinding {
        binding: Binding,
    },
    RemoveBinding {
        id: String,
    },
    UpsertProcessingNode {
        node: ProcessingNode,
    },
    RemoveProcessingNode {
        id: String,
    },
    ReplaceVirtualOutputs {
        outputs: Vec<VirtualOutput>,
    },
    ReplaceDesktopOutputs {
        outputs: Vec<DesktopOutput>,
    },
    ReplaceVisibility {
        visibility: Vec<VisibilityIntent>,
    },
    ReplaceOverrides {
        overrides: Vec<RuntimeOverride>,
    },
    ReplaceCompanionAssignments {
        assignments: Vec<CompanionAssignment>,
    },
    ReplaceVehicleTabs {
        tabs: Vec<VehicleTab>,
    },
    ReplaceEditor {
        editor: EditorLayout,
    },
}
fn edits(document: &mut ProfileDocument, value: &Value) -> Result<(), Value> {
    let edits: Vec<Edit> = serde_json::from_value(value.clone()).map_err(|error| {
        failure(
            "invalid_edit",
            &format!("Typed profile edit is invalid: {error}"),
            false,
        )
    })?;
    if edits.len() > 256 {
        return Err(failure(
            "invalid_edit",
            "A draft accepts at most 256 typed edits.",
            false,
        ));
    }
    for edit in edits {
        match edit {
            Edit::SetName { name } => document.name = name,
            Edit::SetEligibility { eligibility } => document.eligibility = eligibility,
            Edit::UpsertResource { resource } => match document
                .resources
                .iter()
                .position(|item| item.id == resource.id)
            {
                Some(index) => document.resources[index] = resource,
                None => document.resources.push(resource),
            },
            Edit::RemoveResource { id } => {
                remove(&mut document.resources, |resource| &resource.id, &id)?
            }
            Edit::UpsertBinding { binding } => match document
                .bindings
                .iter()
                .position(|item| item.id == binding.id)
            {
                Some(index) => document.bindings[index] = binding,
                None => document.bindings.push(binding),
            },
            Edit::RemoveBinding { id } => {
                remove(&mut document.bindings, |binding| &binding.id, &id)?
            }
            Edit::UpsertProcessingNode { node } => match document
                .processing_nodes
                .iter()
                .position(|item| item.id == node.id)
            {
                Some(index) => document.processing_nodes[index] = node,
                None => document.processing_nodes.push(node),
            },
            Edit::RemoveProcessingNode { id } => {
                remove(&mut document.processing_nodes, |node| &node.id, &id)?
            }
            Edit::ReplaceVirtualOutputs { outputs } => document.virtual_outputs = outputs,
            Edit::ReplaceDesktopOutputs { outputs } => document.desktop_outputs = outputs,
            Edit::ReplaceVisibility { visibility } => document.visibility = visibility,
            Edit::ReplaceOverrides { overrides } => document.overrides = overrides,
            Edit::ReplaceCompanionAssignments { assignments } => {
                document.companion_assignments = assignments
            }
            Edit::ReplaceVehicleTabs { tabs } => document.vehicle_tabs = tabs,
            Edit::ReplaceEditor { editor } => document.editor = editor,
        }
    }
    Ok(())
}
fn remove<T>(items: &mut Vec<T>, id: impl Fn(&T) -> &String, wanted: &str) -> Result<(), Value> {
    let index = items
        .iter()
        .position(|item| id(item) == wanted)
        .ok_or_else(|| {
            failure(
                "invalid_edit",
                &format!("Cannot remove undeclared graph element {wanted}."),
                false,
            )
        })?;
    items.remove(index);
    Ok(())
}

impl Tools {
    pub(super) fn device_prepare(&self, args: &Value, client: &str) -> Result<Value, Value> {
        // The native encoder is pure. Discard its wire commands: preparation
        // never sends firmware instructions or acquires a configuration writer.
        crate::runtime::commands(&args["config"], 1)
            .map_err(|error| failure("invalid_configuration", &error, false))?;
        let (snapshot, target) = match text(args, "transport")? {
            "gateway" => {
                if args.get("key").is_some() || args.get("route").is_some() {
                    return Err(failure(
                        "invalid_arguments",
                        "Gateway preparation accepts gatewayId and moduleId only.",
                        false,
                    ));
                }
                let gateway = text(args, "gatewayId")?;
                let module = text(args, "moduleId")?;
                let data = self.native(
                    "config_read",
                    json!({"gatewayId":gateway,"moduleId":module}),
                    client,
                    None,
                )?;
                let snapshot = data["moduleConfigs"][module].clone();
                if !snapshot.is_object() {
                    return Err(failure("unsupported","This gateway/module does not provide native typed runtime configuration. Inspect its capabilities before setup.",false));
                }
                let boot = data["modules"]
                    .as_array()
                    .and_then(|modules| modules.iter().find(|item| item["id"] == module))
                    .map(|module| module["boot"].clone())
                    .unwrap_or(Value::Null);
                (
                    snapshot,
                    json!({"gatewayId":gateway,"moduleId":module,"boot":boot}),
                )
            }
            "usb" => {
                if args.get("gatewayId").is_some() || args.get("moduleId").is_some() {
                    return Err(failure(
                        "invalid_arguments",
                        "USB preparation accepts key and exact current route only.",
                        false,
                    ));
                }
                let key = text(args, "key")?;
                if !args["route"].is_object() {
                    return Err(failure(
                        "invalid_arguments",
                        "USB preparation requires the exact current native route.",
                        false,
                    ));
                }
                let data = self.native(
                    "module_usb_config_read",
                    json!({"key":key,"route":args["route"]}),
                    client,
                    Some("moduleUsb"),
                )?;
                let snapshot = data["module"]["snapshot"]["config"].clone();
                if !snapshot.is_object() {
                    return Err(failure(
                        "unsupported",
                        "The native USB module has no current typed configuration baseline.",
                        false,
                    ));
                }
                (
                    snapshot,
                    json!({"key":key,"route":data["module"]["snapshot"]["route"]}),
                )
            }
            _ => unreachable!(),
        };
        crate::runtime::validate(&snapshot)
            .map_err(|error| failure("invalid_native_result", &error, false))?;
        let before = &snapshot["config"];
        Ok(
            json!({"transport":args["transport"],"target":target,"baseline":select(&snapshot,&["generation","savedGeneration","owner","dirty","pendingApply","view","valid"]),"baselineHash":digest(before),"currentConfig":before,"config":args["config"],"diff":diff(before,&args["config"]),"validation":{"valid":true,"level":"native-config-encoder","firmwareAcceptance":"Pending explicit Apply; firmware checks physical contact/layout semantics."},"effects":{"claimed":false,"applied":false,"saved":false},"guidance":"Review the diff, establish an explicit native setup claim, inspect fresh route/boot state, and use the complete candidate in the typed config change tool. Preparation grants no write or activation authority."}),
        )
    }
    pub(super) fn prepare(&self, args: &Value, client: &str) -> Result<Value, Value> {
        let expected = args["expectedRevision"].clone();
        let has_document = args.get("document").is_some();
        let has_edits = args.get("edits").is_some();
        if has_document && (has_edits || args.get("name").is_some()) {
            return Err(failure(
                "invalid_arguments",
                "A full document cannot be combined with edits or name. Supply one authoring form.",
                false,
            ));
        }
        let profile_id = args["profileId"]
            .as_str()
            .or_else(|| args["document"]["id"].as_str());
        if args.get("profileId").is_some()
            && has_document
            && args["profileId"] != args["document"]["id"]
        {
            return Err(failure(
                "invalid_arguments",
                "profileId and document.id must identify the same profile.",
                false,
            ));
        }
        let saved=match (profile_id,expected.as_u64()) {
            (Some(id),Some(revision))=>{
                let saved=self.native("profile_assignments_get",json!({"id":id}),client,Some("profiles"))?;
                if saved["document"]["revision"].as_u64()!=Some(revision){return Err(failure("revision_conflict","The saved profile revision changed. Reload it before preparing this edit.",false));}
                Some(saved)
            },
            (Some(id),None) if expected.is_null()&&has_document=>{
                match self.native("profile_get",json!({"id":id}),client,Some("profiles")) {
                    Ok(_)=>return Err(failure("revision_conflict","A new draft requires a fresh profile identity; this profile already exists.",false)),
                    Err(error) if error["error"]["code"]=="not_found"=>{},
                    Err(error)=>return Err(error),
                }
                None
            },
            (None,None) if expected.is_null()&&!has_document=>None,
            _=>return Err(failure("invalid_arguments","Existing profiles require profileId and numeric expectedRevision. New profiles require explicit expectedRevision:null with name or a fresh full document.",false)),
        };
        let mut document = if has_document {
            ProfileDocument::parse_draft(&serde_json::to_vec(&args["document"]).unwrap())
                .map_err(|error| failure("invalid_document", &error, false))?
        } else if let Some(saved) = &saved {
            if args.get("name").is_some() {
                return Err(failure(
                    "invalid_arguments",
                    "Use a set_name typed edit when editing an existing profile.",
                    false,
                ));
            }
            ProfileDocument::parse_draft(&serde_json::to_vec(&saved["document"]).unwrap())
                .map_err(|error| failure("invalid_document", &error, false))?
        } else {
            osg_profiles::new_document(text(args, "name")?)
                .map_err(|error| failure("invalid_document", &error, false))?
        };
        if expected
            .as_u64()
            .is_some_and(|revision| document.revision != revision)
            || expected.is_null() && document.revision != 1
        {
            return Err(failure(
                "revision_conflict",
                "The draft document revision must equal expectedRevision, or 1 for a new profile.",
                false,
            ));
        }
        if has_edits {
            edits(&mut document, &args["edits"])?;
        }
        // Reparse after edits so no malformed name, schema, collection or nested
        // field can hide behind compiler diagnostics or enter the draft cache.
        document = ProfileDocument::parse_draft(&serde_json::to_vec(&document).unwrap())
            .map_err(|error| failure("invalid_document", &error, false))?;
        let mut assignments: Vec<osg_profiles::store::LocalAssignment> =
            if let Some(value) = args.get("assignments") {
                serde_json::from_value(value.clone()).map_err(|error| {
                    failure(
                        "invalid_assignments",
                        &format!("Invalid local assignments: {error}"),
                        false,
                    )
                })?
            } else if let Some(saved) = &saved {
                serde_json::from_value(saved["assignments"].clone()).map_err(|error| {
                    failure(
                        "invalid_native_result",
                        &format!("Invalid saved local assignments: {error}"),
                        false,
                    )
                })?
            } else {
                vec![]
            };
        if args.get("assignments").is_none() {
            assignments.retain(|assignment| {
                document
                    .resources
                    .iter()
                    .any(|resource| resource.id == assignment.resource_id)
            });
            for assignment in &mut assignments {
                let resource = document
                    .resources
                    .iter()
                    .find(|resource| resource.id == assignment.resource_id)
                    .unwrap();
                assignment.ports.retain(|port| {
                    resource
                        .ports
                        .iter()
                        .any(|logical| logical.id == port.profile_port_id)
                });
            }
        }
        osg_profiles::store::validate_assignments(&document, &assignments)
            .map_err(|error| failure("invalid_assignments", &error, false))?;
        let validation = osg_profiles::validate_document(&document);
        let document = serde_json::to_value(document).unwrap();
        let assignments = serde_json::to_value(assignments).unwrap();
        let compilation = self.native(
            "profile_compile",
            json!({"document":document,"assignments":assignments}),
            client,
            Some("profileCompilation"),
        )?;
        let before = saved
            .as_ref()
            .map(|saved| json!({"document":saved["document"],"assignments":saved["assignments"]}))
            .unwrap_or(Value::Null);
        let delta = diff(
            &before,
            &json!({"document":document,"assignments":assignments}),
        );
        let draft_id = uuid::Uuid::new_v4().to_string();
        let bytes = document.to_string().len() + assignments.to_string().len();
        let mut drafts = self.drafts.lock().unwrap_or_else(|p| p.into_inner());
        drafts.retain(|draft| draft.created.elapsed() < DRAFT_LIFETIME);
        // Draft eviction is safe because saving an expired draft fails closed.
        while !drafts.is_empty()
            && (drafts.len() >= MAX_DRAFTS
                || drafts.iter().map(|draft| draft.bytes).sum::<usize>() + bytes > MAX_DRAFT_BYTES)
        {
            drafts.remove(0);
        }
        if bytes > MAX_DRAFT_BYTES {
            return Err(failure(
                "capacity",
                "This prepared draft exceeds its memory budget.",
                false,
            ));
        }
        drafts.push(Draft {
            id: draft_id.clone(),
            client: client.into(),
            document: document.clone(),
            assignments: assignments.clone(),
            expected_revision: expected.clone(),
            created: Instant::now(),
            bytes,
        });
        Ok(
            json!({"draftId":draft_id,"profileId":document["id"],"expectedRevision":expected,"expiresInSeconds":DRAFT_LIFETIME.as_secs(),"document":document,"assignments":assignments,"diff":delta,"validation":{"valid":validation.is_ok(),"errors":validation.err().into_iter().collect::<Vec<_>>()},"compilation":compilation,"effects":{"saved":false,"selected":false,"applied":false}}),
        )
    }
    pub(super) fn save_prepared(&self, args: &Value, client: &str) -> Result<Value, Value> {
        self.save_prepared_at(args, client, Instant::now())
    }
    // A supplied monotonic observation keeps expiry tests independent of the
    // host's uptime. No protocol argument can choose this observation.
    pub(super) fn save_prepared_at(&self, args: &Value, client: &str, now: Instant) -> Result<Value, Value> {
        let id = text(args, "draftId")?;
        let draft=self.drafts.lock().unwrap_or_else(|p|p.into_inner()).iter().find(|draft|draft.id==id&&draft.client==client&&now.saturating_duration_since(draft.created)<DRAFT_LIFETIME).cloned()
            .ok_or_else(||failure("draft_expired","The prepared draft expired or belongs to another client. Prepare the current saved revision again.",false))?;
        self.native("profile_draft_save",json!({"document":draft.document,"expectedRevision":draft.expected_revision,"assignments":draft.assignments}),client,Some("profiles"))
    }
}

/// Stable JSON-pointer diff with keyed graph arrays. Large values are summarized
/// by hash and byte count; the complete proposed document is returned separately.
fn diff(before: &Value, after: &Value) -> Value {
    let mut changes = vec![];
    let mut total = 0;
    walk_diff("", before, after, &mut changes, &mut total);
    json!({"changes":changes,"totalChanges":total,"truncated":total>changes.len(),"beforeHash":digest(before),"afterHash":digest(after)})
}
fn walk_diff(
    path: &str,
    before: &Value,
    after: &Value,
    changes: &mut Vec<Value>,
    total: &mut usize,
) {
    if before == after {
        return;
    }
    // A newly authored graph should show its individual node and binding
    // additions, rather than a single large opaque document replacement.
    if before.is_null() && after.is_object() {
        walk_diff(path, &json!({}), after, changes, total);
        return;
    }
    if after.is_null() && before.is_object() {
        walk_diff(path, before, &json!({}), changes, total);
        return;
    }
    if before.is_null() && after.is_array() {
        walk_diff(path, &json!([]), after, changes, total);
        return;
    }
    if after.is_null() && before.is_array() {
        walk_diff(path, before, &json!([]), changes, total);
        return;
    }
    if let (Some(left), Some(right)) = (before.as_object(), after.as_object()) {
        let keys = left.keys().chain(right.keys()).collect::<BTreeSet<_>>();
        for key in keys {
            walk_diff(
                &format!("{path}/{}", pointer(key)),
                left.get(key).unwrap_or(&Value::Null),
                right.get(key).unwrap_or(&Value::Null),
                changes,
                total,
            );
        }
    } else if let (Some(left), Some(right)) = (keyed(before), keyed(after)) {
        let keys = left.keys().chain(right.keys()).collect::<BTreeSet<_>>();
        for key in keys {
            walk_diff(
                &format!("{path}/{}", pointer(key)),
                left.get(key).copied().unwrap_or(&Value::Null),
                right.get(key).copied().unwrap_or(&Value::Null),
                changes,
                total,
            );
        }
    } else {
        *total += 1;
        if changes.len() < 128 {
            changes.push(json!({"path":if path.is_empty(){"/"}else{path},"kind":if before.is_null(){"added"}else if after.is_null(){"removed"}else{"changed"},"before":summary(before),"after":summary(after)}));
        }
    }
}
fn keyed(value: &Value) -> Option<BTreeMap<String, &Value>> {
    let values = value.as_array()?;
    let mut items = BTreeMap::new();
    for value in values {
        let id = value["id"].as_str()?;
        if items.insert(id.to_owned(), value).is_some() {
            return None;
        }
    }
    Some(items)
}
fn pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
fn summary(value: &Value) -> Value {
    let bytes = value.to_string().len();
    if bytes <= 1024 {
        value.clone()
    } else {
        json!({"summary":true,"bytes":bytes,"sha256":digest(value),"id":value["id"],"name":value["name"]})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_edits_reject_unknown_fields_and_keep_dangling_edges_for_diagnostics() {
        let mut profile = osg_profiles::default_document();
        assert!(edits(
            &mut profile,
            &json!([{"kind":"set_name","name":"A","owner":{}}])
        )
        .is_err());
        assert!(edits(
            &mut profile,
            &json!([{"kind":"patch","path":"/name","value":"A"}])
        )
        .is_err());
        assert!(edits(
            &mut profile,
            &json!([{"kind":"remove_resource","id":"absent"}])
        )
        .is_err());
        edits(
            &mut profile,
            &json!([{"kind":"set_eligibility","eligibility":{"gameIds":["dcs"]}}]),
        )
        .unwrap();
        assert_eq!(profile.eligibility.game_ids, ["dcs"]);
    }
    #[test]
    fn diff_is_deterministic_keyed_bounded_and_does_not_repeat_large_documents() {
        let before = json!({"resources":[{"id":"alpha","name":"Old"},{"id":"beta","name":"Kept"}]});
        let after = json!({"resources":[{"id":"beta","name":"Kept"},{"id":"alpha","name":"New"}]});
        let delta = diff(&before, &after);
        assert_eq!(delta["totalChanges"], 1);
        assert_eq!(delta["changes"][0]["path"], "/resources/alpha/name");
        assert_eq!(delta, diff(&before, &after));
        let before = json!((0..200)
            .map(|index| (format!("field-{index}"), json!(0)))
            .collect::<BTreeMap<_, _>>());
        let after = json!((0..200)
            .map(|index| (format!("field-{index}"), json!(1)))
            .collect::<BTreeMap<_, _>>());
        let delta = diff(&before, &after);
        assert_eq!(delta["changes"].as_array().unwrap().len(), 128);
        assert_eq!(delta["totalChanges"], 200);
        assert_eq!(delta["truncated"], true);
        assert!(summary(&json!("x".repeat(2000)))["summary"] == true);
    }
}
