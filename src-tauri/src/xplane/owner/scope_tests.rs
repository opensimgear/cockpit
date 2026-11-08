use super::*;
use std::sync::mpsc::channel;

fn connection(pid: u32) -> (UdpSocket, Value) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut connection = defaults(12);
    connection["listenPort"] = json!(0);
    connection["port"] = json!(socket.local_addr().unwrap().port());
    connection["processIdentity"] = json!({"pid":pid,"epoch":1,"path":"C:/X-Plane/X-Plane.exe"});
    (socket, connection)
}

fn start(connection: &Value) -> Result<Owner, String> {
    let catalogue = legacy_channels().into_iter().map(|channel| (channel.id.clone(), channel)).collect();
    Owner::start(connection, None, catalogue)
}

#[test]
fn slow_catalogue_open_does_not_block_another_process() {
    let (_slow_socket, slow) = connection(65101);
    let (_fast_socket, fast) = connection(65102);
    let (entered, opening) = channel();
    let (release, finish) = channel();
    let slow_worker = thread::spawn(move || {
        Source::open_with(&slow, vec![], || {
            entered.send(()).unwrap();
            finish.recv().map_err(|error| error.to_string())?;
            start(&slow)
        })
    });
    opening.recv_timeout(Duration::from_secs(3)).unwrap();
    let (ready, result) = channel();
    let fast_worker = thread::spawn(move || {
        ready.send(Source::open_with(&fast, vec![], || start(&fast))).unwrap();
    });

    let before_release = result.recv_timeout(Duration::from_secs(3));
    release.send(()).unwrap();
    let slow = slow_worker.join().unwrap().unwrap();
    fast_worker.join().unwrap();
    let fast = before_release.expect("another process was blocked by catalogue loading").unwrap();
    assert_ne!(slow.address_test(), fast.address_test());
}
