# Serial provisioning boot-race fix — 16 October 2026

The first physical provisioning attempt reported serial-provisioning-ready followed by invalid-command-json. No credentials or pairing offer were retrieved for diagnosis.

Three synthetic 3081-byte status commands sent immediately after opening the Waveshare's CH343 port reproduced invalid-command-json, with startup observed each time. The old helper sent its provisioning command immediately after opening the port. The board was still initializing. Independent 81–8081-byte tests after startup succeeded, separating the boot race from malformed operator input.

## Changes

- The helper first obtains a fresh, nonce-correlated status response. Only harmless status probes are retried every 500 ms within an eight-second deadline; provisioning and arming commands remain single-send operations with no automatic replay.
- UART console startup installs an interrupt-driven 9216-byte receive ring instead of relying on the small hardware FIFO while the network task is busy. Driver conflicts/allocation or VFS setup failure refuse startup; non-UART builds retain their existing console.
- Optional --interactive mode keeps one serial connection open after provisioning. Status, arm, disarm, connect, page and stop run through that connection. A fresh status challenge is still required before arm. Console quit/EOF attempts stop before closing. Keeping the port open avoids resetting the RAM-only installation between pairing and arming on this hardware.
- The local Waveshare terminal now uses interactive mode. At Device command>, use arm/status/disarm/connect; screen_ aliases also work. Reopening a one-shot serial helper on this board can reset it and lose RAM-only pairing, so it is unsuitable for separate live-session commands.

## Validation

Twelve Python helper tests pass, including startup delay/status-only retries, no provisioning before readiness, no mutation retry after a lost response, same-port interactive commands and stop-on-exit. The new serial-start native harness tests UART and non-UART startup, full-record buffer size, driver conflicts/install failures and cleanup. Its production C coverage is 100% lines, branches and functions. Existing network/input sanitizer and coverage tests pass.

The corrected Waveshare image was built and flashed. Physical boot again passes PSRAM and RGB/GT911 initialization. Three repeated open/readiness cycles then successfully admitted 3081-byte and 8081-byte synthetic records and multiple same-port status commands. Private credentials were not used or logged by these probes.

The user confirmed the panel looks correct before this fix. Actual Wi-Fi pairing, assigned live delivery and live touch acceptance remain pending a fresh operator-run setup. Local hardware logs/scripts remain under ignored .research/waveshare-device.
