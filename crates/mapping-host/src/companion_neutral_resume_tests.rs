use super::*;
use osg_mapping::engine::Reason;

fn definition(id: &str) -> Definition {
    Definition { source_id: id.into(), name: id.into(), publication_hash: format!("sha256:{}", "a".repeat(64)),
        controls: vec![ControlDefinition { id: "button".into(), kind: Kind::Button, fallback: Value::Button(false) }] }
}
fn token(id: &str) -> InputToken {
    InputToken { source_id: id.into(), session_id: format!("session-{id}"), generation: 1,
        publication_hash: definition(id).publication_hash }
}
fn setup(ids: &[&str]) -> (CompanionProvider, NativeFeed) {
    let provider = CompanionProvider::new(Arc::new(|| 100));
    for id in ids { provider.register_definition(definition(id)).unwrap(); }
    let mut feed = provider.subscribe_native(&ids.iter().map(|id| NativeSelection {
        definition: definition(id), controls: vec!["button".into()] }).collect::<Vec<_>>()).unwrap();
    for id in ids { provider.baseline_native(&token(id), 0,
        &[Change { control: "button".into(), value: Value::Button(false) }]).unwrap(); }
    feed.baseline().unwrap();
    (provider, feed)
}
fn change(provider: &CompanionProvider, id: &str, sequence: u64, held: bool) {
    provider.changes_native(&token(id), sequence,
        &[Change { control: "button".into(), value: Value::Button(held) }]).unwrap();
}

#[test]
fn held_control_waits_then_actual_neutral_recapture_discards_unchecked_press() {
    let (provider, mut feed) = setup(&["left"]);
    change(&provider, "left", 1, true);
    assert!(feed.try_neutral_baseline_audited().unwrap().is_none());
    change(&provider, "left", 2, false);
    let baseline = feed.try_neutral_baseline_audited().unwrap().unwrap();
    assert_eq!(baseline.len(), 1);
    assert_eq!(baseline[0].frame.reason, Reason::Baseline);
    assert_eq!(baseline[0].frame.changes[0].value, Value::Button(false));
    assert!(feed.poll().unwrap().is_empty(), "Unchecked press must never replay");
    change(&provider, "left", 3, true);
    assert!(feed.poll().unwrap().iter().any(|frame| frame.reason == Reason::Change));
}

#[test]
fn held_first_source_cannot_mask_a_revoked_second_source() {
    let (provider, mut feed) = setup(&["left", "right"]);
    change(&provider, "left", 1, true);
    provider.loss_native(&token("right")).unwrap();
    assert!(feed.try_neutral_baseline_audited().is_err());
}

#[test]
fn native_generation_replacement_cannot_become_a_pause_recovery_baseline() {
    let (provider, mut feed) = setup(&["left"]);
    provider.loss_native(&token("left")).unwrap();
    let mut replacement = token("left"); replacement.generation = 2;
    provider.baseline_native(&replacement, 0,
        &[Change { control: "button".into(), value: Value::Button(false) }]).unwrap();
    assert!(feed.try_neutral_baseline_audited().is_err());
}
