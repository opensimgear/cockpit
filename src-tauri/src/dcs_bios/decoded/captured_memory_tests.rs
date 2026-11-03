use super::*;
use crate::dcs_bios::{Block, CapturePosition};
use std::time::Instant;

fn block(address: u16, bytes: &[u8], sequence: u64, offset: u32) -> CapturedBlock {
    CapturedBlock::new(
        Block {
            address,
            data: bytes.to_vec(),
        },
        (0..bytes.len())
            .map(|index| ByteCapture {
                position: CapturePosition {
                    report_sequence: sequence,
                    byte_offset: offset + index as u32,
                },
                captured_at: Instant::now(),
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn clones_keep_old_bytes_and_capture_while_touched_pages_copy_on_write() {
    let mut original = CapturedMemory::new("source-a".into());
    original.apply(&[block(0, &[4, 5, 6, 7], 1, 10)]).unwrap();
    let old_capture = original.capture(1).unwrap();
    let mut clone = original.clone();
    assert!(Arc::ptr_eq(
        original.pages[0].as_ref().unwrap(),
        clone.pages[0].as_ref().unwrap()
    ));

    clone.apply(&[block(256, &[8, 9], 2, 20)]).unwrap();
    assert!(Arc::ptr_eq(
        original.pages[0].as_ref().unwrap(),
        clone.pages[0].as_ref().unwrap()
    ));
    clone.apply(&[block(0, &[4, 5], 3, 30)]).unwrap();
    assert!(!Arc::ptr_eq(
        original.pages[0].as_ref().unwrap(),
        clone.pages[0].as_ref().unwrap()
    ));
    assert_eq!(original.bytes[0..2], [4, 5]);
    assert_eq!(original.capture(1), Some(old_capture));
    assert_eq!(clone.capture(1).unwrap().position.report_sequence, 3);
}

#[test]
fn empty_acquisition_cannot_apply_even_an_empty_batch() {
    let mut memory = CapturedMemory::new(String::new());
    assert!(memory.apply(&[]).is_err());
    assert!(memory.valid.iter().all(|valid| !valid));
    assert_eq!(memory.acquisition_id(), "");
}

#[test]
fn retains_distinct_capture_for_every_address_and_equal_writes_refresh_evidence() {
    let mut memory = CapturedMemory::new("all-addresses".into());
    let mut blocks = Vec::new();
    for start in (0..ADDRESS_SPACE).step_by(PAGE_SIZE) {
        let bytes = vec![0x5a; PAGE_SIZE];
        blocks.push(block(start as u16, &bytes, 7, start as u32));
    }
    memory.apply(&blocks).unwrap();
    for address in 0..ADDRESS_SPACE {
        let capture = memory.capture(address as u16).unwrap();
        assert_eq!(capture.position.report_sequence, 7);
        assert_eq!(capture.position.byte_offset, address as u32);
    }
    let before = memory.capture(100).unwrap();
    memory.apply(&[block(100, &[0x5a, 0x5a], 8, 900)]).unwrap();
    assert_eq!(memory.bytes[100], 0x5a);
    assert_ne!(memory.capture(100), Some(before));
    assert_eq!(memory.capture(101).unwrap().position.report_sequence, 8);
    assert_eq!(memory.capture(101).unwrap().position.byte_offset, 901);
    assert_eq!(memory.capture(102), Some(blocks[0].captures()[102]));
}

#[test]
fn unrelated_writes_preserve_old_capture_and_apply_rejects_invalid_batches_atomically() {
    let mut memory = CapturedMemory::new("source-a".into());
    memory.apply(&[block(20, &[1, 2], 1, 40)]).unwrap();
    let old = memory.capture(20).unwrap();
    memory.apply(&[block(40, &[3, 4], 2, 50)]).unwrap();
    assert_eq!(memory.capture(20), Some(old));

    let bytes = memory.bytes.clone();
    let valid = memory.valid.clone();
    let capture = memory.capture(20);
    let zero_sequence = CapturedBlock::new(
        Block {
            address: 60,
            data: vec![9, 10],
        },
        vec![
            ByteCapture {
                position: CapturePosition {
                    report_sequence: 0,
                    byte_offset: 0
                },
                captured_at: Instant::now(),
            };
            2
        ],
    )
    .unwrap();
    assert!(memory
        .apply(&[block(60, &[9, 10], 3, 0), zero_sequence])
        .is_err());
    assert_eq!(memory.bytes, bytes);
    assert_eq!(memory.valid, valid);
    assert_eq!(memory.capture(20), capture);

    let oob = CapturedBlock::new(
        Block {
            address: 65_534,
            data: vec![9, 10, 11, 12],
        },
        vec![
            ByteCapture {
                position: CapturePosition {
                    report_sequence: 4,
                    byte_offset: 0
                },
                captured_at: Instant::now(),
            };
            4
        ],
    );
    if let Ok(oob) = oob {
        assert!(memory.apply(&[oob]).is_err());
        assert_eq!(memory.bytes, bytes);
        assert_eq!(memory.valid, valid);
    }
}

#[test]
fn clear_invalidates_all_cells_and_copy_transfers_validity_bytes_and_capture() {
    let mut source = CapturedMemory::new("same".into());
    source.apply(&[block(2, &[1, 2, 3, 4], 5, 10)]).unwrap();
    let mut target = CapturedMemory::new("same".into());
    target.copy_range_from(&source, 2..6).unwrap();
    assert_eq!(target.bytes[2..6], source.bytes[2..6]);
    assert_eq!(target.valid[2..6], source.valid[2..6]);
    assert_eq!(target.capture(3), source.capture(3));

    let mut fresh = CapturedMemory::new("same".into());
    fresh.copy_range_from(&source, 2..6).unwrap();
    source.clear();
    fresh.copy_range_from(&source, 2..6).unwrap();
    assert_eq!(&fresh.bytes[2..6], &[1, 2, 3, 4]);
    assert!(fresh.valid[2..6].iter().all(|valid| !valid));
    assert!(fresh.capture(3).is_none());
    assert!(fresh.copy_range_from(&target, 6..5).is_err());
    assert!(fresh.copy_range_from(&target, 65_537..65_537).is_err());
    assert!(fresh
        .copy_range_from(&CapturedMemory::new("other".into()), 0..1)
        .is_err());

    fresh.apply(&[block(0, &[8, 9], 6, 2)]).unwrap();
    fresh.clear();
    assert!(fresh.valid.iter().all(|valid| !valid));
    assert!(fresh.pages.iter().all(Option::is_none));
    assert_eq!(fresh.acquisition_id(), "same");
}

#[test]
fn retained_heap_bound_counts_full_shared_page_allocations() {
    let mut memory = CapturedMemory::new("bound".into());
    memory.apply(&[block(0, &[1, 2], 1, 0)]).unwrap();
    let clone = memory.clone();
    let page_bytes = std::mem::size_of::<CapturePage>() + 2 * std::mem::size_of::<usize>();
    let calculated = std::mem::size_of::<CapturedMemory>()
        + memory.bytes.capacity()
        + memory.valid.capacity()
        + memory.acquisition_id.capacity()
        + page_bytes;
    assert_eq!(memory.retained_bytes(), calculated);
    assert_eq!(clone.retained_bytes(), calculated);

    let mut full = CapturedMemory::new("full".into());
    let blocks: Vec<_> = (0..ADDRESS_SPACE)
        .step_by(PAGE_SIZE)
        .map(|start| block(start as u16, &vec![0; PAGE_SIZE], 1, start as u32))
        .collect();
    full.apply(&blocks).unwrap();
    let maximum = std::mem::size_of::<CapturedMemory>()
        + full.bytes.capacity()
        + full.valid.capacity()
        + full.acquisition_id.capacity()
        + PAGE_COUNT * page_bytes;
    assert_eq!(full.retained_bytes(), maximum);
}
