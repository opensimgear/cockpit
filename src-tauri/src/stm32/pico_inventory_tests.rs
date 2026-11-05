use super::*;
use crate::transport::duplex::{tcp_fixture, DuplexParts};
use std::io::{BufRead, BufReader, Write};

struct PicoFactory(Option<DuplexParts>);

impl Factory for PicoFactory {
    fn gateway_duplex(&mut self, _: &PortInfo) -> Option<Result<DuplexParts, String>> {
        self.0.take().map(Ok)
    }

    fn ports(&mut self) -> Result<Vec<PortInfo>, String> {
        Ok(self.0.is_some().then(|| PortInfo {
            path: "COM42".into(),
            serial_number: Some("OSG-PICO-0123456789abcdef".into()),
            vendor_id: Some("cafe".into()),
            product_id: Some("4010".into()),
            manufacturer: Some("OpenSimGear".into()),
            friendly_name: Some("Pico Debug".into()),
        }).into_iter().collect())
    }

    fn open(&mut self, _: &PortInfo, _: Duration) -> Result<Box<dyn Connection>, String> {
        panic!("Pico Debug opened a request-only transport")
    }
}

#[test]
fn pico_inventory_wakes_on_change_and_renews_watchdog_without_reading_state() {
    let (parts, mut peer) = tcp_fixture();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let (changed, change) = mpsc::channel();
    let (inspected, inspect) = mpsc::channel();
    let device = thread::spawn(move || {
        let mut lines = BufReader::new(peer.try_clone().unwrap()).lines();
        let state = tests::pico_state();
        let epoch = state["epoch"].as_u64().unwrap();
        let mut debug = tests::debug_state();
        let snapshot = |line: String| {
            let args: Vec<_> = line.split_ascii_whitespace().collect();
            assert_eq!(args.len(), 3, "{line}");
            assert_eq!((args[0], args[2]), ("cockpit", "snapshot"));
            args[1].parse::<u32>().unwrap()
        };
        let id = snapshot(lines.next().unwrap().unwrap());
        writeln!(peer, "{}", json!({"id":id,"ok":true,"state":state,"debug":debug})).unwrap();
        assert_eq!(lines.next().unwrap().unwrap(), "panel-stream");
        writeln!(peer, "{}", json!({"publicationStream":"change","epoch":epoch,"sequence":"1"})).unwrap();
        let id = snapshot(lines.next().unwrap().unwrap());
        writeln!(peer, "{}", json!({"id":id,"ok":true,"state":state,"debug":debug})).unwrap();

        // The first two idle deadlines may only send a compact safety renewal.
        for _ in 0..2 {
            let line = lines.next().unwrap().unwrap();
            let args: Vec<_> = line.split_ascii_whitespace().collect();
            assert_eq!(args.len(), 3, "{line}");
            assert_eq!(args[0], "debug-watchdog");
            assert_eq!(args[2].parse::<u64>().unwrap(), epoch);
            let id = args[1].parse::<u32>().unwrap();
            writeln!(peer, "{}", json!({"id":id,"ok":true})).unwrap();
        }
        writeln!(peer, "{}", json!({"publicationStream":"inventory","epoch":epoch,"sequence":"2"})).unwrap();
        let id = snapshot(lines.next().unwrap().unwrap());
        debug["devices"][0]["axes"][0] = json!(2048);
        writeln!(peer, "{}", json!({"id":id,"ok":true,"state":state,"debug":debug})).unwrap();
        changed.send(()).unwrap();
        inspect.recv_timeout(Duration::from_secs(3)).unwrap();
    });
    let timing = Timing { heartbeat: Duration::from_millis(250), ..Timing::default() };
    let service = Stm32Service::with_role(Box::new(PicoFactory(Some(parts))), timing, Role::PicoDebug);
    service.set_workspace_owned(true);
    let connected = service.request(json!({"command":"connect","port":"COM42","clientId":"pico-inventory-test"}));
    assert_eq!(connected["ok"], true, "{connected}");
    change.recv_timeout(Duration::from_secs(3)).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if service.cached_state()["debug"]["devices"][0]["axes"][0] == 2048 { break; }
        assert!(Instant::now() < until, "Pico workspace never received the changed axis");
        thread::sleep(Duration::from_millis(2));
    }
    inspected.send(()).unwrap();
    device.join().unwrap();
}
