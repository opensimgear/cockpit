//! Bounded JSON-lines worker transport. Deadline checks do not wait for stdin.
use crate::{
    feeder::{Feeder, Message},
    vjoy::Driver,
};
use serde_json::json;
use std::{
    io::{self, BufRead, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
pub const FRAME_LIMIT: usize = 66_560;
pub fn read_frame(input: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    loop {
        let buffer = input.fill_buf()?;
        if buffer.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Truncated worker frame.",
                ))
            };
        }
        let newline = buffer.iter().position(|b| *b == b'\n');
        let count = newline.map_or(buffer.len(), |n| n + 1);
        let payload = newline.unwrap_or(count);
        if payload > FRAME_LIMIT - bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Worker frame exceeds 66560 bytes.",
            ));
        }
        bytes.extend_from_slice(&buffer[..payload]);
        input.consume(count);
        if newline.is_some() {
            return Ok(Some(bytes));
        }
    }
}
pub fn serve<D: Driver, R: BufRead + Send + 'static, W: Write + Send + 'static>(
    driver: D,
    input: R,
    mut output: W,
    backend: &str,
) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(32);
    let overflow = Arc::new(AtomicBool::new(false));
    let read_overflow = overflow.clone();
    // A full pipe cannot keep unbounded requests alive or postpone lease expiry.
    thread::spawn(move || {
        let mut input = input;
        loop {
            let next = match read_frame(&mut input) {
                Ok(Some(bytes)) => serde_json::from_slice::<Message>(&bytes)
                    .map(Some)
                    .map_err(|e| e.to_string()),
                Ok(None) => Ok(None),
                Err(e) => Err(e.to_string()),
            };
            let done = !matches!(next, Ok(Some(_)));
            if sender.try_send(next).is_err() {
                read_overflow.store(true, Ordering::Release);
                break;
            }
            if done {
                break;
            }
        }
    });
    let origin = Instant::now();
    let mut feeder = Feeder::new(driver);
    let (responses, writes) = mpsc::sync_channel::<serde_json::Value>(32);
    let write_failure = Arc::new(AtomicBool::new(false));
    let failed = write_failure.clone();
    let (finished, completion) = mpsc::sync_channel(1);
    let writer = thread::spawn(move || {
        for response in writes {
            if writeln!(output, "{response}")
                .and_then(|_| output.flush())
                .is_err()
            {
                failed.store(true, Ordering::Release);
                break;
            }
        }
        let _ = finished.send(());
    });
    responses
        .try_send(json!({"ok":true,"backend":backend,"protocol":1}))
        .map_err(|_| "Worker output queue failed.")?;
    let result = (|| {
        loop {
            let now = u64::try_from(origin.elapsed().as_micros()).unwrap_or(u64::MAX);
            if overflow.load(Ordering::Acquire) || write_failure.load(Ordering::Acquire) {
                return Err("Worker transport overflowed or closed; releasing outputs.".into());
            }
            feeder.tick(now)?;
            match receiver.recv_timeout(Duration::from_millis(5)) {
                Ok(Ok(Some(message))) => {
                    let received = u64::try_from(origin.elapsed().as_micros()).unwrap_or(u64::MAX);
                    let token = message.token().clone();
                    let result = feeder.receive(message, received);
                    // A negative acknowledgement must never race the parent killing us
                    // before owned output has attempted fallback and release.
                    if result.is_err() {
                        feeder.stop();
                    }
                    responses.try_send(json!({"token":token,"ok":result.is_ok(),"error":result.as_ref().err().map(|e| e.chars().take(256).collect::<String>()),"active":feeder.active()})).map_err(|_| "Worker output queue overflowed.")?;
                    result?;
                    // A worker owns exactly one run. It never restarts from queued old input.
                    if !feeder.active() {
                        break;
                    }
                }
                Ok(Ok(None)) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        Ok::<(), String>(())
    })();
    feeder.stop();
    drop(responses);
    if completion.recv_timeout(Duration::from_millis(50)).is_ok() {
        let _ = writer.join();
    } else {
        return Err(result
            .err()
            .unwrap_or_else(|| "Worker output pipe exceeded its shutdown deadline.".into()));
    }
    result?;
    if write_failure.load(Ordering::Acquire) {
        return Err("Worker output pipe failed.".into());
    }
    if let Some(error) = feeder.fault() {
        Err(error.into())
    } else {
        Ok(())
    }
}
#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
