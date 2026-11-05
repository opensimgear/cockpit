use super::{
    capacity::{CapacityWake, WorkerExit},
    line, write_line, Fault, Limits, Message, Stat, Stats,
};
use std::{
    io::{BufWriter, Write},
    sync::{mpsc::Receiver, Arc, atomic::Ordering},
    time::Instant,
};

pub(super) fn run<W: Write + Send + 'static>(
    mut writer: BufWriter<W>,
    receiver: Receiver<Message>,
    capacity: Arc<CapacityWake>,
    stats: Arc<Stats>,
    limits: Limits,
    mut bytes_written: u64,
) {
    let worker_exit = WorkerExit::new(receiver, capacity, stats.clone());
    let mut failed = false;
    while let Ok(message) = worker_exit.receiver().recv() {
        worker_exit.capacity().dequeued();
        match message {
            Message::Event(event) => {
                stats.queued.fetch_sub(1, Ordering::AcqRel);
                let fields: Vec<String> = event.fields[..event.len]
                    .iter()
                    .map(u64::to_string)
                    .collect();
                let result = line(&serde_json::json!({
                    "type":"event",
                    "sequence":event.sequence.to_string(),
                    "qpc":event.at.qpc.to_string(),
                    "kind":format!("{:?}", event.kind),
                    "fields":fields
                }))
                .and_then(|bytes| {
                    if bytes.len() > 512 {
                        return Err(Fault::Serialization);
                    }
                    write_line(&mut writer, &bytes, &mut bytes_written, limits.bytes)
                });
                if let Err(error) = result {
                    stats.fault(error);
                    break;
                }
                stats.bump(Stat::Written);
                stats.max(Stat::LastWritten, event.sequence);
            }
            Message::Barrier {
                through,
                finish,
                reason,
                deadline,
                reply,
            } => {
                if Instant::now() >= deadline {
                    stats.fault(Fault::FlushTimeout);
                }
                if writer.flush().is_err() {
                    stats.fault(Fault::Io);
                    failed = true;
                }
                if Instant::now() >= deadline {
                    stats.fault(Fault::FlushTimeout);
                }
                let complete = !failed && stats.get(Stat::LastWritten) >= through;
                let mut report = stats.report(through, complete, finish && complete);
                let control = line(&serde_json::json!({
                    "type": if finish { "footer" } else { "checkpoint" },
                    "reason": reason,
                    "report": report
                }));
                // Event bytes were flushed above. Direct control writes cannot
                // be retried later by BufWriter with stale byte counters.
                let result = control.and_then(|bytes| {
                    write_line(writer.get_mut(), &bytes, &mut bytes_written, limits.bytes)
                });
                if let Err(error) = result {
                    stats.fault(error);
                    failed = true;
                }
                if Instant::now() >= deadline {
                    stats.fault(Fault::FlushTimeout);
                }
                report = stats.report(
                    through,
                    complete && !failed,
                    finish && complete && !failed,
                );
                let terminal_error = failed;
                if finish {
                    // Prior records were flushed and the footer was attempted
                    // before the caller can receive completion.
                    drop(writer);
                    stats.finished.store(report.complete, Ordering::Release);
                    let _ = reply.try_send(report);
                    break;
                }
                let _ = reply.try_send(report);
                if terminal_error {
                    break;
                }
            }
        }
    }
}
