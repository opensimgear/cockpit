use super::{CapturedMemory, Snapshot, Spec, State};
use crate::dcs_bios::{
    Block, ByteCapture, CapturePosition, CapturedBlock, CapturedDatagram, IntegerOutput,
};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn integer(address: u16, mask: u16, maximum: u16) -> IntegerOutput {
    IntegerOutput::parse(&json!({"type":"integer", "address":address, "mask":mask,
        "shift_by":0, "max_value":maximum}))
    .unwrap()
}

fn spec() -> Spec {
    Spec {
        counter: integer(0xfffe, 255, 255),
        aircraft_address: 0,
        aircraft_length: 8,
        freshness_ms: 100,
    }
}

fn wide_counter_spec() -> Spec {
    Spec {
        counter: integer(0xfffe, 65535, 65535),
        aircraft_address: 0,
        aircraft_length: 8,
        freshness_ms: 100,
    }
}

fn block(bytes: &mut Vec<u8>, address: u16, data: &[u8]) {
    bytes.extend(address.to_le_bytes());
    bytes.extend((data.len() as u16).to_le_bytes());
    bytes.extend(data);
}

fn frame(name: Option<&str>, output: Option<u16>, counter: u16) -> Vec<u8> {
    let mut bytes = vec![0x55; 4];
    if let Some(name) = name {
        let mut header = [b' '; 8];
        header[..name.len()].copy_from_slice(name.as_bytes());
        block(&mut bytes, 0, &header);
    }
    if let Some(output) = output {
        block(&mut bytes, 8192, &output.to_le_bytes());
    }
    block(&mut bytes, 0xfffe, &counter.to_le_bytes());
    bytes
}

fn report(bytes: Vec<u8>, at: Instant, sequence: u64) -> CapturedDatagram {
    CapturedDatagram {
        bytes: Arc::from(bytes),
        captured_at: at,
        producer_id: "wire".into(),
        sequence,
    }
}

fn feed(state: &mut State, bytes: Vec<u8>, at: Instant, sequence: u64) -> Vec<Snapshot> {
    state.feed(&report(bytes, at, sequence), at).unwrap()
}

fn capture(snapshot: &Snapshot, address: u16) -> ByteCapture {
    snapshot.memory.capture(address).unwrap()
}

#[test]
fn split_value_keeps_each_original_datagram_capture() {
    let t1 = Instant::now();
    let t2 = t1 + Duration::from_millis(5);
    let mut state = State::new(spec(), "owner".into());
    let bytes = frame(Some("Jet"), Some(0x1234), 1);
    let split = 21; // Between the two payload bytes at address 8192.
    assert!(feed(&mut state, bytes[..split].to_vec(), t1, 1).is_empty());
    let snapshots = feed(&mut state, bytes[split..].to_vec(), t2, 2);
    let decoded = snapshots.last().unwrap();

    let low = capture(decoded, 8192);
    let high = capture(decoded, 8193);
    assert_eq!(decoded.memory.bytes[8192..8194], [0x34, 0x12]);
    assert_eq!(
        low.position,
        CapturePosition {
            report_sequence: 1,
            byte_offset: 20
        }
    );
    assert_eq!(low.captured_at, t1);
    assert_eq!(
        high.position,
        CapturePosition {
            report_sequence: 2,
            byte_offset: 0
        }
    );
    assert_eq!(high.captured_at, t2);
}

#[test]
fn carried_header_keeps_prior_marks_and_published_snapshot_is_immutable() {
    let at = Instant::now();
    let later = at + Duration::from_millis(1);
    let mut state = State::new(spec(), "owner".into());
    let old = feed(&mut state, frame(Some("Jet"), Some(7), 1), at, 1)
        .pop()
        .unwrap();
    let mut sparse = vec![0x55; 4];
    block(&mut sparse, 0, b"Ne");
    block(&mut sparse, 0xfffe, &[2, 0]);
    let current = feed(&mut state, sparse, later, 2).pop().unwrap();

    assert_eq!(capture(&old, 0).position.report_sequence, 1);
    assert_eq!(capture(&old, 4).position.report_sequence, 1);
    assert_eq!(capture(&current, 0).position.report_sequence, 2);
    assert_eq!(capture(&current, 1).position.report_sequence, 2);
    assert_eq!(capture(&current, 2), capture(&old, 2));
    assert_eq!(capture(&current, 3), capture(&old, 3));
    assert_eq!(capture(&current, 4), capture(&old, 4));
    assert_eq!(capture(&current, 7), capture(&old, 7));
    assert_eq!(old.memory.string(0, 8).as_deref(), Some("Jet"));
    assert_eq!(current.memory.string(0, 8).as_deref(), Some("Net"));
}

#[test]
fn repeated_addresses_in_one_packet_keep_distinct_byte_offsets() {
    let at = Instant::now();
    let first = frame(Some("Jet"), Some(1), 1);
    let second = frame(None, Some(2), 2);
    let first_len = first.len();
    let mut packet = first;
    packet.extend(second);
    let mut state = State::new(spec(), "owner".into());
    let snapshots = feed(&mut state, packet, at, 1);
    assert_eq!(snapshots.len(), 2);

    let first_mark = capture(&snapshots[0], 8192);
    let second_mark = capture(&snapshots[1], 8192);
    assert_eq!(first_mark.position.report_sequence, 1);
    assert_eq!(second_mark.position.report_sequence, 1);
    assert_ne!(
        first_mark.position.byte_offset,
        second_mark.position.byte_offset
    );
    assert_eq!(second_mark.position.byte_offset, first_len as u32 + 8);
}

#[test]
fn duplicate_bios_counter_cannot_replace_output_capture_marks() {
    let at = Instant::now();
    let later = at + Duration::from_millis(10);
    let mut state = State::new(spec(), "owner".into());
    feed(&mut state, frame(Some("Jet"), Some(11), 1), at, 1);
    feed(&mut state, frame(None, None, 2), at, 2);
    let before = state.snapshot();
    let old_mark = capture(&before, 8192);

    assert!(feed(&mut state, frame(None, Some(99), 2), later, 3).is_empty());
    let after = state.snapshot();
    assert_eq!(after.memory.bytes[8192..8194], [11, 0]);
    assert_eq!(capture(&after, 8192), old_mark);
    assert_eq!(capture(&before, 8192), old_mark);
}

#[test]
fn buffered_counter_byte_keeps_its_original_time_and_does_not_extend_expiry() {
    let t1 = Instant::now();
    let t2 = t1 + Duration::from_millis(10);
    let t3 = t1 + Duration::from_millis(20);
    let t4 = t2 + Duration::from_millis(101);
    let mut state = State::new(wide_counter_spec(), "owner".into());

    // Each counter's high byte is 0x55. It remains buffered as a possible
    // sync marker until the next report starts with another sync run.
    assert!(feed(&mut state, frame(Some("Jet"), Some(1), 0x5501), t1, 1).is_empty());
    let first = feed(&mut state, frame(None, None, 0x5502), t2, 2);
    assert_eq!(
        first
            .last()
            .unwrap()
            .counter_observation
            .as_ref()
            .unwrap()
            .captured_at,
        t1
    );
    let second = feed(&mut state, frame(None, None, 0x5503), t3, 3);
    let live = second.last().unwrap();
    assert_eq!(live.status, "live");
    assert_eq!(live.counter_observation.as_ref().unwrap().captured_at, t2);

    let expired = feed(&mut state, frame(None, None, 0x5504), t4, 4);
    let stale = expired.last().unwrap();
    assert_eq!(stale.status, "stale");
    assert_eq!(stale.captured_at(), Some(t2));
    assert!(!stale.fresh_at(t4));
}

#[test]
fn full_capture_memory_and_snapshot_budget_fit_native_four_mib_limit() {
    let mut memory = CapturedMemory::new("wire".into());
    let blocks: Vec<_> = (0..65_536usize)
        .step_by(256)
        .map(|start| {
            let data = vec![0; 256];
            let captures = (0..256)
                .map(|offset| ByteCapture {
                    position: CapturePosition {
                        report_sequence: 1,
                        byte_offset: (start + offset) as u32,
                    },
                    captured_at: Instant::now(),
                })
                .collect();
            CapturedBlock::new(
                Block {
                    address: start as u16,
                    data,
                },
                captures,
            )
            .unwrap()
        })
        .collect();
    memory.apply(&blocks).unwrap();
    assert!(memory.valid.iter().all(|valid| *valid));
    assert!(memory.retained_bytes() <= CapturedMemory::max_retained_bytes());

    let max_snapshot = CapturedMemory::max_retained_bytes()
        + std::mem::size_of::<Snapshot>()
        + 2 * std::mem::size_of::<usize>()
        + (3 * 512) // producer id, counter acquisition id, and bounded error
        + 8; // aircraft identity width from this spec
    assert!(max_snapshot <= 4 * 1024 * 1024);
}
