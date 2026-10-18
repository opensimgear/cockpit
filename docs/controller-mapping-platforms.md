# Experimental controller mapping: platform setup and recovery

Unified profile authoring, built-in transformations, bounded Rhai logic and
explicit panel sources are available. Live virtual output,
provisioning, visibility changes and live script output remain disabled. Installing
an optional component or saving its preference does not bypass qualification.
See the [qualification matrix](verification/2026-10-15-mapping-qualification-m7.md).

## Windows

Ordinary controller discovery uses Raw Input and needs neither vJoy nor HidHide.
Local designer/CLI previews do not acquire a virtual controller. Virtual output and hiding
have independent opt-ins: a profile that requests neither needs neither driver.

The current inspection adapter accepts the pinned **vJoy 2.2.2.0 x64** DLL and
driver at their expected installation paths. The [M0 record](verification/2026-10-14-controller-mapping-m0.md)
contains release links, artifact hashes and the signed-driver observations.
Use **Set up vJoy** under Devices → Host controllers to open the bundled upstream installer. Complete its
Windows authorization/restart requirements, then use **Inspect vJoy**. A changed
version, missing file or hash mismatch is reported as unavailable. Cockpit does
not download or install a driver on startup, or take a slot owned by another app.

Use **Set up HidHide** under Devices → Host controllers or a selected controller's
Device visibility section to open its independent bundled installer. Both setup
actions are optional and available in the Windows x64 desktop app. Cockpit checks
the pinned installer hash and publisher signature before showing the upstream
wizard, reports cancellation/failure or a restart requirement, and does not
enable output or hiding after setup. See the [installer catalog and licenses](../src-tauri/resources/dependencies/README.md).

The recorded HidHide package is 1.5.230.0
with its 1.4.181.0 driver; these version numbers refer to different artifacts.
At ordinary Windows capture startup, Cockpit grants its exact running executable
access through the supported installed driver: normal mode adds that application,
inverse mode removes it. Hidden-device selections, global flags and unrelated
application entries remain unchanged. Repeated startup is a no-op once permitted.
No profile, installer or elevation is required for this access setup. Pending
visibility recovery blocks a required change; busy or unsupported drivers are
reported in **Startup HidHide access** in controller details. Inventory refreshes
and read-only qualification probes never configure access. Cockpit's saved
device-hiding intent remains gated while qualification is disabled.
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
The script copies the application and optional installer resources into a new
directory and records their hashes and signature status. It does not sign,
install, elevate or change qualification. A valid signature alone does not qualify deployment. The usual
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
availability/version. Missing or denied access does not block profile authoring or portable previews.

The candidate backend supports up to eight targets, each with eight standard
axes, 32 buttons and four hats on x86_64/aarch64. Devices belong to their creator
handles. Stop/creator exit should remove them; explicit Start recreates them, and
stable names do not promise stable event paths or game bindings. Recovery does not
recreate devices. Kernel lifecycle, physical routing and native/Proton bindings
remain unqualified; see [M3L](verification/2026-10-15-linux-virtual-m3l.md).

An evdev grab is broad input suppression, not enumeration hiding: hidraw, raw USB
and other access paths are separate. The isolated-launch candidate is implemented
but remains disabled in its Linux device controls and native service. It requires
a separately approved, root-owned bubblewrap release; none is approved yet.
Its closed device view excludes selected physical siblings, retains observed
unselected/virtual nodes, and stops its owned child when the inventory changes.
The application directory is read-only and session storage is temporary. Host
clients and device permissions are unchanged.

The candidate supports only direct children without host display/audio/input
brokers or network access. Running applications, Steam/Proton and container launch
chains remain unsupported. Kernel-boundary fixtures verify namespace/child
cleanup and inherited-handle exclusion, but do not establish physical hotplug,
device-number reuse or real-game behavior. No automatic fallback broadens the
requested scope. See [isolated-launch evidence and remaining qualification](verification/2026-10-15-unix-visibility-m3v.md#linux-isolated-launch-implementation-follow-up).
Linux development tests need the distribution's `bubblewrap` package as well as
`libudev-dev`; CI exercises the real namespace probes with production gates closed.

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

- Save/import never applies a graph or installs/enables an optional backend.
  Unified schema-2 documents contain logical resources, exact ports, visible
  processing, virtual layouts and profile policies. Native assignments and
  backend slots remain local. Portable profiles are bounded to 512 KiB;
  incompatible controls/layouts are rejected rather than guessed.
- Reconnect requires a fresh complete baseline. Held momentary controls must
  release before a new mapped press. A source gap clears that source's output;
  scripts fault until an explicit restart. Do not replay old pulses after recovery.
- Panel events use independent cursors and gateway/module/boot identity. They
  have no hats in the current contract. Selecting a panel and its duplicate HID
  route is rejected where the app can identify the overlap.
- Old game/mapping configurations are not read, imported or migrated. Retained
  ownership journals are cleanup-only. A saved graph and its captured applied
  revision remain independent. Requested unavailable capabilities fail preflight
  before hardware changes. Windows game launch support does not enable a
  Linux/macOS launcher.
- Emergency Stop remains available. Recovery compares exact owned state and
  preserves conflicting external edits. It cannot resume a recording, replay
  commands/scripts/held input or silently reapply hiding. Start again only through
  fresh native admission. Never delete journals to suppress a recovery diagnostic.

See [Profiles](profiles-guide.md), [script limits](mapping-scripts.md),
[retained game cleanup](mapping-game-profiles.md) and [preview/recovery](mapping-preview.md).
