# Repository-owned simulation acceptance — 2026-10-13

The gateway runtime now belongs to `device-gateway/simulation/`; JSON-configured
device instances belong to `device-template/simulation/`. Cockpit contains normal
USB/local transports and integration orchestration, with no simulation process
factory, input injection controls, emulator flags or bundled simulation firmware.
The optional Pico fixture also moved to the gateway repository.

## Isolated dependency provisioning

Both entry points were executed on macOS arm64 with sanitized native C binaries:

```sh
# From cockpit/
python3 tests/e2e/run.py --sanitize \
  --source device-gateway=/absolute/path/to/device-gateway \
  --source device-template=/absolute/path/to/device-template

# From device-gateway/
python3 tests/e2e/run.py --sanitize --with-cockpit \
  --source device-template=/absolute/path/to/device-template \
  --source cockpit=/absolute/path/to/cockpit
```

These were real isolated clones, not builds linked to adjacent working trees.
The explicit sources include unpublished working-tree changes; each runner records
the source HEAD and snapshot hash. Both automatically cloned the matching
firmware-common pin `35e06787fb9696977fe92d20c5ff5afc1e3e1ebf` and initialized dependencies.
Each initiating repository owns its `.e2e/deps/`, `.e2e/build/`, launch/resolution
manifests and `.e2e/runs/` evidence. No root setup or research checkout is needed.
Default remote-ref runs require publication of the coordinated changes first.

Both E2E entry points passed. The gateway run also built its cloned Cockpit and
passed Cockpit’s acceptance scenario against the gateway-owned binaries.

## Automated coverage

- Cockpit starts before the gateway; a device can also start before the gateway.
- Three separate JSON devices expose different names, axes, buttons, hats,
  encoder and selector profiles through production CAN discovery.
- Configuration ownership, full input mapping, Unicode names, Apply & save,
  persistence and explicit gateway layout Apply use production handlers.
- An independent HID item parser checks report lengths and stable report IDs;
  active reports match the device’s production input output.
- Interrupted device saves and gateway registry writes preserve valid state;
  configuration timeouts recover normally.
- CAN source-node collision recovery, duplicate identity/state rejection and
  unread-CDC backpressure preserve independent process behavior.
- Device and gateway crashes recover saved settings/registrations with fresh
  boot/epoch values. Offline registered slots stay neutral.
- Cockpit restart respects writer contention and fresh request identities;
  closing the client leaves gateway and devices alive.
- Four local-only bootstrap tests per repository cover failed preparation,
  interrupted publication, executable-bit changes and submodule commit changes.

Component verification also passed: 43 Rust adapter tests plus one gateway-service regression, 29 frontend tests,
Svelte checking/autofixer and Vite production
build; six sanitized native gateway boundary tests, three gateway core suites,
ten CAN peer tests; three sanitized device simulation tests and ten sanitized
production device suites; moved Pico C/UF2 and 41 bridge/helper regressions.

## Native Cockpit UI

This initial single-selection check was followed by [simultaneous multi-gateway
acceptance](2026-10-13-multiple-gateways.md), which replaces gateway selection with
independent concurrent connections and one card per gateway.

The Tauri debug app was built into Cockpit’s own `.e2e/build/cockpit/` tree.
It was launched before the native gateway and device processes. An already attached
physical Pico remained available, and the arriving CAN gateway was selected through
normal gateway selection. The gateway initially displayed zero devices.

Three independently launched JSON devices appeared with their configured names.
Through the UI, the pedals were renamed to `UI saved pedals`, changed from three
axes to one button plus two axes, and saved as generation 2. All three controllers
were registered and the explicit layout Apply completed. The pedals produced a
five-byte report `01ffff0000`; the native UI showed Button 01 ON and Axis 1 at 100%.
Stopping that process showed its offline retained slot with neutral inputs.
Restarting with the original JSON restored the saved name, shape and registration.
Closing Cockpit left the gateway and device processes running; test processes were
then stopped explicitly.

A follow-up native UI check verified that the gateway selector retains both
backends after connection, status polling and automatic configuration reads.
Switching Pico → CAN → Pico succeeded. A fresh discovery replaces the cached
inventory so vanished gateways do not remain selectable.

UI evidence is retained locally at `.e2e/runs/ui-fex0jze9/result.json`. Automated
runs retain their result records in each owning repository’s `.e2e/runs/` and source
receipts in `.e2e/resolved-dependencies.json`.

## Scope

The native runtime executes production configuration, flash journal, CAN transport,
registry, descriptors and input reports. Loopback CDC/CAN transports do not create
OS USB/HID devices. Physical STM32 USB enumeration and electrical CAN behavior remain
hardware acceptance work. No release versions were introduced.
