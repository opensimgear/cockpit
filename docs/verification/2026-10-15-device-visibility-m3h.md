# Device visibility M3H implementation evidence

Issue #26 remains In progress. The production HidHide write gate is false.

## Policy and persistent ownership core

The portable validator models HidHide's single global application list and
normal/inverse mode. It accepts a common hide-all policy or a common selected
executable policy and rejects incompatible mixtures before mutation. Visible
selections do not remove externally managed rules. Pre-existing device policies,
application entries and ordering are preserved; a changed global policy is refused
when it would affect external hidden devices. Capture-executable exclusion is
checked explicitly. See the [HidHide policy documentation](https://docs.nefarius.at/projects/HidHide/CLI-Usage/).

The persistent adapter journals the complete validated plan and each single-property
mutation intent before issuing it. Readback is mandatory. Cleanup reverses only the
session's known steps, unhiding its devices before removing its capture exception.
Unknown external state and edited journals produce recovery conflicts. No startup
path automatically reapplies hiding or recovers a journal. A process-exit fixture
bypasses destructors and verifies that the pending journal survives and the OS
file lock can be reacquired; its driver state is a fixture, not a real hiding test.

## Native boundary

The adapter checks the pinned HidHide.sys artifact from the system directory
(SHA-256 e3bb8b6a4e4ff90290b34e710edbef88cf71cec6ba0cadcb8e1685d5dd65ddb2).
It uses the released persistent IOCTLs with bounded two-step list reads and exact
one-byte flags. Reads against the installed driver passed. Tests reject malformed
UTF-16, list terminators, oversized replies, incoherent snapshots, unsupported
artifacts and writes attempted while the native qualification gate is false.
No real hiding setting was changed.

Executable paths are resolved from existing file handles using
[GetFinalPathNameByHandleW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew)
and current HID interfaces use
[CM_Get_Device_Interface_PropertyW](https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_get_device_interface_propertyw)
with the device instance property; VID/PID matching never substitutes for an exact
instance. The release provides persistent settings, so this adapter makes no
automatic-unhide-on-crash claim. HidHide's device handle serializes its configuration
clients; direct external registry edits still require conflict detection.

## Device controls and standalone supervision

The device detail view now stores an independent, disabled-by-default visibility
opt-in and per-source intent. Users can select a running executable or enter its
absolute path, save intent, inspect the dependency, and explicitly Apply or
Restore. Saving intent never opens the driver. Apply and persistent recovery
remain disabled at both the service and native write boundaries. A read-only
inspection of the installed driver in the Windows Tauri app succeeded; the
rendered Apply/Restore buttons were disabled and no JavaScript exceptions occurred.

A worker owns the driver handle, validated source identities and durable journal.
It requires a fresh report after hiding, checks capture without frontend polling,
and restores its owned changes on source loss, instance changes, executable
relocation or cancellation. External policy changes cause conflict reporting.
Active visibility blocks reassignment and uncoordinated game launch; emergency
Restore remains available during game guards. Restart recovery requires an exact
recorded session ID and explicit action. No startup path reapplies stored intent.

## Pending qualification

Physical normal and inverse-mode probes, executable relocation, reconnect, real
process-crash recovery and game visibility remain unqualified. Combined vJoy/game
profile coordination is tracked in #32. Source/report capture through Raw Input
must be qualified separately from DirectInput enumeration; M0 did not establish
universal invisibility. None of these gaps is treated as passing acceptance, and
#26 remains open with both production qualification gates false.

The released driver exposes persistent settings. The journal supports explicit
recovery after process exit; it does not claim automatic unhide after a crash.
Linux/macOS visibility is tracked separately in #29.

## Validation

Windows controller coverage runs 48 passing tests, including fixture-backed IPC
Apply/Restore, missing dependencies, exact source identity, post-hide fresh-report
requirements, executable relocation, cancellation, external conflicts and durable
recovery. Fixtures never change the installed driver's hiding policy.

Measured Windows line coverage: policy 98.32%, journal 94.39%, wire 100%, settings
96.75%, session 98.08%, worker 85.93%, service 86.67%, native visibility 85.78%.
All new and existing controller modules retain the 71% minimum gate on each
supported CI host. The frontend check passes with zero errors or warnings.

The Windows native application regression suite passes 288 tests with three
explicit hardware/archive tests ignored. That local working tree includes
separate ACR changes; exact committed-state validation is reported by CI after
push. The core checkpoint dacd464 passed all 11 CI jobs on Windows, Linux and
macOS: https://github.com/opensimgear/cockpit/actions/runs/38650993121.
