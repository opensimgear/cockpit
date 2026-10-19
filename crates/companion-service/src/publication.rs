use crate::{AxisMode, InputValue, SourceControl, SourceDefinition, SourceKind};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const DOCUMENT_BYTES: usize = 1024 * 1024;
pub const ASSET_BYTES: usize = 2 * 1024 * 1024;
pub const TOTAL_ASSET_BYTES: usize = 8 * 1024 * 1024;
pub const TOTAL_ASSET_PIXELS: u64 = 32 * 1024 * 1024;
pub const FRAME_BYTES: usize = 16 * 1024;
pub const BUNDLE_BYTES: usize = 13 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetPayload {
    pub asset_id: String,
    pub base64: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicationUpload {
    pub canonical_document: String,
    pub publication_hash: String,
    #[serde(default)]
    pub capability_fingerprint: Option<String>,
    pub assets: Vec<AssetPayload>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Publication {
    pub canonical_document: String,
    pub publication_hash: String,
    pub capability_fingerprint: String,
    pub assets: Vec<AssetPayload>,
    pub ui_id: String,
    pub title: String,
    #[serde(skip)]
    pub controls: Vec<SourceControl>,
    #[serde(skip)]
    pub feedback: Vec<crate::FeedbackDescriptor>,
}
pub(crate) fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
pub(crate) fn digest_id(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn label(value: &str) -> bool {
    !value.trim().is_empty()
        && value.encode_utf16().count() <= 120
        && !value.chars().any(|c| c <= '\u{1f}')
}
pub(crate) fn ensure(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn object<'a>(
    v: &'a Value,
    fields: &[&str],
    optional: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let map = v.as_object().ok_or("Expected object.")?;
    ensure(
        fields.iter().all(|f| map.contains_key(*f))
            && map
                .keys()
                .all(|k| fields.contains(&k.as_str()) || optional.contains(&k.as_str())),
        "Unknown or missing publication field.",
    )?;
    Ok(map)
}
fn list(v: &Value, min: usize, max: usize) -> Result<&Vec<Value>, String> {
    let list = v.as_array().ok_or("Expected array.")?;
    ensure(
        (min..=max).contains(&list.len()),
        "Publication array exceeds budget.",
    )?;
    Ok(list)
}
fn text(v: &Value) -> Result<&str, String> {
    let s = v.as_str().ok_or("Expected string.")?;
    ensure(label(s), "Invalid publication text.")?;
    Ok(s)
}
fn stable(v: &Value) -> Result<&str, String> {
    let s = v.as_str().ok_or("Expected stable ID.")?;
    ensure(id(s), "Invalid stable ID.")?;
    Ok(s)
}
fn number(v: &Value, min: f64, max: f64) -> Result<f64, String> {
    let n = v.as_f64().ok_or("Expected number.")?;
    ensure(
        n.is_finite() && (min..=max).contains(&n),
        "Number exceeds publication bounds.",
    )?;
    Ok(n)
}
fn integer(v: &Value, min: u64, max: u64) -> Result<u64, String> {
    let n = v.as_u64().ok_or("Expected integer.")?;
    ensure(
        (min..=max).contains(&n),
        "Integer exceeds publication bounds.",
    )?;
    Ok(n)
}
fn style(v: &Value) -> Result<(), String> {
    for c in object(v, &[], &["foreground", "background", "accent"])?.values() {
        let c = c.as_str().ok_or("Invalid color.")?;
        ensure(
            c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid color.",
        )?;
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
impl Bounds {
    fn contains(self, other: Self) -> bool {
        const EPSILON: f64 = 1e-9;
        other.x >= self.x - EPSILON
            && other.y >= self.y - EPSILON
            && other.x + other.width <= self.x + self.width + EPSILON
            && other.y + other.height <= self.y + self.height + EPSILON
    }
    fn overlaps(self, other: Self) -> bool {
        const EPSILON: f64 = 1e-9;
        (self.x + self.width).min(other.x + other.width) - self.x.max(other.x) > EPSILON
            && (self.y + self.height).min(other.y + other.height) - self.y.max(other.y) > EPSILON
    }
}
fn bounds(v: &Value, message: &str) -> Result<Bounds, String> {
    object(v, &["x", "y", "width", "height"], &[])?;
    let x = number(&v["x"], 0., 1.)?;
    let y = number(&v["y"], 0., 1.)?;
    let w = number(&v["width"], 0., 1.)?;
    let h = number(&v["height"], 0., 1.)?;
    ensure(w > 0. && h > 0. && x + w <= 1. && y + h <= 1., message)?;
    Ok(Bounds {
        x,
        y,
        width: w,
        height: h,
    })
}
fn sorted(mut values: Vec<Value>, key: &str) -> Vec<Value> {
    values.sort_by(|a, b| a[key].as_str().cmp(&b[key].as_str()));
    values
}

impl Publication {
    pub fn definition(&self, instance_id: &str) -> SourceDefinition {
        SourceDefinition {
            source_id: instance_id.into(),
            name: self.title.clone(),
            publication_hash: self.publication_hash.clone(),
            controls: self.controls.clone(),
        }
    }
    pub fn bytes(&self) -> usize {
        self.canonical_document.len() + self.assets.iter().map(|a| a.base64.len()).sum::<usize>()
    }
    pub fn validate(upload: PublicationUpload) -> Result<Self, String> {
        ensure(
            upload.canonical_document.len() <= DOCUMENT_BYTES
                && digest_id(&upload.publication_hash),
            "Invalid publication size/hash.",
        )?;
        ensure(
            format!("sha256:{}", hash(upload.canonical_document.as_bytes()))
                == upload.publication_hash,
            "Publication content hash mismatch.",
        )?;
        let doc: Value = serde_json::from_str(&upload.canonical_document)
            .map_err(|_| "Malformed publication JSON.")?;
        object(
            &doc,
            &["schemaVersion", "ui", "presentation", "targets", "bindings"],
            &[],
        )?;
        ensure(doc["schemaVersion"] == 2, "Unsupported designer schema.")?;
        let canonical = serde_json_canonicalizer::to_vec(&doc)
            .map_err(|_| "Cannot canonicalize publication.")?;
        ensure(
            canonical == upload.canonical_document.as_bytes(),
            "Publication must use canonical validated JSON.",
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
        ensure(ui["schemaVersion"] == 1, "Unsupported logical UI schema.")?;
        let ui_id = stable(&ui["id"])?.to_owned();
        let title = text(&ui["title"])?.to_owned();
        let mut ids = BTreeSet::new();
        let mut unique = |v: &Value| -> Result<String, String> {
            let value = stable(v)?.to_owned();
            ensure(ids.insert(value.clone()), "Duplicate publication ID.")?;
            Ok(value)
        };
        let mut controls = Vec::new();
        for c in list(&ui["controls"], 0, 128)? {
            let kind = c["kind"].as_str().ok_or("Missing control kind.")?;
            let extras: &[&str] = match kind {
                "button" => &[],
                "axis" => &["axisMode", "fallback"],
                "hat" => &["fallback"],
                _ => return Err("Unsupported control kind.".into()),
            };
            let mut fields = vec!["id", "label", "kind"];
            fields.extend_from_slice(extras);
            object(c, &fields, &["style"])?;
            let control_id = unique(&c["id"])?;
            text(&c["label"])?;
            if let Some(s) = c.get("style") {
                style(s)?;
            }
            let kind = match kind {
                "button" => SourceKind::Button,
                "hat" => {
                    ensure(c["fallback"] == -1, "Invalid hat fallback.")?;
                    SourceKind::Hat
                }
                _ => {
                    let axis_mode = match c["axisMode"].as_str() {
                        Some("bipolar") => AxisMode::Bipolar,
                        Some("unipolar") => AxisMode::Unipolar,
                        _ => return Err("Invalid axis mode.".into()),
                    };
                    let fallback = number(
                        &c["fallback"],
                        if axis_mode == AxisMode::Bipolar {
                            -1.
                        } else {
                            0.
                        },
                        1.,
                    )?;
                    SourceKind::Axis {
                        axis_mode,
                        fallback,
                    }
                }
            };
            controls.push(SourceControl {
                id: control_id,
                kind,
            });
        }
        let feedback = list(&ui["feedbackWidgets"], 0, 64)?;
        for f in feedback {
            object(f, &["id", "label", "channelId", "kind"], &["style"])?;
            unique(&f["id"])?;
            text(&f["label"])?;
            stable(&f["channelId"])?;
            ensure(
                matches!(f["kind"].as_str(), Some("text" | "indicator" | "gauge")),
                "Invalid feedback kind.",
            )?;
            if let Some(s) = f.get("style") {
                style(s)?;
            }
        }
        let pages = list(&ui["pages"], 1, 16)?;
        for page in pages {
            object(page, &["id", "title", "items"], &[])?;
            unique(&page["id"])?;
            text(&page["title"])?;
            for item in list(&page["items"], 0, 192)? {
                object(item, &["kind", "refId", "bounds"], &[])?;
                stable(&item["refId"])?;
                bounds(&item["bounds"], "Widget lies outside its page.")?;
            }
        }
        let assets = list(&ui["assets"], 0, 64)?;
        let mut total = 0;
        for asset in assets {
            object(asset, &["id", "sha256", "sizeBytes", "mime"], &[])?;
            unique(&asset["id"])?;
            let digest = asset["sha256"].as_str().ok_or("Invalid asset hash.")?;
            ensure(
                digest_id(&format!("sha256:{digest}")),
                "Invalid asset hash.",
            )?;
            total += integer(&asset["sizeBytes"], 1, ASSET_BYTES as u64)?;
            ensure(
                matches!(
                    asset["mime"].as_str(),
                    Some("image/png" | "image/jpeg" | "image/webp")
                ),
                "Invalid raster MIME.",
            )?;
        }
        ensure(
            total <= TOTAL_ASSET_BYTES as u64 && upload.assets.len() == assets.len(),
            "Asset budget/manifest mismatch.",
        )?;
        let mut asset_ids = BTreeSet::new();
        let mut asset_pixels = 0;
        for payload in &upload.assets {
            ensure(
                asset_ids.insert(&payload.asset_id)
                    && payload.base64.len() <= ASSET_BYTES.div_ceil(3) * 4,
                "Invalid asset payload.",
            )?;
            let manifest = assets
                .iter()
                .find(|a| a["id"] == payload.asset_id)
                .ok_or("Unknown asset.")?;
            let bytes = STANDARD
                .decode(&payload.base64)
                .map_err(|_| "Invalid asset base64.")?;
            ensure(
                STANDARD.encode(&bytes) == payload.base64
                    && bytes.len() as u64 == manifest["sizeBytes"].as_u64().unwrap()
                    && hash(&bytes) == manifest["sha256"].as_str().unwrap(),
                "Asset digest/size mismatch.",
            )?;
            asset_pixels += validate_raster(&bytes, manifest["mime"].as_str().unwrap())?;
            ensure(
                asset_pixels <= TOTAL_ASSET_PIXELS,
                "Asset total decoded pixels exceed budget.",
            )?;
        }
        let p = &doc["presentation"];
        object(p, &["widgets", "groups", "viewports", "grid"], &[])?;
        object(&p["grid"], &["columns", "rows", "snap"], &[])?;
        integer(&p["grid"]["columns"], 1, 128)?;
        integer(&p["grid"]["rows"], 1, 128)?;
        ensure(p["grid"]["snap"].is_boolean(), "Invalid grid.")?;
        let groups = list(&p["groups"], 0, 64)?;
        for group in groups {
            object(group, &["id", "pageId", "label"], &[])?;
            unique(&group["id"])?;
            text(&group["label"])?;
            ensure(
                pages.iter().any(|p| p["id"] == group["pageId"]),
                "Unknown group page.",
            )?;
        }
        let mut viewport_pages = BTreeSet::new();
        let mut viewport_regions = BTreeMap::new();
        for v in list(&p["viewports"], pages.len(), pages.len())? {
            object(
                v,
                &["pageId", "width", "height"],
                &["safeInsets", "backgroundAssetId", "regions"],
            )?;
            let page = stable(&v["pageId"])?;
            ensure(
                viewport_pages.insert(page) && pages.iter().any(|p| p["id"] == page),
                "Invalid viewport page.",
            )?;
            integer(&v["width"], 320, 8192)?;
            integer(&v["height"], 320, 8192)?;
            if let Some(regions) = v.get("regions") {
                let mut areas: Vec<Bounds> = Vec::new();
                for region in list(regions, 1, 32)? {
                    object(region, &["id", "label", "bounds"], &[])?;
                    unique(&region["id"])?;
                    text(&region["label"])?;
                    let area = bounds(
                        &region["bounds"],
                        "Viewport lies outside its companion screen.",
                    )?;
                    ensure(
                        areas.iter().all(|other| !area.overlaps(*other)),
                        "Companion viewports must not overlap.",
                    )?;
                    areas.push(area);
                }
                viewport_regions.insert(page, areas);
            }
            if let Some(i) = v.get("safeInsets") {
                object(i, &["top", "right", "bottom", "left"], &[])?;
                for n in i.as_object().unwrap().values() {
                    number(n, 0., 0.25)?;
                }
                ensure(
                    i["left"].as_f64().unwrap() + i["right"].as_f64().unwrap() < 0.5
                        && i["top"].as_f64().unwrap() + i["bottom"].as_f64().unwrap() < 0.5,
                    "Safe insets leave no usable viewport.",
                )?;
            }
            if let Some(a) = v.get("backgroundAssetId") {
                ensure(
                    assets.iter().any(|asset| asset["id"] == *a),
                    "Unknown background asset.",
                )?;
            }
        }
        let widgets = list(&p["widgets"], 1, 192)?;
        let mut used_controls = BTreeSet::new();
        let mut used_feedback = BTreeSet::new();
        let mut widget_caps = Vec::new();
        for w in widgets {
            object(
                w,
                &[
                    "id",
                    "pageId",
                    "kind",
                    "label",
                    "bounds",
                    "controlIds",
                    "feedbackIds",
                    "config",
                ],
                &["groupId", "style"],
            )?;
            unique(&w["id"])?;
            text(&w["label"])?;
            let widget_bounds = bounds(&w["bounds"], "Widget lies outside its page.")?;
            ensure(
                pages.iter().any(|p| p["id"] == w["pageId"]),
                "Unknown widget page.",
            )?;
            if let Some(regions) = viewport_regions.get(w["pageId"].as_str().unwrap()) {
                ensure(
                    regions.iter().any(|area| area.contains(widget_bounds)),
                    "Widget must be entirely inside one companion viewport.",
                )?;
            }
            if let Some(s) = w.get("style") {
                style(s)?;
            }
            if let Some(g) = w.get("groupId") {
                ensure(
                    groups
                        .iter()
                        .any(|g2| g2["id"] == *g && g2["pageId"] == w["pageId"]),
                    "Invalid group reference.",
                )?;
            }
            let kind = w["kind"].as_str().ok_or("Missing widget kind.")?;
            let (expected, output): (&[&str], Option<&str>) = match kind {
                "button" => (&["button"], None),
                "selector" | "slider" => (&["axis"], None),
                "xy" => (&["axis", "axis"], None),
                "encoder" => (&["button", "button"], None),
                "hat" => (&["hat"], None),
                "text" => (&[], Some("text")),
                "numeric" | "gauge" => (&[], Some("gauge")),
                "icon" | "indicator" => (&[], Some("indicator")),
                _ => return Err("Unsupported widget.".into()),
            };
            let refs = list(&w["controlIds"], expected.len(), expected.len())?;
            for (i, r) in refs.iter().enumerate() {
                let r = stable(r)?;
                ensure(
                    used_controls.insert(r)
                        && ui["controls"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|c| c["id"] == r && c["kind"] == expected[i]),
                    "Invalid widget input reference.",
                )?;
            }
            for r in list(
                &w["feedbackIds"],
                usize::from(output.is_some()),
                usize::from(output.is_some()),
            )? {
                let r = stable(r)?;
                ensure(
                    used_feedback.insert(r)
                        && feedback
                            .iter()
                            .any(|f| f["id"] == r && Some(f["kind"].as_str().unwrap()) == output),
                    "Invalid widget feedback reference.",
                )?;
            }
            validate_config(w, &controls, assets)?;
            let mut config = w["config"].clone();
            for k in ["iconAssetId", "symbol", "prefix", "suffix", "orientation"] {
                config.as_object_mut().unwrap().remove(k);
            }
            if kind == "selector" {
                config["options"] = json!(w["config"]["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|o| json!({"value":o["value"]}))
                    .collect::<Vec<_>>());
            }
            widget_caps.push(json!({"id":w["id"],"kind":w["kind"],"controlIds":w["controlIds"],"feedbackIds":w["feedbackIds"],"config":config}));
        }
        ensure(
            used_controls.len() == controls.len() && used_feedback.len() == feedback.len(),
            "Unbound widget primitive.",
        )?;
        for page in pages {
            let expected: Vec<_> =
                widgets
                    .iter()
                    .filter(|w| w["pageId"] == page["id"])
                    .flat_map(|w| {
                        w["controlIds"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|r| json!({"kind":"control","refId":r,"bounds":w["bounds"]}))
                            .chain(
                                w["feedbackIds"].as_array().unwrap().iter().map(
                                    |r| json!({"kind":"feedback","refId":r,"bounds":w["bounds"]}),
                                ),
                            )
                    })
                    .collect();
            ensure(
                serde_json_canonicalizer::to_vec(&expected).unwrap()
                    == serde_json_canonicalizer::to_vec(&page["items"]).unwrap(),
                "Logical and presentation pages differ.",
            )?;
        }
        let targets = list(&doc["targets"], 1, 8)?;
        for t in targets {
            object(t, &["id", "label", "axes", "buttons", "hats"], &[])?;
            unique(&t["id"])?;
            text(&t["label"])?;
            for a in list(&t["axes"], 0, 16)? {
                ensure(
                    matches!(a.as_str(), Some("bipolar" | "unipolar")),
                    "Invalid target axis.",
                )?;
            }
            integer(&t["buttons"], 0, 128)?;
            integer(&t["hats"], 0, 4)?;
            ensure(
                !t["axes"].as_array().unwrap().is_empty()
                    || t["buttons"].as_u64().unwrap() > 0
                    || t["hats"].as_u64().unwrap() > 0,
                "Empty logical target.",
            )?;
        }
        let mut bound = BTreeSet::new();
        let mut slots = BTreeSet::new();
        let bindings = list(&doc["bindings"], controls.len(), controls.len())?;
        for b in bindings {
            object(b, &["controlId", "targetId", "kind", "index"], &[])?;
            let c = controls
                .iter()
                .find(|c| Some(c.id.as_str()) == b["controlId"].as_str())
                .ok_or("Unknown bound control.")?;
            let t = targets
                .iter()
                .find(|t| t["id"] == b["targetId"])
                .ok_or("Unknown bound target.")?;
            let i = integer(&b["index"], 0, 127)? as usize;
            ensure(
                bound.insert(&c.id) && slots.insert(format!("{}:{}:{i}", t["id"], b["kind"])),
                "Duplicate logical binding.",
            )?;
            ensure(
                match c.kind {
                    SourceKind::Button => {
                        b["kind"] == "button" && i < t["buttons"].as_u64().unwrap() as usize
                    }
                    SourceKind::Hat => {
                        b["kind"] == "hat" && i < t["hats"].as_u64().unwrap() as usize
                    }
                    SourceKind::Axis { axis_mode, .. } => {
                        b["kind"] == "axis" && t["axes"].get(i) == Some(&json!(axis_mode))
                    }
                },
                "Incompatible logical target binding.",
            )?;
        }
        let cap = json!({"uiId":ui_id,"controls":sorted(controls.iter().map(|c|match c.kind{SourceKind::Button=>json!({"id":c.id,"kind":"button"}),SourceKind::Hat=>json!({"id":c.id,"kind":"hat"}),SourceKind::Axis{axis_mode,fallback}=>json!({"id":c.id,"kind":"axis","axisMode":axis_mode,"fallback":fallback})}).collect(),"id"),"feedback":sorted(feedback.iter().map(|f|json!({"id":f["id"],"kind":f["kind"],"channelId":f["channelId"]})).collect(),"id"),"widgets":sorted(widget_caps,"id"),"targets":sorted(targets.iter().map(|t|json!({"id":t["id"],"axes":t["axes"],"buttons":t["buttons"],"hats":t["hats"]})).collect(),"id"),"bindings":sorted(bindings.clone(),"controlId")});
        let capability_fingerprint = format!(
            "sha256:{}",
            hash(&serde_json_canonicalizer::to_vec(&cap).map_err(|e| e.to_string())?)
        );
        if let Some(expected) = upload.capability_fingerprint {
            ensure(
                expected == capability_fingerprint,
                "Capability fingerprint mismatch.",
            )?;
        }
        Ok(Self {
            canonical_document: upload.canonical_document,
            publication_hash: upload.publication_hash,
            capability_fingerprint,
            assets: upload.assets,
            ui_id,
            title,
            controls,
            feedback: crate::feedback_descriptors(&doc)?,
        })
    }
}

fn validate_config(w: &Value, controls: &[SourceControl], assets: &[Value]) -> Result<(), String> {
    let c = &w["config"];
    let kind = w["kind"].as_str().unwrap();
    match kind {
        "button" | "encoder" => {
            object(
                c,
                if kind == "button" {
                    &["buttonMode", "pulseMs", "repeatMs"]
                } else {
                    &["pulseMs", "repeatMs"]
                },
                if kind == "button" {
                    &["iconAssetId"]
                } else {
                    &[]
                },
            )?;
            let pulse = integer(&c["pulseMs"], 0, 1000)?;
            let repeat = integer(&c["repeatMs"], 0, 5000)?;
            let mode = if kind == "encoder" {
                if repeat == 0 {
                    "pulse"
                } else {
                    "repeat"
                }
            } else {
                c["buttonMode"].as_str().unwrap_or("")
            };
            ensure(
                match mode {
                    "momentary" | "toggle" => pulse == 0 && repeat == 0,
                    "pulse" => pulse > 0 && repeat == 0,
                    "repeat" => pulse > 0 && repeat >= 20 && repeat > pulse,
                    _ => false,
                },
                "Invalid pulse/repeat configuration.",
            )?;
        }
        "selector" => {
            object(c, &["options"], &[])?;
            let control = controls
                .iter()
                .find(|control| Some(control.id.as_str()) == w["controlIds"][0].as_str())
                .unwrap();
            let mut vals = Vec::new();
            for o in list(&c["options"], 2, 16)? {
                object(o, &["label", "value"], &[])?;
                text(&o["label"])?;
                let value = number(&o["value"], -1., 1.)?;
                ensure(
                    control.accepts(&InputValue::Number(value)) && !vals.contains(&value),
                    "Invalid selector option.",
                )?;
                vals.push(value);
            }
            ensure(
                matches!(control.fallback(),InputValue::Number(n) if vals.contains(&n)),
                "Selector fallback is not an option.",
            )?;
        }
        "slider" | "xy" => {
            object(
                c,
                if kind == "slider" {
                    &["centering", "deadzone", "inverted", "orientation"]
                } else {
                    &["centering", "deadzone", "invertX", "invertY"]
                },
                &[],
            )?;
            number(&c["deadzone"], 0., 0.49)?;
            ensure(
                matches!(c["centering"].as_str(), Some("spring" | "latched")),
                "Invalid centering.",
            )?;
            if kind == "slider" {
                ensure(
                    c["inverted"].is_boolean()
                        && matches!(c["orientation"].as_str(), Some("horizontal" | "vertical")),
                    "Invalid slider configuration.",
                )?;
            } else {
                ensure(
                    c["invertX"].is_boolean() && c["invertY"].is_boolean(),
                    "Invalid XY configuration.",
                )?;
            }
        }
        "hat" | "indicator" => {
            object(c, &[], &[])?;
        }
        "text" => {
            object(c, &["prefix", "suffix"], &[])?;
            for k in ["prefix", "suffix"] {
                let s = c[k].as_str().ok_or("Invalid text format.")?;
                ensure(
                    s.encode_utf16().count() <= 120 && !s.chars().any(|c| c <= '\u{1f}'),
                    "Invalid text format.",
                )?;
            }
        }
        "numeric" | "gauge" => {
            object(c, &["min", "max", "unit", "precision"], &[])?;
            let min = number(&c["min"], -1e9, 1e9)?;
            let max = number(&c["max"], -1e9, 1e9)?;
            ensure(
                min < max
                    && c["unit"].as_str().is_some_and(|s| {
                        s.encode_utf16().count() <= 120 && !s.chars().any(|c| c <= '\u{1f}')
                    }),
                "Invalid gauge configuration.",
            )?;
            integer(&c["precision"], 0, 4)?;
        }
        "icon" => {
            object(c, &["symbol"], &["iconAssetId"])?;
            ensure(
                matches!(
                    c["symbol"].as_str(),
                    Some("power" | "warning" | "gear" | "check")
                ),
                "Invalid icon.",
            )?;
        }
        _ => return Err("Unsupported widget.".into()),
    }
    if let Some(asset) = c.get("iconAssetId") {
        ensure(
            assets.iter().any(|a| a["id"] == *asset),
            "Unknown icon asset.",
        )?;
    }
    Ok(())
}

fn validate_raster(bytes: &[u8], mime: &str) -> Result<u64, String> {
    let (w, h) = if mime == "image/png" {
        ensure(
            bytes.len() >= 33
                && bytes[..8] == [137, 80, 78, 71, 13, 10, 26, 10]
                && u32::from_be_bytes(bytes[8..12].try_into().unwrap()) == 13
                && &bytes[12..16] == b"IHDR",
            "Invalid PNG.",
        )?;
        let mut position = 8usize;
        let mut data = false;
        let mut ended = false;
        while position + 12 <= bytes.len() {
            let size =
                u32::from_be_bytes(bytes[position..position + 4].try_into().unwrap()) as usize;
            let end = position
                .checked_add(12)
                .and_then(|p| p.checked_add(size))
                .ok_or("Invalid PNG chunk.")?;
            ensure(end <= bytes.len(), "Truncated PNG chunk.")?;
            let kind = &bytes[position + 4..position + 8];
            ensure(
                !matches!(kind, b"acTL" | b"fcTL" | b"fdAT"),
                "Animated images are unsupported.",
            )?;
            if kind == b"IDAT" {
                data = true;
            }
            if kind == b"IEND" {
                ensure(size == 0 && end == bytes.len(), "Invalid PNG end.")?;
                ended = true;
                break;
            }
            position = end;
        }
        ensure(data && ended, "PNG image data/end is missing.")?;
        (
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
        )
    } else if mime == "image/webp" {
        ensure(
            bytes.len() >= 30
                && &bytes[..4] == b"RIFF"
                && &bytes[8..12] == b"WEBP"
                && u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8 == bytes.len(),
            "Invalid WebP.",
        )?;
        let mut position = 12usize;
        while position + 8 <= bytes.len() {
            let size =
                u32::from_le_bytes(bytes[position + 4..position + 8].try_into().unwrap()) as usize;
            let end = position
                .checked_add(8)
                .and_then(|p| p.checked_add(size))
                .and_then(|p| p.checked_add(size % 2))
                .ok_or("Invalid WebP chunk.")?;
            ensure(end <= bytes.len(), "Truncated WebP chunk.")?;
            ensure(
                !matches!(&bytes[position..position + 4], b"ANIM" | b"ANMF"),
                "Animated images are unsupported.",
            )?;
            position = end;
        }
        ensure(position == bytes.len(), "Truncated WebP chunk header.")?;
        match &bytes[12..16] {
            b"VP8X" => {
                ensure(bytes[20] & 2 == 0, "Animated images are unsupported.")?;
                (
                    1 + bytes[24] as u32 + ((bytes[25] as u32) << 8) + ((bytes[26] as u32) << 16),
                    1 + bytes[27] as u32 + ((bytes[28] as u32) << 8) + ((bytes[29] as u32) << 16),
                )
            }
            b"VP8L" => {
                ensure(bytes[20] == 0x2f, "Invalid WebP.")?;
                (
                    1 + bytes[21] as u32 + (((bytes[22] & 63) as u32) << 8),
                    1 + (bytes[22] >> 6) as u32
                        + ((bytes[23] as u32) << 2)
                        + (((bytes[24] & 15) as u32) << 10),
                )
            }
            b"VP8 " => {
                ensure(bytes[23..26] == [0x9d, 1, 0x2a], "Invalid WebP.")?;
                (
                    (u16::from_le_bytes(bytes[26..28].try_into().unwrap()) & 0x3fff) as u32,
                    (u16::from_le_bytes(bytes[28..30].try_into().unwrap()) & 0x3fff) as u32,
                )
            }
            _ => return Err("Unsupported WebP.".into()),
        }
    } else {
        ensure(
            bytes.len() >= 12 && bytes[..2] == [255, 216] && bytes[bytes.len() - 2..] == [255, 217],
            "Invalid JPEG.",
        )?;
        let mut p = 2;
        let mut size = (0, 0);
        while p + 4 < bytes.len() {
            ensure(bytes[p] == 255, "Invalid JPEG marker.")?;
            let marker = bytes[p + 1];
            p += 2;
            if marker == 0xda || marker == 0xd9 {
                break;
            }
            let length = u16::from_be_bytes(bytes[p..p + 2].try_into().unwrap()) as usize;
            ensure(
                length >= 2 && p + length <= bytes.len(),
                "Invalid JPEG segment.",
            )?;
            if matches!(marker, 0xc0 | 0xc1 | 0xc2) {
                ensure(length >= 8, "Invalid JPEG dimensions.")?;
                size = (
                    u16::from_be_bytes(bytes[p + 5..p + 7].try_into().unwrap()) as u32,
                    u16::from_be_bytes(bytes[p + 3..p + 5].try_into().unwrap()) as u32,
                );
                break;
            }
            p += length;
        }
        size
    };
    ensure(
        w > 0 && h > 0 && w <= 8192 && h <= 8192 && (w as u64) * (h as u64) <= 16_777_216,
        "Decoded asset dimensions exceed budget.",
    )?;
    Ok((w as u64) * (h as u64))
}

#[cfg(test)]
mod viewport_tests {
    use super::*;

    fn document() -> Value {
        let left = json!({"x":0.1,"y":0.1,"width":0.2,"height":0.2});
        let right = json!({"x":0.6,"y":0.1,"width":0.2,"height":0.2});
        json!({
            "schemaVersion":2,
            "ui":{
                "schemaVersion":1,"id":"panel","title":"Panel",
                "controls":[
                    {"id":"input-left","label":"Left","kind":"button"},
                    {"id":"input-right","label":"Right","kind":"button"}
                ],
                "pages":[{"id":"page","title":"Page","items":[
                    {"kind":"control","refId":"input-left","bounds":left},
                    {"kind":"control","refId":"input-right","bounds":right}
                ]}],
                "feedbackWidgets":[],"assets":[]
            },
            "presentation":{
                "widgets":[
                    {"id":"widget-left","pageId":"page","kind":"button","label":"Left",
                     "bounds":left,"controlIds":["input-left"],"feedbackIds":[],
                     "config":{"buttonMode":"momentary","pulseMs":0,"repeatMs":0}},
                    {"id":"widget-right","pageId":"page","kind":"button","label":"Right",
                     "bounds":right,"controlIds":["input-right"],"feedbackIds":[],
                     "config":{"buttonMode":"momentary","pulseMs":0,"repeatMs":0}}
                ],
                "groups":[],"viewports":[{"pageId":"page","width":1920,"height":1080}],
                "grid":{"columns":24,"rows":16,"snap":true}
            },
            "targets":[{"id":"target","label":"Controller","axes":[],"buttons":2,"hats":0}],
            "bindings":[
                {"controlId":"input-left","targetId":"target","kind":"button","index":0},
                {"controlId":"input-right","targetId":"target","kind":"button","index":1}
            ]
        })
    }

    fn region(id: &str, x: f64, y: f64, width: f64, height: f64) -> Value {
        json!({"id":id,"label":id,"bounds":{"x":x,"y":y,"width":width,"height":height}})
    }

    fn adjacent_regions() -> Value {
        json!([
            region("viewport-left", 0., 0., 0.5, 1.),
            region("viewport-right", 0.5, 0., 0.5, 1.)
        ])
    }

    fn validate(document: Value) -> Result<Publication, String> {
        let canonical_document = serde_json_canonicalizer::to_string(&document).unwrap();
        let publication_hash = format!("sha256:{}", hash(canonical_document.as_bytes()));
        Publication::validate(PublicationUpload {
            canonical_document,
            publication_hash,
            capability_fingerprint: None,
            assets: vec![],
        })
    }

    #[test]
    fn feedback_only_panel_is_a_valid_publication() {
        let mut doc = document();
        let bounds = json!({"x":0.1,"y":0.1,"width":0.3,"height":0.2});
        doc["ui"]["controls"] = json!([]);
        doc["bindings"] = json!([]);
        doc["ui"]["feedbackWidgets"] = json!([{"id":"output","kind":"text","label":"Text","channelId":"channel"}]);
        doc["ui"]["pages"][0]["items"] = json!([{"kind":"feedback","refId":"output","bounds":bounds}]);
        doc["presentation"]["widgets"] = json!([{"id":"text-widget","pageId":"page","kind":"text","label":"Text","bounds":bounds,"controlIds":[],"feedbackIds":["output"],"config":{"prefix":"","suffix":""}}]);
        assert!(validate(doc).is_ok());
    }

    #[test]
    fn legacy_screen_and_adjacent_viewports_preserve_capabilities() {
        let mut document = document();
        let legacy = validate(document.clone()).unwrap();
        document["presentation"]["viewports"][0]["regions"] = adjacent_regions();
        let multiple = validate(document).unwrap();
        assert_ne!(legacy.publication_hash, multiple.publication_hash);
        assert_eq!(
            legacy.capability_fingerprint,
            multiple.capability_fingerprint
        );
    }

    #[test]
    fn viewport_overlap_is_rejected_including_nested_areas() {
        for right in [
            region("viewport-right", 0.4, 0., 0.6, 1.),
            region("viewport-right", 0.1, 0.1, 0.2, 0.2),
        ] {
            let mut document = document();
            document["presentation"]["viewports"][0]["regions"] =
                json!([region("viewport-left", 0., 0., 0.5, 1.), right]);
            assert_eq!(
                validate(document).unwrap_err(),
                "Companion viewports must not overlap."
            );
        }
    }

    #[test]
    fn viewport_overlap_tolerance_matches_intersection_depth() {
        let outer = bounds(
            &json!({"x":0.,"y":0.,"width":0.5,"height":1.}),
            "Invalid test bounds.",
        )
        .unwrap();
        for (width, overlaps) in [(1e-10, false), (1e-8, true)] {
            let inner = bounds(
                &json!({"x":0.25,"y":0.,"width":width,"height":1.}),
                "Invalid test bounds.",
            )
            .unwrap();
            assert_eq!(outer.overlaps(inner), overlaps);
            assert_eq!(inner.overlaps(outer), overlaps);
        }
    }

    #[test]
    fn widgets_must_fit_one_viewport_including_at_shared_edges() {
        let mut document = document();
        document["presentation"]["viewports"][0]["regions"] = adjacent_regions();
        let edge = json!({"x":0.,"y":0.,"width":0.5,"height":1.});
        document["presentation"]["widgets"][0]["bounds"] = edge.clone();
        document["ui"]["pages"][0]["items"][0]["bounds"] = edge;
        validate(document.clone()).unwrap();
        // Crossing adjacent areas is invalid even when their union covers the widget.
        document["presentation"]["widgets"][0]["bounds"]["x"] = json!(0.1);
        assert_eq!(
            validate(document).unwrap_err(),
            "Widget must be entirely inside one companion viewport."
        );

        let mut document = self::document();
        document["presentation"]["viewports"][0]["regions"] = json!([
            region("viewport-left", 0., 0., 0.4, 1.),
            region("viewport-right", 0.8, 0., 0.2, 1.)
        ]);
        assert_eq!(
            validate(document).unwrap_err(),
            "Widget must be entirely inside one companion viewport."
        );
    }

    #[test]
    fn viewport_bounds_and_region_budget_are_validated() {
        for regions in [
            json!([]),
            json!((0..33)
                .map(|i| region(&format!("viewport-{i}"), 0., 0., 1., 1.))
                .collect::<Vec<_>>()),
            json!([region("viewport", 0.75, 0., 0.5, 1.)]),
            json!([region("viewport", 0., 0.75, 1., 0.5)]),
            json!([region("viewport", -0.1, 0., 1., 1.)]),
            json!([region("viewport", 0., 0., 0., 1.)]),
            json!([region("viewport", 0., 0., 1., 0.)]),
        ] {
            let mut document = document();
            document["presentation"]["viewports"][0]["regions"] = regions;
            assert!(validate(document).is_err());
        }
    }

    #[test]
    fn viewport_ids_are_document_unique_and_labels_are_validated() {
        for (id, label) in [
            ("input-left", "Viewport"),
            ("bad id", "Viewport"),
            ("viewport", " "),
        ] {
            let mut document = document();
            document["presentation"]["viewports"][0]["regions"] = json!([
                {"id":id,"label":label,"bounds":{"x":0.,"y":0.,"width":1.,"height":1.}}
            ]);
            assert!(validate(document).is_err());
        }
    }

    #[test]
    fn viewport_geometry_is_per_page_but_region_ids_are_document_unique() {
        let mut document = document();
        document["presentation"]["viewports"][0]["regions"] =
            json!([region("viewport-first", 0., 0., 1., 1.)]);
        document["ui"]["pages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"second-page","title":"Second page","items":[]}));
        document["presentation"]["viewports"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "pageId":"second-page","width":1920,"height":1080,
                "regions":[region("viewport-second", 0., 0., 1., 1.)]
            }));
        validate(document.clone()).unwrap();
        document["presentation"]["viewports"][1]["regions"][0]["id"] = json!("viewport-first");
        assert_eq!(validate(document).unwrap_err(), "Duplicate publication ID.");
    }
}
