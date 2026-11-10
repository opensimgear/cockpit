use super::*;
use std::{sync::mpsc, thread};

#[test]
fn cancellation_interrupts_the_wait_and_stays_signalled() {
    let wait = CaptureWait::new().unwrap();
    let worker_wait = wait.clone();
    let (enter, entering) = mpsc::sync_channel(1);
    let (finish, finished) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        enter.send(()).unwrap();
        let long = Duration::from_secs(4);
        worker_wait.wait(long).unwrap();
        worker_wait.wait(long).unwrap();
        finish.send(()).unwrap();
    });
    entering.recv_timeout(Duration::from_secs(1)).unwrap();
    wait.cancel();
    finished.recv_timeout(Duration::from_secs(1)).unwrap();
    worker.join().unwrap();
}
