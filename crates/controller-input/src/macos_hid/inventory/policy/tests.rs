use super::*;
use crate::hid::restrictions::reasons;

const GAME: &[u8] = &[
    0x05, 1, 0x09, 4, 0xa1, 1, 0x15, 0, 0x25, 0x7f, 0x75, 8, 0x95, 1, 0x09, 0x30, 0x81, 2, 0xc0,
];

#[test]
fn descriptor_policy_allows_generic_game_and_vendor_inputs() {
    assert!(reasons(GAME).is_empty());
    let mut vendor = GAME.to_vec();
    vendor.splice(0..2, [0x06, 0, 0xff]);
    assert!(reasons(&vendor).is_empty());
}

#[test]
fn nested_pointer_application_is_restricted_but_axis_grouping_is_allowed() {
    for kind in [0, 1, 2] {
        let mut descriptor = GAME[..6].to_vec();
        descriptor.extend([0x09, 1, 0xa1, kind]);
        descriptor.extend(&GAME[6..GAME.len() - 1]);
        descriptor.extend([0xc0, 0xc0]);
        assert_eq!(!reasons(&descriptor).is_empty(), kind == 1, "kind {kind}");
    }
}

#[test]
fn vendor_collection_cannot_hide_keyboard_or_consumer_fields() {
    for page in [7, 0x0c] {
        let mut descriptor = GAME[..6].to_vec();
        descriptor.extend([0x05, page]);
        descriptor.extend(&GAME[6..]);
        assert!(!reasons(&descriptor).is_empty());
    }
}

#[test]
fn malformed_descriptor_and_ambiguous_identity_are_metadata_only() {
    assert!(!reasons(&[0xa1]).is_empty());
    assert!(!reasons(&[0x75, 8, 0x95, 1, 0x81, 2]).is_empty());
}

#[test]
fn native_restrictions_do_not_restrict_ordinary_axes_or_buttons() {
    for (page, usage) in [
        (1, 1),
        (1, 2),
        (1, 6),
        (1, 7),
        (7, 4),
        (0x0c, 1),
        (1, 0x81),
        (0x84, 1),
    ] {
        assert!(native_restricted(page, usage));
    }
    for (page, usage) in [(1, 4), (1, 0x30), (9, 1), (0xff00, 1)] {
        assert!(!native_restricted(page, usage));
    }
}
