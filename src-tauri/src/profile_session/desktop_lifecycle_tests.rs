use super::*;

fn owner() -> RunOwner {
    RunOwner {
        session: "94a5ece4-4e3a-4395-a69b-3aa79c1ecf12".into(),
        generation: 1,
        profile_id: "94a5ece4-4e3a-4395-a69b-3aa79c1ecf13".into(),
        profile_revision: 1,
        game_id: Some("dcs".into()),
    }
}

fn empty_program() -> Program {
    Program {
        sources: vec![],
        nodes: vec![],
        sinks: vec![],
        controllers: vec![],
    }
}

fn empty_wrapper(scope: Option<Scope>, started: bool) -> Desktop {
    Desktop {
        inner: None,
        owner: owner(),
        program: empty_program(),
        scope,
        started,
        wake: None,
        process_observation: None,
    }
}

#[test]
fn absent_game_scope_starts_dormant_and_stops_without_acquiring_output() {
    let reducer = Phase::new(empty_program(), Counter(1), owner().session).unwrap();
    let mut desktop = empty_wrapper(None, false);

    desktop.start(&reducer, None, None).unwrap();
    assert_eq!(desktop.status()["state"], "waiting");
    let expires = std::time::Instant::now();
    desktop.process_observation = Some(crate::game_activation::ProcessObservationLease::Expires(expires));
    assert_eq!(desktop.context_deadline(), Some(expires));
    assert!(!desktop.context_notification_driven(), "Process proof alone cannot qualify a missing adapter");
    desktop.process_observation = Some(crate::game_activation::ProcessObservationLease::Events);
    assert_eq!(desktop.context_deadline(), None);
    assert!(desktop.poll(&[], &reducer).unwrap().is_empty());
    desktop.refresh_context(None).unwrap();
    desktop.keep_alive().unwrap();
    assert!(desktop.inner.is_none());

    desktop.stop().unwrap();
    assert_eq!(desktop.status()["state"], "stopped");
    assert!(desktop.poll(&[], &reducer).unwrap_err().contains("adapter is stopped"));
    assert!(desktop.inner.is_none());
}

#[test]
fn unchanged_application_scope_refresh_is_a_noop_for_a_stopped_inner_adapter() {
    let reducer = Phase::new(empty_program(), Counter(1), owner().session).unwrap();
    let scope = Scope::Application {
        identity: ProcessIdentity {
            executable: "C:\\Fixture\\Game.exe".into(),
            pid: 42,
            created: "10".into(),
        },
    };
    let mut desktop = empty_wrapper(Some(scope.clone()), true);

    desktop.refresh_context(Some(scope)).unwrap();
    desktop.keep_alive().unwrap();
    assert_eq!(desktop.status()["state"], "stopped");
    assert!(desktop.inner.is_none());

    desktop.stop().unwrap();
    assert!(desktop.poll(&[], &reducer).unwrap_err().contains("adapter is stopped"));
}

#[test]
fn binding_a_scope_requires_an_acquired_inner_adapter() {
    let reducer = Phase::new(empty_program(), Counter(1), owner().session).unwrap();
    let scope = Scope::Application {
        identity: ProcessIdentity {
            executable: "C:\\Fixture\\Game.exe".into(),
            pid: 42,
            created: "10".into(),
        },
    };
    let mut desktop = empty_wrapper(None, false);

    assert!(desktop.start(&reducer, None, Some(scope)).unwrap_err().contains("adapter is stopped"));
    assert!(!desktop.started);
    assert!(desktop.inner.is_none());
}

#[test]
fn retired_inner_cannot_start_or_replace_a_scope_with_an_invalid_run() {
    let reducer = Phase::new(empty_program(), Counter(1), owner().session).unwrap();
    let mut desktop = empty_wrapper(Some(Scope::DesktopWide), false);
    assert!(desktop.start(&reducer, None, None).is_err());
    desktop.owner.session = "invalid-run".into();
    assert!(desktop.refresh_context(None).is_err());
    assert!(desktop.inner.is_none());
    assert_eq!(desktop.scope, Some(Scope::DesktopWide));
    assert!(journal_name(&desktop.owner).is_err());
}
