use super::*;
use std::{cell::Cell, collections::VecDeque};
struct Fixture {
    events: VecDeque<Event>,
    valid: Cell<bool>,
    reply: Vec<u8>,
    called: Cell<bool>,
}
impl Transport for Fixture {
    fn verify(&self) -> Result<(), Value> {
        if self.valid.get() {
            Ok(())
        } else {
            Err(failure("permission-denied", "revoked"))
        }
    }
    fn poll(&mut self, _: Duration) -> Result<Option<Event>, Value> {
        Ok(self.events.pop_front())
    }
    fn get(&mut self, _: &Get, _: usize, _: Duration) -> Result<Vec<u8>, Value> {
        self.called.set(true);
        Ok(self.reply.clone())
    }
}
fn capture(numbered: bool) -> Capture<Fixture> {
    let mut raw = vec![5, 1, 9, 4, 0xa1, 1];
    if numbered {
        raw.extend([0x85, 7]);
    }
    raw.extend([
        9, 0x30, 0x15, 0x81, 0x25, 0x7f, 0x75, 8, 0x95, 1, 0x81, 6, 9, 0x31, 0xb1, 2, 0xc0,
    ]);
    Capture::new(
        Fixture {
            events: VecDeque::new(),
            valid: Cell::new(true),
            reply: vec![0xfe],
            called: Cell::new(false),
        },
        Some(parse_descriptor(&raw).unwrap()),
    )
}
fn get(id: u8, max: usize) -> Get {
    Get::parse(
        &json!({"requestId":"r","kind":"feature","reportId":id,"maxBytes":max,"deadlineMs":100}),
    )
    .unwrap()
}
#[test]
fn raw_callback_keeps_os_bytes_and_out_of_band_id() {
    let mut c = capture(true);
    c.io.events.push_back(Event::Report {
        bytes: vec![0xfe],
        report_id: 7,
        timestamp: 99,
    });
    let raw = c.poll(Duration::ZERO).unwrap().unwrap();
    assert_eq!(raw, vec![0xfe]);
    assert_eq!(c.project(&raw)["values"][0]["sample"]["Integer"], "-2");
    assert_eq!(c.provenance().unwrap()["reportId"], 7);
    assert_eq!(c.provenance().unwrap()["timestamp"], "99");
}
#[test]
fn prefixed_callback_is_decoded_without_fabricating_or_doubling_id() {
    let mut c = capture(true);
    c.io.events.push_back(Event::Report {
        bytes: vec![7, 0xfe],
        report_id: 7,
        timestamp: 0,
    });
    let raw = c.poll(Duration::ZERO).unwrap().unwrap();
    assert_eq!(raw, vec![7, 0xfe]);
    assert_eq!(c.project(&raw)["values"][0]["sample"]["Integer"], "-2");
}
#[test]
fn element_event_cannot_be_presented_as_wire_report() {
    let mut c = capture(false);
    c.io.events.push_back(Event::Element {
        sample: json!({"integer":"-2","relative":true}),
        report_id: 0,
        timestamp: 1,
    });
    let raw = c.poll(Duration::ZERO).unwrap().unwrap();
    assert!(raw.is_empty());
    assert_eq!(c.provenance().unwrap()["wireAvailable"], false);
    assert_eq!(c.project(&raw)["elements"][0]["integer"], "-2");
}
#[test]
fn revoked_permission_prevents_buffered_delivery_and_get() {
    let mut c = capture(false);
    c.io.valid.set(false);
    c.io.events.push_back(Event::Report {
        bytes: vec![0xfe],
        report_id: 0,
        timestamp: 0,
    });
    assert_eq!(
        c.poll(Duration::ZERO).unwrap_err()["kind"],
        "permission-denied"
    );
    assert_eq!(
        c.get(&get(0, 1), Duration::from_millis(50)).unwrap_err()["kind"],
        "permission-denied"
    );
    assert!(!c.io.called.get());
}
#[test]
fn get_rejects_undeclared_capacity_and_elapsed_requests_before_io() {
    let mut c = capture(true);
    assert_eq!(
        c.get(&get(8, 8), Duration::from_millis(50)).unwrap_err()["kind"],
        "unsupported"
    );
    assert_eq!(
        c.get(&get(7, 1), Duration::from_millis(50)).unwrap_err()["kind"],
        "capacity"
    );
    assert_eq!(
        c.get(&get(7, 8), Duration::ZERO).unwrap_err()["kind"],
        "timeout"
    );
    assert!(!c.io.called.get());
}
#[test]
fn get_validates_response_length_and_preserves_get_source() {
    let mut c = capture(true);
    let v = c.get(&get(7, 2), Duration::from_millis(50)).unwrap();
    assert_eq!(v["wireHex"], "fe");
    assert_eq!(v["provenance"]["source"], "iohid-get-report");
    assert_eq!(v["samples"]["values"][0]["sample"]["Integer"], "-2");
    c.io.reply = vec![7, 1, 2];
    assert_eq!(
        c.get(&get(7, 8), Duration::from_millis(50)).unwrap_err()["kind"],
        "malformed"
    );
}
#[test]
fn missing_descriptor_keeps_raw_reports_but_cannot_authorize_get() {
    let mut c = capture(false);
    c.parsed = None;
    c.io.events.push_back(Event::Report {
        bytes: vec![3],
        report_id: 12,
        timestamp: 0,
    });
    let raw = c.poll(Duration::ZERO).unwrap().unwrap();
    assert!(c.project(&raw)["unavailable"].is_string());
    assert_eq!(
        c.get(&get(0, 8), Duration::from_millis(50)).unwrap_err()["kind"],
        "unsupported"
    );
}

#[test]
fn native_callback_order_preserves_repeated_relative_changes_and_button_edges() {
    let stream = crate::hid::session::Session::start(|| {
        let mut c = capture(true);
        for _ in 0..512 {
            c.io.events.push_back(Event::Report {
                bytes: vec![0xff],
                report_id: 7,
                timestamp: 0,
            });
        }
        c.io.events.push_back(Event::Element {
            sample: json!({"usagePage":9,"usage":1,"integer":"1"}),
            report_id: 7,
            timestamp: 1,
        });
        Ok(Box::new(c))
    })
    .unwrap();
    let started = Instant::now();
    let mut cursor = 0;
    while cursor < 513 {
        let page = stream.page(cursor).unwrap();
        for row in page["reports"].as_array().unwrap() {
            cursor += 1;
            assert_eq!(row["sequence"], cursor.to_string());
            if cursor <= 512 {
                assert_eq!(row["wireHex"], "ff");
                assert_eq!(row["samples"]["values"][0]["sample"]["Integer"], "-1");
                assert_eq!(row["samples"]["values"][0]["semantics"], "Relative");
            } else {
                assert!(row.get("wireHex").is_none());
                assert_eq!(row["provenance"]["source"], "iohid-element");
                assert_eq!(row["samples"]["elements"][0]["integer"], "1");
            }
        }
        assert!(started.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
}
