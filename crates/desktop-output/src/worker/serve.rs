//! Protocol transport with queue notifications and native deadline waits.
use super::*;
use super::wait::NativeWait;
use std::{io::Write, sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, mpsc}, thread};

pub fn serve<A: Adapter, R: BufRead + Send + 'static, W: Write + Send + 'static>(
    mut adapter: A,
    mut input: R,
    mut output: W,
    clock: fn() -> u64,
) -> Result<(), String> {
    let mut waiter = NativeWait::new()?;
    let reader_wake = waiter.waker();
    let writer_wake = waiter.waker();
    let (send, recv) = mpsc::sync_channel(32);
    let audit_channel = crate::audit::channel();
    let incoming_depth = Arc::new(AtomicUsize::new(0));
    let reader_depth = incoming_depth.clone();
    let failure = Arc::new(AtomicBool::new(false));
    let reader_failure = failure.clone();
    thread::spawn(move || loop {
        let message = read_frame(&mut input).and_then(|b| {
            b.map(|v| serde_json::from_slice::<Request>(&v).map_err(|e| e.to_string()))
                .transpose()
        });
        let done = !matches!(message, Ok(Some(_)));
        let request_sequence = message.as_ref().ok().and_then(|request| request.as_ref()).map(|request| request.sequence.0);
        let queued_at = crate::audit::stamp();
        let tracked = audit_channel.is_some() && request_sequence.is_some();
        if message.is_err() && audit_channel.is_some() {
            crate::audit::record(crate::audit::Kind::QueueFault, &[2, 3, reader_depth.load(Ordering::Acquire) as u64]);
        }
        let queued_depth = if tracked { reader_depth.fetch_add(1, Ordering::AcqRel) + 1 } else { 0 };
        if send.try_send(message).is_err() {
            if tracked {
                reader_depth.fetch_sub(1, Ordering::AcqRel);
            }
            if audit_channel.is_some() {
                crate::audit::record(crate::audit::Kind::QueueFault, &[2, 1, queued_depth as u64]);
            }
            reader_failure.store(true, Ordering::Release);
            reader_wake.notify();
            break;
        }
        reader_wake.notify();
        if let (Some(at), Some(channel), Some(sequence)) = (queued_at, audit_channel, request_sequence) {
            crate::audit::record_at(at, crate::audit::Kind::RequestQueued,
                &[channel, sequence, queued_depth as u64]);
        }
        if done {
            break;
        }
    });
    let (responses, writes) = mpsc::sync_channel::<serde_json::Value>(32);
    let writer_failure = failure.clone();
    let (finished, completion) = mpsc::sync_channel(1);
    let writer = thread::spawn(move || {
        for response in writes {
            if writeln!(output, "{response}")
                .and_then(|_| output.flush())
                .is_err()
            {
                writer_failure.store(true, Ordering::Release);
                writer_wake.notify();
                break;
            }
        }
        let _ = finished.send(());
    });
    responses
        .try_send(serde_json::json!({"protocol":PROTOCOL,"backend":"desktop-input","ok":true}))
        .map_err(|_| "Desktop hello failed.")?;
    let scope_wake = waiter.waker();
    adapter.set_scope_wake(Arc::new(move || scope_wake.notify()));
    let mut worker = Worker::new(adapter);
    let result = (|| loop {
        if failure.load(Ordering::Acquire) {
            return Err("Desktop worker transport failed.".into());
        }
        worker.tick(clock())?;
        let incoming = waiter.receive(&recv, worker.wait_duration(clock()))?;
        if failure.load(Ordering::Acquire) { return Err("Desktop worker transport failed.".into()); }
        match incoming {
            Ok(Ok(Some(request))) => {
                if let Some(channel) = audit_channel {
                    let depth = incoming_depth.fetch_sub(1, Ordering::AcqRel).saturating_sub(1);
                    crate::audit::record(crate::audit::Kind::RequestDequeued, &[channel, request.sequence.0, depth as u64]);
                }
                let stop = matches!(request.command, Command::Stop);
                let delivery = matches!(request.command, Command::Deliver { .. } | Command::DeliverGuarded { .. } | Command::DeliverPointerBatch { .. });
                let ack = worker.receive(request, clock());
                worker.anchor_release_guard(&ack, clock());
                if let Some(channel) = audit_channel {
                    crate::audit::record(crate::audit::Kind::WorkerOutcome, &[
                        channel, ack.sequence.0, ack.operation_id.or_else(|| ack.pointer_batch.as_ref().and_then(|batch| batch.operation_ids.first().copied())).map(|value| value.0).unwrap_or(0),
                        audit_ack_flags(&ack), ack.attempted as u64, ack.confirmed as u64,
                    ]);
                }
                // Explicit debug acceptance hook: prove the actual submission/
                // acknowledgement gap. No retries or production hook is enabled.
                #[cfg(debug_assertions)]
                if delivery
                    && ack.ok
                    && ack.attempted > 0
                    && crate::qualification_gate()
                    && std::env::var("OSG_DESKTOP_TEST_EXIT_AFTER_SEND").as_deref() == Ok("1")
                {
                    std::process::exit(86);
                }
                #[cfg(not(debug_assertions))]
                let _ = delivery;
                let fault = ack.uncertain;
                responses
                    .try_send(serde_json::to_value(ack).map_err(|e| e.to_string())?)
                    .map_err(|_| "Desktop response queue overflow.")?;
                if stop || fault {
                    return Ok(());
                }
            }
            Ok(Ok(None)) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Ok(Err(e)) => return Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    })();
    let cleanup = worker.stop();
    drop(responses);
    if completion.recv_timeout(Duration::from_millis(50)).is_ok() {
        let _ = writer.join();
    } else {
        return Err("Desktop worker response pipe stalled during shutdown.".into());
    }
    if !cleanup {
        return Err("Desktop worker cleanup incomplete.".into());
    }
    if failure.load(Ordering::Acquire) {
        return Err("Desktop worker transport failed during shutdown.".into());
    }
    result
}

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
