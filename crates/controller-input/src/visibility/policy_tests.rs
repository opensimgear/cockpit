use super::*;
pub(crate) fn state() -> State {
    State {
        active: false,
        inverse: false,
        applications: vec![],
        hidden: vec![],
    }
}
pub(crate) fn capture() -> String {
    r"\Device\HarddiskVolume1\Cockpit.exe".into()
}
pub(crate) fn selected(policy: Policy) -> Vec<Selection> {
    vec![Selection {
        instance: r"HID\VID_231D&PID_0200\EXACT".into(),
        policy,
    }]
}
#[test]
fn normal_and_inverse_policies_are_exact_and_capture_is_preserved() {
    let plan = Plan::build(state(), selected(Policy::HideAll), capture()).unwrap();
    assert!(plan.after.active);
    assert!(!plan.after.inverse);
    assert!(plan.after.capture_allowed(&capture()));
    assert_eq!(plan.after.hidden.len(), 1);
    let steps = plan.steps().unwrap();
    assert_eq!(steps.last(), Some(&plan.after));
    assert!(steps[..steps.len() - 1].iter().all(|s| s.hidden.is_empty()));
    let target = r"\Device\HarddiskVolume1\Game.exe";
    let plan = Plan::build(
        state(),
        selected(Policy::HideFromProcess {
            executable: target.into(),
        }),
        capture(),
    )
    .unwrap();
    assert!(plan.after.inverse);
    assert_eq!(plan.after.applications, vec![key(target)]);
    assert!(plan.after.capture_allowed(&capture()));
    assert!(!plan.after.capture_allowed(target));
    assert!(plan
        .after
        .capture_allowed(r"\Device\HarddiskVolume1\Second.exe"));
    let visible = Plan::build(state(), selected(Policy::Visible), capture()).unwrap();
    assert_eq!(visible.before, visible.after);
    assert_eq!(visible.steps().unwrap().len(), 1);
}
#[test]
fn external_policy_is_not_broadened_or_reordered() {
    let before = State {
        active: true,
        inverse: false,
        applications: vec![capture(), r"\Device\HarddiskVolume2\External.exe".into()],
        hidden: vec![r"HID\EXTERNAL\ONE".into()],
    };
    let plan = Plan::build(before.clone(), selected(Policy::HideAll), capture()).unwrap();
    assert_eq!(plan.after.applications, before.applications);
    assert_eq!(plan.after.hidden[0], before.hidden[0]);
    let mut inaccessible = before.clone();
    inaccessible.applications.remove(0);
    assert!(Plan::build(inaccessible, selected(Policy::HideAll), capture()).is_err());
    let mut inactive = before.clone();
    inactive.active = false;
    assert!(Plan::build(inactive, selected(Policy::HideAll), capture()).is_err());
    assert!(Plan::build(
        before.clone(),
        selected(Policy::HideFromProcess {
            executable: r"\Device\HarddiskVolume1\Game.exe".into()
        }),
        capture()
    )
    .is_err());
    let mut external_selected = selected(Policy::Visible);
    external_selected[0].instance = before.hidden[0].clone();
    assert_eq!(
        Plan::build(before.clone(), external_selected, capture())
            .unwrap()
            .after,
        before
    );
    let same = Plan::build(plan.after.clone(), selected(Policy::HideAll), capture()).unwrap();
    assert_eq!(same.after.hidden.len(), 2); // Already hidden does not become a second owned addition.
}
#[test]
fn incompatible_devices_and_processes_fail_before_mutation() {
    let mut selections = selected(Policy::HideAll);
    selections.push(Selection {
        instance: r"HID\VID_0001\OTHER".into(),
        policy: Policy::HideFromProcess {
            executable: r"\Device\HarddiskVolume1\Game.exe".into(),
        },
    });
    assert!(Plan::build(state(), selections, capture()).is_err());
    let mut selections = selected(Policy::HideFromProcess {
        executable: r"\Device\HarddiskVolume1\Game.exe".into(),
    });
    selections.push(Selection {
        instance: r"HID\VID_0001\OTHER".into(),
        policy: Policy::HideFromProcess {
            executable: r"\Device\HarddiskVolume1\Second.exe".into(),
        },
    });
    assert!(Plan::build(state(), selections, capture()).is_err());
    assert!(Plan::build(
        state(),
        selected(Policy::HideFromProcess {
            executable: capture()
        }),
        capture()
    )
    .is_err());
    let mut inverse = state();
    inverse.inverse = true;
    inverse.applications = vec![r"\Device\HarddiskVolume1\Old.exe".into()];
    assert_eq!(
        Plan::build(inverse, selected(Policy::HideAll), capture())
            .unwrap()
            .after
            .applications,
        vec![capture()]
    );
}
#[test]
fn bounds_unknown_paths_and_forged_plans_are_rejected() {
    assert!(Plan::build(state(), vec![], capture()).is_err());
    assert!(Plan::build(
        state(),
        vec![selected(Policy::HideAll)[0].clone(); 17],
        capture()
    )
    .is_err());
    assert!(Plan::build(
        state(),
        vec![selected(Policy::HideAll)[0].clone(); 2],
        capture()
    )
    .is_err());
    for path in [
        "",
        r"C:\game.exe",
        r"\Device\x\..\game.exe",
        r"\Device\x\game.dll",
        "\n",
    ] {
        assert!(executable(path).is_err());
    }
    for path in ["", r"HID\*", r"HID\?", r"HID\x/y", r"ROOT\VJOY\1"] {
        assert!(instance(path).is_err());
    }
    let mut invalid = state();
    invalid.applications = vec![capture(), capture().to_uppercase()];
    assert!(invalid.validate().is_err());
    invalid.applications = vec!["a".repeat(4097)];
    assert!(invalid.validate().is_err());
    invalid.applications = vec!["a".into(); 257];
    assert!(invalid.validate().is_err());
    invalid.applications = (0..32)
        .map(|n| format!("{n}{}", "x".repeat(2048)))
        .collect();
    assert!(invalid.validate().is_err());
    let mut forged = Plan::build(state(), selected(Policy::HideAll), capture()).unwrap();
    forged.after.hidden.push(r"HID\OTHER\1".into());
    assert!(forged.steps().is_err());
}
