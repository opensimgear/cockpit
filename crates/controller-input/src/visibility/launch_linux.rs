//! Linux-only bubblewrap boundary. No setuid launcher, global chmod, grab or shell.
use super::{
    launch::{self, Child, Poll, Request},
    launch_plan::{self, Node},
    session::Source,
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            process::CommandExt,
        },
    },
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

const BWRAP: &str = "/usr/bin/bwrap";
// A deployed helper must be separately approved; request/settings/env cannot override this.
const APPROVED_HELPER_SHA256: Option<&str> = None;
type Inventory = Arc<dyn Fn() -> Result<Vec<Node>, String> + Send + Sync>;

fn group(device: &udev::Device) -> Result<String, String> {
    let mut next = Some(device.clone());
    while let Some(d) = next {
        if d.devtype().is_some_and(|v| v == "usb_device")
            || d.subsystem().is_some_and(|v| v == "hid")
        {
            // Prefer the whole USB device, so sibling interfaces/hidraw cannot bypass a selection.
            if let Some(usb) = d
                .parent_with_subsystem_devtype("usb", "usb_device")
                .map_err(|e| e.to_string())?
            {
                return Ok(usb.syspath().to_string_lossy().into());
            }
            return Ok(d.syspath().to_string_lossy().into());
        }
        next = d.parent();
    }
    Ok(device
        .syspath()
        .parent()
        .ok_or("Missing device topology.")?
        .to_string_lossy()
        .into())
}
fn phys(device: &udev::Device) -> String {
    let mut next = Some(device.clone());
    while let Some(d) = next {
        if let Some(value) = d.attribute_value("phys") {
            return value.to_string_lossy().into();
        }
        next = d.parent();
    }
    String::new()
}
fn inventory() -> Result<Vec<Node>, String> {
    let mut result = Vec::new();
    for subsystem in ["input", "hidraw"] {
        let mut enumeration = udev::Enumerator::new().map_err(|e| e.to_string())?;
        enumeration
            .match_subsystem(subsystem)
            .map_err(|e| e.to_string())?;
        for device in enumeration.scan_devices().map_err(|e| e.to_string())? {
            let Some(path) = device
                .devnode()
                .and_then(Path::to_str)
                .filter(|p| launch_plan::device_path(p))
            else {
                continue;
            };
            let meta = std::fs::symlink_metadata(path)
                .map_err(|e| format!("Input inventory changed: {e}"))?;
            if meta.mode() & libc::S_IFMT != libc::S_IFCHR {
                return Err("Input node is not a direct character device.".into());
            }
            let physical = phys(&device);
            let virtual_device = device
                .syspath()
                .to_string_lossy()
                .contains("/devices/virtual/");
            if virtual_device && !physical.starts_with("opensimgear-virtual:") {
                return Err("Unapproved virtual input path; isolated launch refused.".into());
            }
            let topology = device
                .property_value("ID_PATH")
                .and_then(|v| v.to_str())
                .unwrap_or("");
            result.push(Node {
                path: path.into(),
                group: group(&device)?,
                identity: if path.starts_with("/dev/input/event") && !topology.is_empty() {
                    format!("{topology}:{physical}")
                } else {
                    String::new()
                },
                virtual_device,
                device: meta.rdev(),
                inode: meta.ino(),
                filesystem: meta.dev(),
            });
            if result.len() > 256 {
                return Err("Input inventory exceeds 256 nodes.".into());
            }
        }
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}
fn trusted_program(path: &Path) -> Result<File, String> {
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    for ancestor in canonical.ancestors() {
        let meta = std::fs::symlink_metadata(ancestor).map_err(|e| e.to_string())?;
        if meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return Err(
                "Sandbox helper and its ancestors must be root-owned and not group/world writable."
                    .into(),
            );
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&canonical)
        .map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.mode() & 0o6000 != 0 || meta.mode() & 0o111 == 0 {
        return Err("Require an ordinary non-setuid executable helper.".into());
    }
    Ok(file)
}
fn approve_helper(file: &mut File, approved: Option<&str>) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let expected =
        approved.ok_or("No qualified bubblewrap release is approved; no child was launched.")?;
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 || format!("{:x}", Sha256::digest(bytes)) != expected {
        return Err("Sandbox helper fingerprint does not match the approved release.".into());
    }
    Ok(())
}
fn executable(request: &Request) -> Result<(String, String), String> {
    let bundle = Path::new(&request.directory)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let program = Path::new(&request.executable)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !bundle.is_dir()
        || bundle == Path::new("/")
        || ["/dev", "/proc", "/sys", "/run"]
            .iter()
            .any(|p| bundle.starts_with(p))
    {
        return Err("Choose a real application directory, not a host or device root.".into());
    }
    let relative = program
        .strip_prefix(&bundle)
        .map_err(|_| "Executable must be inside the selected application directory.")?;
    let meta = program.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.mode() & 0o111 == 0 || meta.mode() & 0o6000 != 0 {
        return Err("Choose an ordinary executable; privileged launch is unsupported.".into());
    }
    Ok((
        format!(
            "/app/{}",
            relative
                .to_str()
                .ok_or("Unsupported executable path encoding.")?
        ),
        bundle
            .to_str()
            .ok_or("Unsupported directory encoding.")?
            .into(),
    ))
}
fn pipe() -> Result<(File, File), String> {
    let mut fds = [-1; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) })
}
// Brokers can pass input descriptors over AF_UNIX even if their path is read-only.
// The candidate deliberately supports no host display/audio/desktop broker sockets.
fn filter() -> Result<File, String> {
    let arch = if cfg!(target_arch = "x86_64") {
        0xc000003e
    } else if cfg!(target_arch = "aarch64") {
        0xc00000b7
    } else {
        return Err("Unsupported Linux seccomp architecture.".into());
    };
    let instructions: [(u16, u8, u8, u32); 13] = [
        (0x20, 0, 0, 4),
        (0x15, 1, 0, arch),
        (0x06, 0, 0, 0x80000000),
        (0x20, 0, 0, 0),
        (0x35, 0, 1, 0x40000000),
        (0x06, 0, 0, 0x80000000),
        (0x15, 0, 1, libc::SYS_io_uring_setup as u32),
        (0x06, 0, 0, 0x00050001),
        (0x15, 1, 0, libc::SYS_socket as u32),
        (0x15, 0, 2, libc::SYS_socketpair as u32),
        (0x20, 0, 0, 16),
        (0x15, 1, 0, libc::AF_UNIX as u32),
        (0x06, 0, 0, 0x7fff0000),
    ];
    let mut file = tempfile::tempfile().map_err(|e| e.to_string())?;
    for (code, jt, jf, k) in instructions.into_iter().chain([(0x06, 0, 0, 0x00050001)]) {
        file.write_all(&code.to_ne_bytes())
            .and_then(|_| file.write_all(&[jt, jf]))
            .and_then(|_| file.write_all(&k.to_ne_bytes()))
            .map_err(|e| e.to_string())?;
    }
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    Ok(file)
}
pub(super) fn start(request: Request, sources: Vec<Source>) -> Result<Box<dyn Child>, String> {
    if !launch::QUALIFIED {
        return Err(launch::REASON.into());
    }
    let mut helper = trusted_program(Path::new(BWRAP))?;
    approve_helper(&mut helper, APPROVED_HELPER_SHA256)?;
    let snapshot = inventory()?;
    let allowed = launch_plan::selection(&snapshot, &sources)?;
    Ok(Box::new(Process::spawn(
        request,
        helper,
        snapshot,
        allowed,
        Arc::new(inventory),
        |path| {
            OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(path)
                .map_err(|e| e.to_string())
        },
    )?))
}
struct Process {
    child: std::process::Child,
    status: File,
    status_bytes: Vec<u8>,
    started: Instant,
    running: bool,
    release: Option<File>,
    snapshot: Vec<Node>,
    inventory: Inventory,
    _pins: Vec<File>,
}
impl Process {
    fn spawn(
        request: Request,
        helper: File,
        snapshot: Vec<Node>,
        allowed: Vec<Node>,
        inventory: Inventory,
        mut open: impl FnMut(&str) -> Result<File, String>,
    ) -> Result<Self, String> {
        let (program, bundle) = executable(&request)?;
        let mut pins = Vec::new();
        let mut nodes = Vec::new();
        for node in &allowed {
            let file = open(&node.path)?;
            let meta = file.metadata().map_err(|e| e.to_string())?;
            if (meta.rdev(), meta.ino(), meta.dev()) != (node.device, node.inode, node.filesystem) {
                return Err("Device path changed during preflight.".into());
            }
            nodes.push((node.path.clone(), file.as_raw_fd()));
            pins.push(file);
        }
        if inventory()? != snapshot {
            return Err("Device inventory changed before launch; recapture explicitly.".into());
        }
        let (status, status_writer) = pipe()?;
        let (release_reader, release) = pipe()?;
        // Bubblewrap's release read must block; the status reader stays nonblocking.
        if unsafe { libc::fcntl(release_reader.as_raw_fd(), libc::F_SETFL, 0) } < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let filter = filter()?;
        let args = launch_plan::arguments(
            &request,
            &program,
            &bundle,
            &nodes,
            status_writer.as_raw_fd(),
            filter.as_raw_fd(),
            release_reader.as_raw_fd(),
        );
        let mut inherit: Vec<_> = pins.iter().map(AsRawFd::as_raw_fd).collect();
        inherit.extend([
            helper.as_raw_fd(),
            status_writer.as_raw_fd(),
            filter.as_raw_fd(),
            release_reader.as_raw_fd(),
        ]);
        let mut command = Command::new(format!("/proc/self/fd/{}", helper.as_raw_fd()));
        command
            .args(args)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                // Async-signal-safe only. Keep Rust's exec error pipe CLOEXEC, and prevent
                // inherited capture handles/sockets from bypassing the closed /dev view.
                if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                for fd in &inherit {
                    if libc::fcntl(*fd, libc::F_SETFD, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let child = command
            .spawn()
            .map_err(|e| format!("Sandbox spawn failed: {e}"))?;
        drop(status_writer);
        drop(filter);
        drop(helper);
        Ok(Self {
            child,
            status,
            status_bytes: Vec::new(),
            started: Instant::now(),
            running: false,
            release: Some(release),
            snapshot,
            inventory,
            _pins: pins,
        })
    }
}
impl Child for Process {
    fn poll(&mut self) -> Result<Poll, String> {
        if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
            return Ok(Poll::Exited(status.code().unwrap_or(128)));
        }
        if (self.inventory)()? != self.snapshot {
            return Err("Input topology changed; isolated child must stop. No new paths are added or replayed.".into());
        }
        let mut bytes = [0; 1024];
        match self.status.read(&mut bytes) {
            Ok(n) => self.status_bytes.extend_from_slice(&bytes[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
        if self.status_bytes.len() > 8192 {
            return Err("Sandbox status exceeded its bound.".into());
        }
        while let Some(end) = self.status_bytes.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.status_bytes.drain(..=end).collect();
            let value: serde_json::Value =
                serde_json::from_slice(&line).map_err(|_| "Malformed sandbox status.")?;
            if value["child-pid"].as_u64().is_some_and(|id| id > 0) {
                self.running = true;
            }
        }
        if !self.running && self.started.elapsed() > Duration::from_secs(3) {
            return Err("Sandbox setup exceeded three seconds.".into());
        }
        if self.running {
            if let Some(mut release) = self.release.take() {
                release.write_all(&[1]).map_err(|e| e.to_string())?;
            }
        }
        Ok(if self.running {
            Poll::Running
        } else {
            Poll::Starting
        })
    }
    fn stop(&mut self) -> Result<(), String> {
        if self.child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Ok(());
        }
        self.child.kill().map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(format!("Sandbox termination is unconfirmed for owned PID {}; inspect it before another launch.", self.child.id()))
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
#[path = "launch_linux_tests.rs"]
mod tests;
