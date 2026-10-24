use super::*;
use std::net::UdpSocket;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios")
}
fn connection(port: u16) -> Value {
    json!({"cataloguePath":fixtures(),"commandHost":"127.0.0.1","commandPort":7778,
        "listenAddress":"127.0.0.1","listenPort":port,"multicastGroup":"","multicastInterface":"0.0.0.0","freshnessMs":1500})
}
fn process() -> ProcessIdentity {
    ProcessIdentity {
        pid: 42,
        path: "DCS.exe".into(),
        epoch: 10,
    }
}
fn block(bytes: &mut Vec<u8>, address: u16, data: &[u8]) {
    bytes.extend_from_slice(&address.to_le_bytes());
    bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    bytes.extend_from_slice(data);
}
fn frame(name: &str, counter: u8) -> Vec<u8> {
    let mut bytes = vec![0x55; 4];
    let mut data = vec![0; 24];
    data[..name.len()].copy_from_slice(name.as_bytes());
    block(&mut bytes, 0, &data);
    block(&mut bytes, 65534, &[counter, 0]);
    bytes
}
fn scratch() -> tempfile::TempDir {
    crate::test_directory()
}
fn receive(source: &dcs_bios::Transport) -> Option<Vec<u8>> {
    for _ in 0..100 {
        if let Some(bytes) = source.receive().unwrap() {
            return Some(bytes);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
}
fn poll(reader: &mut Reader, now_ms: u64) -> Option<String> {
    for _ in 0..100 {
        if let Some(vehicle) = reader.poll(now_ms).unwrap() {
            return Some(vehicle);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
}

mod catalogues;
mod telemetry;
