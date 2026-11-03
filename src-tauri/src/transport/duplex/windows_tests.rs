//! Exercise the real Windows overlapped wait/cancel path without serial hardware.
//! Serial-port DCB/control-line qualification still requires an actual device.
use super::*;
use std::{sync::mpsc, time::Duration};
use windows_sys::Win32::{Storage::FileSystem::PIPE_ACCESS_DUPLEX,
    System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT}};

fn fixture() -> (Arc<Shared>, Arc<Shared>) {
    let name = format!("\\\\.\\pipe\\osg-duplex-test-{}", uuid::Uuid::new_v4())
        .encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let server = unsafe { CreateNamedPipeW(name.as_ptr(), PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 1, 4096, 4096, 0, ptr::null()) };
    assert_ne!(server, INVALID_HANDLE_VALUE);
    let server = Handle(server);let completed = event().unwrap();
    let mut connection: OVERLAPPED = unsafe { std::mem::zeroed() };connection.hEvent = completed.0;
    let connection = ptr::addr_of_mut!(connection);
    let result = unsafe { ConnectNamedPipe(server.0, connection) };
    assert!(result != 0 || unsafe { GetLastError() } == ERROR_IO_PENDING);
    let client = unsafe { CreateFileW(name.as_ptr(), GENERIC_READ | GENERIC_WRITE, 0, ptr::null(),
        OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED, ptr::null_mut()) };
    assert_ne!(client, INVALID_HANDLE_VALUE);
    let client = Handle(client);let mut ignored = 0;
    assert_ne!(unsafe { GetOverlappedResult(server.0, connection, &mut ignored, 1) }, 0);
    let share = |port| Arc::new(Shared { port, stop:event().unwrap(), stopped:AtomicBool::new(false) });
    (share(server), share(client))
}

#[test]
fn persistent_stop_cancels_before_submission_and_an_outstanding_overlapped_read() {
    let (server, _peer) = fixture();
    let cancel = Cancel(Arc::downgrade(&server));cancel.cancel();cancel.cancel();
    assert!(server.read(&mut [0]).is_err());
    assert!(server.write(b"late", Instant::now() + Duration::from_secs(1)).is_err());

    let (server, _peer) = fixture();let cancel = Cancel(Arc::downgrade(&server));
    let (entered, ready) = mpsc::channel();let (sent, received) = mpsc::channel();
    let reader = std::thread::spawn(move || {entered.send(()).unwrap();sent.send(server.read(&mut [0])).unwrap();});
    ready.recv_timeout(Duration::from_secs(2)).unwrap();cancel.cancel();
    let result = received.recv_timeout(Duration::from_secs(2));
    if result.is_ok() { reader.join().unwrap(); }
    assert!(result.expect("overlapped read did not acknowledge cancellation").is_err());
}

#[test]
fn overlapped_read_does_not_hold_up_independent_writer_and_stop_between_reads_is_final() {
    let (server, peer) = fixture();let read_server = server.clone();
    let (sent, received) = mpsc::channel();
    let reader = std::thread::spawn(move || {let mut value = [0];let result = read_server.read(&mut value);sent.send((result,value)).unwrap();});
    let deadline = Instant::now() + Duration::from_secs(2);
    assert_eq!(server.write(b"a", deadline).unwrap(), 1);
    let mut value = [0];assert_eq!(peer.read(&mut value).unwrap(), 1);assert_eq!(&value, b"a");
    assert_eq!(peer.write(b"b", deadline).unwrap(), 1);
    let result = received.recv_timeout(Duration::from_secs(2));
    Cancel(Arc::downgrade(&server)).cancel();reader.join().unwrap();
    let (count, value) = result.unwrap();assert_eq!(count.unwrap(), 1);assert_eq!(&value, b"b");
    assert!(server.read(&mut [0]).is_err());
}

#[test]
fn blocked_overlapped_write_observes_deadline_and_waits_for_terminal_cancellation() {
    let (server, _peer) = fixture();let started = Instant::now();
    assert!(server.write(&vec![0; 1024 * 1024], started + Duration::from_millis(40)).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(server.write(b"late", Instant::now() - Duration::from_millis(1)).is_err());
}

#[test]
fn serial_adapter_contract_delivers_first_byte_and_rejects_unavailable_control_status(){
    let (server,peer)=fixture();
    let mut receiver=Receiver(server.clone());let mut writer=Writer(server.clone());
    assert_eq!(receiver.receive(&mut []).unwrap(),0);
    let deadline=Instant::now()+Duration::from_secs(2);
    writer.write_all(b"out",deadline).unwrap();
    let mut bytes=[0;3];let mut offset=0;
    while offset<bytes.len(){offset+=peer.read(&mut bytes[offset..]).unwrap();}
    assert_eq!(&bytes,b"out");
    assert_eq!(peer.write(b"i",deadline).unwrap(),1);
    let mut first=[0];assert_eq!(receiver.receive(&mut first).unwrap(),1);assert_eq!(&first,b"i");
    // Pipes exercise real overlapped I/O but are not serial devices. A missing
    // COM status/control operation must fail rather than invent readiness.
    assert!(writer.control_lines(true,false).is_err());
    assert_eq!(peer.write(b"n!",deadline).unwrap(),2);
    assert!(receiver.receive(&mut [0;2]).is_err());
    Cancel(Arc::downgrade(&server)).cancel();
    assert!(receiver.receive(&mut [0]).is_err());
    assert!(writer.write_all(b"late",deadline).is_err());
    assert!(writer.control_lines(false,false).is_err());
}

#[test]
fn absent_native_device_open_reports_the_os_error_without_creating_a_device(){
    let port=PortInfo{path:format!("\\\\.\\pipe\\osg-absent-{}",uuid::Uuid::new_v4()),
        serial_number:None,vendor_id:None,product_id:None,manufacturer:None,friendly_name:None};
    let error=open(&port).err().expect("a nonexistent private endpoint cannot open");
    assert!(error.contains("Could not open USB duplex port"));
}
