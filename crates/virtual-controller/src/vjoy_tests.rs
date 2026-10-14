use super::*;
use osg_mapping::profile::{Axis as LogicalAxis, AxisRange};
pub(crate) fn binding() -> Binding {
    Binding {
        target: "main".into(),
        slot: 1,
        axes: AXIS_USAGES.to_vec(),
        layout: Layout {
            buttons: 128,
            axes: (0..16)
                .map(|_| LogicalAxis {
                    range: AxisRange::Bipolar,
                    fallback: -32768,
                })
                .collect(),
            hats: 4,
        },
    }
}
#[test]
fn v3_report_offsets_ranges_all_buttons_and_diagonal_hats() {
    let b = binding();
    let mut state = b.fallback();
    state.buttons.fill(true);
    state.axes.fill(32767);
    state.hats = vec![Some(0), Some(1), Some(7), None];
    let report = b.report(&state).unwrap();
    assert_eq!(std::mem::size_of_val(&report), 124);
    assert_eq!(report.device, 1);
    assert!(report.words[..16].iter().all(|v| *v == 32767));
    for index in [18, 23, 24, 25] {
        assert_eq!(report.words[index], -1);
    }
    assert_eq!(&report.words[19..23], &[0, 4500, 31500, -1]);
    let neutral = b.report(&b.fallback()).unwrap();
    assert!(neutral.words[..19].iter().all(|v| *v == 0));
    assert_eq!(&neutral.words[19..23], &[-1, -1, -1, -1]);
    let mut unipolar = b.clone();
    unipolar.layout.axes[0].range = AxisRange::Unipolar;
    unipolar.layout.axes[0].fallback = 17;
    assert_eq!(unipolar.report(&unipolar.fallback()).unwrap().words[3], 17);
    for field in 0..4 {
        let mut invalid = state.clone();
        match field {
            0 => invalid.axes[0] = 32768,
            1 => invalid.hats[0] = Some(8),
            2 => {
                invalid.buttons.pop();
            }
            _ => {
                invalid.axes.pop();
            }
        }
        assert!(b.report(&invalid).is_err());
    }
}
#[test]
fn shapes_are_exact_and_invalid_local_assignments_fail_before_driver_use() {
    let b = binding();
    let slot = Slot {
        id: 1,
        status: Status::Free,
        owner_pid: None,
        buttons: 128,
        axes: AXIS_USAGES
            .iter()
            .map(|u| Axis {
                usage: *u,
                min: 0,
                max: 32767,
            })
            .collect(),
        continuous_hats: 4,
        discrete_hats: 0,
        force_feedback: false,
    };
    b.match_slot(&slot).unwrap();
    for index in 0..6 {
        let mut bad = slot.clone();
        match index {
            0 => bad.buttons = 1,
            1 => bad.discrete_hats = 1,
            2 => bad.force_feedback = true,
            3 => bad.axes[0].min = 1,
            4 => {
                bad.axes.pop();
            }
            _ => bad.axes[0].usage = 999,
        }
        assert!(b.match_slot(&bad).is_err());
    }
    for index in 0..7 {
        let mut bad = b.clone();
        match index {
            0 => bad.slot = 0,
            1 => bad.target = "".into(),
            2 => bad.target = "bad/target".into(),
            3 => bad.axes[0] = 999,
            4 => bad.axes[0] = bad.axes[1],
            5 => {
                bad.axes.pop();
            }
            _ => bad.layout.buttons = 129,
        }
        assert!(bad.validate().is_err());
    }
}
