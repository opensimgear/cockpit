use super::*;
use serde_json::{json, Value};
use std::sync::mpsc;

#[test]
fn errors_and_recovery_notify_once_outside_writer_and_database_locks() {
    let directory = crate::test_directory();
    let database = crate::Database::open(directory.path().join("observations.sqlite3")).unwrap();
    let repository = Repository::new(database.clone(), "observations", "status", 1024, |value: &Value| {
        if value.is_object() { Ok(()) } else { Err("Invalid observation".into()) }
    }).unwrap();
    repository.load().unwrap();
    let writer = Arc::new(ObservationWriter::new(repository).unwrap());
    let weak = Arc::downgrade(&writer);
    let (sent, received) = mpsc::channel();
    writer.watch_error(Arc::new(move || {
        // Both reads require locks held during publication/save; this callback
        // would deadlock if the worker kept either lock while notifying.
        let error = weak.upgrade().unwrap().error();
        database.read("observations", "status").unwrap();
        sent.send(error).unwrap();
    }));
    writer.queue(json!([])).unwrap();
    assert_eq!(received.recv_timeout(Duration::from_secs(2)).unwrap(), Some("Invalid observation".into()));
    writer.queue(json!({"valid":true})).unwrap();
    writer.flush().unwrap();
    assert_eq!(received.recv_timeout(Duration::from_secs(2)).unwrap(), None);
    writer.queue(json!({"valid":2})).unwrap();
    writer.flush().unwrap();
    assert!(received.recv_timeout(Duration::from_millis(50)).is_err());
    writer.shutdown().unwrap();
}
