use super::*;
use std::{cell::Cell, collections::VecDeque};
struct Fixture {
    reports: VecDeque<io::Receive>,
    reply: Vec<u8>,
    valid: Cell<bool>,
    expected_length: usize,
}
impl Transport for Fixture {
    fn verify(&self) -> Result<(), Value> {
        if self.valid.get() {
            Ok(())
        } else {
            Err(failure("stale-generation", "descriptor changed"))
        }
    }
    fn poll(&mut self, _: Duration) -> io::Receive {
        self.reports.pop_front().unwrap_or(io::Receive::Timeout)
    }
    fn get(&mut self, _: &Get, len: usize, _: Duration) -> Result<Vec<u8>, Value> {
        assert_eq!(len, self.expected_length);
        Ok(self.reply.clone())
    }
}
fn capture(numbered: bool) -> Capture<Fixture> {
    let mut bytes = vec![0x05, 1, 0x09, 4, 0xa1, 1];
    if numbered {
        bytes.extend([0x85, 7]);
    }
    bytes.extend([
        0x09, 0x30, 0x15, 0x81, 0x25, 0x7f, 0x75, 8, 0x95, 1, 0x81, 6, 0x09, 0x31, 0xb1, 2, 0xc0,
    ]);
    let parsed = parse(&bytes).unwrap();
    assert!(parsed.diagnostics.is_empty());
    let reply = if numbered { vec![7, 0xfe] } else { vec![0xfe] };
    Capture::new(
        Fixture {
            reports: VecDeque::new(),
            reply,
            valid: Cell::new(true),
            expected_length: 2,
        },
        parsed,
    )
}
fn get(id: u8, kind: &str, max: usize) -> Get {
    Get::parse(&json!({"requestId":"r","reportId":id,"kind":kind,"maxBytes":max,"deadlineMs":100}))
        .unwrap()
}
#[test]
fn fresh_capture_rechecks_authority_before_delivering_a_report() {
    let mut c = capture(true);
    c.io.reports.push_back(io::Receive::Report(vec![7, 0xfe]));
    c.io.valid.set(false);
    assert_eq!(c.poll(Duration::ZERO).unwrap_err()["kind"], "stale-generation");
}
#[test]
fn linux_stream_preserves_repeated_deltas_unknown_ids_and_order() {
    let mut c = capture(true);
    for _ in 0..250 {
        c.io.reports.push_back(io::Receive::Report(vec![7, 0xfe]));
    }
    c.io.reports.push_back(io::Receive::Report(vec![8, 1]));
    c.io.reports.push_back(io::Receive::Removed);
    for _ in 0..250 {
        let bytes = c.poll(Duration::ZERO).unwrap().unwrap();
        let v = c.project(&bytes);
        assert_eq!(v["raw"], "07fe");
        assert_eq!(v["report_id"], 7);
        assert_eq!(v["values"][0]["sample"]["Integer"], "-2");
        assert_eq!(v["values"][0]["semantics"], "Relative");
    }
    let unknown = c.poll(Duration::ZERO).unwrap().unwrap();
    assert_eq!(c.project(&unknown)["values"], json!([]));
    assert_eq!(c.poll(Duration::ZERO).unwrap_err()["kind"], "removed");
    assert_eq!(c.poll(Duration::ZERO).unwrap(), None);
}

#[test]
fn sustained_relative_motion_and_button_edges_survive_cursor_paging() {
    use crate::hid::session::Session;
    let raw = [0x05,1,0x09,4,0xa1,1,
        0x05,9,0x09,1,0x15,0,0x25,1,0x75,1,0x95,1,0x81,2,
        0x75,7,0x81,1,0x05,1,0x09,0x30,0x15,0x81,0x25,0x7f,0x75,8,0x81,6,0xc0];
    let stream=Session::start(move ||{
        let mut c=Capture::new(capture(false).io,parse(&raw)?);
        for i in 0..512 {c.io.reports.push_back(io::Receive::Report(vec![(i%2) as u8,0xff]));}
        c.io.reports.push_back(io::Receive::Removed);
        Ok(Box::new(c))
    }).unwrap();
    let started=Instant::now();
    let mut cursor=0;
    while cursor<512 {
        let page=stream.page(cursor).unwrap();
        assert_eq!(page["gap"],false);
        for row in page["reports"].as_array().unwrap() {
            let bit=(cursor%2).to_string();
            cursor+=1;
            assert_eq!(row["sequence"],cursor.to_string());
            assert_eq!(row["samples"]["values"][0]["sample"]["Integer"],bit);
            assert_eq!(row["samples"]["values"][2]["sample"]["Integer"],"-1");
            assert_eq!(row["samples"]["values"][2]["semantics"],"Relative");
        }
        assert!(started.elapsed()<Duration::from_secs(8));
        std::thread::yield_now();
    }
}
#[test]
fn get_distinguishes_selector_buffer_from_returned_unnumbered_payload() {
    for numbered in [true, false] {
        let mut c = capture(numbered);
        let id = if numbered { 7 } else { 0 };
        for kind in ["input", "feature"] {
            let result = c.get(&get(id, kind, 2), Duration::from_secs(1)).unwrap();
            assert_eq!(result["samples"]["values"][0]["sample"]["Integer"], "-2");
            assert_eq!(result["wireHex"], if numbered { "07fe" } else { "fe" });
        }
        assert!(c.get(&get(4, "input", 2), Duration::from_secs(1)).is_err());
        assert!(c.get(&get(id, "input", 2), Duration::ZERO).is_err());
        if numbered {
            assert!(c.get(&get(id, "input", 1), Duration::from_secs(1)).is_err());
        }
        c.io.reply = vec![];
        assert_eq!(
            c.get(&get(id, "input", 2), Duration::from_secs(1))
                .unwrap_err()["kind"],
            "malformed"
        );
        c.io.valid.set(false);
        assert_eq!(
            c.get(&get(id, "input", 2), Duration::from_secs(1))
                .unwrap_err()["kind"],
            "stale-generation"
        );
        c.last_verified = Instant::now() - Duration::from_secs(1);
        assert_eq!(
            c.poll(Duration::ZERO).unwrap_err()["kind"],
            "stale-generation"
        );
    }
}
