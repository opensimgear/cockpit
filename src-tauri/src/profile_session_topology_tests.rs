use super::*;
use osg_mapping::{
    engine::Counter,
    program::{Comparison, Rounding},
};
use osg_profiles::{
    compiler::compile,
    compiler_models::{BackendCapabilities, CompileSnapshot},
};
use serde_json::{json, Value};

fn fixture() -> (ProfileDocument, Phase, OwnedRun) {
    let document = ProfileDocument::parse(include_bytes!(
        "../../tests/fixtures/profile-authoring/visible-presets.json"
    ))
    .unwrap();
    let snapshot = CompileSnapshot {
        inventory_session: "fixture".into(),
        inventory_revision: "1".into(),
        assignments: vec![],
        resources: vec![],
        game_context: None,
        backend: BackendCapabilities::default(),
        inventory_diagnostics: vec![],
        read_only: false,
    };
    let report = compile(&document, &snapshot);
    let session = uuid::Uuid::new_v4().to_string();
    let phase = Phase::new(report.plan.unwrap().program, Counter(3), session.clone()).unwrap();
    let run = OwnedRun {
        owner: RunOwner {
            session,
            generation: 3,
            profile_id: document.id.clone(),
            profile_revision: document.revision,
            game_id: None,
        },
        snapshot: osg_profiles::session::AppliedSnapshot::capture(&document)
            .unwrap()
            .summary()
            .clone(),
        program_hash: report.program_hash.unwrap(),
        capture_fingerprint: report.capture_fingerprint,
        receipts: vec![],
        attempted: vec![],
    };
    (document, phase, run)
}
fn request(run: &OwnedRun, section: &str, offset: usize, limit: usize) -> Request {
    Request::parse(&json!({"command":"profile_runtime_topology","owner":run.owner,"section":section,"offset":offset,"limit":limit})).unwrap()
}
#[test]
fn immutable_applied_nodes_and_exact_edges_are_sorted_paged_and_passive() {
    let (mut document, mut phase, run) = fixture();
    let baseline_effect_count = fixture().1.drain().len();
    let clock = phase.at_us();
    let token = phase.run_token();
    let page = capture(&document, &phase, &run, &request(&run, "nodes", 0, 64)).unwrap();
    let Items::Nodes(nodes) = &page.items else {
        panic!("nodes")
    };
    assert_eq!(
        page.total,
        document.resources.len() + document.processing_nodes.len() + document.virtual_outputs.len()
    );
    assert!(nodes.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert_eq!(page.program_hash, run.program_hash);
    assert_eq!(page.applied_snapshot, run.snapshot);
    assert_eq!(page.captured_us, clock.0.to_string());
    assert!(nodes
        .iter()
        .any(|node| matches!(node.kind, NodeKind::Processing) && node.summary.contains("Units")));
    let script = nodes
        .iter()
        .find(|node| node.summary.contains("Isolated script"))
        .unwrap();
    assert_eq!(
        script.summary,
        "Isolated script · declared ports only; code is not exposed"
    );
    let json = serde_json::to_value(&page).unwrap();
    assert!(json["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|node| node.as_object().unwrap().len() == 4));
    for node in &document.processing_nodes {
        if let Operation::Script { source, .. } = &node.operation {
            assert!(!serde_json::to_string(&page).unwrap().contains(source));
        }
    }
    let edges = capture(&document, &phase, &run, &request(&run, "edges", 0, 64)).unwrap();
    let Items::Edges(first) = edges.items else {
        panic!("edges")
    };
    let tail = capture(
        &document,
        &phase,
        &run,
        &request(&run, "edges", first.len(), 64),
    )
    .unwrap();
    let Items::Edges(last) = tail.items else {
        panic!("edges")
    };
    let mut actual = first;
    actual.extend(last);
    let mut expected = document.bindings.clone();
    expected.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(actual, expected);
    assert_eq!(tail.total, document.bindings.len());
    document.resources.reverse();
    document.processing_nodes.reverse();
    document.virtual_outputs.reverse();
    document.bindings.reverse();
    assert_eq!(
        serde_json::to_value(
            capture(&document, &phase, &run, &request(&run, "nodes", 0, 64)).unwrap()
        )
        .unwrap(),
        json
    );
    assert_eq!(phase.at_us(), clock);
    assert_eq!(phase.run_token(), token);
    assert_eq!(
        phase.drain().len(),
        baseline_effect_count,
        "Metadata projection must preserve every pending startup effect without adding any output."
    );
}
#[test]
fn topology_rejects_changed_owner_document_stopped_reducer_and_invalid_pages() {
    let (document, mut phase, run) = fixture();
    let mut req = request(&run, "nodes", 0, 1);
    req.owner.generation += 1;
    assert!(capture(&document, &phase, &run, &req).is_err());
    req.owner = run.owner.clone();
    let mut changed = document.clone();
    changed.revision += 1;
    assert!(capture(&changed, &phase, &run, &req).is_err());
    changed = document.clone();
    changed.processing_nodes[0].operation = Operation::Constant {
        value: osg_profiles::PortValue::Boolean(true),
    };
    assert!(capture(&changed, &phase, &run, &req).is_err());
    let mut foreign = run.clone();
    foreign.snapshot.profile_id = uuid::Uuid::new_v4().to_string();
    assert!(capture(&document, &phase, &foreign, &req).is_err());
    foreign = run.clone();
    foreign.owner.session = uuid::Uuid::new_v4().to_string();
    let foreign_req = request(&foreign, "nodes", 0, 1);
    assert!(capture(&document, &phase, &foreign, &foreign_req).is_err());
    assert!(capture(&document, &phase, &run, &request(&run, "nodes", 336, 1)).is_err());
    let total =
        document.resources.len() + document.processing_nodes.len() + document.virtual_outputs.len();
    let empty = capture(&document, &phase, &run, &request(&run, "nodes", total, 1)).unwrap();
    assert!(matches!(empty.items,Items::Nodes(nodes)if nodes.is_empty()));
    phase.stop();
    assert!(capture(&document, &phase, &run, &req).is_err());
}
#[test]
fn request_is_strict_bounded_and_has_no_browser_graph_token_or_sample_fields() {
    let (_, _, run) = fixture();
    let base = json!({"command":"profile_runtime_topology","owner":run.owner,"section":"nodes"});
    for patch in [
        json!({"limit":0}),
        json!({"limit":65}),
        json!({"offset":513}),
        json!({"offset":-1}),
        json!({"section":"ports"}),
        json!({"command":"profile_apply"}),
        json!({"document":{}}),
        json!({"token":"secret"}),
        json!({"samples":[]}),
    ] {
        let mut bad = base.clone();
        bad.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(Request::parse(&bad).is_err(), "{bad}");
    }
    for (key, value) in [
        ("session", json!("wrong")),
        ("profileId", json!("bad")),
        ("generation", json!(0)),
        ("profileRevision", json!(0)),
        ("generation", json!(9_007_199_254_740_992u64)),
        ("gameId", json!("Invalid Game")),
        ("extra", Value::Null),
    ] {
        let mut bad = base.clone();
        bad["owner"][key] = value;
        assert!(Request::parse(&bad).is_err(), "{bad}");
    }
    let default = Request::parse(&base).unwrap();
    assert_eq!(default.limit, 32);
    assert_eq!(default.offset, 0);
}
#[test]
fn transform_summaries_are_bounded_even_for_extreme_numbers_and_never_expose_text() {
    let operations = [
        Operation::DeclareQuantity {
            quantity: "é".repeat(64),
            unit: "m".repeat(128),
        },
        Operation::RangeMap {
            input_min: -1e300,
            input_max: 1e300,
            output_min: -1e299,
            output_max: 1e299,
            rounding: Rounding::Nearest,
        },
        Operation::Scale {
            factor: 1e-300,
            offset: 1e300,
        },
        Operation::Clamp { min: -1e300, max: 1e300 },
        Operation::Deadzone {
            center_percent: 50.0,
            deadzone_percent: 10.0,
            rounding: Rounding::None,
        },
        Operation::Compare {
            comparison: Comparison::Greater,
            threshold: 1e300,
        },
        Operation::Hysteresis {
            press: 1e300,
            release: 1e-300,
            comparison: Comparison::Greater,
        },
        Operation::Constant {
            value: osg_profiles::PortValue::Number(1e300),
        },
        Operation::Select {
            when_true: osg_profiles::PortValue::Text("private text".into()),
            when_false: osg_profiles::PortValue::Boolean(false),
        },
    ];
    for operation in operations {
        let text = summary(&operation);
        assert!(text.len() <= 256, "{text}");
        assert!(!text.contains("private text"));
    }
    assert_eq!(number(0.25), "0.25");
    assert_eq!(number(0.0), "0");
    assert_eq!(summary(&Operation::Clamp { min: 0.0, max: 10.0 }), "Clamp 0…10");
    assert_eq!(
        summary(&Operation::DeclareQuantity {
            quantity: "Reported MSL altitude".into(),
            unit: "ft".into(),
        }),
        "Quantity Reported MSL altitude [ft]"
    );
}

#[test]
fn builtin_summaries_show_configured_timing_polarity_thresholds_and_curve_values() {
    let cases = [
        (
            json!({"kind":"axis","deadZone":120,"invert":true,"scale":950,"curve":[{"input":0,"output":123},{"input":32767,"output":32767}]}),
            vec!["deadzone 120", "inverted", "scale 950", "0→123"],
        ),
        (
            json!({"kind":"axis","deadZone":0,"invert":false,"scale":1000,"curve":[]}),
            vec!["normal", "curve linear"],
        ),
        (
            json!({"kind":"split","positive":true}),
            vec!["positive half"],
        ),
        (
            json!({"kind":"split","positive":false}),
            vec!["negative half"],
        ),
        (
            json!({"kind":"button-axis","range":"bipolar","pressed":32767,"released":-32768}),
            vec!["bipolar", "pressed 32767, released -32768"],
        ),
        (
            json!({"kind":"threshold","press":20000,"release":10000}),
            vec!["press 20000, release 10000"],
        ),
        (
            json!({"kind":"gesture","gesture":"tap","holdUs":750000,"doubleUs":250000,"pulseUs":100000}),
            vec!["tap", "hold 750 ms", "double 250 ms", "pulse 100 ms"],
        ),
        (
            json!({"kind":"gesture","gesture":"hold","holdUs":750000,"doubleUs":250000,"pulseUs":100000}),
            vec!["hold 750 ms"],
        ),
        (
            json!({"kind":"gesture","gesture":"double-tap","holdUs":750000,"doubleUs":250000,"pulseUs":100000}),
            vec!["double-tap"],
        ),
        (
            json!({"kind":"pulse","durationUs":125000,"repeatUs":500000}),
            vec!["duration 125 ms", "repeat 500 ms"],
        ),
        (
            json!({"kind":"pulse","durationUs":100000,"repeatUs":0}),
            vec!["duration 100 ms", "no repeat"],
        ),
        (
            json!({"kind":"macro","steps":[{"afterUs":0,"pressed":true},{"afterUs":123456,"pressed":false}]}),
            vec!["2 steps", "0 µs on → 123.456 ms off"],
        ),
        (json!({"kind":"merge"}), vec!["merge"]),
        (json!({"kind":"chord"}), vec!["chord"]),
        (json!({"kind":"gate"}), vec!["gate"]),
        (json!({"kind":"toggle"}), vec!["toggle"]),
    ];
    for (rule, expected) in cases {
        let operation = Operation::Builtin {
            rule: serde_json::from_value(rule).unwrap(),
            inputs: vec!["in".into()],
        };
        let before = serde_json::to_value(&operation).unwrap();
        let text = summary(&operation);
        for value in expected {
            assert!(text.contains(value), "{value}: {text}");
        }
        assert!(text.contains("explicit inputs: in"));
        assert!(text.len() < 256);
        assert_eq!(serde_json::to_value(&operation).unwrap(), before);
    }
    let mut pulse = Operation::Builtin {
        rule: osg_mapping::rules::Rule::Pulse {
            duration_us: 100000,
            repeat_us: 0,
        },
        inputs: vec!["in".into()],
    };
    let original = summary(&pulse);
    if let Operation::Builtin {
        rule: osg_mapping::rules::Rule::Pulse { duration_us, .. },
        ..
    } = &mut pulse
    {
        *duration_us = 125000;
    }
    assert_ne!(summary(&pulse), original);
    assert_eq!(
        summary(&pulse),
        "pulse · duration 125 ms · no repeat · explicit inputs: in"
    );
}

#[test]
fn builtin_summary_truncation_preserves_utf8_and_keeps_long_macro_parameters_private_to_inspector()
{
    let operation = Operation::Builtin {
        rule: osg_mapping::rules::Rule::Macro {
            steps: (0..16)
                .map(|index| osg_mapping::rules::Step {
                    after_us: index * 3000000,
                    pressed: index % 2 == 0,
                })
                .collect(),
        },
        inputs: vec!["in".into()],
    };
    let text = summary(&operation);
    assert!(text.len() < 256);
    assert!(text.ends_with('…'));
    assert!(text.contains("16 steps"));
    assert!(text.contains("duration 45000 ms"));
    let unicode = summary(&Operation::Builtin {
        rule: osg_mapping::rules::Rule::Toggle,
        inputs: vec!["é".repeat(40); 8],
    });
    assert!(unicode.len() < 256);
    assert!(unicode.ends_with('…'));
    assert!(!unicode.contains('�'));
    assert_eq!(
        bounded_summary("é".repeat(126) + "abc"),
        "é".repeat(126) + "abc"
    );
}
