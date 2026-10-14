# Apple virtual HID implementation evidence

Issue #28 remains In progress. There is no approved signed/notarized helper
artifact, entitlement authorization or qualified native deployment. Both
production activation gates remain false and the approved helper fingerprint is
absent. An entitlement plist, cross-check or unsigned CI binary is not evidence
of live-device readiness.

## Implemented boundary

A separate `osg-macos-hid-helper` binary uses the bounded feeder protocol, lease
and creator lifetime already exercised for Linux. The main Cockpit executable
does not gain a HID entitlement. Its macOS workspace reports the correct
unavailable capability and stores local templates independently of activation.
The planned helper location is `/Library/Application Support/OpenSimGear/Cockpit/HID/`.
Only a compiled-in fingerprint of an externally qualified signed/notarized
release may be accepted; settings, environment and IPC cannot supply it. The
installation and ancestors must be root-owned and not writable by other users.
The helper checks its OS-provided virtual-HID entitlement and owns an exclusive
OS lock before creating anything. None of these checks grants Apple approval.

Generic joystick descriptors support eight standard axes, 32 buttons and four
hats. The complete-report codec validates every control, range and hat vector;
missing, duplicate or unknown fields are refused. A descriptor interpreter test
checks report bit lengths across every supported boundary combination. Serial
markers and the Virtual transport exclude helper output from physical capture.

The native adapter uses owned CoreFoundation dictionaries/data and
[IOHIDUserDevice](https://github.com/apple-oss-distributions/IOKitUser/blob/main/hid.subproj/IOHIDUserDevice.h)
objects, with explicit report errors and automatic release. It is a candidate
IOHIDUserDevice deployment path for macOS 15+ on Intel/Apple silicon. No version
or architecture is qualified yet; the CoreHID path remains a qualification/design
choice, not a second implemented backend. Apple documents the required
[virtual-HID entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.hid.virtual.device).

Parent/helper loss, stale tokens, transport overflow and lease expiry use the
shared feeder lifecycle. Recovery never recreates an ephemeral device or resumes
mapping. Devices are closed before releasing their creator lock. Explicit Start
is required for recreation.

## Build and packaging

`scripts/package-macos-hid.py` stages a release build and entitlement file on a
native Mac, records its hash/architecture/build OS, and explicitly records
signing/notarization as not performed. It does not install, sign, modify OS
protections or enable the capability. CI builds the helper and runs only its
non-activating `--inspect` path. An actual release still requires authorized
signing/provisioning, notarization, installation/upgrade validation and a pinned
artifact after native qualification.

## Validation and remaining work

Portable tests cover descriptor/report agreement, directional hats, invalid
reports, absent/changed/oversized/symlinked helpers and refusal of unsigned or
unapproved activation. Native macOS tests construct CoreFoundation properties
and inject create/report function pointers; no live IOHID device is created.
Rust target checking for aarch64-apple-darwin passes, including the native helper
and tests. Linux and Windows virtual-output/host suites pass; portable HID codec
coverage is 99.15% and helper policy coverage 77.78%, above the 71% module gate.
Frontend checks pass. Native linking/tests/coverage are verified by
macOS CI after push; cross-checking alone does not establish that result.

Still open: Apple-approved signing/entitlement path, supported OS/architecture
matrix, two live devices, physical routing, real game recognition and bindings,
parent/helper crash cleanup, permission denial, authorized upgrades/recreation,
and measured binding retention. The current gates remain disabled for all of
these gaps.
