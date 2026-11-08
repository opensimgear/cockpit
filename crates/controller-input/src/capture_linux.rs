//! Wait on evdev reports, completed udev events and explicit controller commands.
use super::*;
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

const FAILED: i16 = libc::POLLERR | libc::POLLHUP | libc::POLLNVAL;
const MAX_TOPOLOGY_EVENTS: usize = 64;

pub(crate) struct Wake(OwnedFd);

impl Wake {
    pub(crate) fn new() -> Result<Self, String> {
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if fd < 0 {
            return Err(format!(
                "Cannot create the controller wake: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(Self(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    pub(crate) fn signal(&self) -> Result<(), String> {
        let increment = 1u64.to_ne_bytes();
        self.transfer(|| unsafe {
            libc::write(
                self.0.as_raw_fd(),
                increment.as_ptr().cast(),
                increment.len(),
            )
        })
    }

    fn clear(&self) -> Result<(), String> {
        let mut counter = [0u8; size_of::<u64>()];
        self.transfer(|| unsafe {
            libc::read(
                self.0.as_raw_fd(),
                counter.as_mut_ptr().cast(),
                counter.len(),
            )
        })
    }

    fn transfer(&self, mut operation: impl FnMut() -> isize) -> Result<(), String> {
        loop {
            if operation() == size_of::<u64>() as isize {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            match error.kind() {
                io::ErrorKind::Interrupted => continue,
                // A full counter already has a pending wake. An empty read can
                // occur after a notification has already been drained.
                io::ErrorKind::WouldBlock => return Ok(()),
                _ => return Err(format!("Controller wake failed: {error}")),
            }
        }
    }
}

fn descriptor(fd: RawFd) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}

fn wait(descriptors: &mut [libc::pollfd], deadline: Option<Instant>) -> Result<(), String> {
    loop {
        let timeout = deadline.map_or(-1, |deadline| {
            let nanos = deadline
                .saturating_duration_since(Instant::now())
                .as_nanos();
            nanos.div_ceil(1_000_000).min(i32::MAX as u128) as i32
        });
        if unsafe {
            libc::poll(
                descriptors.as_mut_ptr(),
                descriptors.len() as libc::nfds_t,
                timeout,
            )
        } >= 0
        {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(format!("Cannot wait for Linux controller events: {error}"));
        }
    }
}

fn monitor() -> Result<udev::MonitorSocket, String> {
    if !Path::new("/run/udev/control").exists() {
        return Err(
            "Linux controller hotplug notifications require a running udev service.".into(),
        );
    }
    udev::MonitorBuilder::new()
        .and_then(|builder| builder.match_subsystem("input"))
        .and_then(|builder| builder.listen())
        .map_err(|error| format!("Cannot subscribe to Linux controller hotplug: {error}"))
}

fn topology(socket: &udev::MonitorSocket) -> Result<(bool, BTreeSet<String>), String> {
    let mut changed = false;
    let mut removed = BTreeSet::new();
    let mut events = socket.iter();
    for _ in 0..MAX_TOPOLOGY_EVENTS {
        // The udev iterator maps the native null result to None for both an
        // empty queue and an error. Preserve the distinction on this thread.
        unsafe {
            *libc::__errno_location() = 0;
        }
        let Some(event) = events.next() else {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(0) || error.kind() == io::ErrorKind::WouldBlock {
                break;
            }
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!(
                "Linux controller hotplug notifications were lost: {error}"
            ));
        };
        let Some(node) = event.devnode().filter(|node| {
            node.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("event"))
        }) else {
            continue;
        };
        changed = true;
        if event.event_type() == udev::EventType::Remove {
            removed.insert(node.to_string_lossy().into_owned());
        }
    }
    Ok((changed, removed))
}

pub(crate) fn run(
    shared: Shared,
    stop: Arc<AtomicBool>,
    control: crate::visibility::suppression::Control,
    wake: Arc<Wake>,
) -> Result<(), String> {
    // Register before scanning so any concurrent arrival or removal is queued.
    let monitor = monitor()?;
    run_ready(
        Capture::new(crate::linux::Linux::new(), shared),
        &stop,
        control,
        &wake,
        monitor.as_raw_fd(),
        || topology(&monitor),
    )
}

fn run_ready<P: Platform>(
    mut capture: Capture<P>,
    stop: &AtomicBool,
    control: crate::visibility::suppression::Control,
    wake: &Arc<Wake>,
    topology_fd: RawFd,
    mut read_topology: impl FnMut() -> Result<(bool, BTreeSet<String>), String>,
) -> Result<(), String> {
    control.watch_capture(wake);
    capture.control = control;
    capture.scan()?;
    if capture.tick_selected(|device| device.baseline_due) {
        wake.signal()?;
    }
    let mut descriptors = Vec::new();
    while !stop.load(Ordering::Acquire) {
        descriptors.clear();
        descriptors.extend([descriptor(wake.0.as_raw_fd()), descriptor(topology_fd)]);
        descriptors.extend(
            capture
                .devices
                .values()
                .filter_map(|device| device.input.descriptor())
                .map(descriptor),
        );
        wait(&mut descriptors, capture.baseline_deadline)?;
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let [command, hotplug, reports @ ..] = descriptors.as_slice() else {
            unreachable!()
        };
        if (command.revents | hotplug.revents) & FAILED != 0 {
            return Err("Linux controller wake or hotplug continuity was lost.".into());
        }
        if command.revents & libc::POLLIN != 0 {
            wake.clear()?;
        }
        let (rescan, removed) = if hotplug.revents & libc::POLLIN != 0 {
            read_topology()?
        } else {
            (false, BTreeSet::new())
        };
        let mut ready = BTreeSet::new();
        let mut failed = BTreeSet::new();
        for report in reports {
            if report.revents & FAILED != 0 {
                failed.insert(report.fd);
            } else if report.revents & libc::POLLIN != 0 {
                ready.insert(report.fd);
            }
        }
        capture.devices.retain(|path, device| {
            if !removed.contains(path)
                && !device
                    .input
                    .descriptor()
                    .is_some_and(|fd| failed.contains(&fd))
            {
                return true;
            }
            let mut registry = capture.shared.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(id) = registry.id_for_path(&device.identity_path) {
                registry.disconnect(&id);
            }
            false
        });
        let recovery = capture.tick_selected(|device| {
            device.baseline_due
                || device
                    .input
                    .descriptor()
                    .is_some_and(|fd| ready.contains(&fd))
        });
        if rescan {
            capture.scan()?;
        }
        // Recover a gap immediately when the report already contained its
        // resynchronization boundary. Otherwise wait for the next kernel report.
        if (rescan || recovery) && capture.tick_selected(|device| device.baseline_due) {
            wake.signal()?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "capture_linux_tests.rs"]
mod tests;
