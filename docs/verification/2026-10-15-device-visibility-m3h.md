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

## Pending implementation and qualification

The per-device UI, local intent storage, source-generation supervision and explicit
Apply/Restore service integration are still being implemented. Physical normal and
inverse-mode probes, executable relocation, reconnect, combined vJoy sessions and
real game visibility remain unqualified. Source/report capture through Raw Input
must be qualified separately from DirectInput enumeration; M0 did not establish
universal invisibility. None of these gaps is treated as passing acceptance.

The new policy, journal, wire and Windows adapter are included in the existing
native controller coverage gate (71% minimum per module). Initial Windows measured
coverage: policy 98.37%, journal 94.21%, wire 100%, native visibility adapter 87.42%.

Validation for this core checkpoint: 35 Windows and 36 Linux controller tests pass.
The Windows native application regression suite passes 288 tests with three
explicit hardware/archive tests ignored. That working tree includes separate ACR
changes; exact committed-state validation is reported by CI after push. Linux
visibility coverage is policy 98.37%, journal 94.82%, wire 100%; every existing
controller module also retains its 71% coverage gate.
