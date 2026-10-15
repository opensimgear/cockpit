# M3V: Linux/macOS visibility implementation and qualification gaps

Issue #29 remains open. Native visibility/suppression is disabled by default and by compiled qualification gates. Saving intent, inspecting capabilities and importing settings cannot grab, seize, launch or alter device nodes. Virtual output remains a separate opt-in.

## Implemented

The per-controller UI distinguishes Windows HidHide policy from Linux/macOS event suppression. Unsupported scopes are rejected by the service, not silently broadened. macOS per-process hiding is unavailable. Linux already-running processes and unsupported launch paths are rejected. The Linux controlled-launch implementation is now connected to its own native supervisor and UI; its production qualification gate remains false and no helper fingerprint is approved. Ordinary requests still fail before launching a child or changing namespaces.

Broad suppression is owned by the existing capture handle. A bounded command slot carries the exact source/session/generation/interface baseline to the capture thread. A new session validates identity before applying, records every attempted handle before mutation, and releases every owned handle on partial failure. Linux uses EVIOCGRAB through the existing evdev descriptor. macOS closes/reopens the existing IOHID device using seizure options and replaces callback subscriptions/queues. Both native entry points remain gated off.

Each transition invalidates captured values and requires a fresh kernel baseline within two seconds. Capture gaps, identity changes, disconnects and Stop release the session; failed release closes the owned capture handle. Capture thread exit/panic closes handles. Reconnect does not reapply suppression. Kernel handles supply ephemeral ownership, so no persistent hiding policy is replayed at startup. A stuck OS call still needs native failure qualification and is not claimed to have a proven recovery bound.

## Automated evidence

Portable fixtures exercise same-handle ownership, partial apply/release failures, stop-before-apply, stale identities, gaps, disconnects, baseline timeout, fresh baselines, unsupported policies and missing helper denial. Tests make no real grabs or seizures. Per-module coverage is required above 70% on all native CI platforms. Windows Tauri continues to run with unqualified actions disabled.

## Remaining acceptance

Real selected/unselected probe clients must establish event delivery versus enumeration and alternative hidraw/raw USB/HID paths. The implemented Linux namespace adapter still needs physical controller, authorized helper and real application qualification. macOS seizure needs a supported device/API/OS matrix, permissions and cleanup tests. Real Stop/crash/reconnect and visibility-only/output-only checks remain open. No fixture or cross-target type-check qualifies these native capabilities.

## Linux isolated-launch implementation follow-up

The service accepts a bounded direct-child request with an absolute executable,
application directory and argument array. It requires independent visibility
opt-in and exact current Linux capture source IDs. Running PIDs, other launch
kinds, unknown request fields, stale generations, duplicate selection and missing
baselines are rejected. A separate worker owns the child without UI polling;
visibility edits and overlapping sessions are blocked until it stops. Ordinary
startup and imports never launch or replay a session.

The native adapter resolves selected capture identities through udev, groups USB
sibling interfaces (including joystick and hidraw paths), and excludes the whole
selected physical group. A bounded snapshot retains unselected nodes and
Cockpit-marked virtual nodes. Each retained node is pinned and its filesystem,
inode and device number are checked before launch. The inventory is checked again
before execution and throughout the session. A changed inventory or source
generation stops the child; new paths are never added to a running view.

The candidate uses [bubblewrap's mount/user/PID namespace interface](https://github.com/containers/bubblewrap/blob/v0.9.0/bwrap.xml).
It starts from a fresh `/dev`, exposes only retained device nodes, and gives the
child read-only system/application files, private procfs, empty sysfs/runtime
directories and temporary session storage. No host device chmod, global mount,
grab, service, or privileged helper installation occurs. Raw USB is not exposed.
All ambient descriptors become close-on-exec; only the explicit setup/device
descriptors are inherited. A seccomp filter rejects foreign syscall ABIs,
AF_UNIX sockets and io_uring setup. This intentionally excludes host display,
audio, D-Bus and input broker sharing, network access, Steam/Proton and container
launch chains. It is not a qualified graphical game launcher.

A release pipe keeps the application blocked while the supervisor checks
cancellation and source/inventory health. Setup has a three-second deadline;
Stop kills and reaps the owned bubblewrap process with a two-second observation
deadline. Bubblewrap's parent-death/PID-namespace handling owns descendant
cleanup. Unconfirmed cleanup or supervisor panic keeps the session blocked with
an attention status rather than admitting a second child. Restart never resumes
it. These software deadlines do not establish bounds for a stuck kernel call.

The helper must be an ordinary, root-owned, non-setuid executable with protected
ancestors and an approved SHA-256. The approved fingerprint is deliberately
absent. Service and native activation gates remain false; settings, environment
variables and IPC cannot enable them. Test-only injected factories exercise the
service lifecycle without opening the production gate.

## Follow-up validation and exact limits

- Linux controller suite: 68 passing tests, including actual disposable bubblewrap
  namespaces on WSL with bubblewrap 0.9.0. Fixture nodes use existing `/dev/null`
  and `/dev/zero` under authored controller names; no physical controller or uinput
  device was created, hidden, grabbed or driven.
- Namespace checks verify selected-node absence, allowed-node read access,
  inherited-child scope, ambient-handle exclusion, broker socket rejection,
  cancellation before application execution, inventory-change termination and
  cleanup after abrupt parent exit without destructors. The host nodes remain
  accessible. These are kernel-boundary software checks, not controller/game
  qualification.
- Linux production-line coverage: launch supervisor 91.57%, device plan 100%,
  native namespace adapter 76.22%, visibility service 92.64%. Windows portable
  launch coverage is 91.45%/100%; every measured controller module exceeds 71%.
- Controller coverage runs independent test fixtures serially because native fork
  probes briefly inherit process-wide advisory file locks. Worker concurrency,
  ownership conflicts and process failure remain explicitly tested.
- Full local Linux application regression: 310 passed, one existing hardware
  test ignored, plus native framing/service tests. Frontend check reports zero
  errors/warnings; all 152 transport/helper tests pass. Local application checks
  include unrelated existing working-tree changes; exact committed-state results
  are reported by CI on issue #29.

**Qualification remains open.** A static bind-mounted view plus polling does not
by itself prove safety across kernel character-device-number reuse. Real unplug,
replug, rapid reuse, physical sibling interfaces and virtual-creator failure must
be qualified (and the boundary revised if needed) before activation. No native
Linux controller/game matrix, graphical broker path, Steam/Proton case or macOS
seizure was qualified here. Acceptance criteria 1, 2, 4 and 6 therefore remain
unchecked; criteria 3 and 5 have implementation/negative-path evidence. The
implementation gap is closed, but #29 must remain open for qualification.
