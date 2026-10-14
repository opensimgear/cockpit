# Controller mapping M0 experiments

These are bounded, reproducible experiments for Cockpit #21. They are **not** the
production mapping service. Selected source files and the Rhai lockfile are tracked
explicitly inside the otherwise ignored .research directory. Downloaded sources,
drivers/installers, host snapshots, registry backups, logs and binaries stay ignored.

See [the contract candidate](../../docs/controller-mapping-contracts.md) and
[qualification evidence](../../docs/verification/2026-10-14-controller-mapping-m0.md).

## Portable checks

From the repository root:

~~~sh
python3 -m unittest discover -s .research/controller-mapping-m0 -v
cargo test --locked --manifest-path .research/controller-mapping-m0/rhai/Cargo.toml
cargo run --locked --manifest-path .research/controller-mapping-m0/rhai/Cargo.toml -- operations
cargo run --locked --manifest-path .research/controller-mapping-m0/rhai/Cargo.toml -- deadline
~~~

The protocol experiment rejects oversized/malformed envelopes, keeps source
baselines/generations independent, validates exact HidHide policy combinations and
refuses restoration after external changes. The evdev experiment tests SYN_REPORT
boundaries and SYN_DROPPED recovery using authored input events. Rhai's raw engine
has no host I/O functions, and the tests exercise infinite loops, progress
cancellation, recursion, syntax errors and deterministic simple evaluation.

These tests do not validate a complete mapping document, HID parser, script
transaction service, OS sandbox, physical device, game binding or helper deployment.

## Native startup and Linux readiness

~~~sh
CARGO_TARGET_DIR=.research/controller-mapping-m0/native-target cargo build --locked \
  --manifest-path src-tauri/Cargo.toml --bin osg-gateway-service
python3 .research/controller-mapping-m0/probe-startup.py
python3 .research/controller-mapping-m0/probe-linux.py
~~~

The startup probe keeps stdin open until the correlated status response arrives,
then closes and waits with bounded cleanup. The Linux probe reads capabilities and
attempts an ordinary-user uinput open; it does not change permissions, grab a device
or create an output. Its short-lived namespace probe is not game-isolation evidence.

## Windows probes

Run the saved PowerShell scripts from a shell allowed to execute locally authored
scripts. If needed, use a process-only policy for that shell; do not change system
signing protections.

- probe-vjoy.ps1 reads driver/DLL versions and slot capabilities. The selected
  installation is C:\Program Files\vJoy\x64\vJoyInterface.dll.
- probe-vjoy-report.ps1 checks the 2.2.2 driver/DLL pair and a FREE slot, captures its
  complete 124-byte V3 report, resubmits it unchanged, compares every declared field
  on readback and relinquishes ownership in finally. It refuses BUSY slots and never
  provisions or resets a device. Explicitly choose -Slot if slot 1 is unsuitable.
- RawInputProbe.cs creates a message-only window and registers only joystick,
  gamepad and multi-axis usages with RIDEV_INPUTSINK. It captures for five seconds,
  reports device IDs hashed from local paths, bounds message size/hash retention,
  unregisters and closes. It never registers keyboard/mouse usages or emits input.
  Its virtualVjoy flag identifies vJoy only; other non-vJoy devices can still be
  software/simulated devices and must not be assumed physical.

Build the Raw Input probe with the Windows .NET Framework x64 compiler, for example
from the repository root in PowerShell:

~~~powershell
& C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe /nologo /platform:x64 /r:System.Web.Extensions.dll /out:.research\controller-mapping-m0\RawInputProbe.exe .research\controller-mapping-m0\RawInputProbe.cs
& .\.research\controller-mapping-m0\RawInputProbe.exe
~~~

The background-registration result alone does not establish capture while a game
is foreground. Record the exact device, foreground game, driver versions and
observed input changes in that separate acceptance run.

## Remaining experiments

Live provisioning must snapshot/materialize implicit default slots as well as
explicit descriptors and global FFB registration. Do not treat AcquireVJD as
creation or blindly wrap vJoyConfig. Refuse reconfiguration while any feeder owns
a slot; verify restoration after failure/cancellation.

Live HidHide tests need an explicitly selected physical instance, capture and
non-allowlisted probe executables, a journal and exact policy validation. Persistent
and process-owned cleanup must not be conflated. The current policy script never
writes HidHide settings.

macOS input/helper/seizure experiments require a native Mac. CoreHID's
HIDVirtualDevice API requires macOS 15 or later. An authorized signed helper and
real joystick/game checks are required before enabling virtual creation. No Mac,
authorization or native execution is available in this run, so this backend remains
unavailable and no minimum supported deployment/architecture claim is made.

## Live Windows visibility experiment

HidHideSessionProbe.cs uses the selected release's persistent IOCTL ABI. It holds
the exclusive control handle while changing the two lists; global active/inverse
flags are never changed. Only an explicitly selected MOZA HID instance and a
separate local capture executable are added. A durable journal is flushed before
either write. A separate hidden guardian waits for the helper to exit (or stops it
after 120 seconds), verifies the journal's recognized intermediate state, and
restores it. External changes cause a conflict and retain the journal.

The capture exception temporarily grants this probe access to every device in the
existing hidden set; this is an explicitly authorized qualification experiment,
not an exception to the production policy validator. Inverse-mode testing is
refused when it would alter the policy of other hidden devices.

Compile HidHideSessionProbe.cs like RawInputProbe.cs, with the System.Web.Extensions
reference. The selftest argument checks eight recovery/planning assertions without
opening the driver. The snapshot argument reads counts and global flags only.
For a live run, copy the two binaries to a new local directory, make separate
capture.exe and observer.exe copies of RawInputProbe.exe, then invoke:

~~~powershell
.\HidHideSessionProbe.exe session '<selected HID instance>' '.\capture.exe' '.\observer.exe' '<new absolute journal path>' normal
~~~

Use fully qualified executable paths. The fail mode injects an exception after
the writes; crash exits with code 86 without executing finally. Both require
separate, unused journal paths. Do not delete journals or binaries while a
guardian is active. This is bounded process-exit recovery, not reboot or total
machine-failure recovery.

RawInputProbe now also checks read-handle access and DirectInput enumeration.
The normal test asserts that the observer loses DirectInput visibility and gets
Win32 error 5 for a read handle, while the capture executable keeps access and
reports. It requires a single matching VID/PID collection and verifies observer
access after restoration. Raw Input can continue exposing reports to the observer;
it must not be treated as universal per-process input isolation.

## Narrow vJoy provisioning experiment

probe-vjoy-provision.ps1 defaults to Plan and requires a local evidence Directory.
The preflight accepts only the qualified 2.2.2 driver and the observed implicit
default: slot 1 FREE, 16 axes (0..32767), eight buttons, no POVs/FFB, and no custom
parameter keys. It rejects running games/feeders/configuration tools. The target
ROOT\HIDCLASS\0000 must identify the ready vjoy service.

The exact 123-byte default descriptor is generated by
generate-default-descriptor.c from the selected release's
[hidReportDescSingle.h](https://github.com/BrunnerInnovation/vJoy/blob/bd268d1fa5934a900a79e76e519d4aa804e0ee3d/driver/sys/hidReportDescSingle.h),
saved locally under upstream/vjoy-hidReportDescSingle.h. Its SHA-256 must be
3ccc044ec270e74cb588c56b1b49c7e9aba74eb878666efca43855d6b1ea1e93.
The generator uses DEV_GEN=1, DEV_FFB=0, DEV_ADDITIONAL=0, TLID=1 and the release
axis maximum 0x7fff. Downloads and generated descriptor bytes remain ignored.

Place the descriptor as default-descriptor.bin alongside probe-vjoy.ps1,
probe-vjoy-report.ps1 and probe-vjoy-provision.ps1 in a fresh local directory.
After Plan passes, Run requires Windows administrator authorization. It journals
the original capabilities and OEM fingerprints, materializes the default slot 1,
adds identical slot 2, restarts only vJoy, verifies both layouts, roundtrips a full
report on slot 2, then removes its two exact descriptor keys and restores the
implicit default. It never invokes vJoyConfig or writes OEM FFB registration.

A guardian handles parent-process failure; unexpected registry edits or a new
feeder owner cause conflict rather than forced cleanup. Fail injects an error
after provisioning, but that path has not been live-qualified in this run. Use
a new directory for every run. No reboot is performed. General custom-layout
editing, FFB-capable layouts, concurrent configurators and reboot recovery remain
unqualified; this research helper is not a general provisioning service.
