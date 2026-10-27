use super::{Reconciler, Scope};
use crate::activity::{
    event,
    observations::{Batch, Observation},
};
use serde_json::json;
use std::{collections::BTreeMap, time::Instant};

fn fact(subject: &str, state: &str) -> Observation {
    let (kind, id) = subject.split_once(':').unwrap();
    Observation {
        observed_at:Instant::now(),
        event: event(
            kind,
            &format!("{kind}_changed"),
            "info",
            format!("{subject} changed"),
            Some(id.into()),
            None,
        ),
        subject: subject.into(),
        state: state.into(),
    }
}

fn batch(started: Instant, events: Vec<Observation>, states: &[(&str, &str)]) -> Batch {
    Batch {
        started,
        states: states
            .iter()
            .map(|(subject, state)| ((*subject).into(), (*state).into()))
            .collect(),
        events,
    }
}

fn expected(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(subject, state)| ((*subject).into(), (*state).into()))
        .collect()
}

#[test]
fn a_preexisting_observation_cannot_be_claimed_by_a_later_noop() {
    let mut reconciler=Reconciler::default();
    let earlier=fact("controller:wheel","online");let event_key=earlier.event.event_key.clone();
    reconciler.begin("later-connect".into(),Scope::request(&json!({"command":"controller_update","sourceId":"wheel"})));
    reconciler.finish("later-connect",expected(&[("controller:wheel","online")]));
    let ready=reconciler.observe(batch(Instant::now(),vec![earlier],&[]));
    assert_eq!(ready.len(),1);assert_eq!(ready[0].event_key,event_key);
}

#[test]
fn observation_started_before_completion_is_deferred_then_matched_once() {
    let mut reconciler = Reconciler::default();
    reconciler.begin(
        "save-profile".into(),
        Scope::request(&json!({"command":"profile_save","id":"profile-1"})),
    );
    let started = Instant::now();
    assert!(reconciler
        .observe(batch(
            started,
            vec![fact("profile:profile-1", "revision-2")],
            &[]
        ))
        .is_empty());

    // The native command event is emitted by the separate action queue exactly once.
    let canonical = event(
        "profiles",
        "profile_save",
        "info",
        "Profile saved".into(),
        Some("profile-1".into()),
        None,
    );
    reconciler.finish(
        "save-profile",
        expected(&[("profile:profile-1", "revision-2")]),
    );
    assert!(reconciler.ready().is_empty());
    assert_eq!(canonical.event_type, "profile_save");
}

#[test]
fn observation_after_finish_is_consumed_by_the_terminal_receipt() {
    let mut reconciler = Reconciler::default();
    reconciler.begin(
        "forget-device".into(),
        Scope::request(&json!({"command":"controller_forget","sourceId":"hid-1"})),
    );
    reconciler.finish(
        "forget-device",
        expected(&[("controller:hid-1", "removed")]),
    );
    let started = Instant::now();
    assert!(reconciler
        .observe(batch(
            started,
            vec![fact("controller:hid-1", "removed")],
            &[]
        ))
        .is_empty());
    assert!(reconciler.ready().is_empty());
}

#[test]
fn a_post_finish_noop_watermark_expires_receipt_before_a_later_independent_event() {
    let mut reconciler = Reconciler::default();
    reconciler.begin(
        "disable".into(),
        Scope::request(&json!({"command":"disable","gatewayId":"gateway-1","moduleId":"module-1"})),
    );
    reconciler.finish(
        "disable",
        expected(&[("module:gateway-1:module-1", "disabled")]),
    );
    let later_capture = Instant::now();
    assert!(reconciler
        .observe(batch(
            later_capture,
            vec![],
            &[("module:gateway-1:module-1", "still-enabled")]
        ))
        .is_empty());

    let independent = fact("module:gateway-1:module-1", "disabled");
    let key = independent.event.event_key.clone();
    let ready = reconciler.observe(batch(Instant::now(), vec![independent], &[]));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].event_key, key);
}

#[test]
fn two_quick_real_actions_keep_both_canonical_events_and_consume_two_facts() {
    let mut reconciler = Reconciler::default();
    let scope = Scope::request(&json!({"command":"controller_update","sourceId":"hid-1"}));
    let first = event(
        "devices",
        "controller_update",
        "info",
        "Name changed".into(),
        Some("hid-1".into()),
        None,
    );
    let second = event(
        "devices",
        "controller_update",
        "info",
        "Name changed".into(),
        Some("hid-1".into()),
        None,
    );
    assert_ne!(first.event_key, second.event_key);
    reconciler.begin("action-one".into(), scope);
    reconciler.finish(
        "action-one",
        expected(&[("controller:hid-1", "same-state")]),
    );
    reconciler.begin(
        "action-two".into(),
        Scope::request(&json!({"command":"controller_update","sourceId":"hid-1"})),
    );
    reconciler.finish(
        "action-two",
        expected(&[("controller:hid-1", "same-state")]),
    );

    let ready = reconciler.observe(batch(
        Instant::now(),
        vec![
            fact("controller:hid-1", "same-state"),
            fact("controller:hid-1", "same-state"),
        ],
        &[],
    ));
    assert!(
        ready.is_empty(),
        "both native facts duplicate their respective command events"
    );
    assert_eq!(
        [first, second]
            .iter()
            .map(|event| event.event_key.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
}

#[test]
fn unrelated_background_observations_survive_a_wildcard_creation_scope() {
    let mut reconciler = Reconciler::default();
    reconciler.begin(
        "create-profile".into(),
        Scope::request(&json!({"command":"profile_create"})),
    );
    let unrelated = fact("game:game-1", "installed");
    let unrelated_key = unrelated.event.event_key.clone();
    assert!(reconciler
        .observe(batch(
            Instant::now(),
            vec![fact("profile:new-profile", "revision-1"), unrelated],
            &[]
        ))
        .iter()
        .any(|event| event.event_key == unrelated_key));
    reconciler.finish(
        "create-profile",
        expected(&[("profile:new-profile", "revision-1")]),
    );
    assert!(reconciler.ready().is_empty());
}

#[test]
fn failed_or_abandoned_actions_release_any_deferred_observations() {
    for abandon in [false, true] {
        let mut reconciler = Reconciler::default();
        reconciler.begin(
            "action".into(),
            Scope::request(&json!({"command":"companion_revoke","companionId":"tablet"})),
        );
        let observed = fact("companion:tablet", "changed");
        let observed_key = observed.event.event_key.clone();
        assert!(reconciler
            .observe(batch(Instant::now(), vec![observed], &[]))
            .is_empty());
        if abandon {
            reconciler.abandon("action");
        } else {
            reconciler.finish("action", BTreeMap::new());
        }
        assert_eq!(
            reconciler
                .ready()
                .iter()
                .map(|event| event.event_key.clone())
                .collect::<Vec<_>>(),
            [observed_key]
        );
    }
}

#[test]
fn captures_started_before_finish_cannot_expire_a_terminal_claim() {
    let mut reconciler = Reconciler::default();
    reconciler.begin(
        "remove".into(),
        Scope::request(&json!({"command":"controller_forget","sourceId":"hid-1"})),
    );
    let started_before_finish = Instant::now();
    reconciler.finish("remove", expected(&[("controller:hid-1", "removed")]));
    assert!(reconciler
        .observe(batch(
            started_before_finish,
            vec![],
            &[("controller:hid-1", "old-state")]
        ))
        .is_empty());

    assert!(reconciler
        .observe(batch(
            Instant::now(),
            vec![fact("controller:hid-1", "removed")],
            &[]
        ))
        .is_empty());
    assert!(reconciler.ready().is_empty());
}

#[test]
fn one_action_receipt_can_reconcile_multiple_entities() {
    let mut reconciler = Reconciler::default();
    reconciler.begin(
        "visibility".into(),
        Scope::request(
            &json!({"command":"controller_visibility_restore","sourceIds":["hid-1","hid-2"]}),
        ),
    );
    reconciler.finish(
        "visibility",
        expected(&[
            ("controller:hid-1", "visible"),
            ("controller:hid-2", "visible"),
        ]),
    );
    let facts = vec![
        fact("controller:hid-1", "visible"),
        fact("controller:hid-2", "visible"),
    ];
    let canonical = event(
        "devices",
        "controller_visibility_restore",
        "info",
        "Controllers restored".into(),
        None,
        None,
    );
    assert!(reconciler
        .observe(batch(Instant::now(), facts, &[]))
        .is_empty());
    assert_eq!(canonical.event_type, "controller_visibility_restore");
}

#[test]
fn only_profile_session_receipts_hold_lifecycle_watermarks() {
    let mut reconciler = Reconciler::default();
    reconciler.begin(
        "metadata".into(),
        Scope::request(&json!({"command":"profile_editor_save","document":{"id":"profile-1"}})),
    );
    assert!(!reconciler.watches_session());
    reconciler.finish("metadata", expected(&[("profile:profile-1", "revision-2")]));
    assert!(!reconciler.watches_session());

    reconciler.begin(
        "apply".into(),
        Scope::request(&json!({"command":"profile_apply"})),
    );
    assert!(reconciler.watches_session());
    reconciler.finish("apply", expected(&[("profile_session:current", "running")]));
    assert!(reconciler.watches_session());
    let _ = reconciler.observe(batch(
        Instant::now(),
        vec![fact("profile_session:current", "running")],
        &[],
    ));
    assert!(!reconciler.watches_session());
}
