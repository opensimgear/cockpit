//! One bounded JSON record per write operation. Windows applies SerialPort's
//! timeout to writes too, so the worker's short read polling interval is not a
//! sufficient transmit deadline, especially on 115200-baud UART companions.
use serde_json::{json, Value};
use serialport::SerialPort;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const MAX_RECORD_BYTES: usize = 8192;
const WRITE_MARGIN_MS: u64 = 250;
const MAX_WRITE_MS: u64 = 3000;

fn budget(baud: u32, bytes: usize) -> Result<Duration, String> {
    if baud == 0 {
        return Err("USB baud rate is unavailable.".into());
    }
    // The transport uses 8N1: one start bit, eight data bits and one stop bit.
    let millis = (bytes as u64 * 10 * 1000).div_ceil(u64::from(baud)) + WRITE_MARGIN_MS;
    if millis > MAX_WRITE_MS {
        return Err("USB record exceeds the baud rate's bounded write deadline.".into());
    }
    Ok(Duration::from_millis(millis))
}
fn remaining_timeout(timeout: Duration, elapsed: Duration) -> Result<Duration, String> {
    // Windows floors the timeout to whole milliseconds. A positive fraction
    // would become zero and disable the total write timeout.
    timeout
        .checked_sub(elapsed)
        .filter(|remaining| *remaining >= Duration::from_millis(1))
        .ok_or_else(|| "USB write deadline expired.".to_owned())
}
fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}
fn increment(record: &mut Value, key: &str, count: u64) {
    record[key] = json!(record[key].as_u64().unwrap_or(0).saturating_add(count));
}

pub(super) fn send(
    port: &mut dyn SerialPort,
    value: &Value,
    status: &Arc<Mutex<Value>>,
    resynchronize: bool,
) -> Result<(), String> {
    if !value.is_object() {
        return Err("Invalid serial record.".into());
    }
    let mut bytes =
        Zeroizing::new(serde_json::to_vec(value).map_err(|_| "Invalid serial record.")?);
    if bytes.len() > MAX_RECORD_BYTES {
        return Err("Serial record exceeds device budget.".into());
    }
    if resynchronize {
        bytes.insert(0, b'\n');
    }
    bytes.push(b'\n');
    let read_timeout = port.timeout();
    let baud = port.baud_rate().ok();
    let timeout = baud
        .ok_or_else(|| "Cannot read USB baud rate.".to_owned())
        .and_then(|baud| budget(baud, bytes.len()));
    let started = Instant::now();
    let mut written = 0usize;
    let mut partial = 0u64;
    let result = match &timeout {
        Err(error) => Err(error.clone()),
        Ok(timeout) => {
            let write_result = (|| {
                while written < bytes.len() {
                    let remaining = remaining_timeout(*timeout, started.elapsed())?;
                    port.set_timeout(remaining)
                        .map_err(|_| "Cannot configure USB write timeout.")?;
                    match port.write(&bytes[written..]) {
                        Ok(0) => return Err(
                            "USB write made no progress; the incomplete record was not retried."
                                .to_owned(),
                        ),
                        Ok(count) if count <= bytes.len() - written => {
                            if count < bytes.len() - written {
                                partial = partial.saturating_add(1);
                            }
                            written += count;
                        }
                        Ok(_) => {
                            return Err("USB transport returned an invalid write count.".to_owned())
                        }
                        // A failed write can have transmitted an uncertain prefix.
                        // Never replay the record or retry the failed suffix.
                        Err(error) => {
                            return Err(format!(
                                "USB write failed ({:?}); the record was not retried.",
                                error.kind()
                            ))
                        }
                    }
                }
                Ok(())
            })();
            // Restore even after a failed timeout update: an implementation may
            // change its settings before reporting the error. The caller closes
            // the link if restoration fails rather than polling with a long read.
            match port.set_timeout(read_timeout) {
                Ok(()) => write_result,
                Err(_) => Err(match write_result {
                    Ok(()) => "USB read timeout could not be restored.".to_owned(),
                    Err(error) => format!("{error} USB read timeout could not be restored."),
                }),
            }
        }
    };
    let timeout_ms = timeout
        .as_ref()
        .map(|timeout| millis(*timeout))
        .unwrap_or(0);
    let elapsed_ms = millis(started.elapsed());
    let mut state = status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let record = &mut state["serialIo"];
    if !record.is_object() {
        *record = json!({});
    }
    record["baudRate"] = json!(baud);
    record["readTimeoutMs"] = json!(millis(read_timeout));
    increment(record, "writeAttempts", 1);
    increment(record, "writesCompleted", u64::from(written == bytes.len()));
    increment(record, "writeFailures", u64::from(result.is_err()));
    increment(record, "partialWrites", partial);
    increment(record, "totalWrittenBytes", written as u64);
    record["lastWriteBytes"] = json!(bytes.len());
    record["lastWriteWrittenBytes"] = json!(written);
    record["lastWriteTimeoutMs"] = json!(timeout_ms);
    record["lastWriteElapsedMs"] = json!(elapsed_ms);
    record["maxWriteElapsedMs"] = json!(record["maxWriteElapsedMs"]
        .as_u64()
        .unwrap_or(0)
        .max(elapsed_ms));
    record["maxWriteBytes"] = json!(record["maxWriteBytes"]
        .as_u64()
        .unwrap_or(0)
        .max(bytes.len() as u64));
    record["lastWriteError"] = json!(result.as_ref().err());
    if let Err(error) = &result {
        record["lastWriteFailure"] = json!({"bytes":bytes.len(),"writtenBytes":written,"timeoutMs":timeout_ms,"error":error});
    }
    result
}

#[cfg(test)]
#[path = "serial_write_tests.rs"]
mod tests;
