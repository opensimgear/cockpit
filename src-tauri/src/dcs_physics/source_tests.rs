use super::{tests::packet, *};
use osg_streams::{Delivery, LossReason};
use std::{
    net::{Ipv4Addr, UdpSocket},
    time::{Duration, Instant},
};
type Reader = osg_streams::NativeSubscription<Snapshot, Snapshot>;
fn sender() -> UdpSocket {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap()
}
fn send(sender: &UdpSocket, source: &Source, sequence: u32, time: f64) {
    sender
        .send_to(
            packet(sequence, time).to_string().as_bytes(),
            source.local_addr(),
        )
        .unwrap();
}
fn delivery(reader: &mut Reader) -> Delivery<Snapshot, Snapshot> {
    reader
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("native source commit")
}
fn captured(reader: &mut Reader, sequence: u32) -> Snapshot {
    loop {
        match delivery(reader) {
            Delivery::Snapshot(value) if value.value.sequence == Some(sequence) => {
                return value.value
            }
            Delivery::Event(value) if value.value.sequence == Some(sequence) => return value.value,
            _ => {}
        }
    }
}
fn live() -> (Source, UdpSocket, Reader) {
    let source = Source::bind_loopback_ephemeral().unwrap();
    let sender = sender();
    let mut reader = source.subscribe().unwrap();
    assert!(matches!(delivery(&mut reader), Delivery::Snapshot(_)));
    send(&sender, &source, 1, 1.0);
    assert!(!captured(&mut reader, 1).fresh);
    send(&sender, &source, 2, 1.05);
    assert!(captured(&mut reader, 2).fresh);
    (source, sender, reader)
}
#[test]
fn independent_worker_commits_every_ordered_datagram_without_consumer_pumps() {
    let (source, sender, mut reader) = live();
    let last_sequence = 2 + (source::LIMITS.max_pending_frames / 2) as u32;
    for sequence in 3..=last_sequence {
        send(&sender, &source, sequence, sequence as f64 / 20.0 + 1.0);
    }
    let mut last_capture = 0;
    for sequence in 3..=last_sequence {
        let Delivery::Event(event) = delivery(&mut reader) else {
            panic!("ordered report expected")
        };
        assert_eq!(event.value.sequence, Some(sequence));
        assert!(event.value.fresh);
        assert!(event.captured_us > 0);
        last_capture = event.captured_us;
        assert!(event.clock_id.starts_with("dcs-ownship:"));
    }
    assert_eq!(source.snapshot().sequence, Some(last_sequence));
    for _ in 0..10 {
        assert_eq!(source.snapshot().sequence, Some(last_sequence));
    }
    let mut late = source.view().subscribe().unwrap();
    let Delivery::Snapshot(snapshot) = delivery(&mut late) else {
        panic!("atomic snapshot")
    };
    assert_eq!(snapshot.value.sequence, Some(last_sequence));
    assert_eq!(snapshot.captured_us, last_capture);
    assert_eq!(snapshot.value.captured_at, source.snapshot().captured_at);
}
#[test]
fn fresh_deadline_is_a_native_event_and_replays_cannot_extend_it() {
    let (source, sender, mut reader) = live();
    let initial = source.snapshot();
    let capture_instant = initial.captured_at;
    let started = Instant::now();
    assert!(reader
        .recv_timeout(Duration::from_millis(400))
        .unwrap()
        .is_none());
    send(&sender, &source, 2, 1.05);
    let passive = source.view().snapshot();
    assert_eq!(passive.sequence, initial.sequence);
    assert!(passive.age_ms.unwrap() >= 350);
    assert_eq!(passive.captured_at, capture_instant);
    let Delivery::Event(expired) = delivery(&mut reader) else {
        panic!("deadline event")
    };
    assert_eq!(expired.value.status, "stale");
    assert!(!expired.value.fresh);
    assert!(expired.value.values.is_empty());
    assert!(expired.value.age_ms.unwrap() > FRESHNESS_MS);
    assert_eq!(expired.value.captured_at, capture_instant);
    assert!(started.elapsed() < Duration::from_millis(1350));
    assert_eq!(source.snapshot().status, "stale");
    send(&sender, &source, 3, 1.1);
    assert!(!captured(&mut reader, 3).fresh);
    send(&sender, &source, 4, 1.15);
    assert!(captured(&mut reader, 4).fresh);
}

#[test]
fn packet_gaps_publish_loss_and_require_an_advancing_baseline() {
    let (source, sender, mut reader) = live();
    send(&sender, &source, 4, 1.2);
    let Delivery::Loss(loss) = delivery(&mut reader) else {
        panic!("source gap loss")
    };
    assert_eq!(loss.reason, LossReason::GenerationReplaced);
    let reset = captured(&mut reader, 4);
    assert!(!reset.fresh);
    assert!(reset.values.is_empty());
    send(&sender, &source, 5, 1.25);
    assert!(captured(&mut reader, 5).fresh);
}
#[test]
fn source_wrap_is_continuous_and_pause_denial_frozen_stop_are_ordered() {
    let source = Source::bind_loopback_ephemeral().unwrap();
    let sender = sender();
    let mut reader = source.subscribe().unwrap();
    send(&sender, &source, u32::MAX, 1.0);
    captured(&mut reader, u32::MAX);
    send(&sender, &source, 0, 1.05);
    let wrapped = captured(&mut reader, 0);
    assert!(wrapped.fresh);
    assert_eq!(wrapped.capture_sequence, 2);
    for (sequence, field, status) in [
        (1, "paused", "paused"),
        (2, "allowed", "export-denied"),
        (3, "stopped", "stopped"),
    ] {
        let mut value = packet(sequence, 1.1 + sequence as f64 / 20.0);
        value[field] = json!(field != "allowed");
        sender
            .send_to(value.to_string().as_bytes(), source.local_addr())
            .unwrap();
        let snapshot = captured(&mut reader, sequence);
        assert_eq!(snapshot.status, status);
        assert!(snapshot.values.is_empty());
    }
    send(&sender, &source, 4, 1.25);
    assert_eq!(captured(&mut reader, 4).status, "paused");
}
#[test]
fn malformed_unknown_sources_withdraw_without_renewing_native_evidence() {
    let (source, sender, mut reader) = live();
    let before = source.snapshot();
    let mut unknown = packet(3, 1.1);
    unknown["source"] = json!("other-source");
    sender
        .send_to(unknown.to_string().as_bytes(), source.local_addr())
        .unwrap();
    let Delivery::Event(error) = delivery(&mut reader) else {
        panic!("invalid packet withdrawal")
    };
    assert_eq!(error.value.sequence, before.sequence);
    assert_eq!(error.value.malformed_frames, 1);
    assert!(!error.value.fresh);
    assert!(error.value.values.is_empty());
    assert!(error.value.age_ms.unwrap() >= before.age_ms.unwrap());
    sender
        .send_to(&vec![b'x'; MAX_PACKET + 1], source.local_addr())
        .unwrap();
    let Delivery::Event(error) = delivery(&mut reader) else {
        panic!("oversize withdrawal")
    };
    assert_eq!(error.value.malformed_frames, 2);
}
#[test]
fn slow_reader_overflow_is_explicit_while_other_readers_preserve_order() {
    let (source, sender, mut reader) = live();
    let mut slow = source.subscribe().unwrap();
    delivery(&mut slow);
    for sequence in 3..1103 {
        send(&sender, &source, sequence, 1.0 + sequence as f64 / 20.0);
        assert_eq!(captured(&mut reader, sequence).sequence, Some(sequence));
    }
    let Delivery::Loss(loss) = delivery(&mut slow) else {
        panic!("bounded queue must overflow")
    };
    assert_eq!(loss.reason, LossReason::Overflow);
    assert!(slow.try_recv().unwrap().is_none());
    let mut recovered = source.subscribe().unwrap();
    assert_eq!(captured(&mut recovered, 1102).sequence, Some(1102));
}
#[test]
fn passive_views_cannot_keep_socket_alive_and_immediate_shutdown_is_race_safe() {
    for _ in 0..20 {
        let source = Source::bind_loopback_ephemeral().unwrap();
        let view = source.view();
        let address = source.local_addr();
        let mut reader = view.subscribe().unwrap();
        let started = Instant::now();
        drop(source);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(view.snapshot().values.is_empty());
        assert_eq!(view.snapshot().malformed_frames, 0);
        assert!(view.snapshot().error.as_deref().unwrap().contains("closed"));
        assert!(view.subscribe().is_err());
        let Delivery::Loss(loss) = delivery(&mut reader) else {
            panic!("closed loss")
        };
        assert_eq!(loss.reason, LossReason::Closed);
        UdpSocket::bind(address).expect("views must not retain receive ownership");
    }
}

#[test]
fn cancellation_remains_prompt_during_continuous_receive_traffic() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    let (source, sender, mut reader) = live();
    let address = source.local_addr();
    let view = source.view();
    let stop = Arc::new(AtomicBool::new(false));
    let traffic_stop = stop.clone();
    let traffic_worker = std::thread::spawn(move || {
        let mut sequence = 3;
        while !traffic_stop.load(Ordering::Acquire) {
            let report = packet(sequence, sequence as f64 / 20.0 + 1.0);
            if sender
                .send_to(report.to_string().as_bytes(), address)
                .is_err()
            {
                break;
            }
            sequence = sequence.wrapping_add(1);
        }
    });
    delivery(&mut reader);
    let started = Instant::now();
    drop(source);
    let shutdown_elapsed = started.elapsed();
    stop.store(true, Ordering::Release);
    traffic_worker.join().unwrap();
    assert!(shutdown_elapsed < Duration::from_secs(1));
    assert_eq!(
        view.snapshot().error.as_deref(),
        Some("DCS ownship source closed.")
    );
    assert!(view.snapshot().values.is_empty());
    UdpSocket::bind(address).expect("shutdown must release the receive socket under traffic");
}
