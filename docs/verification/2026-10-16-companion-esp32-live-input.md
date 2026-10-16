# ESP32 companion live-input checkpoint — 16 October 2026

Issue [#63](https://github.com/opensimgear/cockpit/issues/63) remains in progress. This records the implementation and local validation, and supersedes the earlier network checkpoint's display-only limitation. No board was flashed.

## Implemented

- Generic single-pointer momentary-button input through the existing board callback, with independent 20 ms touch polling. No controller, bus, pin or calibration assumption was introduced. Display-only/headless adapters cannot arm.
- Explicit serial arming bound to the current session ID, generation, publication hash and a consumed one-shot local epoch. The helper correlates its fresh status request by a random request ID. Fresh released touch, a successfully painted current panel and healthy host acknowledgements are required.
- Full neutral baseline across all panel buttons before live input. Arming commits only after the matching source-accepted ACK and an unchanged local epoch. Touch during baseline preparation, ambiguous/lost ACKs and stale arm tickets cannot enable input.
- Ordered 64-edge queue, at most 16 edges per input frame, independent batch/heartbeat counters, at least 10 ms between input batches and reservation removal only after the matching ACK. Pending/in-flight edges expire at 250 ms. Overflow and counter exhaustion cancel without replay.
- Independent 600 ms touch/render and 1000 ms authenticated-ACK leases, with 250 ms heartbeats, below the host's 1500 ms source lease. A late touch poll or ACK cannot hide an expired lease/backlog.
- Local disarm before closure I/O. Stop/disarm/page cancellation requests bounded host closure when possible; transport/ACK failures abort the connection. Host socket loss or lease expiry releases only that source. Explicit reconnect and a new neutral-baseline arm are required; no automatic takeover or rearm.
- Current feedback snapshot/delta envelope validation, including instance/session/publication identity, binding generation, output bindings, typed range/unit checks, source status and provenance. Malformed/stale envelopes fail closed. Legacy fixture feedback remains supported until a modern envelope has been accepted.
- Immutable display snapshots and short shared-state locks remain in place; no rendering, network request or driver wait occurs inside the input queue's state operations.

Source acceptance does not prove native virtual-controller submission or game confirmation. Production host mapping/backend gates were not weakened. No production host code was changed for this milestone; the host test uses the trusted feedback publisher API.

## Validation

All commands and reproduction steps are in [the firmware guide](../../apps/companion-esp32/README.md).

- Native C suites pass with warnings-as-errors, AddressSanitizer and UndefinedBehaviorSanitizer. Cases include press/release ordering and batching, sustained held buttons, rejected/stale arm tickets, baseline touch races, lost baseline ACK, cancellation during input ACK, delayed in-flight ACK, overflow/backlog, stalled touch/render, flush failure, headless rejection, counter exhaustion and explicit fresh reconnect.
- Portable coverage (lines / branches / functions): companion 97.62 / 77.14 / 94.12%; raster 100 / 91.91 / 100%; esp_lcd_bridge 100 / 100 / 100%; json 100 / 73.99 / 100%; publication 100 / 77.23 / 100%; settings 100 / 74.17 / 100%; client 98.61 / 77.55 / 96%; input 100 / 81.93 / 100%. Every measured module passes its 71% gates.
- Four shared-designer exporter tests pass; coverage 96.83% lines, 95.59% branches and 92.86% functions. Fixture/header publication identity remains unchanged.
- Seven serial-helper tests pass, including current identity whitelisting, held/headless rejection, correlated status and distinct connected/armed terminal statuses.
- Both actual Cockpit Hub tests pass: display-only publish/pair/approve/assign/open/feedback/revoke, plus neutral-baseline admission, ordered repeated control edges, held heartbeats, isolated lease expiry with another source held, stale generation rejection, fresh-session baseline, socket loss and explicit close. They use an in-process SourceSink recorder, not physical WSS or a native/game backend.
- Final ESP-IDF v5.5.2 cross-builds pass for ESP32 (826368-byte application) and ESP32-S3 (819504-byte application), both within the configured single 0x177000-byte app partition. Both use the headless adapter. Native runtime size is 5456 bytes and raster state 536 bytes; these are not measured peak MCU memory.
- CI now includes the live-input native runner and host baseline/isolated-lease test through the existing workflow. Remote CI execution has not been claimed.

Local evidence: .research/esp32-network/{coverage.json,*-test.log}, .research/esp32-native/{coverage.json,preview.png}, .research/esp32-live-build.log and .research/esp32s3-live-build.log. Windows Cargo used CARGO_INCREMENTAL=0 because the WSL share does not support its incremental lock operation.

## Remaining qualification

The user's exact 800×480 S3 and smaller WROOM-32 board drivers, physical touch/rotation/calibration, shared-bus synchronization, physical Wi-Fi/TLS/serial behavior, secure storage/recovery, peak memory/stack and sustained render/input latency remain unqualified. A driver must return current touch state, propagate I/O failure and bound its calls. The software lease budgets require physical measurement before acceptance.

Durable downloaded-panel/power-loss recovery, richer widgets/assets and designer target UI remain open. RAM-only installations require re-pairing after reboot. OTA and broader board qualification remain #64/#65. No physical LAN, actual game/backend or screen acceptance checkbox is marked complete.
