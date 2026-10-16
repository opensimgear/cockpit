use super::*;
use crate::uinput::tests::binding;
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct State {
    ops: Vec<(u64, Vec<u8>)>,
    fail: usize,
    version: u32,
    short: bool,
    write_error: bool,
    writes: Vec<Vec<u8>>,
}
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<State>>);
impl Calls for Fake {
    fn arg(&mut self, r: u64, v: usize) -> io::Result<()> {
        self.bytes(r, &mut v.to_ne_bytes())
    }
    fn bytes(&mut self, r: u64, b: &mut [u8]) -> io::Result<()> {
        let mut s = self.0.lock().unwrap();
        s.ops.push((r, b.to_vec()));
        if s.fail == s.ops.len() {
            return Err(io::Error::other("fixture failure"));
        }
        if r == VERSION {
            b.copy_from_slice(&s.version.to_ne_bytes());
        }
        Ok(())
    }
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let mut s = self.0.lock().unwrap();
        s.writes.push(b.to_vec());
        if s.write_error {
            Err(io::Error::other("write failed"))
        } else {
            Ok(b.len() - usize::from(s.short))
        }
    }
}
fn fake() -> Fake {
    let f = Fake::default();
    f.0.lock().unwrap().version = 5;
    f
}
#[test]
fn ioctl_setup_exact_layout_and_syn_frame_and_destroy() {
    let f = fake();
    let t = Template::new(binding(1)).unwrap();
    let mut h = Handle::create(f.clone(), &t).unwrap();
    let events = t
        .events(&t.binding.report(&t.binding.fallback()).unwrap())
        .unwrap();
    h.emit(&events).unwrap();
    let s = f.0.lock().unwrap();
    assert_eq!(s.ops.last().unwrap().0, CREATE);
    let setup = &s.ops.iter().find(|(r, _)| *r == SETUP).unwrap().1;
    assert_eq!(setup.len(), 92);
    assert_eq!(&setup[..2], &6u16.to_ne_bytes());
    assert_eq!(s.writes[0].len(), events.len() * 24);
    assert_eq!(&s.writes[0][..16], &[0; 16]);
    drop(s);
    assert!(h.emit(&[]).is_err());
    assert!(h
        .emit(&vec![
            Event {
                kind: 0,
                code: 0,
                value: 0
            };
            50
        ])
        .is_err());
    assert!(h
        .emit(&[
            Event {
                kind: 0,
                code: 0,
                value: 0
            },
            Event {
                kind: 0,
                code: 0,
                value: 0
            }
        ])
        .is_err());
    f.0.lock().unwrap().short = true;
    assert!(h.emit(&events).is_err());
    f.0.lock().unwrap().write_error = true;
    assert!(h.emit(&events).is_err());
    drop(h);
    assert_eq!(f.0.lock().unwrap().ops.last().unwrap().0, DESTROY);
}
#[test]
fn every_configuration_failure_closes_before_creation() {
    let t = Template::new(binding(1)).unwrap();
    let f = fake();
    drop(Handle::create(f.clone(), &t).unwrap());
    let count = f.0.lock().unwrap().ops.len() - 1;
    for step in 1..=count {
        let f = fake();
        f.0.lock().unwrap().fail = step;
        assert!(Handle::create(f.clone(), &t).is_err());
        assert_eq!(f.0.lock().unwrap().ops.len(), step);
    }
    let f = fake();
    f.0.lock().unwrap().version = 4;
    assert!(Handle::create(f, &t).is_err());
}
#[test]
fn gate_readiness_lock_and_non_driver_paths_do_not_create_devices() {
    assert!(Native::open().is_err());
    assert_eq!(readiness()["qualified"], false);
    let lease = owner_lock().unwrap();
    assert!(owner_lock().is_err());
    drop(lease);
    drop(owner_lock().unwrap());
    let dir = tempfile::tempdir().unwrap();
    assert!(open_node(&dir.path().join("missing")).is_err());
    let path = dir.path().join("regular");
    std::fs::write(&path, []).unwrap();
    assert!(open_node(&path).is_err());
    let link = dir.path().join("link");
    std::os::unix::fs::symlink("/dev/null", &link).unwrap();
    assert!(open_node(&link).is_err());
    let mut k = Kernel(open_node(Path::new("/dev/null")).unwrap());
    assert!(k.arg(VERSION, 0).is_err());
    assert!(k.bytes(VERSION, &mut [0; 4]).is_err());
    assert_eq!(k.write(b"fixture").unwrap(), 7);
    assert!(ioctl_result(0).is_ok());
    let mut k = Kernel(open_node(Path::new("/dev/full")).unwrap());
    assert!(k.write(b"fixture").is_err());
}

#[test]
fn wire_layout_matches_the_installed_linux_uapi_headers() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("abi.c");
    let binary = dir.path().join("abi");
    std::fs::write(&source,r#"#include <linux/uinput.h>
#include <stddef.h>
#include <stdio.h>
int main(void) { printf("%zu %zu %zu %zu %lu %lu %lu %lu %lu %lu %lu %lu",sizeof(struct input_event),sizeof(struct uinput_setup),sizeof(struct uinput_abs_setup),offsetof(struct input_event,type),(unsigned long)UI_DEV_CREATE,(unsigned long)UI_DEV_DESTROY,(unsigned long)UI_DEV_SETUP,(unsigned long)UI_ABS_SETUP,(unsigned long)UI_SET_EVBIT,(unsigned long)UI_SET_KEYBIT,(unsigned long)UI_SET_PHYS,(unsigned long)UI_GET_VERSION); }
"#).unwrap();
    assert!(std::process::Command::new("cc")
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .status()
        .unwrap()
        .success());
    let output = std::process::Command::new(&binary).output().unwrap();
    assert!(output.status.success());
    let values = String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .map(|s| s.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![24, 92, 28, 16, CREATE, DESTROY, SETUP, ABS, EVBIT, KEYBIT, PHYS, VERSION]
    );
}

#[test]
fn dropping_owner_releases_lock_even_while_a_forked_child_holds_the_file_description() {
    use std::os::fd::FromRawFd;
    // Every file is private to this regression: no live controller or per-user
    // output lock is acquired, and other parallel tests need not serialize.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("owner.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    file.try_lock().unwrap();
    let lease = OwnerLease(file);
    let mut ready = [0; 2];
    let mut release = [0; 2];
    assert_eq!(
        unsafe { libc::pipe2(ready.as_mut_ptr(), libc::O_CLOEXEC) },
        0
    );
    let ready_read = unsafe { File::from_raw_fd(ready[0]) };
    let ready_write = unsafe { File::from_raw_fd(ready[1]) };
    assert_eq!(
        unsafe { libc::pipe2(release.as_mut_ptr(), libc::O_CLOEXEC) },
        0
    );
    let release_read = unsafe { File::from_raw_fd(release[0]) };
    let release_write = unsafe { File::from_raw_fd(release[1]) };
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        // A fork from Rust's multithreaded test process must not allocate, lock,
        // run destructors or call non-async-signal-safe Rust/library routines.
        unsafe {
            libc::alarm(5);
            libc::close(ready[0]);
            libc::close(release[1]);
            let mut byte = 1u8;
            if libc::write(ready[1], (&byte as *const u8).cast(), 1) != 1 {
                libc::_exit(2);
            }
            if libc::read(release[0], (&mut byte as *mut u8).cast(), 1) != 1 {
                libc::_exit(3);
            }
            libc::close(ready[1]);
            libc::close(release[0]);
            libc::_exit(0);
        }
    }
    assert!(pid > 0, "fork failed");
    struct Child(libc::pid_t);
    impl Drop for Child {
        fn drop(&mut self) {
            if self.0 > 0 {
                unsafe {
                    libc::kill(self.0, libc::SIGKILL);
                    while libc::waitpid(self.0, std::ptr::null_mut(), 0) < 0
                        && *libc::__errno_location() == libc::EINTR
                    {}
                }
            }
        }
    }
    let mut child = Child(pid);
    drop(ready_write);
    drop(release_read);
    let mut poll = libc::pollfd {
        fd: ready_read.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    assert_eq!(
        unsafe { libc::poll(&mut poll, 1, 2000) },
        1,
        "child did not reach its inherited-FD wait"
    );
    let mut byte = 0u8;
    assert_eq!(
        unsafe { libc::read(ready_read.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) },
        1
    );
    assert_eq!(byte, 1);
    assert_eq!(
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
        0
    );
    drop(lease);
    let replacement = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    replacement
        .try_lock()
        .expect("an inherited FD must not retain a released owner's lock");
    let _replacement = OwnerLease(replacement);
    assert_eq!(
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
        0,
        "child must still hold the inherited file description"
    );
    assert_eq!(
        unsafe { libc::write(release_write.as_raw_fd(), (&byte as *const u8).cast(), 1) },
        1
    );
    let mut status = 0;
    assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
    child.0 = 0;
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 0);
}
