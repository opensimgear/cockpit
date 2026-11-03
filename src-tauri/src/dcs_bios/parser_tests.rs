use super::*;
use std::time::Duration;

fn report(bytes: Vec<u8>, sequence: u64, captured_at: Instant) -> CapturedDatagram {
    CapturedDatagram {
        bytes: bytes.into(),
        sequence,
        captured_at,
        producer_id: "test".into(),
    }
}

fn block(bytes: &mut Vec<u8>, address: u16, data: &[u8]) {
    bytes.extend(address.to_le_bytes());
    bytes.extend((data.len() as u16).to_le_bytes());
    bytes.extend(data);
}

fn frame(data: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x55; 4];
    block(&mut bytes, 0x1000, data);
    block(&mut bytes, 0xfffe, &[1, 0]);
    bytes
}

fn blocks(events: &[CapturedExportEvent]) -> &[CapturedBlock] {
    match &events[0] {
        CapturedExportEvent::Frame(blocks) => blocks,
        CapturedExportEvent::Reset => panic!("expected a frame"),
    }
}

fn position(sequence: u64, offset: u32) -> CapturePosition {
    CapturePosition {
        report_sequence: sequence,
        byte_offset: offset,
    }
}

#[test]
fn payload_retains_three_datagrams_and_does_not_take_the_fourth_eof_time() {
    let start = Instant::now();
    let bytes = frame(&[1, 2, 3, 4, 5, 6]);
    let mut parser = CapturedParser::new(0xfffe);
    let pieces = [&bytes[..9], &bytes[9..11], &bytes[11..14], &bytes[14..]];
    let mut events = Vec::new();
    for (index, bytes) in pieces.into_iter().enumerate() {
        let at = start + Duration::from_secs(index as u64);
        events.extend(
            parser
                .feed(&report(bytes.into(), index as u64 + 1, at))
                .unwrap(),
        );
        if index < 3 {
            assert!(events.is_empty());
        }
    }
    let payload = &blocks(&events)[0];
    assert_eq!(payload.block().data, [1, 2, 3, 4, 5, 6]);
    let expected = [(1, 8), (2, 0), (2, 1), (3, 0), (3, 1), (3, 2)];
    for (capture, (sequence, offset)) in payload.captures().iter().zip(expected) {
        assert_eq!(capture.position, position(sequence, offset));
        assert_eq!(
            capture.captured_at,
            start + Duration::from_secs(sequence - 1)
        );
    }
    assert!(blocks(&events)[1].captures().iter().all(|capture| {
        capture.position.report_sequence == 4
            && capture.captured_at == start + Duration::from_secs(3)
    }));
}

#[test]
fn fragmented_header_does_not_supply_payload_marks() {
    let at = Instant::now();
    let bytes = frame(&[3, 4]);
    let mut parser = CapturedParser::new(0xfffe);
    for (index, bytes) in [&bytes[..5], &bytes[5..7], &bytes[7..8]]
        .into_iter()
        .enumerate()
    {
        assert!(parser
            .feed(&report(bytes.into(), index as u64 + 1, at))
            .unwrap()
            .is_empty());
    }
    let events = parser.feed(&report(bytes[8..].into(), 4, at)).unwrap();
    assert_eq!(blocks(&events)[0].captures()[0].position, position(4, 0));
    assert_eq!(blocks(&events)[0].captures()[1].position, position(4, 1));
}

#[test]
fn one_to_three_pending_sync_candidates_keep_original_payload_marks() {
    let at = Instant::now();
    for pending in 1..=3 {
        let mut parser = CapturedParser::new(0xfffe);
        let mut bytes = vec![0x55; 4];
        bytes.extend([0, 0x10, 4, 0]);
        bytes.extend(vec![0x55; pending]);
        assert!(parser.feed(&report(bytes, 10, at)).unwrap().is_empty());
        let mut rest = vec![7; 4 - pending];
        block(&mut rest, 0xfffe, &[1, 0]);
        let events = parser
            .feed(&report(rest, 11, at + Duration::from_secs(1)))
            .unwrap();
        let payload = &blocks(&events)[0];
        for offset in 0..pending {
            assert_eq!(
                payload.captures()[offset].position,
                position(10, 8 + offset as u32)
            );
            assert_eq!(payload.captures()[offset].captured_at, at);
        }
        assert_eq!(payload.captures()[pending].position, position(11, 0));
    }
}

#[test]
fn four_to_seven_sync_run_uses_earliest_marks_for_leading_payload_bytes() {
    let at = Instant::now();
    for leading in 0..=3 {
        let mut parser = CapturedParser::new(0xfffe);
        let mut first = vec![0x55; 4];
        first.extend([0xfc, 0xff, 4, 0]);
        first.extend(vec![7; 4 - leading]);
        first.extend(vec![0x55; leading + 2]);
        let mut events = parser.feed(&report(first, 20, at)).unwrap();
        let mut next = vec![0x55; 2];
        block(&mut next, 0xfffe, &[2, 0]);
        events.extend(
            parser
                .feed(&report(next, 21, at + Duration::from_secs(1)))
                .unwrap(),
        );
        assert_eq!(events.len(), 2, "run {}", leading + 4);
        let payload = &blocks(&events)[0];
        assert_eq!(payload.block().data.len(), 4);
        for offset in 0..4 {
            assert_eq!(
                payload.captures()[offset].position,
                position(20, 8 + offset as u32)
            );
            assert_eq!(payload.captures()[offset].captured_at, at);
        }
        assert_eq!(
            blocks(&events[1..])[0].captures()[0].position,
            position(21, 6)
        );
    }
}

#[test]
fn metadata_and_repeated_addresses_keep_distinct_payload_offsets_in_one_datagram() {
    let at = Instant::now();
    let mut bytes = vec![0x55; 4];
    block(&mut bytes, 0x1000, &[1, 2]);
    block(&mut bytes, 0x1000, &[3, 4]);
    block(&mut bytes, 0xfffe, &[1, 0]);
    bytes.extend(frame(&[5, 6]));
    let events = CapturedParser::new(0xfffe)
        .feed(&report(bytes, 30, at))
        .unwrap();
    assert_eq!(events.len(), 2);
    let first = blocks(&events);
    assert_eq!(first[0].block().address, first[1].block().address);
    for (payload, offset) in [(&first[0], 8), (&first[1], 14), (&first[2], 20)] {
        assert_eq!(payload.captures()[0].position, position(30, offset));
        assert_eq!(payload.captures()[1].position, position(30, offset + 1));
    }
    assert_eq!(
        blocks(&events[1..])[0].captures()[0].position,
        position(30, 30)
    );
    assert!(position(30, 30) > position(30, 8));
    assert!(position(31, 0) > position(30, u32::MAX));
}

#[test]
fn explicit_reset_clears_payload_and_pending_marker_marks() {
    let at = Instant::now();
    let mut parser = CapturedParser::new(0xfffe);
    let mut incomplete = vec![0x55; 4];
    incomplete.extend([0, 0x10, 4, 0, 1, 0x55, 0x55]);
    assert!(parser.feed(&report(incomplete, 1, at)).unwrap().is_empty());
    parser.reset();
    assert!(parser.core.sync_captures.is_empty());
    assert!(parser.core.captures.is_empty());
    assert_eq!(parser.core.sync, 0);
    assert!(parser
        .feed(&report(vec![0x55, 0, 0, 0, 0], 2, at))
        .unwrap()
        .is_empty());
    let events = parser.feed(&report(frame(&[8, 9]), 3, at)).unwrap();
    assert_eq!(events.len(), 1);
    assert!(blocks(&events)
        .iter()
        .flat_map(|block| block.captures())
        .all(|c| c.position.report_sequence == 3));
}

#[test]
fn malformed_frame_drops_previous_payload_marks_and_recovers() {
    let at = Instant::now();
    for header in [[0, 0, 0, 0], [0, 0, 1, 0], [1, 0, 2, 0], [254, 255, 4, 0]] {
        let mut parser = CapturedParser::new(0xfffe);
        let mut bytes = vec![0x55; 4];
        block(&mut bytes, 0x1000, &[1, 2]);
        bytes.extend(header);
        let events = parser.feed(&report(bytes, 1, at)).unwrap();
        assert!(matches!(&events[..], [CapturedExportEvent::Reset]));
        assert!(parser.core.blocks.is_empty());
        assert!(parser.core.captures.is_empty());
        let events = parser.feed(&report(frame(&[3, 4]), 2, at)).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(blocks(&events)[0].captures()[0].position, position(2, 8));
    }
}

#[test]
fn long_sync_run_is_saturated_and_discards_pending_payload_marks() {
    let at = Instant::now();
    let mut parser = CapturedParser::new(0xfffe);
    let mut bytes = vec![0x55; 4];
    bytes.extend([0, 0x10, 4, 0, 1]);
    bytes.extend(vec![0x55; 100_000]);
    assert!(parser.feed(&report(bytes, 1, at)).unwrap().is_empty());
    assert_eq!(parser.core.sync, 8);
    assert!(parser.core.sync_captures.is_empty());
    let mut next = Vec::new();
    block(&mut next, 0xfffe, &[1, 0]);
    let events = parser.feed(&report(next, 2, at)).unwrap();
    assert!(matches!(&events[0], CapturedExportEvent::Reset));
    assert_eq!(
        blocks(&events[1..])[0].captures()[0].position,
        position(2, 4)
    );
}

#[test]
fn invalid_sequence_clears_pending_state() {
    let at = Instant::now();
    let mut parser = CapturedParser::new(0xfffe);
    assert!(parser
        .feed(&report(vec![0x55; 3], 1, at))
        .unwrap()
        .is_empty());
    assert!(parser.feed(&report(frame(&[1, 2]), 0, at)).is_err());
    assert_eq!(parser.core.sync, 0);
    let events = parser.feed(&report(frame(&[3, 4]), 2, at)).unwrap();
    assert_eq!(events.len(), 1);
}

#[test]
fn captured_block_rejects_misaligned_marks() {
    assert!(CapturedBlock::new(
        Block {
            address: 0,
            data: vec![1, 2]
        },
        vec![]
    )
    .is_err());
}

#[test]
fn native_event_limit_accepts_sixteen_and_rejects_overflow_without_partial_output() {
    let at = Instant::now();
    for count in [16, 17, 1000] {
        let mut bytes = Vec::new();
        for _ in 0..count {
            bytes.extend(frame(&[1, 2]));
        }
        let mut parser = CapturedParser::new(0xfffe);
        let result = parser.feed(&report(bytes, 1, at));
        if count == 16 {
            assert_eq!(result.unwrap().len(), 16);
        } else {
            assert_eq!(result.unwrap_err(), EVENT_OVERFLOW);
            assert!(!parser.core.active);
            assert!(parser.core.blocks.is_empty());
            assert!(parser.core.captures.is_empty());
            assert_eq!(
                parser.feed(&report(frame(&[3, 4]), 2, at)).unwrap().len(),
                1
            );
        }
    }
}

#[test]
fn event_limit_stops_the_iterator_during_parse_and_also_counts_resets() {
    let mut bytes = Vec::new();
    for _ in 0..100 {
        bytes.extend([0x55; 4]);
        bytes.extend([0, 0, 0, 0]);
    }
    let mut consumed = 0;
    let mut parser = ParserCore::<()>::new(0xfffe);
    let result = parser.feed_marked(
        bytes
            .into_iter()
            .inspect(|_| consumed += 1)
            .map(|byte| (byte, ())),
        Some(16),
    );
    assert!(result.is_err());
    assert_eq!(consumed, 17 * 8);
    assert_eq!(parser.sync, 0);
}

#[test]
fn marked_frames_obey_total_size_and_block_count_bounds() {
    let at = Instant::now();
    let mut bytes = vec![0x55; 4];
    for _ in 0..3 {
        block(&mut bytes, 0, &vec![0; 65_534]);
    }
    let events = CapturedParser::new(0xfffe)
        .feed(&report(bytes, 1, at))
        .unwrap();
    assert!(matches!(&events[..], [CapturedExportEvent::Reset]));
    let mut bytes = vec![0x55; 4];
    for _ in 0..8193 {
        block(&mut bytes, 0x1000, &[1, 2]);
    }
    let events = CapturedParser::new(0xfffe)
        .feed(&report(bytes, 1, at))
        .unwrap();
    assert!(matches!(&events[..], [CapturedExportEvent::Reset]));
}
