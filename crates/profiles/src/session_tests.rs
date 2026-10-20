use super::*;
use crate::{new_document, DEFAULT_PROFILE_ID};

#[test]
fn selected_draft_and_captured_snapshot_are_independent() {
    let mut session = Session::default();
    let mut draft = new_document("Flight").unwrap();
    session.select(&draft.id, SelectionMode::Manual);
    session.capture_selected(&draft).unwrap();
    let frozen = session.snapshot().unwrap().document().clone();
    draft.name = "Edited after capture".into();
    draft.revision = 2;
    draft.eligibility.game_ids.push("dcs".into());
    assert_eq!(session.snapshot().unwrap().document(), &frozen);
    assert_eq!(
        session.status().applied_snapshot.as_ref().unwrap().revision,
        1
    );
    assert_eq!(session.status().state, SessionState::Stopped);
}

#[test]
fn automatic_selection_never_clears_fault() {
    let mut session = Session::default();
    let document = new_document("Flight").unwrap();
    assert!(!session.select(&document.id, SelectionMode::Automatic));
    assert_ne!(session.status().selected_profile_id, document.id);
    session.set_mode(SelectionMode::Automatic);
    assert!(session.select(&document.id, SelectionMode::Automatic));
    session.capture_selected(&document).unwrap();
    session.fault("Output unavailable".into());
    session.select(DEFAULT_PROFILE_ID, SelectionMode::Automatic);
    assert_eq!(session.status().state, SessionState::Faulted);
    assert!(session.capture_selected(&default_document()).is_err());
}

#[test]
fn capture_rejects_a_different_selected_identity() {
    let mut session = Session::default();
    assert!(session
        .capture_selected(&new_document("Other").unwrap())
        .is_err());
    assert!(session.snapshot().is_none());
    session.capture_selected(&default_document()).unwrap();
    assert_eq!(
        session.snapshot().unwrap().summary().profile_id,
        DEFAULT_PROFILE_ID
    );
}

#[test]
fn removed_selection_falls_back_without_rearming() {
    let mut session = Session::default();
    let document = new_document("Flight").unwrap();
    session.select(&document.id, SelectionMode::Manual);
    session.fault("Output unavailable".into());
    session.fallback_if_removed("another-id");
    assert_eq!(session.status().selected_profile_id, document.id);
    session.fallback_if_removed(&document.id);
    assert_eq!(session.status().selected_profile_id, DEFAULT_PROFILE_ID);
    assert_eq!(session.status().state, SessionState::Faulted);
}

#[test]
fn manual_selection_pins_until_automatic_mode_is_explicitly_resumed() {
    let mut session = Session::default();
    let manual = new_document("Manual choice").unwrap();
    let automatic = new_document("Detected game").unwrap();
    session.set_mode(SelectionMode::Automatic);
    session.select(&manual.id, SelectionMode::Manual);
    assert_eq!(session.status().selection_mode, SelectionMode::Manual);
    assert!(!session.select(&automatic.id, SelectionMode::Automatic));
    assert_eq!(session.status().selected_profile_id, manual.id);
    session.set_mode(SelectionMode::Automatic);
    assert!(session.select(&automatic.id, SelectionMode::Automatic));
    assert_eq!(session.status().selected_profile_id, automatic.id);
    assert_eq!(session.status().state, SessionState::Stopped);
}
