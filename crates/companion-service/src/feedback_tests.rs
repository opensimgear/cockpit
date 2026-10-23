use super::*;
use crate::{Publication, PublicationUpload};
use serde_json::json;
fn publication() -> Publication {
    Publication::validate_legacy_test_fixture(
        serde_json::from_str::<PublicationUpload>(include_str!(
            "../tests/fixtures/c1-flight-publication.json"
        ))
        .unwrap(),
    )
    .unwrap()
}
fn identity() -> Value {
    json!({"clientId":"client","sessionId":"session","generation":"1","controllerInstanceId":"instance","publicationHash":format!("sha256:{}","a".repeat(64))})
}
fn sample(d: &FeedbackDescriptor) -> FeedbackSample {
    FeedbackSample {
        channel_id: d.channel_id.clone(),
        output_id: d.output_ids[0].clone(),
        value: Some(match d.kind {
            FeedbackKind::Boolean => FeedbackValue::Boolean(true),
            FeedbackKind::Number => FeedbackValue::Number(42.),
            FeedbackKind::Text => FeedbackValue::Text(String::new()),
        }),
        status: FeedbackStatus::Fresh,
        provenance: FeedbackProvenance::GameConfirmed,
        source_kind: FeedbackSource::Simulator,
        source_age_ms: 0,
        unit: d.unit.clone().unwrap_or_else(|| "state".into()),
        range: d.range.clone(),
    }
}
fn bindings(schema: &[FeedbackDescriptor]) -> Vec<FeedbackBinding> {
    schema
        .iter()
        .map(|d| FeedbackBinding {
            channel_id: d.channel_id.clone(),
            output_id: d.output_ids[0].clone(),
        })
        .collect()
}
#[test]
fn descriptors_merge_only_compatible_published_outputs_and_preserve_unit_range() {
    let p = publication();
    let mut d: Value = serde_json::from_str(&p.canonical_document).unwrap();
    assert_eq!(p.feedback.len(), 5);
    assert_eq!(
        p.feedback
            .iter()
            .find(|d| d.kind == FeedbackKind::Number)
            .unwrap()
            .unit
            .as_deref(),
        Some("%")
    );
    d["ui"]["feedbackWidgets"][3]["channelId"] = d["ui"]["feedbackWidgets"][0]["channelId"].clone();
    let schema = feedback_descriptors(&d).unwrap();
    assert_eq!(schema.len(), 4);
    assert_eq!(
        schema
            .iter()
            .find(|d| d.kind == FeedbackKind::Boolean && d.output_ids.len() == 2)
            .unwrap()
            .output_ids
            .len(),
        2
    );
    d["ui"]["feedbackWidgets"][1]["channelId"] = d["ui"]["feedbackWidgets"][0]["channelId"].clone();
    assert!(feedback_descriptors(&d).is_err());
    let mut d: Value = serde_json::from_str(&p.canonical_document).unwrap();
    d["ui"]["feedbackWidgets"][4]["channelId"] = d["ui"]["feedbackWidgets"][2]["channelId"].clone();
    assert!(feedback_descriptors(&d).is_ok());
    d["presentation"]["widgets"][11]["config"]["min"] = json!(-1);
    assert!(feedback_descriptors(&d).is_err());
    for fault in 0..6 {
        let mut d: Value = serde_json::from_str(&p.canonical_document).unwrap();
        match fault {
            0 => d["ui"]["feedbackWidgets"] = Value::Null,
            1 => d["presentation"]["widgets"] = Value::Null,
            2 => d["ui"]["feedbackWidgets"][0]["channelId"] = json!("bad/id"),
            3 => d["ui"]["feedbackWidgets"][0]["kind"] = json!("unknown"),
            4 => d["presentation"]["widgets"][9]["config"]["max"] = json!(-1),
            _ => d["ui"]["feedbackWidgets"][0]["id"] = json!("missing"),
        };
        assert!(feedback_descriptors(&d).is_err());
    }
}
#[test]
fn published_unit_limits_match_c1_utf16_and_always_allow_a_valid_sample() {
    let p = publication();
    for unit in [
        "x".repeat(120),
        "😀".repeat(60),
        String::new(),
        "   ".into(),
    ] {
        let mut d: Value = serde_json::from_str(&p.canonical_document).unwrap();
        d["presentation"]["widgets"][9]["config"]["unit"] = json!(unit);
        let canonical = serde_json_canonicalizer::to_string(&d).unwrap();
        let published = Publication::validate_legacy_test_fixture(PublicationUpload {
            publication_hash: format!("sha256:{}", crate::publication::hash(canonical.as_bytes())),
            canonical_document: canonical,
            capability_fingerprint: None,
            assets: vec![],
        })
        .unwrap();
        let descriptor = published
            .feedback
            .iter()
            .find(|f| f.output_ids.contains(&"flight-output-2".to_string()))
            .unwrap();
        assert_eq!(
            descriptor.unit.as_deref(),
            Some(if unit.trim().is_empty() {
                "ratio"
            } else {
                &unit
            })
        );
        validate_feedback_samples(
            &published.feedback,
            &bindings(&published.feedback),
            &[sample(descriptor)],
        )
        .unwrap();
    }
    for unit in ["x".repeat(121), "😀".repeat(61), "unsafe\nunit".into()] {
        let mut d: Value = serde_json::from_str(&p.canonical_document).unwrap();
        d["presentation"]["widgets"][9]["config"]["unit"] = json!(unit);
        assert!(feedback_descriptors(&d).is_err());
        let canonical = serde_json_canonicalizer::to_string(&d).unwrap();
        assert!(Publication::validate_legacy_test_fixture(PublicationUpload {
            publication_hash: format!("sha256:{}", crate::publication::hash(canonical.as_bytes())),
            canonical_document: canonical,
            capability_fingerprint: None,
            assets: vec![],
        })
        .is_err());
    }
}
#[test]
fn published_types_units_ranges_outputs_and_provenance_are_authoritative() {
    let schema = publication().feedback;
    let bindings = bindings(&schema);
    for d in &schema {
        let valid = sample(d);
        validate_feedback_samples(&schema, &bindings, &[valid.clone()]).unwrap();
        for fault in 0..11 {
            let mut bad = valid.clone();
            match fault {
                0 => bad.output_id = "unknown-output".into(),
                1 => bad.channel_id = "unknown-channel".into(),
                2 => bad.unit = "\n".into(),
                3 => bad.unit = "x".repeat(121),
                4 => bad.source_age_ms = 86_400_001,
                5 => bad.source_kind = FeedbackSource::Host,
                6 => bad.value = None,
                7 => bad.status = FeedbackStatus::Unknown,
                8 => bad.status = FeedbackStatus::Disconnected,
                9 => {
                    bad.value = Some(if d.kind == FeedbackKind::Text {
                        FeedbackValue::Boolean(false)
                    } else {
                        FeedbackValue::Text("wrong".into())
                    })
                }
                _ => bad.range = Some(FeedbackRange { min: -1., max: 1. }),
            };
            assert!(
                validate_feedback_samples(&schema, &bindings, &[bad]).is_err(),
                "fault{fault}"
            );
        }
        let mut unknown = valid.clone();
        unknown.status = FeedbackStatus::Unknown;
        unknown.value = None;
        validate_feedback_samples(&schema, &bindings, &[unknown.clone()]).unwrap();
        unknown.status = FeedbackStatus::Disconnected;
        validate_feedback_samples(&schema, &bindings, &[unknown.clone()]).unwrap();
        unknown.status = FeedbackStatus::Stale;
        validate_feedback_samples(&schema, &bindings, &[unknown]).unwrap();
        assert!(validate_feedback_samples(&schema, &bindings, &[valid.clone(), valid]).is_err());
    }
    let number = schema
        .iter()
        .find(|d| d.kind == FeedbackKind::Number)
        .unwrap();
    for value in [f64::NAN, f64::INFINITY, 101., -1.] {
        let mut bad = sample(number);
        bad.value = Some(FeedbackValue::Number(value));
        assert!(validate_feedback_samples(&schema, &bindings, &[bad]).is_err());
    }
    let mut bad = sample(number);
    bad.unit = "percent".into();
    assert!(validate_feedback_samples(&schema, &bindings, &[bad]).is_err());
    let text = schema
        .iter()
        .find(|d| d.kind == FeedbackKind::Text)
        .unwrap();
    let mut bad = sample(text);
    bad.value = Some(FeedbackValue::Text("x".repeat(257)));
    assert!(validate_feedback_samples(&schema, &bindings, &[bad]).is_err());
}
#[test]
fn snapshots_deltas_age_each_source_independently_and_never_refresh_omitted_data() {
    let schema = publication().feedback;
    let bindings = bindings(&schema);
    let mut book = FeedbackBook::default();
    book.configure(schema.clone(), &bindings, 0).unwrap();
    let mut cursor = FeedbackCursor::default();
    let mut client = FeedbackAccumulator::default();
    let identity = identity();
    let initial = book.deliver(&mut cursor, &identity, 0);
    assert_eq!(initial["mode"], "snapshot");
    assert_eq!(
        client
            .apply(&initial, &identity, &schema, 0)
            .unwrap()
            .unwrap()
            .len(),
        5
    );
    let samples = schema.iter().map(sample).collect::<Vec<_>>();
    book.publish(&samples, FeedbackMode::Snapshot, 0).unwrap();
    let delta = book.deliver(&mut cursor, &identity, 100);
    assert_eq!(delta["mode"], "delta");
    let values = client
        .apply(&delta, &identity, &schema, 100)
        .unwrap()
        .unwrap();
    assert!(values.iter().all(|s| s.source_age_ms == 100));
    book.publish(&samples[..1], FeedbackMode::Delta, 500)
        .unwrap();
    let delta = book.deliver(&mut cursor, &identity, 500);
    assert_eq!(delta["values"].as_array().unwrap().len(), 1);
    let values = client
        .apply(&delta, &identity, &schema, 500)
        .unwrap()
        .unwrap();
    assert_eq!(
        values
            .iter()
            .find(|s| s.channel_id == samples[0].channel_id)
            .unwrap()
            .source_age_ms,
        0
    );
    assert!(values
        .iter()
        .filter(|s| s.channel_id != samples[0].channel_id)
        .all(|s| s.source_age_ms == 500));
    let duplicate = book.deliver(&mut cursor, &identity, 750);
    assert!(client
        .apply(&duplicate, &identity, &schema, 750)
        .unwrap()
        .is_none());
    book.publish(&[], FeedbackMode::Delta, 1100).unwrap();
    let values = client
        .apply(
            &book.deliver(&mut cursor, &identity, 1100),
            &identity,
            &schema,
            1100,
        )
        .unwrap()
        .unwrap();
    assert!(values
        .iter()
        .filter(|s| s.channel_id != samples[0].channel_id)
        .all(|s| s.status == FeedbackStatus::Stale));
    book.publish(&samples[..1], FeedbackMode::Snapshot, 1200)
        .unwrap();
    let values = client
        .apply(
            &book.deliver(&mut cursor, &identity, 1200),
            &identity,
            &schema,
            1200,
        )
        .unwrap()
        .unwrap();
    assert!(values
        .iter()
        .filter(|s| s.channel_id != samples[0].channel_id)
        .all(|s| s.status == FeedbackStatus::Unknown && s.value.is_none()));
}
#[test]
fn reset_disconnect_rate_limits_and_exhaustion_are_bounded_and_atomic() {
    let schema = publication().feedback;
    let bindings = bindings(&schema);
    let mut book = FeedbackBook::default();
    book.configure(schema.clone(), &bindings, 0).unwrap();
    let generation = book.generation;
    book.configure(schema.clone(), &bindings, 1).unwrap();
    assert_eq!(book.generation, generation);
    book.publish(&[sample(&schema[0])], FeedbackMode::Delta, 10)
        .unwrap();
    let sequence = book.sequence;
    assert!(book
        .publish(&[sample(&schema[0])], FeedbackMode::Delta, 43)
        .is_err());
    assert_eq!(book.sequence, sequence);
    book.publish(&[sample(&schema[0])], FeedbackMode::Delta, 44)
        .unwrap();
    book.reset(45).unwrap();
    assert!(book.generation > generation);
    let mut cursor = FeedbackCursor::default();
    assert!(book.deliver(&mut cursor, &identity(), 45)["values"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["status"] == "unknown" && s.get("value").is_none()));
    book.disconnect(46);
    assert!(book.deliver(&mut cursor, &identity(), 46)["values"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["status"] == "disconnected"));
    let mut invalid = bindings.clone();
    invalid[0].output_id = "other-output".into();
    assert!(book.configure(schema.clone(), &invalid, 47).is_err());
    invalid = bindings.clone();
    invalid.push(invalid[0].clone());
    assert!(book.configure(schema.clone(), &invalid, 47).is_err());
    let mut sample = sample(&schema[0]);
    sample.source_age_ms = 999;
    assert_eq!(sample.aged(0).status, FeedbackStatus::Fresh);
    assert_eq!(sample.aged(1).status, FeedbackStatus::Stale);
    assert_eq!(sample.aged(u64::MAX).source_age_ms, 86_400_000);
    book.sequence = u64::MAX;
    assert!(book.publish(&[], FeedbackMode::Snapshot, 100).is_err());
    assert!(book.reset(100).is_err());
    book.disconnect(100);
    assert!(book.values.is_empty());
    book.generation = u64::MAX;
    assert!(book.reset(100).is_err());
    assert!(book.configure(schema, &[], 100).is_err());
}
#[test]
fn malformed_or_old_frames_cannot_partially_mutate_a_materialized_snapshot() {
    let schema = publication().feedback;
    let bindings = bindings(&schema);
    let mut book = FeedbackBook::default();
    book.configure(schema.clone(), &bindings, 0).unwrap();
    let mut cursor = FeedbackCursor::default();
    let frame = book.deliver(&mut cursor, &identity(), 0);
    let mut client = FeedbackAccumulator::default();
    for fault in 0..16 {
        let mut bad = frame.clone();
        match fault {
            0 => bad["clientId"] = json!("other"),
            1 => bad["sessionId"] = json!("old"),
            2 => bad["generation"] = json!("2"),
            3 => bad["controllerInstanceId"] = json!("other"),
            4 => bad["publicationHash"] = json!("old"),
            5 => bad["version"] = json!(2),
            6 => bad["type"] = json!("input"),
            7 => bad["sequence"] = json!("01"),
            8 => bad["sequence"] = json!("18446744073709551616"),
            9 => bad["mode"] = json!("other"),
            10 => bad["bindings"][0]["outputId"] = json!("wrong-output"),
            11 => bad["values"][0]["outputId"] = json!("wrong-output"),
            12 => {
                bad["values"].as_array_mut().unwrap().pop();
            }
            13 => bad["extra"] = json!(true),
            14 => bad["values"][0]["sourceAgeMs"] = json!(-1),
            _ => bad["values"][0]["value"] = json!(true),
        };
        assert!(
            client.apply(&bad, &identity(), &schema, 0).is_err(),
            "fault{fault}"
        );
        assert!(client.values.is_empty());
    }
    client.apply(&frame, &identity(), &schema, 0).unwrap();
    let prior = client.values.clone();
    let mut stale = frame.clone();
    stale["sequence"] = json!("0");
    assert!(client.apply(&stale, &identity(), &schema, 1).is_err());
    stale = frame.clone();
    stale["bindingGeneration"] = json!("0");
    assert!(client.apply(&stale, &identity(), &schema, 1).is_err());
    assert_eq!(client.values, prior);
    let mut delta = frame.clone();
    delta["mode"] = json!("delta");
    delta.as_object_mut().unwrap().remove("bindings");
    let mut fresh = FeedbackAccumulator::default();
    assert!(fresh.apply(&delta, &identity(), &schema, 0).is_err());
    delta["bindingGeneration"] = json!("2");
    assert!(client.apply(&delta, &identity(), &schema, 0).is_err());
}
#[test]
fn sixty_four_maximum_text_values_use_bounded_latest_state_without_an_outbound_queue() {
    let schema = (0..64)
        .map(|i| FeedbackDescriptor {
            channel_id: format!("c{i}{}", "x".repeat(60)),
            output_ids: vec![format!("o{i}{}", "x".repeat(60))],
            kind: FeedbackKind::Text,
            unit: None,
            range: None,
        })
        .collect::<Vec<_>>();
    let bindings = bindings(&schema);
    let samples = schema
        .iter()
        .map(|d| {
            let mut s = sample(d);
            s.value = Some(FeedbackValue::Text(
                char::from_u32(0x754c).unwrap().to_string().repeat(256),
            ));
            s.unit = char::from_u32(0x754c).unwrap().to_string().repeat(32);
            s
        })
        .collect::<Vec<_>>();
    let mut book = FeedbackBook::default();
    book.configure(schema.clone(), &bindings, 0).unwrap();
    book.publish(&samples, FeedbackMode::Snapshot, 0).unwrap();
    for t in 1..100 {
        book.publish(&samples, FeedbackMode::Delta, t * 34).unwrap();
    }
    assert_eq!(book.values.len(), 64);
    let frame = book.deliver(&mut FeedbackCursor::default(), &identity(), 3400);
    let bytes = serde_json::to_vec(&frame).unwrap();
    assert!(bytes.len() > crate::server::FRAME_BYTES);
    assert!(bytes.len() < FEEDBACK_BYTES);
    let mut client = FeedbackAccumulator::default();
    assert_eq!(
        client
            .apply(&frame, &identity(), &schema, 3400)
            .unwrap()
            .unwrap()
            .len(),
        64
    );
    let mut oversized = frame;
    oversized["junk"] = json!("x".repeat(FEEDBACK_BYTES));
    assert!(client.apply(&oversized, &identity(), &schema, 0).is_err());
    assert!(book
        .publish(
            &[samples, schema.iter().take(1).map(sample).collect()].concat(),
            FeedbackMode::Delta,
            4000
        )
        .is_err());
}
