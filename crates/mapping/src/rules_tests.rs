use super::*;
const AX: Kind = Kind::Axis {
    range: AxisRange::Bipolar,
};
const UNI: Kind = Kind::Axis {
    range: AxisRange::Unipolar,
};
fn evaluate(rule: &Rule, inputs: Vec<Value>, kind: Kind) -> Option<Value> {
    State::default().apply(rule, &inputs, kind, 1)
}
#[test]
fn curve_endpoints_deadzones_inversion_scaling_merge_split() {
    for range in [AxisRange::Bipolar, AxisRange::Unipolar] {
        let kind = Kind::Axis { range };
        let rule = Rule::Axis {
            dead_zone: 1000,
            invert: false,
            scale: 1000,
            curve: vec![
                Point {
                    input: range.min(),
                    output: range.min(),
                },
                Point {
                    input: 32767,
                    output: 32767,
                },
            ],
        };
        assert_eq!(rule.validate(&[kind]).unwrap(), kind);
        for value in [range.min(), 0, 32767] {
            assert_eq!(
                evaluate(&rule, vec![Value::Axis(value)], kind),
                Some(Value::Axis(value))
            );
        }
        assert_eq!(
            evaluate(&rule, vec![Value::Axis(500)], kind),
            Some(Value::Axis(0))
        );
    }
    let invert = Rule::Axis {
        dead_zone: 0,
        invert: true,
        scale: 2000,
        curve: vec![],
    };
    assert_eq!(
        evaluate(&invert, vec![Value::Axis(-32768)], AX),
        Some(Value::Axis(32767))
    );
    assert_eq!(
        evaluate(&invert, vec![Value::Axis(0)], UNI),
        Some(Value::Axis(32767))
    );
    assert_eq!(Rule::Merge.validate(&[UNI, UNI]).unwrap(), AX);
    assert_eq!(
        evaluate(
            &Rule::Merge,
            vec![Value::Axis(30000), Value::Axis(10000)],
            AX
        ),
        Some(Value::Axis(-20000))
    );
    for positive in [true, false] {
        let r = Rule::Split { positive };
        assert_eq!(r.validate(&[AX]).unwrap(), UNI);
        assert_eq!(
            evaluate(&r, vec![Value::Axis(-32768)], UNI),
            Some(Value::Axis(if positive { 0 } else { 32767 }))
        );
    }
    let curve = Rule::Axis {
        dead_zone: 0,
        invert: false,
        scale: 1000,
        curve: vec![
            Point {
                input: -32768,
                output: -100,
            },
            Point {
                input: 0,
                output: 0,
            },
            Point {
                input: 32767,
                output: 100,
            },
        ],
    };
    assert_eq!(
        evaluate(&curve, vec![Value::Axis(16384)], AX),
        Some(Value::Axis(50))
    );
}
#[test]
fn strict_bounds_prevent_invalid_types_and_unbounded_timing() {
    let valid = vec![
        (Rule::Chord, vec![Kind::Button; 2]),
        (Rule::Gate, vec![AX, Kind::Button]),
        (Rule::Toggle, vec![Kind::Button]),
        (
            Rule::ButtonAxis {
                range: AxisRange::Unipolar,
                pressed: 32767,
                released: 0,
            },
            vec![Kind::Button],
        ),
        (
            Rule::Threshold {
                press: 100,
                release: 50,
            },
            vec![AX],
        ),
        (
            Rule::Gesture {
                gesture: Gesture::Tap,
                hold_us: 1000,
                double_us: 1000,
                pulse_us: 1000,
            },
            vec![Kind::Button],
        ),
        (
            Rule::Pulse {
                duration_us: 1000,
                repeat_us: 2000,
            },
            vec![Kind::Button],
        ),
        (
            Rule::Macro {
                steps: vec![
                    Step {
                        after_us: 0,
                        pressed: true,
                    },
                    Step {
                        after_us: 1000,
                        pressed: false,
                    },
                ],
            },
            vec![Kind::Button],
        ),
    ];
    for (rule, kinds) in valid {
        assert!(rule.validate(&kinds).is_ok());
        assert!(rule.validate(&[]).is_err());
    }
    for rule in [
        Rule::Axis {
            dead_zone: 32767,
            invert: false,
            scale: 1000,
            curve: vec![],
        },
        Rule::Axis {
            dead_zone: 0,
            invert: false,
            scale: 4001,
            curve: vec![],
        },
        Rule::Axis {
            dead_zone: 0,
            invert: false,
            scale: 1000,
            curve: vec![Point {
                input: 0,
                output: 0,
            }],
        },
        Rule::Threshold {
            press: 10,
            release: 20,
        },
        Rule::Threshold {
            press: 40000,
            release: 0,
        },
    ] {
        assert!(rule.validate(&[AX]).is_err());
    }
    for rule in [
        Rule::Gesture {
            gesture: Gesture::Hold,
            hold_us: 0,
            double_us: 1000,
            pulse_us: 1000,
        },
        Rule::Pulse {
            duration_us: 1000,
            repeat_us: 1000,
        },
        Rule::Macro { steps: vec![] },
        Rule::Macro {
            steps: vec![Step {
                after_us: 0,
                pressed: true,
            }],
        },
        Rule::Macro {
            steps: vec![Step {
                after_us: 1,
                pressed: false,
            }],
        },
        Rule::ButtonAxis {
            range: AxisRange::Unipolar,
            pressed: -1,
            released: 0,
        },
    ] {
        assert!(rule.validate(&[Kind::Button]).is_err());
    }
    assert!(Rule::Merge.validate(&[AX, AX]).is_err());
    assert!(Rule::Chord.validate(&[Kind::Button]).is_err());
}
#[test]
fn noisy_threshold_chords_modifier_and_toggle() {
    let mut s = State::default();
    let r = Rule::Threshold {
        press: 100,
        release: 50,
    };
    for (v, expected) in [
        (101, false),
        (49, false),
        (99, false),
        (100, true),
        (99, true),
        (51, true),
        (50, false),
    ] {
        assert_eq!(
            s.apply(&r, &[Value::Axis(v)], Kind::Button, 1),
            Some(Value::Button(expected))
        );
    }
    let mut s = State::default();
    for (a, b, expected) in [
        (true, true, false),
        (false, true, false),
        (true, true, true),
        (true, false, false),
    ] {
        assert_eq!(
            s.apply(
                &Rule::Chord,
                &[Value::Button(a), Value::Button(b)],
                Kind::Button,
                1
            ),
            Some(Value::Button(expected))
        );
    }
    assert_eq!(
        evaluate(&Rule::Gate, vec![Value::Axis(42), Value::Button(false)], AX),
        None
    );
    assert_eq!(
        evaluate(&Rule::Gate, vec![Value::Axis(42), Value::Button(true)], AX),
        Some(Value::Axis(42))
    );
    let mut s = State::default();
    for (pressed, expected) in [
        (true, false),
        (false, false),
        (true, true),
        (true, true),
        (false, true),
        (true, false),
    ] {
        assert_eq!(
            s.apply(&Rule::Toggle, &[Value::Button(pressed)], Kind::Button, 1),
            Some(Value::Button(expected))
        );
    }
    let mut s = State::default();
    let r = Rule::ButtonAxis {
        range: AxisRange::Bipolar,
        pressed: 42,
        released: -42,
    };
    s.apply(&r, &[Value::Button(false)], AX, 1);
    assert_eq!(
        s.apply(&r, &[Value::Button(true)], AX, 2),
        Some(Value::Axis(42))
    );
}
fn button(s: &mut State, r: &Rule, at: u64, down: bool) -> bool {
    s.apply(r, &[Value::Button(down)], Kind::Button, at) == Some(Value::Button(true))
}
#[test]
fn deferred_tap_double_tap_hold_pulse_repeat_and_ordered_macro() {
    for gesture in [Gesture::Tap, Gesture::DoubleTap, Gesture::Hold] {
        let r = Rule::Gesture {
            gesture,
            hold_us: 10000,
            double_us: 20000,
            pulse_us: 1000,
        };
        let mut s = State::default();
        assert!(!button(&mut s, &r, 1, false));
        assert!(!button(&mut s, &r, 2, true));
        assert!(!button(&mut s, &r, 100, false));
        assert_eq!(
            button(&mut s, &r, 20100, false),
            matches!(gesture, Gesture::Tap)
        );
        assert!(!button(&mut s, &r, 21100, false));
        assert!(!button(&mut s, &r, 22000, true));
        assert!(!button(&mut s, &r, 22100, false));
        assert!(!button(&mut s, &r, 23000, true));
        assert_eq!(
            button(&mut s, &r, 23100, false),
            matches!(gesture, Gesture::DoubleTap)
        );
        assert!(!button(&mut s, &r, 25000, true));
        assert_eq!(
            button(&mut s, &r, 35000, true),
            matches!(gesture, Gesture::Hold)
        );
        assert!(!button(&mut s, &r, 35001, false));
    }
    let r = Rule::Pulse {
        duration_us: 1000,
        repeat_us: 3000,
    };
    let mut s = State::default();
    button(&mut s, &r, 1, false);
    assert!(button(&mut s, &r, 2, true));
    assert_eq!(s.next(), Some(1002));
    assert!(!button(&mut s, &r, 1002, true));
    assert!(button(&mut s, &r, 3002, true));
    button(&mut s, &r, 3003, false);
    assert!(!button(&mut s, &r, 4002, false));
    assert_eq!(s.next(), None);
    let r = Rule::Macro {
        steps: vec![
            Step {
                after_us: 0,
                pressed: true,
            },
            Step {
                after_us: 1000,
                pressed: false,
            },
            Step {
                after_us: 2000,
                pressed: true,
            },
            Step {
                after_us: 3000,
                pressed: false,
            },
        ],
    };
    let mut s = State::default();
    button(&mut s, &r, 1, false);
    assert!(button(&mut s, &r, 2, true));
    assert!(!button(&mut s, &r, 1002, false));
    assert!(button(&mut s, &r, 2002, false));
    assert!(!button(&mut s, &r, 3002, false));
    assert_eq!(s.next(), None);
}
