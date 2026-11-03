use super::*;
const MAX_FRAME_EVENTS: usize = 16;
use crate::dcs_bios::IntegerOutput;
use serde_json::json;
use std::time::Duration;

fn spec() -> Spec {
    Spec {
        counter: integer(0xfffe, 255, 255),
        aircraft_address: 0,
        aircraft_length: 8,
        freshness_ms: 100,
    }
}
#[test]
fn decoded_datagram_batch_is_bounded_before_snapshots_can_grow_without_limit() {
    let at = Instant::now();
    for count in [MAX_FRAME_EVENTS, MAX_FRAME_EVENTS + 1] {
        let mut state = State::new(spec(), "bounded".into());
        let bytes = (1..=count)
            .flat_map(|counter| frame(Some("Jet"), Some(counter as u16), counter as u16))
            .collect();
        let result = state.feed(&report(bytes, at, "wire", 1), at);
        if count == MAX_FRAME_EVENTS {
            assert_eq!(result.unwrap().len(), MAX_FRAME_EVENTS);
            assert!(state.snapshot().fresh_at(at));
        } else {
            assert!(result.unwrap_err().contains("16-frame bound"));
            assert!(!state.snapshot().fresh_at(at));
            assert!(state.snapshot().memory.valid.iter().all(|valid| !valid));
        }
    }
}
fn integer(address: u16, mask: u16, maximum: u16) -> IntegerOutput {
    IntegerOutput::parse(&json!({"type":"integer","address":address,"mask":mask,
        "shift_by":0,"max_value":maximum}))
    .unwrap()
}
fn block(bytes: &mut Vec<u8>, address: u16, data: &[u8]) {
    bytes.extend(address.to_le_bytes());
    bytes.extend((data.len() as u16).to_le_bytes());
    bytes.extend(data);
}
// Authored wire fixture, including the real sync/address/count framing.
fn frame(name: Option<&str>, value: Option<u16>, counter: u16) -> Vec<u8> {
    let mut bytes = vec![0x55; 4];
    if let Some(name) = name {
        let mut header = [b' '; 8];
        header[..name.len()].copy_from_slice(name.as_bytes());
        block(&mut bytes, 0, &header);
    }
    if let Some(value) = value {
        block(&mut bytes, 8192, &value.to_le_bytes());
    }
    block(&mut bytes, 0xfffe, &counter.to_le_bytes());
    bytes
}
fn report(bytes: Vec<u8>, at: Instant, producer: &str, sequence: u64) -> CapturedDatagram {
    CapturedDatagram {
        bytes: bytes.into(),
        captured_at: at,
        producer_id: producer.into(),
        sequence,
    }
}
fn feed(state: &mut State, bytes: Vec<u8>, at: Instant, sequence: u64) -> Snapshot {
    state
        .feed(&report(bytes, at, "wire", sequence), at)
        .unwrap();
    state.snapshot()
}
fn live(at: Instant) -> State {
    let mut state = State::new(spec(), "canonical-owner".into());
    assert_eq!(
        feed(&mut state, frame(Some("Jet"), Some(11), 1), at, 1).status,
        "waiting"
    );
    assert_eq!(feed(&mut state, frame(None, None, 2), at, 2).status, "live");
    state
}
fn value(snapshot: &Snapshot) -> Option<u16> {
    integer(8192, 65535, 65535).read(&snapshot.memory)
}

#[test]
fn waits_for_identity_and_consecutive_progress_then_preserves_native_values() {
    let at = Instant::now();
    let mut state = State::new(spec(), "canonical-owner".into());
    let baseline = feed(&mut state, frame(Some("Jet"), Some(0), 255), at, 1);
    assert_eq!(baseline.status, "waiting");
    assert!(!baseline.fresh_at(at));
    let current = feed(&mut state, frame(None, None, 0), at, 2);
    assert!(current.fresh_at(at));
    assert_eq!(current.producer_id, "canonical-owner");
    assert_eq!(
        current
            .counter_observation
            .as_ref()
            .map(|o| o.position.report_sequence),
        Some(2)
    );
    assert_eq!(value(&current), Some(0));
    assert_eq!(current.counter, Some(0));
    let maximum = feed(&mut state, frame(None, Some(65535), 1), at, 3);
    assert_eq!(value(&maximum), Some(65535));
    assert_eq!(value(&current), Some(0));
    assert_eq!(maximum.epoch, current.epoch);
}

#[test]
fn bios_duplicates_never_change_memory_or_renew_native_capture() {
    let at = Instant::now();
    let mut state = live(at);
    let before = state.snapshot();
    let later = at + Duration::from_millis(50);
    assert!(state
        .feed(
            &report(frame(Some("Other"), Some(99), 2), later, "wire", 3),
            later
        )
        .unwrap()
        .is_empty());
    assert!(state
        .feed(&report(frame(None, None, 2), later, "wire", 4), later)
        .unwrap()
        .is_empty());
    let duplicate = state.snapshot();
    assert_eq!(duplicate.aircraft, before.aircraft);
    assert_eq!(duplicate.epoch, before.epoch);
    assert_eq!(duplicate.captured_at(), before.captured_at());
    assert_eq!(
        duplicate
            .counter_observation
            .as_ref()
            .map(|o| o.position.report_sequence),
        before
            .counter_observation
            .as_ref()
            .map(|o| o.position.report_sequence)
    );
    assert_eq!(value(&duplicate), Some(11));
    // The separate raw transport cursor did accept both reports. A subsequent
    // advancing frame must not look like a transport gap and lose continuity.
    let advancing = feed(&mut state, frame(None, Some(22), 3), later, 5);
    assert_eq!(
        advancing
            .counter_observation
            .as_ref()
            .map(|o| o.position.report_sequence),
        Some(5)
    );
    assert_eq!(advancing.status, "live");
    assert_eq!(advancing.epoch, before.epoch);
}

#[test]
fn duplicate_shutdown_explicitly_withdraws_aircraft_without_accepting_replay() {
    let at = Instant::now();
    for name in ["", "NONE"] {
        let mut state = live(at);
        let before = state.snapshot();
        let published = state
            .feed(&report(frame(Some(name), None, 2), at, "wire", 3), at)
            .unwrap();
        assert_eq!(published.len(), 1);
        let current = &published[0];
        assert_eq!(current.aircraft, None);
        assert_eq!(current.counter, Some(2));
        assert!(current.epoch > before.epoch);
        assert_eq!(value(&current), None);
        assert_eq!(current.status, "stopped");
        assert_eq!(current.captured_at(), Some(at));
        assert!(!current.fresh_at(at));
        assert!(current.deadline().is_none());
        let replay = feed(&mut state, frame(Some("Jet"), Some(99), 2), at, 4);
        assert_eq!(replay.aircraft, None);
        assert_eq!(value(&replay), None);
    }
}

#[test]
fn duplicate_and_late_transport_reports_are_ignored() {
    let at = Instant::now();
    let mut state = live(at);
    let before = state.snapshot();
    for sequence in [2, 1] {
        assert!(state
            .feed(
                &report(frame(Some("Other"), Some(99), 3), at, "wire", sequence),
                at
            )
            .unwrap()
            .is_empty());
    }
    assert_eq!(
        state
            .snapshot()
            .counter_observation
            .as_ref()
            .map(|o| o.position.report_sequence),
        before
            .counter_observation
            .as_ref()
            .map(|o| o.position.report_sequence)
    );
    assert_eq!(state.snapshot().captured_at(), before.captured_at());
    assert_eq!(value(&state.snapshot()), Some(11));
}

#[test]
fn transport_gap_and_replacement_require_a_new_baseline() {
    let at = Instant::now();
    for (producer, sequence) in [("wire", 4), ("replacement", 1)] {
        let mut state = live(at);
        let publications = state
            .feed(
                &report(frame(Some("Jet"), Some(33), 3), at, producer, sequence),
                at,
            )
            .unwrap();
        assert!(publications.len() >= 2);
        assert_eq!(value(&publications[0]), None);
        let baseline = publications.last().unwrap();
        assert_eq!(baseline.status, "waiting");
        assert_eq!(value(baseline), Some(33));
        assert_eq!(baseline.producer_id, "canonical-owner");
        let next = state
            .feed(
                &report(frame(None, None, 4), at, producer, sequence + 1),
                at,
            )
            .unwrap();
        assert_eq!(next.last().unwrap().status, "live");
    }
}

#[test]
fn bios_forward_gap_never_inherits_sparse_outputs_or_old_header() {
    let at = Instant::now();
    let mut state = live(at);
    let before = state.snapshot();
    let gap = feed(&mut state, frame(None, Some(44), 4), at, 3);
    assert!(gap.epoch > before.epoch);
    assert_eq!(gap.aircraft, None);
    assert_eq!(gap.status, "waiting");
    let identity = feed(&mut state, frame(Some("Jet"), None, 5), at, 4);
    assert_eq!(value(&identity), None);
    assert_eq!(identity.status, "waiting");
    assert_eq!(feed(&mut state, frame(None, None, 6), at, 5).status, "live");
}

#[test]
fn complete_forward_gap_is_baseline_and_cannot_reuse_pre_gap_values() {
    let at = Instant::now();
    let mut state = live(at);
    let baseline = feed(&mut state, frame(Some("Jet"), None, 129), at, 3);
    assert_eq!(baseline.status, "waiting");
    assert_eq!(baseline.aircraft.as_deref(), Some("Jet"));
    assert_eq!(value(&baseline), None);
    assert_eq!(
        feed(&mut state, frame(None, None, 130), at, 4).status,
        "live"
    );
}

#[test]
fn backward_or_half_range_counter_discards_the_entire_frame() {
    let at = Instant::now();
    for counter in [1, 130] {
        let mut state = live(at);
        let reset = feed(&mut state, frame(Some("Other"), Some(99), counter), at, 3);
        assert_eq!(reset.aircraft, None);
        assert_eq!(reset.counter, None);
        assert_eq!(reset.captured_at(), None);
        assert_eq!(value(&reset), None);
        assert_eq!(
            feed(&mut state, frame(Some("Jet"), None, 3), at, 4).status,
            "waiting"
        );
        assert_eq!(feed(&mut state, frame(None, None, 4), at, 5).status, "live");
    }
}

#[test]
fn partial_identity_can_complete_but_requires_its_own_progress_baseline() {
    let at = Instant::now();
    let mut state = State::new(spec(), "owner".into());
    let mut first = vec![0x55; 4];
    block(&mut first, 0, b"Jet ");
    block(&mut first, 8192, &[7, 0]);
    block(&mut first, 0xfffe, &[1, 0]);
    assert_eq!(feed(&mut state, first, at, 1).aircraft, None);
    let mut second = vec![0x55; 4];
    block(&mut second, 4, b"    ");
    block(&mut second, 0xfffe, &[2, 0]);
    let identity = feed(&mut state, second, at, 2);
    assert_eq!(identity.aircraft.as_deref(), Some("Jet"));
    assert_eq!(identity.status, "waiting");
    assert_eq!(value(&identity), None);
    assert_eq!(feed(&mut state, frame(None, None, 3), at, 3).status, "live");
}

#[test]
fn aircraft_change_keeps_only_current_complete_candidate_header_and_current_blocks() {
    let at = Instant::now();
    let mut state = live(at);
    let old = state.snapshot();
    let mut changed = vec![0x55; 4];
    block(&mut changed, 0, b"New ");
    block(&mut changed, 8194, &[9, 0]);
    block(&mut changed, 0xfffe, &[3, 0]);
    let current = feed(&mut state, changed, at, 3);
    assert_eq!(current.aircraft.as_deref(), Some("New"));
    assert_eq!(current.memory.string(0, 8).as_deref(), Some("New"));
    assert_eq!(value(&current), None);
    assert_eq!(integer(8194, 65535, 65535).read(&current.memory), Some(9));
    assert_eq!(current.status, "waiting");
    assert!(current.epoch > old.epoch);
    assert_eq!(value(&old), Some(11));
    assert_eq!(feed(&mut state, frame(None, None, 4), at, 4).status, "live");
}

#[test]
fn datagram_batch_and_fragmented_frames_publish_atomic_immutable_memories() {
    let at = Instant::now();
    let mut state = State::new(spec(), "owner".into());
    let first = frame(Some("Jet"), Some(1), 1);
    let cut = first.len() - 1;
    assert!(state
        .feed(&report(first[..cut].into(), at, "wire", 1), at)
        .unwrap()
        .is_empty());
    let mut bytes = first[cut..].to_vec();
    bytes.extend(frame(None, Some(2), 2));
    let snapshots = state.feed(&report(bytes, at, "wire", 2), at).unwrap();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].status, "waiting");
    assert_eq!(snapshots[1].status, "live");
    assert_eq!(
        snapshots[0]
            .counter_observation
            .as_ref()
            .map(|o| o.position.report_sequence),
        Some(2)
    );
    assert_eq!(value(&snapshots[0]), Some(1));
    assert_eq!(value(&snapshots[1]), Some(2));
    feed(&mut state, frame(None, Some(3), 3), at, 3);
    assert_eq!(value(&snapshots[0]), Some(1));
    assert_eq!(value(&snapshots[1]), Some(2));
}

#[test]
fn native_timestamp_defines_exact_once_only_expiry_and_frozen_replay_is_rejected() {
    let at = Instant::now();
    let mut state = live(at);
    let captured = at;
    assert_eq!(
        state.snapshot().deadline(),
        Some(captured + Duration::from_millis(101))
    );
    assert!(state
        .expire(captured + Duration::from_millis(100))
        .unwrap()
        .is_none());
    let stale = state
        .expire(captured + Duration::from_millis(101))
        .unwrap()
        .unwrap();
    assert_eq!(stale.status, "stale");
    assert_eq!(stale.aircraft.as_deref(), Some("Jet"));
    assert_eq!(stale.counter, Some(2));
    assert_eq!(value(&stale), None);
    assert_eq!(stale.captured_at(), Some(captured));
    assert_eq!(
        stale.age_ms(captured + Duration::from_millis(101)),
        Some(101)
    );
    assert_eq!(stale.deadline(), None);
    assert!(!stale.fresh_at(captured + Duration::from_millis(101)));
    assert!(state
        .expire(captured + Duration::from_secs(5))
        .unwrap()
        .is_none());
    let later = captured + Duration::from_secs(5);
    assert!(state
        .feed(
            &report(frame(Some("Jet"), Some(88), 2), later, "wire", 3),
            later
        )
        .unwrap()
        .is_empty());
    let replay = state.snapshot();
    assert_eq!(replay.status, "stale");
    assert_eq!(replay.epoch, stale.epoch);
    assert_eq!(replay.captured_at(), Some(captured));
    assert_eq!(
        replay
            .counter_observation
            .as_ref()
            .map(|o| o.position.report_sequence),
        stale
            .counter_observation
            .as_ref()
            .map(|o| o.position.report_sequence)
    );
    assert_eq!(replay.age_ms(later), Some(5000));
    assert_eq!(value(&replay), None);
    assert_eq!(
        feed(&mut state, frame(Some("Jet"), Some(33), 3), later, 4).status,
        "waiting"
    );
    assert_eq!(
        feed(&mut state, frame(None, None, 4), later, 5).status,
        "live"
    );
}

#[test]
fn queued_and_future_capture_cannot_revive_or_rebase_frozen_state() {
    let at = Instant::now();
    for capture in [at, at + Duration::from_secs(2)] {
        let mut state = live(at);
        let now = at + Duration::from_secs(1);
        state
            .feed(
                &report(frame(Some("Jet"), Some(99), 3), capture, "wire", 3),
                now,
            )
            .unwrap();
        let rejected = state.snapshot();
        assert!(!rejected.fresh_at(now));
        assert_eq!(rejected.captured_at(), None);
        assert_eq!(rejected.counter, Some(2));
        assert_eq!(value(&rejected), None);
    }
}

#[test]
fn captured_age_is_not_the_time_a_consumer_reads_the_frame() {
    let at = Instant::now();
    let mut state = live(at);
    let capture = at + Duration::from_millis(10);
    let observed = at + Duration::from_millis(90);
    state
        .feed(
            &report(frame(None, Some(3), 3), capture, "wire", 3),
            observed,
        )
        .unwrap();
    assert_eq!(state.snapshot().captured_at(), Some(capture));
    assert_eq!(state.snapshot().age_ms(observed), Some(80));
    assert!(state.snapshot().fresh_at(observed));
}

#[test]
fn malformed_frames_withdraw_values_and_preserve_prior_snapshot() {
    let at = Instant::now();
    let mut state = live(at);
    let old = state.snapshot();
    let malformed = feed(&mut state, vec![0x55, 0x55, 0x55, 0x55, 1, 0, 2, 0], at, 3);
    assert_eq!(malformed.malformed_frames, 1);
    assert_eq!(malformed.aircraft, None);
    assert_eq!(malformed.counter, None);
    assert_eq!(value(&malformed), None);
    assert_eq!(value(&old), Some(11));
    assert!(malformed.epoch > old.epoch);
    let mut invalid_name = vec![0x55; 4];
    block(&mut invalid_name, 0, &[0xff; 8]);
    block(&mut invalid_name, 0xfffe, &[3, 0]);
    assert_eq!(feed(&mut state, invalid_name, at, 4).malformed_frames, 2);
}

#[test]
fn terminal_publication_error_and_epoch_exhaustion_leave_no_fresh_memory() {
    let at = Instant::now();
    let mut state = live(at);
    let invalid = state.invalidate("publication failed").unwrap();
    assert_eq!(invalid.status, "error");
    assert_eq!(invalid.error.as_deref(), Some("publication failed"));
    assert_eq!(value(&invalid), None);
    assert_eq!(invalid.captured_at(), None);
    assert!(state
        .feed(&report(frame(Some("Jet"), Some(99), 3), at, "wire", 3), at)
        .is_err());
    let mut exhausted = live(at);
    exhausted.snapshot.epoch = u64::MAX;
    assert_eq!(exhausted.invalidate("failed").err().unwrap(), EXHAUSTED);
    assert_eq!(exhausted.snapshot().status, "error");
    assert_eq!(value(&exhausted.snapshot()), None);
    assert_eq!(exhausted.snapshot().captured_at(), None);
}
