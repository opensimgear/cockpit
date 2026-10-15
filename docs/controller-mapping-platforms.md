# Experimental controller mapping: platform setup and recovery

Recording previews, built-in transformations, bounded Rhai logic, explicit panel
sources and captured game profiles are available. Live virtual output,
provisioning, visibility changes and live script output remain disabled. Installing
an optional component or saving its preference does not bypass qualification.
See the [qualification matrix](verification/2026-10-15-mapping-qualification-m7.md).

## Windows

Ordinary controller discovery uses Raw Input and needs neither vJoy nor HidHide.
Recording previews do not acquire a virtual controller. Virtual output and hiding
have independent opt-ins: a profile that requests neither needs neither driver.

The current inspection adapter accepts the pinned **vJoy 2.2.2.0 x64** DLL and
driver at their expected installation paths. The [M0 record](verification/2026-10-14-controller-mapping-m0.md)
contains release links, artifact hashes and the signed-driver observations.
Install that upstream package separately if inspection is wanted, complete its
Windows authorization/restart requirements, then use **Inspect vJoy**. A changed
version, missing file or hash mismatch is reported as unavailable. Cockpit does
not download or install a driver on startup, or take a slot owned by another app.

HidHide is a separate upstream installation. The recorded package is 1.5.230.0
with its 1.4.181.0 driver; these version numbers refer to different artifacts.
Keep any existing device selections and application exceptions. Cockpit's saved
visibility intent does not edit that policy while qualification is disabled.
The normal/inverse policy is global; incompatible per-device requests are
rejected. Executable selection refers to every instance at that path, not one
PID. The recorded normal-mode test blocked DirectInput enumeration/direct HID
access, while Raw Input still delivered reports. It does not prove universal
hiding or game compatibility. Inverse mode remains unqualified.

The feeder, inspection, provisioning and Rhai workers are private modes of the
same Cockpit executable. The provisioner uses an explicit Windows authorization
boundary and operation-owned journal; ordinary startup does not elevate. No
separate downloaded helper executable is substituted. For packaging review:

```powershell
pnpm build
pwsh -File scripts/package-windows-mapping.ps1 `
  -Executable src-tauri/target/release/osg-cockpit.exe `
  -OutputDirectory .research/windows-mapping-release -BuildKind release
```

Release staging requires an artifact signed by the external release process.
Development staging accepts an unsigned build and labels it `development`.
The script copies only the application into a new directory and records its hash
and signature status. It does not bundle drivers, sign, install, elevate or change
qualification. A valid signature alone does not qualify deployment. The usual
Tauri bundle remains the application installer; driver setup stays independent.

vJoy slots are persistent driver devices. Stopping a feeder relinquishes ownership
after attempting fallback; it does not delete persistent slots. Provisioning
supports only recognized non-FFB layouts (up to 16 axes, 128 buttons, four
continuous hats). Unknown/FFB/discrete-POV layouts are refused. Actual creation,
refresh, UAC cancellation, total-process failure and game-binding retention still
need qualification. Do not treat a stale ownership journal as permission to reset
or delete another application's device.

## Linux

The evdev/udev capture adapter is implemented but disabled in ordinary startup
pending native physical-device checks. A read-only qualification probe and its
permission requirements are described in [Host controllers](host-controllers.md#linux-and-macos-qualification-probes).
Build hosts need the distribution's libudev development package. Runtime capture
needs read access to the selected `/dev/input/event*` nodes; event-node numbers
can change after reconnect and are not portable profile identities.

uinput is independent of capture permissions. A future qualified installation
needs kernel uinput support and a narrowly scoped session ACL or administrator
managed rule for `/dev/uinput`. Complete the distribution's rule reload/login
requirements; package sandbox permissions must allow the same resources. Cockpit
does not install rules or broaden node permissions. Avoid running the whole app
as root or granting global input-device access. **Inspect** only queries uinput
availability/version. Missing or denied access does not block recording previews.

The candidate backend supports up to eight targets, each with eight standard
axes, 32 buttons and four hats on x86_64/aarch64. Devices belong to their creator
handles. Stop/creator exit should remove them; explicit Start recreates them, and
stable names do not promise stable event paths or game bindings. Recovery does not
recreate devices. Kernel lifecycle, physical routing and native/Proton bindings
remain unqualified; see [M3L](verification/2026-10-15-linux-virtual-m3l.md).

An evdev grab is broad input suppression, not enumeration hiding: hidraw, raw USB
and other access paths are separate. Process-specific hiding and the proposed
isolated launcher are unavailable. Creating a namespace in WSL does not establish
Steam/Proton isolation, broker/child-process scope, inherited-handle handling or
hotplug protection. No automatic fallback broadens a requested visibility scope.

## macOS

The desktop's configured minimum is macOS 11; that is not the virtual-helper
support matrix. IOHID capture is implemented and normally disabled pending native
qualification. The explicit read-only probe reports access denial with Privacy &
Security guidance. It never requests seizure or grants itself input permission.

Virtual output requires a separate Apple-authorized, signed/notarized helper with
the virtual-HID entitlement. The current candidate uses IOHIDUserDevice for macOS
15+ on Intel/Apple silicon; **no OS/architecture combination is qualified**. An
entitlement plist or an ad-hoc signature is not authorization. The ordinary app
does not acquire that entitlement. Stage an unsigned candidate on a native Mac:

```sh
python3 scripts/package-macos-hid.py --output-dir .research/apple-helper
```

This records a release build, hash, architecture and build OS without installation
or signing. A qualified release would need approved signing/provisioning,
notarization, protected installation at
`/Library/Application Support/OpenSimGear/Cockpit/HID/osg-macos-hid-helper`,
root-owned non-writable ancestors and a compiled-in approved fingerprint.
There is currently no such fingerprint. Environment variables, profile imports
and local preferences cannot supply one. See [M3M](verification/2026-10-15-apple-virtual-m3m.md).

Candidate devices are ephemeral creator-owned objects, with eight axes, 32 buttons
and four hats. Explicit Start would be needed after Stop/helper loss; recovery
does not recreate output. Actual joystick recognition, authorization, cleanup,
upgrades and binding retention are unqualified. IOHID seizure is broad input
suppression, not hiding from selected processes. Per-process hiding is unsupported;
broad suppression also remains disabled pending its separate qualification.

## Profiles, device lifetime and recovery

- Save/import never starts a mapping or installs/enables an optional integration.
  Portable mapping documents contain logical roles and targets. Native identities,
  output assignments and opt-ins remain local. Imports are bounded to 64 KiB;
  incompatible input controls/layouts are rejected rather than guessed.
- Reconnect requires a fresh complete baseline. Held momentary controls must
  release before a new mapped press. A source gap clears that source's output;
  scripts fault until an explicit restart. Do not replay old pulses after recovery.
- Panel events use independent cursors and gateway/module/boot identity. They
  have no hats in the current contract. Selecting a panel and its duplicate HID
  route is rejected where the app can identify the overlap.
- Existing schema-1 game profiles/journals remain readable. Capturing a mapping
  is an explicit schema-2 upgrade; editor changes afterward do not change the
  saved capture. Requested unavailable capabilities fail preflight before hardware
  changes. Windows DCS/Rally launch support does not imply a Linux/macOS launcher.
- Emergency Stop remains available. Interrupted game sessions require explicit
  recovery, which leaves mapping stopped. Ordinary cleanup may resume a prior
  recording mapping only with fresh baselines. Conflicting external settings or
  ownership are preserved with a diagnostic. Never delete recovery files merely
  to suppress that diagnostic.

See [profiles](mapping-profiles.md), [script limits](mapping-scripts.md),
[game captures](mapping-game-profiles.md) and [preview/recovery](mapping-preview.md).
