//! Explicit /dev/uinput access. Inspection queries version/access without creating.
//! ioctl ABI follows linux/uinput.h on supported x86_64/aarch64 Linux targets.
use crate::uinput::{self, Backend, Device, Event, Template};
use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::Path,
};
const CREATE: u64 = 0x5501;
const DESTROY: u64 = 0x5502;
const SETUP: u64 = 0x405c5503;
const ABS: u64 = 0x401c5504;
const EVBIT: u64 = 0x40045564;
const KEYBIT: u64 = 0x40045565;
const ABSBIT: u64 = 0x40045567;
const PHYS: u64 = 0x4008556c;
const VERSION: u64 = 0x8004552d;
pub fn readiness() -> serde_json::Value {
    let result = open_node(Path::new("/dev/uinput")).and_then(|file| {
        let mut kernel = Kernel(file);
        let mut version = [0u8; 4];
        kernel
            .bytes(VERSION, &mut version)
            .map_err(|e| format!("Unable to query the uinput protocol: {e}"))?;
        let version = u32::from_ne_bytes(version);
        if version < 5 {
            return Err("The uinput kernel interface must support protocol 5 or newer.".into());
        }
        Ok(version)
    });
    match result {
        Ok(version) => {
            serde_json::json!({"state":"unqualified","qualified":false,"permissionVerified":true,"protocol":version,"reason":uinput::REASON})
        }
        Err(error) => {
            serde_json::json!({"state":"setup-required","qualified":false,"permissionVerified":false,"reason":format!("{error} Ask your administrator to enable the uinput kernel interface and grant only your session access to /dev/uinput. Do not run Cockpit as root or grant all input nodes global write access.")})
        }
    }
}

trait Calls {
    fn arg(&mut self, request: u64, value: usize) -> io::Result<()>;
    fn bytes(&mut self, request: u64, bytes: &mut [u8]) -> io::Result<()>;
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize>;
}
struct Kernel(File);
impl Calls for Kernel {
    fn arg(&mut self, request: u64, value: usize) -> io::Result<()> {
        ioctl_result(unsafe { libc::ioctl(self.0.as_raw_fd(), request as libc::c_ulong, value) })
    }
    fn bytes(&mut self, request: u64, bytes: &mut [u8]) -> io::Result<()> {
        ioctl_result(unsafe {
            libc::ioctl(
                self.0.as_raw_fd(),
                request as libc::c_ulong,
                bytes.as_mut_ptr(),
            )
        })
    }
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = unsafe { libc::write(self.0.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(result as usize)
        }
    }
}
fn ioctl_result(result: i32) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn open_node(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| {
            format!("uinput access failed: {e}; grant a scoped session ACL, then retry explicitly.")
        })?;
    if !file
        .metadata()
        .map_err(|e| e.to_string())?
        .file_type()
        .is_char_device()
    {
        return Err("uinput path is not a character device.".into());
    }
    Ok(file)
}
struct Handle<C: Calls> {
    calls: C,
    created: bool,
}
impl<C: Calls> Handle<C> {
    fn create(mut calls: C, t: &Template) -> Result<Self, String> {
        let result = (|| -> io::Result<()> {
            let mut version = [0u8; 4];
            calls.bytes(VERSION, &mut version)?;
            if u32::from_ne_bytes(version) < 5 {
                return Err(io::Error::other("uinput protocol 5 or newer is required"));
            }
            let mut physical = t.physical.as_bytes().to_vec();
            physical.push(0);
            calls.bytes(PHYS, &mut physical)?;
            if t.binding.layout.buttons > 0 {
                calls.arg(EVBIT, 1)?;
                for n in 0..t.binding.layout.buttons {
                    calls.arg(KEYBIT, usize::from(0x120 + n))?;
                }
            }
            let axes = t
                .binding
                .axes
                .iter()
                .map(|u| ((*u - 0x30) as u16, 0, 32767))
                .chain((0..t.binding.layout.hats).flat_map(|h| {
                    [
                        (16 + u16::from(h) * 2, -1, 1),
                        (17 + u16::from(h) * 2, -1, 1),
                    ]
                }));
            for (code, min, max) in axes {
                calls.arg(EVBIT, 3)?;
                calls.arg(ABSBIT, usize::from(code))?;
                let mut data = [0u8; 28];
                data[..2].copy_from_slice(&code.to_ne_bytes());
                data[8..12].copy_from_slice(&i32::to_ne_bytes(min));
                data[12..16].copy_from_slice(&i32::to_ne_bytes(max));
                calls.bytes(ABS, &mut data)?;
            }
            let mut setup = [0u8; 92];
            // BUS_VIRTUAL, local identifiers (not a claim to a physical USB VID).
            for (i, value) in [6u16, 0, 0xc0c0 + u16::from(t.binding.slot), 1]
                .into_iter()
                .enumerate()
            {
                setup[i * 2..i * 2 + 2].copy_from_slice(&value.to_ne_bytes());
            }
            setup[8..8 + t.name.len()].copy_from_slice(t.name.as_bytes());
            calls.bytes(SETUP, &mut setup)?;
            calls.arg(CREATE, 0)?;
            Ok(())
        })();
        result.map_err(|e| format!("uinput setup failed; creator handle closed: {e}"))?;
        Ok(Self {
            calls,
            created: true,
        })
    }
}
impl<C: Calls> Device for Handle<C> {
    fn emit(&mut self, events: &[Event]) -> Result<(), String> {
        if events.len() > 49
            || events.last()
                != Some(&Event {
                    kind: 0,
                    code: 0,
                    value: 0,
                })
            || events[..events.len().saturating_sub(1)]
                .iter()
                .any(|e| e.kind == 0)
        {
            return Err("uinput requires one bounded complete SYN_REPORT batch.".into());
        }
        let mut bytes = vec![0u8; events.len() * 24];
        for (event, chunk) in events.iter().zip(bytes.chunks_exact_mut(24)) {
            chunk[16..18].copy_from_slice(&event.kind.to_ne_bytes());
            chunk[18..20].copy_from_slice(&event.code.to_ne_bytes());
            chunk[20..24].copy_from_slice(&event.value.to_ne_bytes());
        }
        match self.calls.write(&bytes) {
            Ok(n) if n == bytes.len() => Ok(()),
            Ok(_) => Err("Partial uinput report; stop and close the device without replay.".into()),
            Err(e) => Err(format!(
                "uinput report failed; stop and close the device: {e}"
            )),
        }
    }
}
impl<C: Calls> Drop for Handle<C> {
    fn drop(&mut self) {
        if self.created {
            let _ = self.calls.arg(DESTROY, 0);
            self.created = false;
        }
    }
}
/// Release this process's ownership explicitly before closing. CLOEXEC alone
/// does not close a file description temporarily inherited between fork and exec.
struct OwnerLease(File);
impl Drop for OwnerLease {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
pub struct Native {
    _lease: OwnerLease,
}
impl Native {
    pub fn open() -> Result<Self, String> {
        if !uinput::QUALIFIED {
            return Err(uinput::REASON.into());
        }
        if !cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
            return Err("This uinput ABI architecture is not supported.".into());
        }
        Ok(Self {
            _lease: owner_lock()?,
        })
    }
}
fn owner_lock() -> Result<OwnerLease, String> {
    let uid = unsafe { libc::geteuid() };
    let path = std::env::temp_dir().join(format!("opensimgear-cockpit-uinput-{uid}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| e.to_string())?;
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.uid() != uid || m.mode() & 0o077 != 0 {
        return Err("Unsafe uinput ownership lock; preserved without modification.".into());
    }
    file.try_lock()
        .map_err(|_| "Another Cockpit output owner is still alive; no device is recreated.")?;
    Ok(OwnerLease(file))
}
pub struct NativeDevice(Handle<Kernel>);
impl Device for NativeDevice {
    fn emit(&mut self, events: &[Event]) -> Result<(), String> {
        self.0.emit(events)
    }
}
impl Backend for Native {
    type Device = NativeDevice;
    fn ready(&mut self) -> Result<(), String> {
        drop(open_node(Path::new("/dev/uinput"))?);
        Ok(())
    }
    fn create(&mut self, t: &Template) -> Result<NativeDevice, String> {
        Ok(NativeDevice(Handle::create(
            Kernel(open_node(Path::new("/dev/uinput"))?),
            t,
        )?))
    }
}
#[cfg(test)]
#[path = "linux_uinput_tests.rs"]
mod tests;
