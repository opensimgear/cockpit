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
