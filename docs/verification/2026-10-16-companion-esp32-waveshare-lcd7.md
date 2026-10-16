# Waveshare LCD7 driver checkpoint — 16 October 2026

Issue [#63](https://github.com/opensimgear/cockpit/issues/63) remains in progress. The supplied rear-board photograph identifies Waveshare ESP32-S3-Touch-LCD-7 Rev 1.2. The official matching schematic identifies N8R8, ST7262 RGB, GT911 and CH422G. This is a new local working-tree implementation, not yet committed/pushed or flashed.

## Implementation

- Selectable waveshare-lcd7-s3 CMake profile with pinned ESP-IDF v5.5.2, N8R8 flash/octal-PSRAM defaults, PSRAM XIP and a real 800×480 RGB565 adapter. Default builds remain headless; incompatible target and conflicting cached board selection are rejected.
- Explicit LCD/touch reset and backlight control through CH422G; SD remains deselected. GPIO4 selects GT911 0x5d at reset and returns to input. GT911 identity and existing 800×480 geometry are validated without rewriting calibration. Initialization errors report a fixed stage and clean up allocated resources.
- Two IDF-owned 768000-byte PSRAM frames and two 16000-byte internal bounce buffers, 16 MHz PCLK and vendor GPIO/timing mapping. Synchronous span copying never retains the renderer scratch. An optional generic board presentation callback waits three new VSYNCs before reusing the previous frame; timeout is bounded to 240 ms and latches a display failure. Rendering/presentation stays outside the shared network lock.
- Polled single-pointer GT911 reports retain state across no-ready polls, expire a held cached point after 200 ms without a new ready report, and reject multitouch, finger substitution, invalid coordinates, bus failures and malformed reports. Startup needs a real released report before arming. Existing explicit arm, neutral baseline, queue/lease and reconnect behavior is preserved.
- No vendor/LVGL runtime or managed components were added. Source references and exact pin/resource configuration are in [the board guide](../../apps/companion-esp32/boards/waveshare-lcd7/README.md).

## Local verification

- The new driver harness passes warnings-as-errors, AddressSanitizer and UndefinedBehaviorSanitizer. Fault injection checks every initialization boundary and cleanup, reset/output sequencing, wiring/timings, framebuffer span bounds/copying, frame-swap fencing/timeouts, touch coordinates, release/hold/no-ready behavior, stale reports, multitouch, substituted contacts and I2C failures.
- Driver production coverage: 100% lines, 87.36% branches and 100% functions, passing the 71% per-module gate. Mocks do not constitute physical I2C, DMA or scanout evidence.
- Existing portable renderer/raster/bridge and network/input suites pass their sanitizer and coverage gates.
- Final real-board profile cross-build passes: application 869600 bytes (0xd44e0), 43% of the configured single app partition free. Existing ESP32 and ESP32-S3 headless builds also pass after the generic API/build changes. Wrong-MCU profile configuration is rejected as expected.
- CI includes the board-native runner/coverage and the real profile cross-build/artifacts. Remote CI success has not been claimed. No USB device was accessed or flashed.

Reproduce with python3 apps/companion-esp32/test/waveshare.py and the board guide's IDF command. Evidence: .research/esp32-waveshare-native/{coverage.json,board-test.log}, .research/esp32-waveshare-build.log, .research/esp32-waveshare-wrong-target.log, .research/esp32-live-build.log and .research/esp32s3-live-build.log.

## Physical checks still required

Flash using the UART/CH343 connector; verify detected flash/PSRAM, readable landscape output and colors, live panel delivery, released-touch arming, corner alignment, stationary held-button reporting, release/cancellation, Wi-Fi loss, frame stability, peak memory/stack and latency under load. If a stationary touch produces no fresh reports within 200 ms, investigate controller cadence before changing the declared safety budget. Start host mapping in recording mode; native/game output remains a separate acceptance step.

The photograph resolves this board's identity; physical behavior/calibration/performance remain unqualified. The smaller WROOM-32 board is still unidentified. No hardware acceptance checkbox is marked complete.
