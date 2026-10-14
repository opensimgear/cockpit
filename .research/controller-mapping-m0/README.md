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
