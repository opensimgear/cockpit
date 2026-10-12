# Cockpit native configuration acceptance — 2026-10-12

This work-in-progress acceptance exercises the visible Cockpit UI, its production
Rust STM32 adapter and the native emulator linking production gateway/module C.
The browser bridge forwards requests to Rust; no JavaScript firmware snapshots or
mock network replies are used. The C gateway and module exchange generated Cyphal
transfers through real libcanard CAN frames.

## Implemented flow

A setup session claims one module writer and edits a bounded draft. The editor
supports names, expansion banks, momentary/maintained buttons and exclusive groups,
axes with ADC mapping/HID usage/calibration/inversion/deadzone, hats, encoders and
selectors. Apply validates and activates; Apply & save commits the validated draft
through the module journal before activation. Discard and loading build defaults
operate on the draft. A separate explicit gateway Apply layout publishes a changed
registered HID profile after a detach/reconnect. The frozen profile includes control
counts, axis usages and axis neutral values.

Module settings generation is independent of HID schema revision. Every activation
renews the CAN lease, including changes to names, raw calibration endpoints,
inversion and deadzone that preserve the registered HID profile. The registered
gateway descriptor stays fixed until layout Apply; a profile
mismatch produces neutral reports. USB/CAN queues and old delivery generations are
invalidated before the new profile becomes active. Claims and cached configuration
are bound to gateway identity and current module boot/configuration generation.

## Browser acceptance

Playwright drove the real Cockpit page at `http://127.0.0.1:5173/` in Chrome.
Emulator-only administration injected physical contacts/ADC samples, reboots and
flash faults through the same Rust service. Configuration, ownership, save, discard,
registration and layout actions were driven through visible UI controls.

The observed sequence passed:

1. Discover the native gateway/module automatically and start a setup session.
2. Save `Cockpit complete é` with expansion bank 1, three mapped buttons,
   two calibrated axes, one hat, one encoder and one three-position selector.
   Readback shows active/saved generation 2, eight effective HID buttons and a
   six-byte report; generated descriptor bytes are nonempty and match the C core.
3. End setup, add the controller and explicitly Apply layout. Cockpit reconnects
   by gateway UID, preserves the saved module name and shows an active matching
   profile with report ID 1. The emulator records one gateway reconnect.
4. Inject a momentary button, an expanded maintained contact, north hat and
   calibrated ADC endpoints. Cockpit displays buttons 1/3/7 on, both axes at 100%
   and hat N. Module and gateway report bytes agree: `4500ffffffff`; delivery ACKs
   are emitted only through simulated gateway USB completion.
5. Enable direct module USB ownership. The retained gateway slot becomes neutral
   (`000800000080`) and the visible held buttons turn off.
6. Enter an invalid calibration range. Cockpit shows the error and disables Apply;
   Discard restores the active values.
7. Apply a deadzone change without saving. Active generation becomes 3 while saved
   generation stays 2. The gateway needs no layout Apply or extra USB reconnect.
   Replay of an input from the old lease is rejected.
8. Reboot the module. Boot advances to 2 and saved generation 2 restores the earlier
   deadzone and complete configuration; stale setup/configuration cache is cleared.
9. Interrupt a settings erase during Apply & save. Cockpit reports the failed save;
   the active configuration, saved generation and gateway layout remain unchanged.
   Discard restores the saved name. Loading build defaults creates an applyable
   draft without changing active settings, then Discard restores it.
10. End setup and confirm the firmware writer is released.
11. Increase mapped buttons from three to four on the registered module and save.
    The module exposes nine HID buttons and a seven-byte report. The gateway keeps
    its old descriptor neutral with a profile mismatch until explicit Apply layout.
    After the second gateway reconnect, the new profile matches and the added
    button produces visible live input.
12. Use another writer through the shared direct-USB handler to save a name change
    without changing the module boot or HID profile. Cockpit invalidates the previous
    generation's cached name/mappings. Opening a fresh setup session reads the new active configuration.
    No additional gateway USB layout Apply is required.

Screenshots, browser snapshots, scripts and firmware-state evidence are retained
locally under the workspace `.research/` directory. These are development artifacts;
the maintained reproduction commands and scenario are recorded here.

## Automated and target checks

The native emulator acceptance covers all mappings at the maximum 128-button,
8-axis, 4-hat layout; independent HID item parsing; writer competition/expiry;
wrong identity/source/boot/lease/epoch guards; fragmented request rejection;
completed-request replay; reply loss/readback; direct ownership; old input and USB
completion rejection; flash interruption; saved reboot and process restart.

The Rust-to-C acceptance additionally checks full control/Unicode configuration,
readback, registry Apply, name retention, module boot invalidation, rejected flash
writes, invalid local calibration, external same-boot configuration generation
invalidation and lost-commit-reply reconciliation without replaying the mutation.
Firmware-native suites run with AddressSanitizer and
UndefinedBehaviorSanitizer. STM32 gateway and all three module profiles cross-build.

An erased persistent store commits factory generation 1 before accepting setup.
The final C acceptance includes interrupted first user saves and interrupted
factory initialization; each cut restores the valid baseline on restart. Factory
recovery is limited to the deterministic incomplete first record with the other
page still erased. Arbitrary corrupt nonblank settings fail closed.

| Check | Result |
| --- | --- |
| Complete browser scenario above | Passed; final active/saved generation 4, two explicit gateway reconnects, writer released |
| Rust tests | 38 passed |
| Frontend transport/configuration tests | 29 passed |
| Actual Rust → native C transaction acceptance | Passed |
| Native C emulator acceptance | Five flows passed with ASan/UBSan |
| Module firmware native suites | 10/10 passed in Release and with ASan/UBSan; all 80 baseline operations and all 80 first-save operations interrupted/recovered |
| Gateway firmware native suites / Python peer | 3/3 and 10/10 passed |
| Canonical generated bindings and vendor snapshots | Passed; 90 shared files and freshly generated CDC fixtures checked |
| Ten-module Cyphal integration and seeded fault stress | Passed with sanitizers |
| Svelte diagnostics / frontend build | Zero errors/warnings; build passed |
| STM32 cross-builds | Gateway and example/shifter/expanded module profiles passed |

The gateway image uses 51,528 bytes flash and 76,672 bytes SRAM. Module images
use 65,840 / 65,824 / 66,004 bytes flash and 71,032 bytes SRAM each. SRAM figures
include the reserved stack/heap; they do not establish measured runtime stack use.

## Reproduce

From the parent workspace, using matching adjacent checkouts and the pinned common
source described in the [emulator guide](../../simulator/native-gateway/README.md):

```sh
python3 cockpit/simulator/native-gateway/build.py --sanitize
CARGO_TARGET_DIR="$PWD/.research/cockpit-native-target" cargo build \
  --manifest-path cockpit/src-tauri/Cargo.toml --bin osg-emulator-service
python3 cockpit/simulator/native-gateway/test_native.py
pnpm --dir cockpit native:runtime:test
OSG_NATIVE_EMULATOR="$PWD/.research/native-gateway/native-gateway" \
  OSG_EMULATOR_SERVICE="$PWD/.research/cockpit-native-target/debug/osg-emulator-service" \
  pnpm --dir cockpit web:dev
```

Follow the browser sequence above. Use isolated `OSG_NATIVE_STATE_DIR` under
`.research/` when retaining flash across emulator process restart. Restart the
Vite/Rust service after replacing the native executable so it loads the new code.

## Boundary

The native emulator simulates physical USB submission/completion and flash
operations. It does not create an OS USB device or establish actual USB descriptor
enumeration, STM32 HAL timing, electrical CAN behavior or physical brownout
performance. Those remain the assembled-board acceptance step. No firmware version
or production release was created, and no connected hardware was flashed.
