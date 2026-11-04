//! Write one bounded serial record with a baud-aware native deadline.
use serde_json::{json, Value};
use crate::native_duplex::DeadlineWriter;
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
fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}
fn increment(record: &mut Value, key: &str, count: u64) {
    record[key] = json!(record[key].as_u64().unwrap_or(0).saturating_add(count));
}

fn encode(value: &Value, resynchronize: bool) -> Result<Zeroizing<Vec<u8>>, String> {
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
    Ok(bytes)
}

pub(super) fn send_native(
    port: &mut dyn DeadlineWriter, baud: u32, value: &Value,
    status: &Arc<Mutex<Value>>, resynchronize: bool,
) -> Result<(), String> {
    let bytes = encode(value, resynchronize)?;
    let timeout = budget(baud, bytes.len());
    let started = Instant::now();
    let result = match &timeout {
        Ok(timeout) => port.write_all(&bytes, started + *timeout),
        Err(error) => Err(error.clone()),
    };
    report(status, baud, &timeout, started, bytes.len(), &result);
    result
}

fn report(status: &Arc<Mutex<Value>>, baud: u32, timeout: &Result<Duration, String>,
    started: Instant, bytes: usize, result: &Result<(), String>,
) {
    let written = result.is_ok().then_some(bytes);
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
    record["readTimeoutMs"] = Value::Null;
    increment(record, "writeAttempts", 1);
    increment(record, "writesCompleted", u64::from(result.is_ok()));
    increment(record, "writeFailures", u64::from(result.is_err()));
    increment(record, "totalWrittenBytes", written.unwrap_or(0) as u64);
    record["lastWriteBytes"] = json!(bytes);
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
        .max(bytes as u64));
    record["lastWriteError"] = json!(result.as_ref().err());
    if let Err(error) = &result {
        record["lastWriteFailure"] = json!({"bytes":bytes,"writtenBytes":written,"timeoutMs":timeout_ms,"error":error});
    }
}


#[cfg(test)]
#[path = "serial_write_tests.rs"]
mod tests;
