use super::*;

fn button(state: &mut State, rule: &Rule, down: bool, now: u64) -> bool {
    state.apply(rule, &[Value::Button(down)], Kind::Button, now)
        == Some(Value::Button(true))
}

#[test]
fn seed_resets_armed_pulse_and_suppresses_held_baseline() {
    let pulse = Rule::Pulse {
        duration_us: 1_000,
        repeat_us: 2_000,
    };
    let mut state = State::default();
    assert!(!button(&mut state, &pulse, false, 0));
    assert!(button(&mut state, &pulse, true, 1));

    assert_eq!(state.seed(&pulse, &[Value::Button(true)], Kind::Button, 10), Some(Value::Button(false)));
    assert_eq!(state.next(), None);
    assert!(!button(&mut state, &pulse, true, 11));
    assert!(!button(&mut state, &pulse, false, 12));
    assert!(button(&mut state, &pulse, true, 13));
    assert_eq!(state.next(), Some(1_013));
}

#[test]
fn seed_suppresses_toggle_macro_and_gesture_baseline_actions() {
    let toggle = Rule::Toggle;
    let macro_rule = Rule::Macro {
        steps: vec![
            Step { after_us: 0, pressed: true },
            Step { after_us: 1_000, pressed: false },
        ],
    };
    let gesture = Rule::Gesture {
        gesture: Gesture::Tap,
        hold_us: 1_000,
        double_us: 2_000,
        pulse_us: 1_000,
    };
    for rule in [&toggle, &macro_rule, &gesture] {
        let mut state = State::default();
        // Make the pre-seed state nontrivial so seed must clear it.
        button(&mut state, &toggle, false, 0);
        button(&mut state, &toggle, true, 1);
        assert_eq!(state.seed(rule, &[Value::Button(true)], Kind::Button, 10), Some(Value::Button(false)));
        assert_eq!(state.next(), None);
        assert!(!button(&mut state, rule, true, 11));
        assert!(!button(&mut state, rule, false, 12));
        assert_eq!(state.next(), None);
        if matches!(rule, Rule::Toggle) {
            assert!(button(&mut state, rule, true, 13));
        } else if matches!(rule, Rule::Macro { .. }) {
            assert!(button(&mut state, rule, true, 13));
        } else {
            assert!(!button(&mut state, rule, true, 13));
            assert!(!button(&mut state, rule, false, 14));
            assert_eq!(state.next(), Some(2_014));
            assert!(button(&mut state, rule, false, 2_014));
            assert_eq!(state.next(), Some(3_014));
            assert!(!button(&mut state, rule, false, 3_014));
        }
    }
}

#[test]
fn seed_preserves_combinational_axis_and_gate_levels() {
    let axis = Rule::Split { positive: true };
    let mut state = State::default();
    assert_eq!(state.seed(&axis, &[Value::Axis(12_345)], Kind::Axis { range: AxisRange::Unipolar }, 5), Some(Value::Axis(12_345)));
    let gate = Rule::Gate;
    assert_eq!(state.seed(&gate, &[Value::Axis(777), Value::Button(true)], Kind::Axis { range: AxisRange::Bipolar }, 6), Some(Value::Axis(777)));
    assert_eq!(state.next(), None);
}
