use super::*;
use crate::dcs_bios::{
    decoded::{CapturedMemory, CounterObservation},
    Block, ByteCapture, CapturePosition, CapturedBlock, IntegerOutput,
};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn snapshot(at: Instant, address: u16, bytes: &[u8], sequence: u64) -> Snapshot {
    let mut memory = CapturedMemory::new("acq-1".into());
    let mut data = bytes.to_vec();
    if data.len() % 2 != 0 {
        data.push(0);
    }
    let captures = (0..data.len())
        .map(|offset| ByteCapture {
            position: CapturePosition {
                report_sequence: sequence,
                byte_offset: offset as u32,
            },
            captured_at: at,
        })
        .collect();
    memory
        .apply(&[CapturedBlock::new(Block { address, data }, captures).unwrap()])
        .unwrap();
    Snapshot {
        producer_id: "decoded-owner".into(),
        epoch: 4,
        aircraft: Some("Test".into()),
        counter: Some(sequence as u16),
        counter_observation: Some(CounterObservation {
            acquisition_id: "acq-1".into(),
            position: CapturePosition {
                report_sequence: sequence,
                byte_offset: (bytes.len() as u32 + 10).max(99),
            },
            captured_at: at,
        }),
        freshness_ms: 1000,
        status: "live",
        malformed_frames: 0,
        error: None,
        memory: Arc::new(memory),
    }
}

fn integer_source(mask: u16) -> OutputSource {
    OutputSource::Integer(
        IntegerOutput::parse(&json!({"type":"integer","address":100,
        "mask":mask,"shift_by":0,"max_value":mask}))
        .unwrap(),
    )
}

#[test]
fn integer_proof_includes_both_bytes_and_keeps_original_capture_when_read_is_deferred() {
    let t = Instant::now();
    let snapshot = snapshot(t, 100, &[0x05, 0], 1);
    let source = integer_source(0x000f);
    let now = t + Duration::from_millis(200);
    let mut projection = Projection::new(&snapshot, now).unwrap();
    let value = source.read_captured(&mut projection, now).unwrap();
    assert_eq!(value.value(), &json!(5));
    let proof = value.contributors().unwrap();
    assert_eq!(proof.len(), 2);
    assert_eq!(
        proof.iter().map(|c| c.input.clone()).collect::<Vec<_>>(),
        vec![InputId::Byte(100), InputId::Byte(101)]
    );
    assert!(proof.iter().all(|c| c.capture.captured_at == t));
    assert!(proof
        .iter()
        .all(|c| c.capture.producer_id.as_ref() == "acq-1"));
}

#[test]
fn string_proof_covers_full_fixed_width_including_bytes_after_nul() {
    let t = Instant::now();
    let snapshot = snapshot(t, 200, b"A\0BCD", 2);
    let source = OutputSource::String {
        address: 200,
        length: 5,
    };
    let now = t + Duration::from_millis(10);
    let mut projection = Projection::new(&snapshot, now).unwrap();
    let value = source.read_captured(&mut projection, now).unwrap();
    assert_eq!(value.value(), &json!("A"));
    assert_eq!(value.contributors().unwrap().len(), 5);
    assert_eq!(
        value.contributors().unwrap().last().unwrap().input,
        InputId::Byte(204)
    );
}

#[test]
fn counter_observation_renews_lease_without_relabeling_sparse_bytes_or_mutating_old_snapshot() {
    let t = Instant::now();
    let old = snapshot(t, 100, &[7, 0], 1);
    let mut updated = old.clone();
    updated.counter = Some(2);
    updated.counter_observation = Some(CounterObservation {
        acquisition_id: "acq-1".into(),
        position: CapturePosition {
            report_sequence: 2,
            byte_offset: 99,
        },
        captured_at: t + Duration::from_secs(2),
    });
    let now = t + Duration::from_millis(2500);
    let source = integer_source(0xffff);
    assert!(Projection::new(&old, now).is_none());
    let mut projection = Projection::new(&updated, now).unwrap();
    let value = source.read_captured(&mut projection, now).unwrap();
    assert!(value
        .contributors()
        .unwrap()
        .iter()
        .all(|c| c.capture.sequence == 1));
    assert!(value
        .contributors()
        .unwrap()
        .iter()
        .all(|c| c.capture.captured_at == t));
    assert!(value.is_current(now));
}

#[test]
fn absent_capture_and_unreadable_memory_reject_the_value() {
    let t = Instant::now();
    let snapshot = snapshot(t, 100, &[1, 0], 1);
    let source = OutputSource::String {
        address: 100,
        length: 3,
    };
    let mut projection = Projection::new(&snapshot, t).unwrap();
    assert!(source.read_captured(&mut projection, t).is_none());
    let empty = Snapshot {
        memory: Arc::new(CapturedMemory::new("acq-1".into())),
        ..snapshot
    };
    let mut projection = Projection::new(&empty, t).unwrap();
    assert!(integer_source(0xffff)
        .read_captured(&mut projection, t)
        .is_none());
}

#[test]
fn per_projection_cache_reuses_byte_contribution_arcs() {
    let t = Instant::now();
    let snapshot = snapshot(t, 100, &[7, 0], 1);
    let mut projection = Projection::new(&snapshot, t).unwrap();
    let source = integer_source(0xffff);
    let first = source.read_captured(&mut projection, t).unwrap();
    let second = source.read_captured(&mut projection, t).unwrap();
    for (a, b) in first
        .contributors()
        .unwrap()
        .iter()
        .zip(second.contributors().unwrap())
    {
        assert!(Arc::ptr_eq(a, b));
    }
}

#[test]
fn separate_projection_contexts_keep_old_and_new_snapshot_values_with_their_own_proofs() {
    let t = Instant::now();
    let old = snapshot(t, 100, &[7, 0], 1);
    let updated = snapshot(t + Duration::from_millis(100), 100, &[9, 0], 2);
    let now = t + Duration::from_millis(200);
    let source = integer_source(0xffff);
    let mut old_projection = Projection::new(&old, now).unwrap();
    let mut new_projection = Projection::new(&updated, now).unwrap();
    let old_value = source.read_captured(&mut old_projection, now).unwrap();
    let new_value = source.read_captured(&mut new_projection, now).unwrap();
    assert_eq!(old_value.value(), &json!(7));
    assert_eq!(old_value.contributors().unwrap()[0].capture.sequence, 1);
    assert_eq!(new_value.value(), &json!(9));
    assert_eq!(new_value.contributors().unwrap()[0].capture.sequence, 2);
}

#[test]
fn maximum_string_width_and_projection_reference_budget_count_shared_proof_in_full() {
    use crate::native_capture::{ProjectionBudget, MAX_PROJECTION_REFERENCES};
    let at = Instant::now();
    let snapshot = snapshot(at, 100, &vec![b'A'; 1024], 1);
    let mut projection = Projection::new(&snapshot, at).unwrap();
    let source = OutputSource::String {
        address: 100,
        length: 1024,
    };
    let value = source.read_captured(&mut projection, at).unwrap();
    assert_eq!(value.contributors().unwrap().len(), 1024);
    assert_eq!(value.value().as_str().unwrap().len(), 1024);
    let mut budget = ProjectionBudget::default();
    let mut retained = 0usize;
    for _ in 0..MAX_PROJECTION_REFERENCES / 1024 {
        assert!(budget.admit(&value));
        retained = retained.checked_add(value.retained_bytes()).unwrap();
    }
    assert!(!budget.admit(&value));
    assert!(retained > 1024 * 1024);
    assert!(
        retained < 512 * 1024 * 1024,
        "conservatively count full shared witness and lease allocations"
    );
}
