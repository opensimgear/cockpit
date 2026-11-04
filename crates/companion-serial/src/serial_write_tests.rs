use super::*;

#[derive(Default)]
struct Writer {
    writes: Vec<(Vec<u8>, Instant)>,
    fail: bool,
}

impl DeadlineWriter for Writer {
    fn write_all(&mut self, bytes: &[u8], deadline: Instant) -> Result<(), String> {
        self.writes.push((bytes.to_vec(), deadline));
        if self.fail { Err("Write result is unknown.".into()) } else { Ok(()) }
    }
    fn control_lines(&mut self, _: bool, _: bool) -> Result<(), String> { Ok(()) }
}

#[test]
fn sends_one_baud_budgeted_record_and_prefixes_resynchronization() {
    let mut writer = Writer::default();
    let status = Arc::new(Mutex::new(Value::Null));
    let value = json!({"value":"x".repeat(6000)});
    let started = Instant::now();
    send_native(&mut writer, 115_200, &value, &status, true).unwrap();
    let (bytes, deadline) = &writer.writes[0];
    assert_eq!(bytes.first(), Some(&b'\n'));
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(serde_json::from_slice::<Value>(&bytes[1..bytes.len()-1]).unwrap(), value);
    assert!(*deadline >= started + budget(115_200, bytes.len()).unwrap());
    assert!(budget(115_200, bytes.len()).unwrap() > budget(921_600, bytes.len()).unwrap());
    let record = &status.lock().unwrap()["serialIo"];
    assert_eq!(record["writesCompleted"], 1);
    assert_eq!(record["lastWriteWrittenBytes"], bytes.len());
    assert!(record["readTimeoutMs"].is_null());
}

#[test]
fn failed_native_write_is_never_replayed_and_reports_unknown_bytes() {
    let mut writer = Writer { fail: true, ..Writer::default() };
    let status = Arc::new(Mutex::new(Value::Null));
    let value = json!({"command":"status"});
    assert!(send_native(&mut writer, 115_200, &value, &status, false).is_err());
    assert_eq!(writer.writes.len(), 1);
    writer.fail = false;
    send_native(&mut writer, 115_200, &value, &status, false).unwrap();
    let record = &status.lock().unwrap()["serialIo"];
    assert_eq!(record["writeAttempts"], 2);
    assert_eq!(record["writeFailures"], 1);
    assert_eq!(record["writesCompleted"], 1);
    assert!(record["lastWriteFailure"]["writtenBytes"].is_null());
}

#[test]
fn invalid_and_over_budget_records_never_reach_the_writer() {
    let mut writer = Writer::default();
    let status = Arc::new(Mutex::new(Value::Null));
    for (baud, value) in [(115_200, json!(false)), (115_200, json!({"value":"x".repeat(9000)})),
        (1, json!({"command":"status"})), (0, json!({"command":"status"}))] {
        assert!(send_native(&mut writer, baud, &value, &status, false).is_err());
    }
    assert!(writer.writes.is_empty());
}
