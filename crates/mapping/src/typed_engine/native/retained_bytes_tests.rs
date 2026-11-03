use super::*;

fn wide(sequence: u64, baseline: bool) -> NativeFrame {
    let producer = "p".repeat(MAX_NATIVE_ID_BYTES);
    let kind = if baseline {
        NativeChangeKind::Baseline
    } else {
        NativeChangeKind::Update
    };
    let captures = (0..1024)
        .map(|offset| NativeCapture {
            producer_id: producer.clone(),
            generation: Counter(1),
            sequence: Counter(sequence),
            byte_offset: Some(offset),
            captured_us: Counter(sequence),
        })
        .collect();
    let contributors = (0..1024)
        .map(|input| NativeContributor {
            input: NativeInput::Byte(input),
            capture: u32::from(input),
            validity: NativeValidity::Sampled {
                valid_until_us: Counter(sequence + 100_000),
            },
        })
        .collect();
    let mut changes = vec![NativeChange {
        port_id: "x".into(),
        continuity: Counter(1),
        kind,
        sample: value(sequence % 2 == 0),
        contributors,
    }];
    if baseline {
        changes.push(NativeChange {
            port_id: "y".into(),
            continuity: Counter(1),
            kind,
            sample: SignalSample::Unknown,
            contributors: vec![],
        });
    }
    NativeFrame {
        source: "game".into(),
        generation: Counter(1),
        sequence: Counter(sequence),
        clock_id: "clock".into(),
        captures,
        leases: vec![],
        changes,
    }
}

#[test]
fn wide_fifo_evidence_hits_retained_bytes_before_reference_count_without_advancing_authority() {
    let mut phase = phase(true, NativePolicy::Sampled);
    let first = wide(1, true);
    assert!(super::super::budget::within_bytes(
        &first,
        MAX_NATIVE_FRAME_BYTES
    ));
    admit(&mut phase, first, 1);
    let inflight = phase.script_requests().pop().unwrap();
    let mut rejected = None;
    for sequence in 2..100 {
        let frame = wide(sequence, false);
        assert!(super::super::budget::within_bytes(
            &frame,
            MAX_NATIVE_FRAME_BYTES
        ));
        if let Err(error) = phase.accept_admissions([frame.into()], Counter(sequence)) {
            rejected = Some((sequence, error));
            break;
        }
    }
    let (sequence, error) =
        rejected.expect("Wide immutable FIFO proof retention must be bounded in bytes.");
    assert!(error.contains("byte budget"), "{error}");
    assert!(sequence < crate::script::QUEUE_LIMIT as u64);
    assert!(sequence as usize * 1024 < MAX_NATIVE_REFERENCES);
    assert_eq!(phase.at_us(), Counter(sequence - 1));
    assert_eq!(phase.native.sources["game"], (1, sequence - 1));
    assert!(phase.script_ticket_current(&inflight.ticket));
    assert!(phase
        .check_native_retained_bytes(&phase.native, std::iter::empty())
        .is_ok());
    let loss = frame(
        sequence,
        &[
            ("x", NativeChangeKind::Withdraw, 1, None, 0, 0),
            ("y", NativeChangeKind::Withdraw, 1, None, 0, 0),
        ],
    );
    admit(&mut phase, loss, sequence);
    assert!(phase.states["script"].bindings.values().all(|proof| proof.bindings.is_empty()), "Loss must release old capture payloads even when a new quality callback is pending");
    assert!(phase.states["script"].pending.iter().all(|request| !matches!(request.inputs["input"], SignalSample::Known(_))));
    assert!(!phase.script_ticket_current(&inflight.ticket));
    assert!(phase
        .check_native_retained_bytes(&phase.native, std::iter::empty())
        .is_ok());
}

#[test]
fn shared_immutable_payload_is_charged_once_across_many_consumers() {
    let mut phase = phase(false, NativePolicy::Sampled);
    admit(&mut phase, wide(1, true), 1);
    let proof = &phase.values[&ep("game", "x")].proof;
    let clones = vec![proof.clone(); 4096];
    assert!(phase
        .check_native_retained_bytes(&phase.native, clones.iter())
        .is_ok());
}
