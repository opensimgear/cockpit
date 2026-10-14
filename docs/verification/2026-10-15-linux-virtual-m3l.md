# Linux virtual controller implementation evidence

Issue #27 remains In progress. Live uinput output is disabled by both the mapping
service and native helper qualification gates. Saving templates and ordinary
startup never create a device. Recording preview remains independent of setup.

## Implementation

The existing virtual-controller workspace now reports the native backend. On
Linux it offers locally persisted templates for each profile's logical targets,
with up to eight standard axes, 32 buttons and four hats per controller, and up to
eight simultaneous targets. Unsupported layouts are rejected rather than
truncated. Template numbers identify local devices, never event-node paths.
Names and physical markers are deterministic; the latter use the existing
`opensimgear-virtual:` exclusion prefix. The evdev capture adapter also excludes
virtual sysfs transports. No keyboard, mouse or force-feedback capabilities are
created.

Explicit inspection opens only `/dev/uinput` and queries its version, without
creating or changing an input device. Creation uses protocol 5 capability/setup
ioctls, BUS_VIRTUAL identity, nonblocking creator handles and close-on-exec.
Reports contain every configured control and one final SYN_REPORT. Short writes,
failed setup and unsupported kernel interfaces fail the operation. The shared
feeder supplies initial/final fallback, preserves ordered reports, and releases
all acquired handles on partial creation, report failure, Stop or lease expiry.

A process-wide OS file lock prevents overlapping Cockpit creator workers. A fresh
worker cannot recover while a previous owner retains that lock. Recovery never
recreates ephemeral devices or replays mapping reports. Explicit Start is required
for recreation. The kernel owns final handle-close cleanup; this lifetime and
real game binding retention still require native qualification.

The ABI is limited to x86_64 and aarch64 Linux. It follows the kernel's
[uinput interface](https://docs.kernel.org/input/uinput.html) and
[uinput header](https://github.com/torvalds/linux/blob/master/include/uapi/linux/uinput.h).
The shared feeder currently transports normalized reports using its existing
slot/report contract; Linux converts only its advertised subset into evdev codes.

## User-directed setup

On the intended Linux system, an administrator must enable the uinput kernel
interface and grant the user's session access specifically to `/dev/uinput`.
Use the distribution's narrowly scoped udev/session ACL mechanism and complete
any required rule reload or login. Selected physical controller nodes separately
need read access. Package sandboxes must permit these same selected resources.
Cockpit neither installs rules nor changes permissions. Do not run the entire
application as root or make all input/HID nodes globally writable.

## Validation and qualification gaps

Automated Linux tests cover two outputs, exact axis/button/hat reports, every
setup failure, missing/invalid device paths, partial writes, lease expiry,
creator destruction, explicit restart and recovery without recreation. The
native tests inject syscall results; real-node inspection only queries access
and protocol. They do not claim native game or physical device qualification.

Linux virtual-output suite: 44 passing tests, including an independent C/header
check of struct sizes, offsets and ioctl constants. Portable template/report/lifecycle
coverage is 100%; Linux syscall boundary coverage exceeds 80%. Host tests cover
saved templates and their bounds without activation. Windows output regression
suite: 72 passing tests; Windows and Linux host coverage each exceed 71% per
module. Frontend check passes without errors/warnings, and the Windows Tauri UI
still renders vJoy controls with creation, Start and recovery disabled.

The real Linux native service acceptance probe passes synthetic routing, ordered
edges, restart persistence and refusal of live activation. Explicit inspection
reports this WSL system's `/dev/uinput` permission denial, while preview continues.
Its private feeder entry exits without activation. The pinned Rust 1.90 Windows
application regression also passes 288 tests with three hardware/archive tests
explicitly ignored.

Still open: two real controllers driven from physical inputs, Stop/process-crash
removal, explicit recreation, descriptor enumeration, native game binding
retention, a qualified Steam/Proton case, and distribution/session permission
matrices. Existing M1P capture qualification gaps also remain open. No live
uinput device has been created by this milestone's validation.
