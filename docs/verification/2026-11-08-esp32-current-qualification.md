# Current Sunton and Waveshare firmware builds

Both companion board images compiled from the Git archive of Cockpit
`2ae30263b7aeb86e46cc74d1650d0691d1d75347`. The private WSL build used ESP-IDF
v5.5.2 at `30aaf64524299d3bde422ca9a2848090d1bc5d0f` and Xtensa GCC 14.2.0.
GitHub timed out on the tool download. Espressif's mirror supplied the
same pinned assets, which the SDK installer verified before extraction.
[ESP-IDF release](https://github.com/espressif/esp-idf/releases/tag/v5.5.2)
and [Espressif mirror instructions](https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-guides/tools/idf-tools.html).

| Board | Application bytes | Application SHA-256 |
| --- | ---: | --- |
| Sunton ESP32-3248S035R | 1,652,304 | `509cb089bd1e2ab4a1263ca4ad4a7e42b77238626ca4516c59a3f62badc7147d` |
| Waveshare ESP32-S3 Touch LCD 7 | 1,667,024 | `e8bfc8113661b5635ce3655d968e4d9f93c55dabb7095fe33f7543dd37633211` |

Both images fit the 2 MiB application partition. The partition-table SHA-256
is `599048fa47885e74561228921092f655eb26c40b82fe86304813c5349ec66291` on both
boards. The local binaries, configurations and logs are under
`.research/reactive/esp32-current-activation/`.

The Sunton and Waveshare driver suites and the portable rendering, networking,
storage, workspace, serial, render package, content store, widget source,
widget state, panel source, installer packaging and provisioning suites
passed. All seven generated fixture checks and four panel-export tests
passed. Panel-export coverage was 96.83% lines, 95.59% branches and 92.86%
functions.

The first whole-panel LVGL run reached catalogue case 6, which requires
37,419 bytes of JSON input. Its test harness was still using the default
32,768-byte reply limit. The corrected harness uses the generated rich
profile's limits for the shared catalogue and the per-target limits for the
compatibility cases. It also installs the bounded JSON allocator while it
compiles panels. The rich and compact renderer runs passed, along with the
firmware/LVGL software-client sanitizer smoke. Whole-panel compositor
coverage was 100% lines, 84.31% branches and 100% functions.

Physical touch, display, USB and Wi-Fi qualification remain in #199.

## Sunton touch interrupt candidate

The candidate based on `137022d6` uses GPIO36 PENIRQ to wake the Sunton
touch task. It masks the interrupt during SPI conversions and checks the
live level after restoring it. Quiet checks renew the 600 ms safety lease.
Held contacts still require coordinate sampling. The Sunton profile disables
station modem sleep. The Waveshare profile keeps its existing Wi-Fi settings.

Both images built with the same pinned SDK and fit their application
partitions. These hashes identify the exact application images.

| Board | Application bytes | Application SHA-256 |
| --- | ---: | --- |
| Sunton ESP32-3248S035R | 1,652,672 | `be7e4da760523394c08e3750c21118c04aa6160c6213d0f6d21ab7558c7e7062` |
| Waveshare ESP32-S3 Touch LCD 7 | 1,666,976 | `9cb7140c6456af4efdaba29fda628605f03b8bffe698b70b3a7f27e313c45cf8` |

The compiled Sunton Wi-Fi object references the power-save override. The
Waveshare object has no such reference. Negative compile checks rejected
Sunton builds with disconnected Wi-Fi sleep, Bluetooth, or automatic system
power management enabled. All 13 firmware host suites passed, including the
LVGL sanitizer smoke. The Sunton driver reached 100% line coverage, including
release while re-enabling the interrupt and failures to mask and restore it.

The board builds and software tests leave physical interrupt timing open.
#199 covers rapid taps, release during rendering, and USB and Wi-Fi recovery.

## Waveshare touch interrupt and held-report deadline

The candidate based on `9a1253b7` requires GPIO interrupts for both boards.
Waveshare reads on GT911 INT, an early-report or fault retry, or the next
actual health or held-report deadline. Sunton keeps its board-specific
held-coordinate checks. The pinned ESP-IDF toolchain built both images.

| Board | Application bytes | Application SHA-256 |
| --- | ---: | --- |
| Sunton ESP32-3248S035R | 1,652,656 | `e4aae8a77398f140cda233e8eda4e81402efa7c593a24e91833833e7883b42fa` |
| Waveshare ESP32-S3 Touch LCD 7 | 1,666,960 | `ddbdc1813883d323f86ee48e184a88ac5b6521546f110357f3fa584280ffe0b9` |

All 13 firmware host suites passed, including the LVGL sanitizer smoke.
Both board drivers reached 100% line coverage. The Waveshare fixture covered
one follow-up after an early interrupt, an unchanged deadline after a
no-ready read, invalidation at 200 ms, and recovery of a ready release when
the interrupt was missed. The Waveshare driver reached 86.32% branch and
100% function coverage.

Physical tests in #199 still need to measure a stationary hold, report
cadence, release and stale-input cancellation on the actual GT911 panel.

## USB startup correlation

The host retries one pending status request per baud attempt. A valid delayed
reply can identify the device until that attempt ends. The request identity
retires before the port reopens on the next baud and on the first accepted
reply. The firmware-ready notification can trigger one early retry per
attempt. Only a matching status reply can establish identity.

The baseline Windows serial worker failed the delayed-reply and ready-hint
regressions. With the fix, all 63 Windows serial tests passed. The fake
blocking serial peer also delivered a retired reply through the reopened
port and verified that the worker published only the current attempt.
The changed worker reached 87.62% line coverage. The configured serial
gates passed at 97.87–100%. The larger serial library file reached 68.47%
across its other entry points and platform operations.

The rebuilt Windows dev Cockpit exposed both embedded firmware profiles and
passed companion visibility and panel-assignment reopen in a private
database. The delayed-reply and baud tests use the real serial worker with
an authored transport.

Physical USB start and reconnect remain in #199. Current firmware uses a
status watch after identification. The older firmware status policy is a
separate decision.

## Feedback after session replacement

The ESP32 client harness now opens and replaces a live session over each
of its two mocked transports. Both sessions accept an initial feedback
snapshot and withdraw the value on Stop. The replacement rejects a delayed
snapshot from the old session and a delta received before a new snapshot.
Direct USB also rejects the old link and still rejects the old session
when its frame is wrapped with the new link ID. Both transports accept
the replacement snapshot and its next delta while the input lease and
disarmed state stay independent of feedback.

All six network-suite binaries passed with AddressSanitizer and
UndefinedBehaviorSanitizer in Ubuntu WSL2. All nine coverage gates passed.
The client reached 93.97% lines, 73.07% branches and 95.45% functions.
The publication parser reached 99.58% lines, 75.13% branches and
100% functions. The suite was built from `1361c52d` with the two updated
test files.

A separate Windows dev Cockpit run passed pinned native client pairing,
three presence handoffs, cancellation of stalled TLS in 55.75 ms, and
Forget. Its local artifact is
`.research/reactive/companion-client-io-native/1794255687556/evidence.json`,
SHA-256 `e26d7aadbc6a287268b7acc5e52e74e1e2b2a685a65df680a9a8b8b89327ce1b`.
The firmware harness uses authored connections. Physical Wi-Fi and USB
delivery and board recovery remain in #199.
