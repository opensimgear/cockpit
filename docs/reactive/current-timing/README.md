# Current timing migration matrix

The current scan covers Cockpit `f8428056`, device-gateway `dfd83084`,
device-template `c5457884` and firmware-common `64025cce`. The five tables
record each of its 471 sites, including inline fixtures and declarations that
the scanner finds in production files.

- [Portal, browser workers, native companion and ESP32](portal-and-companions.md)
- [Application and profile workers](application-workers.md)
- [Game and simulator workers](games.md)
- [Native gateway, module and transport services](native-services.md)
- [Shared crates and upstream firmware](crates-and-firmware.md)

`node scripts/reactive/audit.mjs --committed --check-current-dispositions`
rescans the four committed trees and verifies every site has exactly one row.
The command prints the resolved revisions. A new, moved, duplicate or deleted
site requires a corresponding matrix update.
`node scripts/reactive/audit.mjs --committed --check-dispositions` independently
verifies the original 247 historical entries. Both committed checks read their
disposition tables from the same revision as the inventory they verify.

## Acquisition and publication without a named timer call

The scanner matches named scheduling and readiness primitives. The source
review also records elapsed-time comparisons, interrupt or OS registration,
and source cadences that live inside the owners below.

| Owner | Boundary and current disposition |
| --- | --- |
| STM32 module board loop | `device-template/firmware/src/main.c:144,162,170,248,260` samples USB presence, services CAN and reads physical inputs at the 1 ms collector cadence. Layout changes take an immediate baseline. The HID path submits changed reports and repeats an unchanged report at 10 ms when ready. Keep capture and report cadence separate from Cockpit subscription delivery. |
| STM32 gateway board loop | `device-gateway/firmware/src/main.c:140,176,221` checks board USB presence, services CAN and emits changed HID reports plus the 10 ms unchanged-report repeat. TinyUSB callbacks retire submitted reports. The gateway's CDC producers own panel and publication updates. |
| Pico Debug board loop | `device-gateway/simulation/pico/firmware/src/main.c:110,125,130` reads USB connection state, ticks the debug core and emits changed or 10 ms repeated HID reports. CDC3 inventory is sequenced and has a separate watchdog lease. |
| Controller capture | The macOS probe waits for IOHID callbacks, and the Linux probe waits for evdev readiness and completed udev notifications. Explicit commands and shutdown wake both probes. Windows production capture wakes on Raw Input reports, device messages and shutdown. Linux and macOS platform qualification remain open. |
| Topology and transport owners | Linux udev waits on its native event and cancellation descriptors. macOS IOKit waits in the cancellable run loop. Windows callbacks wake serial and device inventory; socket connection and teardown use their own operation budgets. See the [native service notes](native-services.md) for related scanner exclusions. |
| Game process and setup owners | Native process, file and provider watches own topology where available. Scope-specific discovery and qualification can retain timed fallbacks. Assetto shared-memory capture has its recorded approval. The remaining boundaries and ownership evidence are in [acquisition](../acquisition.md#unresolved-acquisition-boundaries). |
| Devices read model | Cockpit ownership and topology changes publish committed state. The approved Windows check shares one read-only vJoy helper while Devices status is visible. [Acquisition](../acquisition.md#external-vjoy-ownership) |

The [historical frontend migration table](../ui-refresh-classification.md)
maps retired UI refreshes to current subscriptions. [Acquisition](../acquisition.md)
records the source decisions still needed for active fallbacks, firmware and
hardware qualification.
