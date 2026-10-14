use super::*;
use crate::fixtures::profile;
#[test]
fn portable_round_trip_bounds_and_strict_json() {
    let profile = profile();
    assert_eq!(
        Profile::parse(&profile.export().unwrap())
            .unwrap()
            .export()
            .unwrap(),
        profile.export().unwrap()
    );
    let mut bytes = profile.export().unwrap();
    bytes.resize(DOCUMENT_LIMIT, b' ');
    assert!(Profile::parse(&bytes).is_ok());
    bytes.push(b' ');
    assert!(Profile::parse(&bytes).is_err());
    let text = String::from_utf8(profile.export().unwrap()).unwrap();
    for invalid in [
        text.replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1),
        text.replacen("\"schema\":1", "\"schema\":2", 1),
        text.replacen("\"schema\":1", "\"schema\":1,\"enableOutput\":true", 1),
        text.replacen("\"kind\":\"source\"", "\"kind\":\"virtual-output\"", 1),
        "[]".into(),
        "{".into(),
    ] {
        assert!(Profile::parse(invalid.as_bytes()).is_err(), "{invalid}");
    }
    assert!(identifier("one.two-3_4"));
    assert!(!identifier("../unsafe"));
    assert!(!identifier(""));
    assert!(!label(" \n"));
    assert!(!label(&"a".repeat(81)));
    for value in [Value::Axis(-32768), Value::Axis(32767)] {
        assert!(Kind::Axis {
            range: AxisRange::Bipolar
        }
        .accepts(value));
    }
    assert!(!Kind::Axis {
        range: AxisRange::Unipolar
    }
    .accepts(Value::Axis(-1)));
    assert!(!Kind::Hat.accepts(Value::Hat(Some(8))));
    assert!(Kind::Hat.accepts(Value::Hat(Some(7))));
    assert!(!Kind::Button.accepts(Value::Axis(0)));
}
#[test]
fn bounded_documents_and_unique_names() {
    let valid = profile();
    let mut cases = Vec::new();
    let mut p = valid.clone();
    p.id = "path".into();
    cases.push(p);
    let mut p = valid.clone();
    p.name = "".into();
    cases.push(p);
    let mut p = valid.clone();
    p.roles.clear();
    cases.push(p);
    let mut p = valid.clone();
    p.roles = vec![p.roles[0].clone(); 17];
    cases.push(p);
    let mut p = valid.clone();
    p.targets = vec![p.targets[0].clone(); 9];
    cases.push(p);
    let mut p = valid.clone();
    p.nodes = vec![p.nodes[0].clone(); 257];
    cases.push(p);
    let mut p = valid.clone();
    p.roles[1] = p.roles[0].clone();
    cases.push(p);
    let mut p = valid.clone();
    p.roles[0].controls = vec![p.roles[0].controls[0].clone(); 257];
    cases.push(p);
    let mut p = valid.clone();
    let duplicate = p.roles[0].controls[0].clone();
    p.roles[0].controls.push(duplicate);
    cases.push(p);
    let mut p = valid.clone();
    p.targets.push(p.targets[0].clone());
    cases.push(p);
    let mut p = valid.clone();
    p.nodes.push(p.nodes[0].clone());
    cases.push(p);
    let mut p = valid.clone();
    p.nodes[0].inputs.clear();
    cases.push(p);
    let mut p = valid.clone();
    p.roles = (0..16)
        .map(|index| {
            let mut role = p.roles[0].clone();
            role.id = format!("role{index}");
            role.controls = (0..256)
                .map(|n| Control {
                    id: format!("control{n}"),
                    kind: Kind::Button,
                })
                .collect();
            role
        })
        .collect();
    cases.push(p);
    for p in cases {
        assert!(p.compile().is_err());
    }
    let mut layout = valid.targets[0].layout.clone();
    layout.buttons = 129;
    assert!(layout.validate().is_err());
    layout.buttons = 0;
    layout.axes.clear();
    layout.hats = 0;
    assert!(layout.validate().is_err());
    layout.hats = 5;
    assert!(layout.validate().is_err());
    layout.hats = 1;
    layout.axes = vec![Axis {
        range: AxisRange::Unipolar,
        fallback: -1,
    }];
    assert!(layout.validate().is_err());
    layout.axes = vec![
        Axis {
            range: AxisRange::Bipolar,
            fallback: 0
        };
        17
    ];
    assert!(layout.validate().is_err());
    assert!(valid.targets[0]
        .layout
        .kind(&OutputControl::Button { index: 99 })
        .is_none());
    assert!(valid.targets[0]
        .layout
        .kind(&OutputControl::Hat { index: 99 })
        .is_none());
}
#[test]
fn cycles_references_types_and_writer_arbitration() {
    let valid = profile();
    let mut p = valid.clone();
    p.nodes[0].inputs = vec![Input::Node {
        node: "left-trigger".into(),
    }];
    assert!(p.compile().err().unwrap().contains("cycle"));
    let mut p = valid.clone();
    p.nodes[0].inputs = vec![Input::Node {
        node: "right-trigger".into(),
    }];
    p.nodes[1].inputs = vec![Input::Node {
        node: "left-trigger".into(),
    }];
    assert!(p.compile().is_err());
    for input in [
        Input::Node {
            node: "unknown".into(),
        },
        Input::Source {
            role: "unknown".into(),
            control: "trigger".into(),
        },
        Input::Source {
            role: "left".into(),
            control: "unknown".into(),
        },
    ] {
        let mut p = valid.clone();
        p.nodes[0].inputs = vec![input];
        assert!(p.compile().is_err());
    }
    let mut p = valid.clone();
    p.nodes[0].output.as_mut().unwrap().target = "missing".into();
    assert!(p.compile().is_err());
    let mut p = valid.clone();
    p.nodes[0].output.as_mut().unwrap().control = OutputControl::Axis { index: 0 };
    assert!(p.compile().is_err());
    let mut p = valid.clone();
    let mut extra = p.nodes[2].clone();
    extra.id = "axis-extra".into();
    p.nodes.push(extra);
    assert!(p.compile().err().unwrap().contains("Ambiguous"));
    p.arbitration.push(Arbitration {
        output: p.nodes[2].output.clone().unwrap(),
        priority: vec!["axis-extra".into(), "left-x".into()],
    });
    assert!(p.compile().is_ok());
    let good = p.clone();
    p.arbitration[0].priority.reverse();
    assert!(p.compile().is_ok());
    p.arbitration[0].priority.push("left-x".into());
    assert!(p.compile().is_err());
    let mut p = good.clone();
    p.arbitration.push(p.arbitration[0].clone());
    assert!(p.compile().is_err());
    let mut p = good.clone();
    p.arbitration[0].output.target = "unknown".into();
    assert!(p.compile().is_err());
    let mut p = valid;
    p.arbitration.push(Arbitration {
        output: p.nodes[0].output.clone().unwrap(),
        priority: vec!["left-trigger".into(), "right-trigger".into()],
    });
    assert!(p.compile().is_err());
}
#[test]
fn visibility_intent_never_contains_local_process_paths() {
    let mut p = profile();
    p.roles[0].origin = Origin::HostController;
    p.requirements.visibility = vec![Visibility::Visible {
        role: "left".into(),
    }];
    assert!(p.compile().is_ok());
    p.requirements.visibility = vec![Visibility::HideAll {
        role: "left".into(),
    }];
    assert!(p.compile().is_err());
    p.requirements.device_hiding = true;
    assert!(p.compile().is_ok());
    p.requirements.visibility = vec![Visibility::HideFromProcess {
        role: "left".into(),
        application: "dcs".into(),
    }];
    assert!(p.compile().is_ok());
    p.requirements.visibility = vec![Visibility::HideFromProcess {
        role: "left".into(),
        application: "C:\\Games\\game.exe".into(),
    }];
    assert!(p.compile().is_err());
    p.requirements.visibility = vec![Visibility::Visible {
        role: "right".into(),
    }];
    assert!(p.compile().is_err());
    p.requirements.visibility = vec![Visibility::Visible {
        role: "missing".into(),
    }];
    assert!(p.compile().is_err());
    p.requirements.visibility = vec![
        Visibility::Visible {
            role: "left".into()
        };
        2
    ];
    assert!(p.compile().is_err());
    p.requirements.visibility = vec![
        Visibility::Visible {
            role: "left".into()
        };
        17
    ];
    assert!(p.compile().is_err());
}
