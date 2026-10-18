# Host controller discovery

Windows Cockpit captures joystick, gamepad and multi-axis HID application collections through a dedicated Raw Input message thread. Capture continues while another application has focus. It does not request exclusive access, install drivers, provision virtual devices or alter hiding policy. Keyboard and mouse collections are excluded. Unknown standard controllers are usable without a model database; unknown, relative and array fields remain diagnostic.

Open **Devices**, select a physical controller in List or Topology, and inspect its shared details and input pages. A published OpenSimGear HID collection is associated with its source gateway/module when native serial/interface/report evidence proves the relationship; it remains an input capability without increasing the physical-device count. Gateway and module configuration is reachable from the same Devices details. Ambiguous or unresolved identities stay separate. Local names and roles do not change firmware or Windows product names. See the [Devices guide](system-inventory.md).

## Identity and reconnects

Each remembered source has a local UUID. Matching combines backend, VID/PID, collection/interface and a SHA-256 capability fingerprint. A unique serial-backed record can reconnect at another port. Duplicate serials and identical models are never merged by their name. Without a serial, the current path is only a local observation: a new path creates a new candidate. Select that connected candidate and use **Reassign this connection** to transfer a compatible offline identity explicitly. Reassignment retains the old UUID/name/role and requires a fresh baseline. Offline records can be forgotten explicitly.

Every connection/fault establishes a new generation; counters are decimal strings to preserve 64-bit values in JavaScript. Report-ID groups update atomically. Initially unknown controls remain unknown until observed. Button releases omitted by HidP within a received report are explicit zeroes; other report IDs remain untouched. A device that emits only changed reports does not expire. Unplug clears availability and values. Decode failures clear the baseline and expose diagnostics.

## Input values and bounds

Native ranges, signedness, field width, usage, report ID and link collection remain visible. Signed axes normalize to -32768…32767; unsigned axes to 0…32767. These numerical conventions do not infer a pedal's physical rest or calibrate a stick. Four-way hats map to even compass positions, eight-way hats to 0…7; declared out-of-range null states are neutral. Unknown usages display raw values without invented semantics.

The registry holds at most 256 remembered sources and 256 fields per collection. Inventory and control pages contain at most 32 entries. Metadata uses an atomically replaced, bounded 1 MiB local `controllers.json` beside Cockpit's existing settings. Invalid existing settings are diagnosed and never overwritten automatically. Unsupported descriptors and oversized report batches fail closed with diagnostics. No input values pass through the UI to reach the native registry.

Discovery is read-only and independent of optional vJoy/HidHide. Known vJoy VID/PID outputs cannot become sources. Arbitrary third-party virtual HID devices cannot always be distinguished from hardware and are not certified as physical devices by this label. Linux/macOS return unavailable capability status in this milestone; no unsupported fallback is silently selected.

## Development verification

Run `cargo test --manifest-path crates/controller-input/Cargo.toml` on each platform. On Windows this exercises the real message-window lifecycle plus authored HID descriptor/report/API-failure fixtures. `cargo run --manifest-path crates/controller-input/Cargo.toml --example inspect` performs five seconds of read-only native capture without saving settings or emitting controls. Logs may contain local hardware details; keep them in ignored research directories.

`python scripts/controller-coverage.py` instruments the native library and gates each compiled production module at 71% line coverage. Windows reports its adapter independently. `pnpm controllers:coverage` gates frontend helpers at 71% lines/branches/functions. Software fixtures do not establish device, game, driver or platform qualification.

## Linux and macOS qualification probes

Linux evdev/udev and macOS IOHID capture are implemented behind a qualification gate. Ordinary Cockpit startup keeps them disabled and shows the reason in Host controllers. Output and device hiding are independent optional features.

On a native test system, an explicit read-only developer probe is available:

~~~sh
cargo run --locked --manifest-path crates/controller-input/Cargo.toml --example inspect -- --qualification-probe
~~~

The probe lasts five seconds, uses no persistent metadata, and prints the inventory and the first page of each controller's values. It never installs drivers, changes permissions, grabs/seizes devices or creates output. Linux requires the system udev development package to build and read access to the selected evdev node to capture. Use session ACLs or a narrowly scoped udev rule; do not run all of Cockpit as root or make all input nodes writable. macOS uses system IOHID input access and reports denied access with Privacy & Security guidance; it does not request access automatically.

The same local UUID and explicit reassignment rules apply on each platform. Recycled eventN/registry identifiers alone are not persistent identity. A missing reliable serial/topology observation requires explicit reassignment. Linux pairs ABS_HAT X/Y controls and re-queries kernel state after SYN_DROPPED. macOS invalidates a lost callback stream and rejects stale values against baseline timestamps.

See [M1P evidence and remaining qualification](verification/2026-10-14-controller-capture-m1p.md). WSL's empty inventory and macOS compilation are not hardware acceptance. Normal activation remains unavailable until native device, background input, identity and report/baseline qualification is recorded.
