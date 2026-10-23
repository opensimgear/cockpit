use super::*;
fn fixture() -> PublicationUpload {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/compiled-toggle-publication.json"
    ))
    .unwrap()
}
fn document(upload: &PublicationUpload) -> Value {
    serde_json::from_str(&upload.canonical_document).unwrap()
}
fn rehash(mut upload: PublicationUpload, doc: Value) -> PublicationUpload {
    upload.canonical_document = String::from_utf8(canonical(&doc).unwrap()).unwrap();
    upload.publication_hash = format!("sha256:{}", hash(upload.canonical_document.as_bytes()));
    upload.capability_fingerprint = None;
    upload
}
fn resize_manifest(artifact: &mut Value) {
    for _ in 0..20 {
        let size = canonical(artifact).unwrap().len() as u64;
        let r = &mut artifact["resources"];
        r["manifestBytes"] = json!(size);
        let resident = r["transferBytes"].as_u64().unwrap() * 3
            + r["dictionaryBytes"].as_u64().unwrap()
            + size * 4
            + r["frameMapBytes"].as_u64().unwrap() * 2
            + r["audioWorkingBytes"].as_u64().unwrap()
            + artifact["width"].as_u64().unwrap() * artifact["height"].as_u64().unwrap() * 8
            + 768;
        artifact["resources"]["residentBytes"] = json!(resident);
        artifact["resources"]["activeCandidateBytes"] = json!(resident * 2);
        if canonical(artifact).unwrap().len() as u64 == size {
            return;
        }
    }
    panic!("Manifest fixed point did not converge");
}
fn mutate_artifact(mutator: impl FnOnce(&mut Value)) -> PublicationUpload {
    let upload = fixture();
    let mut doc = document(&upload);
    let old = doc["presentation"]["artifacts"][0]["hash"].clone();
    let artifact = &mut doc["presentation"]["artifacts"][0]["artifact"];
    mutator(artifact);
    artifact["behaviorHash"] = json!(hash(&canonical(&artifact["behavior"]).unwrap()));
    resize_manifest(artifact);
    let new = hash(&canonical(artifact).unwrap());
    doc["presentation"]["artifacts"][0]["hash"] = json!(new);
    for widget in doc["presentation"]["widgets"].as_array_mut().unwrap() {
        if widget["artifactHash"] == old {
            widget["artifactHash"] = json!(new);
        }
    }
    rehash(upload, doc)
}

#[test]
fn compiled_publication_admits_actual_typescript_compiler_fixture_and_semantic_identity() {
    let upload = fixture();
    let expected = upload.capability_fingerprint.clone().unwrap();
    let publication = Publication::validate(upload).unwrap();
    assert_eq!(publication.capability_fingerprint, expected);
    assert_eq!(publication.controls.len(), 1);
    assert_eq!(publication.feedback.len(), 1);
    assert_eq!(publication.feedback[0].kind, crate::FeedbackKind::Boolean);
    let mut changed = document(&fixture());
    changed["ui"]["title"] = json!("Renamed panel");
    changed["presentation"]["widgets"][0]["label"] = json!("Renamed Toggle");
    let same = Publication::validate(rehash(fixture(), changed)).unwrap();
    assert_eq!(same.capability_fingerprint, expected);
    let cue = Publication::validate(mutate_artifact(|a| {
        a["behavior"]["regions"][0]["transitions"][0]["actions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"cue","cue":"beep"}))
    }))
    .unwrap();
    assert_eq!(cue.capability_fingerprint, expected);
    let changed = Publication::validate(mutate_artifact(|a| {
        a["behavior"]["signals"][0]["initial"] = json!(true)
    }));
    assert!(changed.is_err());
}

#[test]
fn compiled_publication_rejects_hashes_offsets_pixel_payloads_missing_selectors_and_resource_lies()
{
    let mutations: Vec<Box<dyn Fn(&mut Value)>> = vec![
        Box::new(|a| a["tiles"][0]["offset"] = json!(1)),
        Box::new(|a| a["tiles"][0]["sha256"] = json!("0".repeat(64))),
        Box::new(|a| a["frames"][0]["tiles"][0] = json!(32768)),
        Box::new(|a| {
            a["frames"].as_array_mut().unwrap().remove(0);
        }),
        Box::new(|a| a["pixelFormat"] = json!("svg")),
        Box::new(|a| a["layers"] = json!([])),
        Box::new(|a| a["resources"]["dictionaryBytes"] = json!(1)),
        Box::new(|a| a["domains"][0]["values"] = json!([])),
        Box::new(|a| a["domains"][0]["values"] = json!([false])),
        Box::new(|a| {
            a["behavior"]["signals"].as_array_mut().unwrap().push(json!({
                "id":"mode","source":"local","type":"enum","values":["idle","run"],"initial":"idle"
            }));
            a["domains"].as_array_mut().unwrap().push(json!({
                "signal":"mode","values":["idle"],"numeric":false,"maxErrorPixels":0,"smoothingMs":0
            }));
        }),
        Box::new(|a| {
            a["behavior"]["signals"].as_array_mut().unwrap().push(json!({
                "id":"rpm","source":"local","type":"number","min":0,"max":8000,"initial":0
            }));
            a["domains"].as_array_mut().unwrap().push(json!({
                "signal":"rpm","values":[1,8000],"numeric":true,"maxErrorPixels":1,"smoothingMs":0
            }));
        }),
        Box::new(|a| a["domains"][0]["smoothingMs"] = json!(60001)),
        Box::new(|a| a["hitRegions"][0]["events"]["activate"] = json!("absent")),
        Box::new(|a| {
            a["behavior"]["regions"][0]["transitions"][0]["actions"][0]["signal"] =
                json!("confirmed")
        }),
        Box::new(|a| a["behavior"]["signals"][1]["freshnessMs"] = json!(0)),
    ];
    for (index, mutation) in mutations.into_iter().enumerate() {
        assert!(
            Publication::validate(mutate_artifact(mutation)).is_err(),
            "mutation {index}"
        );
    }
    let mut upload = fixture();
    let mut pixels = STANDARD.decode(&upload.assets[0].base64).unwrap();
    pixels[0] ^= 1;
    upload.assets[0].base64 = STANDARD.encode(pixels);
    assert!(Publication::validate(upload).is_err());
    let mut upload = fixture();
    upload.assets.clear();
    assert!(Publication::validate(upload).is_err());
    let mut upload = fixture();
    upload.assets.push(upload.assets[0].clone());
    assert!(Publication::validate(upload).is_err());
    let mut upload = fixture();
    upload.capability_fingerprint = Some(format!("sha256:{}", "0".repeat(64)));
    assert!(Publication::validate(upload).is_err());
    let mut doc = document(&fixture());
    doc["presentation"]["artifacts"][0]["hash"] = json!("0".repeat(64));
    assert!(Publication::validate(rehash(fixture(), doc)).is_err());
}

#[test]
fn compiled_panel_rejects_missing_duplicate_or_incompatible_semantic_endpoint_bindings() {
    let mutations: Vec<Box<dyn Fn(&mut Value)>> = vec![
        Box::new(|d| d["ui"]["controls"][0]["kind"] = json!("hat")),
        Box::new(|d| d["presentation"]["widgets"][0]["bounds"]["width"] = json!(0.1)),
        Box::new(|d| d["presentation"]["widgets"][0]["inputBindings"] = json!([])),
        Box::new(|d| {
            d["presentation"]["widgets"][0]["outputBindings"][0]["portId"] = json!("absent")
        }),
        Box::new(|d| d["ui"]["feedbackWidgets"][0]["kind"] = json!("text")),
        Box::new(|d| d["bindings"][0]["index"] = json!(1)),
        Box::new(|d| d["targets"][0]["hats"] = json!(1)),
        Box::new(|d| d["ui"]["pages"][0]["items"] = json!([])),
        Box::new(|d| {
            let widget = d["presentation"]["widgets"][0].clone();
            d["presentation"]["widgets"]
                .as_array_mut()
                .unwrap()
                .push(widget);
        }),
    ];
    for (index, mutation) in mutations.into_iter().enumerate() {
        let mut doc = document(&fixture());
        mutation(&mut doc);
        assert!(
            Publication::validate(rehash(fixture(), doc)).is_err(),
            "mutation {index}"
        );
    }
}

#[test]
fn compiled_capability_and_cached_asset_claims_are_bounded_and_never_change_authority() {
    let p = Publication::validate(fixture()).unwrap();
    let caps = artifact::default_capabilities();
    assert!(check_capabilities(&p, &caps).is_ok());
    let mut tiny = caps.clone();
    tiny["maxResidentBytes"] = json!(768);
    assert!(check_capabilities(&p, &tiny).is_err());
    let mut old = caps.clone();
    old["version"] = json!(2);
    assert!(validate_capabilities(&old).is_err());
    let mut extra = caps.clone();
    extra["renderer"] = json!("layers");
    assert!(validate_capabilities(&extra).is_err());
    let doc = document(&fixture());
    let a = &doc["presentation"]["artifacts"][0]["artifact"];
    let mut artifact_only = caps.clone();
    artifact_only["maxResidentBytes"] = a["resources"]["activeCandidateBytes"].clone();
    // A single widget can fit while the enclosing canonical panel and retained
    // candidate cannot. The panel must independently account for its document.
    assert!(check_capabilities(&p, &artifact_only).unwrap_err().contains("panel exceeds"));
    let sha = doc["ui"]["assets"][0]["sha256"].clone();
    assert_eq!(cached_payloads(&p, &json!([sha])).unwrap().len(), 0);
    assert_eq!(
        cached_payloads(&p, &json!(["0".repeat(64)])).unwrap().len(),
        p.assets.len()
    );
    assert!(cached_payloads(&p, &json!([sha, sha])).is_err());
    assert!(cached_payloads(&p, &json!(["not-a-hash"])).is_err());
    assert!(cached_payloads(&p, &json!({})).is_err());
    assert_eq!(p.controls.len(), 1);
}

fn pcm_wave() -> Vec<u8> {
    let mut bytes = vec![0u8; 204];
    bytes[..4].copy_from_slice(b"RIFF");
    bytes[4..8].copy_from_slice(&196u32.to_le_bytes());
    bytes[8..16].copy_from_slice(b"WAVEfmt ");
    bytes[16..20].copy_from_slice(&16u32.to_le_bytes());
    bytes[20..22].copy_from_slice(&1u16.to_le_bytes());
    bytes[22..24].copy_from_slice(&1u16.to_le_bytes());
    bytes[24..28].copy_from_slice(&8000u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&16000u32.to_le_bytes());
    bytes[32..34].copy_from_slice(&2u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&16u16.to_le_bytes());
    bytes[36..40].copy_from_slice(b"data");
    bytes[40..44].copy_from_slice(&160u32.to_le_bytes());
    bytes
}
#[test]
fn compiled_media_validation_accepts_baked_motion_and_pcm_then_rejects_unsupported_or_corrupt_media(
) {
    let upload = fixture();
    let doc = document(&upload);
    let mut a = doc["presentation"]["artifacts"][0]["artifact"].clone();
    let mut bytes = upload
        .assets
        .iter()
        .map(|p| (p.asset_id.clone(), STANDARD.decode(&p.base64).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let from = a["frames"][0]["id"].clone();
    let to = a["frames"][1]["id"].clone();
    a["motions"] = json!([{"id":"fade","fromFrameId":from,"toFrameId":to,"frames":[from,to],"durationMs":100,"interruption":"reverse"}]);
    let wave = pcm_wave();
    let sha = hash(&wave);
    let id = format!("audio-{}", &sha[..32]);
    a["assets"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":id,"sha256":sha,"sizeBytes":wave.len(),"mime":"audio/wav"}));
    bytes.insert(id.clone(), wave.clone());
    a["sounds"] = json!([{"id":"click","cue":"click","assetId":id,"cooldownMs":100,"stopOnExit":true,"volume":0.5}]);
    a["resources"]["mediaBytes"] = json!(wave.len());
    a["resources"]["audioWorkingBytes"] = json!(wave.len() * 16);
    a["resources"]["transferBytes"] =
        json!(a["resources"]["transferBytes"].as_u64().unwrap() + wave.len() as u64);
    resize_manifest(&mut a);
    let caps = artifact::default_capabilities();
    artifact::validate(&a, &bytes, &caps).unwrap();
    let mut understated_audio = a.clone();
    understated_audio["resources"]["audioWorkingBytes"] = json!(0);
    resize_manifest(&mut understated_audio);
    assert!(artifact::validate(&understated_audio, &bytes, &caps)
        .unwrap_err().contains("resource report"));
    let mut silent = caps.clone();
    silent["audio"] = json!(false);
    assert!(artifact::validate(&a, &bytes, &silent).is_err());
    let mut static_only = caps.clone();
    static_only["motion"] = json!(false);
    assert!(artifact::validate(&a, &bytes, &static_only).is_err());
    let mut smoothed = a.clone();
    smoothed["motions"] = json!([]);
    smoothed["domains"][0]["smoothingMs"] = json!(120);
    resize_manifest(&mut smoothed);
    assert!(artifact::validate(&smoothed, &bytes, &static_only)
        .unwrap_err().contains("smoothing"));
    for offset in [0usize, 8, 20, 22, 24, 32, 34, 36, 40] {
        let mut invalid = wave.clone();
        invalid[offset] ^= 255;
        assert!(validate_wave(&invalid).is_err(), "offset {offset}");
    }
    let mut corrupt = a.clone();
    corrupt["motions"][0]["frames"] = json!([to, from]);
    resize_manifest(&mut corrupt);
    assert!(artifact::validate(&corrupt, &bytes, &caps).is_err());
    let mut duplicate = a.clone();
    let motion = duplicate["motions"][0].clone();
    duplicate["motions"].as_array_mut().unwrap().push(motion);
    resize_manifest(&mut duplicate);
    assert!(artifact::validate(&duplicate, &bytes, &caps).is_err());
    let mut invalid = a.clone();
    invalid["sounds"][0]["volume"] = json!(2);
    resize_manifest(&mut invalid);
    assert!(artifact::validate(&invalid, &bytes, &caps).is_err());
}
