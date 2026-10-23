//! Native admission for compiled raster panels. Authoring layers cannot enter this format.
use super::*;
mod artifact;
mod behavior;

fn canonical(value: &Value) -> Result<Vec<u8>, String> {
    serde_json_canonicalizer::to_vec(value).map_err(|e| e.to_string())
}
fn digest(value: &Value) -> Result<&str, String> {
    let value = value.as_str().ok_or("Expected compiled digest.")?;
    ensure(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Invalid compiled digest.",
    )?;
    Ok(value)
}
fn without(value: &Value, fields: &[&str]) -> Value {
    let mut value = value.clone();
    for key in fields {
        value.as_object_mut().unwrap().remove(*key);
    }
    value
}
fn behavior_semantics(value: &Value) -> Value {
    let mut value = value.clone();
    for region in value["regions"].as_array_mut().unwrap() {
        for transition in region["transitions"].as_array_mut().unwrap() {
            transition["actions"] = json!(transition
                .get("actions")
                .and_then(Value::as_array)
                .map(|actions| actions
                    .iter()
                    .filter(|action| action["type"] != "cue")
                    .cloned()
                    .collect::<Vec<_>>())
                .unwrap_or_default());
        }
    }
    value
}
pub(crate) fn validate_capabilities(value: &Value) -> Result<(), String> {
    artifact::capabilities(value)
}
pub(crate) fn validate_behavior(value: &Value) -> Result<(), String> {
    behavior::validate(value)
}
pub(crate) fn validate_wave(bytes: &[u8]) -> Result<(), String> {
    artifact::wave(bytes)
}

fn payload_bytes(
    manifest: &[Value],
    payloads: &[AssetPayload],
    caps: &Value,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    ensure(
        manifest.len() <= 64 && payloads.len() == manifest.len(),
        "Compiled asset budget/closure mismatch.",
    )?;
    let mut descriptors = BTreeSet::new();
    let mut total = 0;
    for asset in manifest {
        artifact::descriptor(asset, caps)?;
        ensure(
            descriptors.insert(asset["id"].as_str().unwrap()),
            "Duplicate compiled asset ID.",
        )?;
        total += asset["sizeBytes"].as_u64().unwrap();
    }
    ensure(
        total <= TOTAL_ASSET_BYTES as u64,
        "Compiled asset union exceeds budget.",
    )?;
    let mut bytes = BTreeMap::new();
    for payload in payloads {
        ensure(
            !bytes.contains_key(&payload.asset_id)
                && payload.base64.len() <= ASSET_BYTES.div_ceil(3) * 4,
            "Invalid/duplicate compiled payload.",
        )?;
        let descriptor = manifest
            .iter()
            .find(|a| a["id"] == payload.asset_id)
            .ok_or("Unknown compiled payload.")?;
        let data = STANDARD
            .decode(&payload.base64)
            .map_err(|_| "Invalid compiled asset base64.")?;
        ensure(
            STANDARD.encode(&data) == payload.base64
                && data.len() as u64 == descriptor["sizeBytes"].as_u64().unwrap()
                && hash(&data) == descriptor["sha256"].as_str().unwrap(),
            "Compiled asset digest/size mismatch.",
        )?;
        bytes.insert(payload.asset_id.clone(), data);
    }
    Ok(bytes)
}

fn panel(
    doc: &Value,
    payloads: &[AssetPayload],
    caps: &Value,
) -> Result<(Vec<SourceControl>, String, String), String> {
    validate_capabilities(caps)?;
    object(
        doc,
        &["schemaVersion", "ui", "presentation", "targets", "bindings"],
        &[],
    )?;
    ensure(
        doc["schemaVersion"] == 4,
        "Unsupported compiled panel schema.",
    )?;
    let ui = &doc["ui"];
    object(
        ui,
        &[
            "schemaVersion",
            "id",
            "title",
            "controls",
            "pages",
            "feedbackWidgets",
            "assets",
        ],
        &[],
    )?;
    ensure(
        ui["schemaVersion"] == 2,
        "Unsupported compiled logical UI schema.",
    )?;
    let ui_id = stable(&ui["id"])?.to_owned();
    let title = text(&ui["title"])?.to_owned();
    let mut ids = BTreeSet::new();
    let mut unique = |value: &Value| -> Result<(), String> {
        ensure(
            ids.insert(stable(value)?.to_owned()),
            "Duplicate compiled panel ID.",
        )
    };
    let controls = list(&ui["controls"], 0, 128)?;
    let mut source_controls = Vec::new();
    for control in controls {
        let axis = control["kind"] == "axis";
        object(
            control,
            if axis {
                &["id", "label", "kind", "axisMode", "fallback"]
            } else {
                &["id", "label", "kind"]
            },
            &[],
        )?;
        unique(&control["id"])?;
        text(&control["label"])?;
        let kind = if axis {
            ensure(
                control["axisMode"] == "unipolar",
                "Compiled axis must be unipolar.",
            )?;
            SourceKind::Axis {
                axis_mode: AxisMode::Unipolar,
                fallback: number(&control["fallback"], 0., 1.)?,
            }
        } else {
            ensure(
                control["kind"] == "button",
                "Unsupported compiled control endpoint.",
            )?;
            SourceKind::Button
        };
        source_controls.push(SourceControl {
            id: stable(&control["id"])?.into(),
            kind,
        });
    }
    let feedback = list(&ui["feedbackWidgets"], 0, 64)?;
    let mut channels = BTreeSet::new();
    for f in feedback {
        object(f, &["id", "label", "channelId", "kind"], &[])?;
        unique(&f["id"])?;
        text(&f["label"])?;
        ensure(
            channels.insert(stable(&f["channelId"])?),
            "Duplicate compiled feedback channel.",
        )?;
        ensure(
            matches!(f["kind"].as_str(), Some("indicator" | "gauge" | "text")),
            "Invalid compiled feedback type.",
        )?;
    }
    let pages = list(&ui["pages"], 1, 16)?;
    for page in pages {
        object(page, &["id", "title"], &[])?;
        unique(&page["id"])?;
        text(&page["title"])?;
    }
    let assets = list(&ui["assets"], 1, 64)?;
    for asset in assets {
        unique(&asset["id"])?;
    }
    let bytes = payload_bytes(assets, payloads, caps)?;
    let p = &doc["presentation"];
    object(p, &["format", "viewports", "widgets", "artifacts"], &[])?;
    ensure(
        p["format"] == "compiled-raster-v1",
        "Unsupported compiled presentation.",
    )?;
    let viewports = list(&p["viewports"], pages.len(), pages.len())?;
    let mut views = BTreeSet::new();
    for viewport in viewports {
        object(viewport, &["pageId", "width", "height"], &["background"])?;
        if let Some(background) = viewport.get("background") {
            let color = background
                .as_str()
                .ok_or("Invalid panel background color.")?;
            ensure(
                color.len() == 7
                    && color.starts_with('#')
                    && color[1..].bytes().all(|b| b.is_ascii_hexdigit()),
                "Invalid panel background color.",
            )?;
        }
        ensure(
            views.insert(stable(&viewport["pageId"])?)
                && pages.iter().any(|page| page["id"] == viewport["pageId"]),
            "Invalid compiled viewport page.",
        )?;
        integer(&viewport["width"], 320, 8192)?;
        integer(&viewport["height"], 320, 8192)?;
    }
    let artifacts = list(&p["artifacts"], 1, 192)?;
    let mut hashes = BTreeSet::new();
    let mut closure = BTreeMap::new();
    for entry in artifacts {
        object(entry, &["hash", "artifact"], &[])?;
        let sha = digest(&entry["hash"])?;
        ensure(hashes.insert(sha), "Duplicate compiled artifact.")?;
        ensure(
            hash(&canonical(&entry["artifact"])?) == sha,
            "Compiled artifact content hash mismatch.",
        )?;
        artifact::validate(&entry["artifact"], &bytes, caps)?;
        for asset in entry["artifact"]["assets"].as_array().unwrap() {
            let id = asset["id"].as_str().unwrap();
            if let Some(old) = closure.insert(id, asset) {
                ensure(old == asset, "Conflicting compiled asset identity.")?;
            }
        }
    }
    ensure(
        closure.len() == assets.len()
            && assets.iter().all(|asset| {
                closure
                    .get(asset["id"].as_str().unwrap())
                    .is_some_and(|descriptor| **descriptor == *asset)
            }),
        "Compiled panel asset closure mismatch.",
    )?;
    let widgets = list(&p["widgets"], 1, 192)?;
    let mut used_artifacts = BTreeSet::new();
    let mut used_controls = BTreeSet::new();
    let mut used_feedback = BTreeSet::new();
    let mut widget_caps = vec![];
    let mut surface_bytes = 0;
    for widget in widgets {
        object(
            widget,
            &[
                "id",
                "pageId",
                "label",
                "bounds",
                "artifactHash",
                "inputBindings",
                "outputBindings",
            ],
            &[],
        )?;
        unique(&widget["id"])?;
        text(&widget["label"])?;
        let viewport = viewports
            .iter()
            .find(|v| v["pageId"] == widget["pageId"])
            .ok_or("Unknown compiled widget page.")?;
        let b = &widget["bounds"];
        object(b, &["x", "y", "width", "height"], &[])?;
        let x = number(&b["x"], 0., 1.)?;
        let y = number(&b["y"], 0., 1.)?;
        let w = number(&b["width"], 0., 1.)?;
        let h = number(&b["height"], 0., 1.)?;
        ensure(
            w > 0. && h > 0. && x + w <= 1. + 1e-10 && y + h <= 1. + 1e-10,
            "Widget outside compiled panel.",
        )?;
        let a = &artifacts
            .iter()
            .find(|entry| entry["hash"] == widget["artifactHash"])
            .ok_or("Unknown widget artifact.")?["artifact"];
        ensure(
            a["width"].as_f64() == Some((w * viewport["width"].as_f64().unwrap()).round())
                && a["height"].as_f64() == Some((h * viewport["height"].as_f64().unwrap()).round()),
            "Widget raster size differs from placement; recompile.",
        )?;
        used_artifacts.insert(widget["artifactHash"].as_str().unwrap());
        surface_bytes += a["width"].as_u64().unwrap() * a["height"].as_u64().unwrap() * 8
            + 768
            + a["resources"]["dictionaryBytes"].as_u64().unwrap()
            + a["resources"]["frameMapBytes"].as_u64().unwrap() * 2
            + a["resources"]["audioWorkingBytes"].as_u64().unwrap();
        let inputs = a["inputs"].as_array().unwrap();
        let input_bindings = list(&widget["inputBindings"], inputs.len(), inputs.len())?;
        let mut ports = BTreeSet::new();
        for binding in input_bindings {
            object(binding, &["portId", "controlId"], &[])?;
            let port = inputs
                .iter()
                .find(|p| p["id"] == binding["portId"])
                .ok_or("Unknown compiled input port.")?;
            let control = controls
                .iter()
                .find(|c| c["id"] == binding["controlId"])
                .ok_or("Unknown compiled input endpoint.")?;
            ensure(
                ports.insert(port["id"].as_str().unwrap())
                    && used_controls.insert(control["id"].as_str().unwrap()),
                "Duplicate compiled input binding.",
            )?;
            let neutral = &a["behavior"]["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["id"] == port["id"])
                .unwrap()["neutral"];
            match port["type"].as_str() {
                Some("boolean") => ensure(
                    control["kind"] == "button" && *neutral == false,
                    "Compiled button neutral must be false.",
                )?,
                Some("number") => {
                    let min = port["min"].as_f64().unwrap();
                    let max = port["max"].as_f64().unwrap();
                    let fallback = (neutral.as_f64().unwrap() - min) / (max - min);
                    ensure(
                        control["kind"] == "axis"
                            && control["axisMode"] == "unipolar"
                            && control["fallback"].as_f64() == Some(fallback),
                        "Compiled numeric input neutral mismatch.",
                    )?;
                }
                Some("enum") => {
                    let values = port["values"].as_array().unwrap();
                    ensure(
                        values.len() > 1,
                        "A controller enum requires at least two values.",
                    )?;
                    let index = values.iter().position(|v| v == neutral).unwrap();
                    ensure(
                        control["kind"] == "axis"
                            && control["axisMode"] == "unipolar"
                            && control["fallback"].as_f64()
                                == Some(index as f64 / (values.len() - 1) as f64),
                        "Compiled enum input neutral mismatch.",
                    )?;
                }
                _ => return Err("Unsupported compiled input.".into()),
            }
        }
        let outputs = a["outputs"].as_array().unwrap();
        let output_bindings = list(&widget["outputBindings"], outputs.len(), outputs.len())?;
        let mut ports = BTreeSet::new();
        for binding in output_bindings {
            object(binding, &["portId", "feedbackId"], &[])?;
            let port = outputs
                .iter()
                .find(|p| p["id"] == binding["portId"])
                .ok_or("Unknown compiled output port.")?;
            let f = feedback
                .iter()
                .find(|f| f["id"] == binding["feedbackId"])
                .ok_or("Unknown compiled feedback endpoint.")?;
            ensure(
                ports.insert(port["id"].as_str().unwrap())
                    && used_feedback.insert(f["id"].as_str().unwrap()),
                "Duplicate compiled output binding.",
            )?;
            ensure(
                f["kind"]
                    == match port["type"].as_str() {
                        Some("boolean") => "indicator",
                        Some("number") => "gauge",
                        _ => "text",
                    },
                "Compiled feedback type mismatch.",
            )?;
            ensure(
                a["behavior"]["signals"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s["id"] == port["signal"] && s["source"] == "feedback")
                    && a["behavior"]["events"].as_array().unwrap().iter().any(|e| {
                        e["updates"]
                            .as_array()
                            .is_some_and(|updates| updates.contains(&port["signal"]))
                    }),
                "Feedback port needs an admitting feedback event.",
            )?;
        }
        widget_caps.push(json!({"id":widget["id"],"behavior":behavior_semantics(&a["behavior"]),"inputs":sorted(inputs.iter().map(|p|without(p,&["label"])).collect(),"id"),"outputs":sorted(outputs.iter().map(|p|without(p,&["label"])).collect(),"id"),"inputBindings":sorted(input_bindings.clone(),"portId"),"outputBindings":sorted(output_bindings.clone(),"portId")}));
    }
    ensure(
        used_artifacts.len() == artifacts.len()
            && used_controls.len() == controls.len()
            && used_feedback.len() == feedback.len(),
        "Unbound compiled panel data.",
    )?;
    ensure(
        pages
            .iter()
            .all(|page| widgets.iter().any(|w| w["pageId"] == page["id"])),
        "Every page needs a compiled widget.",
    )?;
    let targets = list(&doc["targets"], if controls.is_empty() { 0 } else { 1 }, 8)?;
    for target in targets {
        object(target, &["id", "label", "axes", "buttons", "hats"], &[])?;
        unique(&target["id"])?;
        text(&target["label"])?;
        let axes = list(&target["axes"], 0, 16)?;
        ensure(
            axes.iter().all(|v| v == "unipolar"),
            "Invalid compiled target axes.",
        )?;
        let buttons = integer(&target["buttons"], 0, 128)?;
        ensure(
            target["hats"] == 0 && (!axes.is_empty() || buttons > 0),
            "Invalid compiled target.",
        )?;
    }
    let bindings = list(&doc["bindings"], controls.len(), controls.len())?;
    let mut used = BTreeSet::new();
    let mut slots = BTreeSet::new();
    for binding in bindings {
        object(binding, &["controlId", "targetId", "kind", "index"], &[])?;
        let control = controls
            .iter()
            .find(|c| c["id"] == binding["controlId"])
            .ok_or("Unknown logical control binding.")?;
        let target = targets
            .iter()
            .find(|t| t["id"] == binding["targetId"])
            .ok_or("Unknown logical target binding.")?;
        ensure(
            used.insert(control["id"].as_str().unwrap()),
            "Duplicate logical control binding.",
        )?;
        let index = integer(&binding["index"], 0, 127)?;
        ensure(
            slots.insert(format!("{}:{}:{index}", target["id"], binding["kind"])),
            "Duplicate target slot.",
        )?;
        ensure(
            control["kind"] == binding["kind"]
                && if binding["kind"] == "button" {
                    index < target["buttons"].as_u64().unwrap()
                } else {
                    binding["kind"] == "axis"
                        && index < target["axes"].as_array().unwrap().len() as u64
                },
            "Incompatible logical binding.",
        )?;
    }
    let manifest = canonical(doc)?.len() as u64;
    let asset_bytes = assets
        .iter()
        .map(|a| a["sizeBytes"].as_u64().unwrap())
        .sum::<u64>();
    let resident = asset_bytes * 3 + manifest * 4 + surface_bytes;
    ensure(
        manifest <= DOCUMENT_BYTES as u64
            && asset_bytes <= caps["maxTotalBytes"].as_u64().unwrap()
            && resident * 2 <= caps["maxResidentBytes"].as_u64().unwrap(),
        "Compiled panel exceeds asset/active-candidate memory budget.",
    )?;
    let projection = json!({"uiId":ui_id,"controls":sorted(controls.iter().map(|c|without(c,&["label"])).collect(),"id"),"feedback":sorted(feedback.iter().map(|f|without(f,&["label"])).collect(),"id"),"widgets":sorted(widget_caps,"id"),"targets":sorted(targets.iter().map(|t|without(t,&["label"])).collect(),"id"),"bindings":sorted(bindings.clone(),"controlId")});
    let fingerprint = format!("sha256:{}", hash(&canonical(&projection)?));
    Ok((source_controls, fingerprint, title))
}
pub(super) fn validate(upload: PublicationUpload, doc: Value) -> Result<Publication, String> {
    ensure(
        serde_json::to_vec(&upload)
            .map_err(|e| e.to_string())?
            .len()
            <= BUNDLE_BYTES,
        "Compiled publication wire budget exceeded.",
    )?;
    let (controls, capability_fingerprint, title) =
        panel(&doc, &upload.assets, &artifact::default_capabilities())?;
    if let Some(expected) = upload.capability_fingerprint {
        ensure(
            expected == capability_fingerprint,
            "Capability fingerprint mismatch.",
        )?;
    }
    Ok(Publication {
        #[cfg(any(test, feature = "legacy-test-fixtures"))]
        raster_cache: Default::default(),
        canonical_document: upload.canonical_document,
        publication_hash: upload.publication_hash,
        capability_fingerprint,
        assets: upload.assets,
        ui_id: doc["ui"]["id"].as_str().unwrap().into(),
        title,
        controls,
        feedback: feedback_descriptors(&doc)?,
    })
}
pub(crate) fn check_capabilities(publication: &Publication, caps: &Value) -> Result<(), String> {
    let doc: Value = serde_json::from_str(&publication.canonical_document)
        .map_err(|_| "Invalid retained compiled publication.")?;
    panel(&doc, &publication.assets, caps).map(|_| ())
}
pub(crate) fn cached_payloads(
    publication: &Publication,
    have: &Value,
) -> Result<Vec<AssetPayload>, String> {
    let have = list(have, 0, 64)?;
    let mut hashes = BTreeSet::new();
    for value in have {
        ensure(
            hashes.insert(digest(value)?),
            "Duplicate cached asset digest.",
        )?;
    }
    let doc: Value = serde_json::from_str(&publication.canonical_document)
        .map_err(|_| "Invalid retained publication.")?;
    let assets = doc["ui"]["assets"]
        .as_array()
        .ok_or("Missing retained asset closure.")?;
    Ok(publication
        .assets
        .iter()
        .filter(|payload| {
            !assets.iter().any(|a| {
                a["id"] == payload.asset_id
                    && a["sha256"].as_str().is_some_and(|sha| hashes.contains(sha))
            })
        })
        .cloned()
        .collect())
}
pub(crate) fn feedback_descriptors(doc: &Value) -> Result<Vec<crate::FeedbackDescriptor>, String> {
    let widgets = doc["presentation"]["widgets"]
        .as_array()
        .ok_or("Missing compiled widgets.")?;
    let artifacts = doc["presentation"]["artifacts"]
        .as_array()
        .ok_or("Missing compiled artifacts.")?;
    let mut result = vec![];
    for f in doc["ui"]["feedbackWidgets"]
        .as_array()
        .ok_or("Missing compiled feedback.")?
    {
        let widget = widgets
            .iter()
            .find(|w| {
                w["outputBindings"]
                    .as_array()
                    .is_some_and(|bindings| bindings.iter().any(|b| b["feedbackId"] == f["id"]))
            })
            .ok_or("Unbound compiled feedback.")?;
        let binding = widget["outputBindings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["feedbackId"] == f["id"])
            .unwrap();
        let a = &artifacts
            .iter()
            .find(|a| a["hash"] == widget["artifactHash"])
            .ok_or("Unknown compiled feedback artifact.")?["artifact"];
        let port = a["outputs"]
            .as_array()
            .ok_or("Missing compiled output ports.")?
            .iter()
            .find(|p| p["id"] == binding["portId"])
            .ok_or("Unknown compiled feedback port.")?;
        let (kind, range) = match port["type"].as_str() {
            Some("boolean") => (crate::FeedbackKind::Boolean, None),
            Some("number") => (
                crate::FeedbackKind::Number,
                Some(crate::FeedbackRange {
                    min: port["min"].as_f64().ok_or("Missing feedback minimum.")?,
                    max: port["max"].as_f64().ok_or("Missing feedback maximum.")?,
                }),
            ),
            Some("enum") => (crate::FeedbackKind::Text, None),
            _ => return Err("Invalid compiled feedback kind.".into()),
        };
        result.push(crate::FeedbackDescriptor {
            channel_id: stable(&f["channelId"])?.into(),
            output_ids: vec![stable(&f["id"])?.into()],
            kind,
            unit: None,
            range,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
