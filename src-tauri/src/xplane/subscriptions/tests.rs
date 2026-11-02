use super::*;

fn planner() -> SubscriptionPlanner {
    SubscriptionPlanner::new(["guard/a", "guard/b"]).unwrap()
}

#[test]
fn union_shares_aliases_and_isolates_observer_changes() {
    let mut planner = planner();
    let a = planner
        .add_lease("reader-a", ["sim/alpha", "sim/shared"])
        .unwrap();
    assert_eq!(
        a.added.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(),
        ["sim/alpha", "sim/shared"]
    );
    let alpha_index = planner.index_for("sim/alpha").unwrap();
    let shared_index = planner.index_for("sim/shared").unwrap();

    let b = planner
        .add_lease("reader-b", ["sim/shared", "sim/beta"])
        .unwrap();
    assert_eq!(
        b.added.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(),
        ["sim/beta"]
    );
    assert_eq!(planner.index_for("sim/shared"), Some(shared_index));

    let removed = planner.remove_lease("reader-a").unwrap();
    assert_eq!(
        removed
            .removed
            .iter()
            .map(|s| s.path.as_str())
            .collect::<Vec<_>>(),
        ["sim/alpha"]
    );
    assert_eq!(planner.path_for_index(alpha_index), None);
    assert_eq!(planner.index_for("sim/shared"), Some(shared_index));
    assert_eq!(
        planner.selected().map(|s| s.path).collect::<Vec<_>>(),
        ["sim/beta", "sim/shared"]
    );
}

#[test]
fn replacement_is_atomic_and_preserves_unchanged_paths() {
    let mut planner = planner();
    planner.add_lease("reader", ["keep", "retire"]).unwrap();
    let keep_index = planner.index_for("keep").unwrap();
    let before = planner.current().collect::<Vec<_>>();

    assert!(matches!(
        planner.replace_lease("reader", ["keep", "\0bad"]),
        Err(PlannerError::InvalidPath(_))
    ));
    assert_eq!(planner.current().collect::<Vec<_>>(), before);
    assert_eq!(planner.index_for("keep"), Some(keep_index));

    let delta = planner.replace_lease("reader", ["keep", "new"]).unwrap();
    assert_eq!(
        delta.removed,
        [WireSubscription {
            path: "retire".into(),
            index: 4
        }]
    );
    assert_eq!(
        delta.added,
        [WireSubscription {
            path: "new".into(),
            index: 5
        }]
    );
    assert_eq!(planner.index_for("keep"), Some(keep_index));
}

#[test]
fn guards_are_permanent_and_excluded_from_selected_limit() {
    let mut planner = SubscriptionPlanner::new(["guard/a", "guard/a"]).unwrap();
    let guard_index = planner.index_for("guard/a").unwrap();
    let paths = (0..MAX_SELECTED_PATHS)
        .map(|n| format!("selected/{n:04}"))
        .collect::<Vec<_>>();
    let delta = planner.add_lease("reader", paths).unwrap();
    assert_eq!(delta.added.len(), MAX_SELECTED_PATHS);
    assert_eq!(planner.index_for("guard/a"), Some(guard_index));
    assert_eq!(planner.selected().count(), MAX_SELECTED_PATHS);
    assert_eq!(
        planner
            .replace_lease("reader", ["selected/0000", "extra"])
            .unwrap()
            .added
            .len(),
        1
    );
}

#[test]
fn lease_and_path_limits_fail_without_mutating_state() {
    let mut planner = planner();
    let before = planner.current().collect::<Vec<_>>();
    let too_long = "x".repeat(MAX_PATH_BYTES + 1);
    assert!(matches!(
        planner.add_lease("bad", [too_long]),
        Err(PlannerError::InvalidPath(_))
    ));
    assert!(matches!(
        planner.add_lease("bad", ["", "valid"]),
        Err(PlannerError::InvalidPath(_))
    ));
    assert_eq!(planner.current().collect::<Vec<_>>(), before);

    for n in 0..MAX_LEASES {
        planner
            .add_lease(format!("reader-{n}"), std::iter::empty::<String>())
            .unwrap();
    }
    assert_eq!(
        planner
            .add_lease("overflow", std::iter::empty::<String>())
            .unwrap_err(),
        PlannerError::TooManyLeases
    );
    assert_eq!(
        planner.remove_lease("missing").unwrap_err(),
        PlannerError::LeaseMissing("missing".into())
    );
    assert_eq!(planner.leases.len(), MAX_LEASES);
}

#[test]
fn retired_indices_are_never_reused() {
    let mut planner = planner();
    let first = planner.add_lease("a", ["first"]).unwrap().added[0].index;
    planner.remove_lease("a").unwrap();
    let second = planner.add_lease("b", ["second"]).unwrap().added[0].index;
    assert!(second > first);
    assert_eq!(planner.path_for_index(first), None);
    assert_eq!(planner.path_for_index(second), Some("second"));
}

#[test]
fn index_exhaustion_is_checked_and_atomic() {
    let mut planner = planner();
    planner.set_next_index_for_test(i32::MAX as i64);
    let last = planner.add_lease("last", ["at-max"]).unwrap();
    assert_eq!(last.added[0].index, i32::MAX);
    let before = planner.current().collect::<Vec<_>>();
    assert_eq!(
        planner
            .add_lease("exhausted", ["cannot-allocate"])
            .unwrap_err(),
        PlannerError::IndexExhausted
    );
    assert_eq!(planner.current().collect::<Vec<_>>(), before);
    assert_eq!(planner.leases.contains_key("exhausted"), false);
}

#[test]
fn rotation_retires_every_active_index_atomically() {
    let mut planner = SubscriptionPlanner::with_start_index(["guard/z", "guard/a"], 20).unwrap();
    planner
        .add_lease("reader", ["selected/a", "selected/b"])
        .unwrap();
    let before = planner.current().collect::<Vec<_>>();
    assert_eq!(before[0].path, "guard/a");
    assert_eq!(planner.index_for("guard/z"), Some(20));

    let delta = planner.rotate().unwrap();
    assert_eq!(delta.removed.len(), 4);
    assert_eq!(delta.added.len(), 4);
    for old in before {
        assert_eq!(planner.path_for_index(old.index), None);
        assert_ne!(planner.index_for(&old.path), Some(old.index));
    }

    let mut exhausted = planner.clone();
    exhausted.set_next_index_for_test(i32::MAX as i64);
    let snapshot = exhausted.current().collect::<Vec<_>>();
    assert_eq!(
        exhausted.rotate().unwrap_err(),
        PlannerError::IndexExhausted
    );
    assert_eq!(exhausted.current().collect::<Vec<_>>(), snapshot);
}
