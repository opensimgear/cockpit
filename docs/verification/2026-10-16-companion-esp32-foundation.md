# ESP32 companion foundation verification — 16 October 2026

Issue: [#63](https://github.com/opensimgear/cockpit/issues/63), implementation in progress.

This checkpoint adds apps/companion-esp32 and its dedicated CI workflow. Existing Cockpit runtime/frontend files are unchanged. This is a local working-tree checkpoint, not a claim of committed/pushed implementation or completed story acceptance.

## Implemented

- Validated offline conversion of existing designer bundles into immutable embedded C panels, preserving the shared outer publication hash and stable input/output/channel identities.
- Portable, heap-free C runtime with normalized aspect-preserving layouts and pluggable drawing/touch boundaries.
- Momentary recording-only input with release-to-rearm, drag-out/page/connection cancellation and recorder-failure disarm.
- Typed feedback validation, provenance and fresh/stale/unknown/disconnected presentation.
- Small-buffer RGB565 raster adapter and an esp_lcd bridge with explicit synchronous transfer fencing.
- ESP-IDF entry point, headless default board, deterministic two-page designer fixture, native harness and dual-MCU CI definition.

## Local validation

- Exporter: four Node test groups pass, including deterministic generated-header/shared-publication identity, unsupported documents/capabilities/budgets, style/inset changes and CLI output. Coverage: 96.83% lines, 95.59% branches, 92.86% functions.
- GCC 13 native build: strict C11 warnings-as-errors, AddressSanitizer and UndefinedBehaviorSanitizer pass. Renderer bounds and RGB565 writes exercised at 800×480, 320×240 and 480×800. The 800×480 PNG was visually inspected for labels, values, status and widget layout.
- Native runtime coverage: 100% lines/functions, 79.61% branches. Raster: 100% lines/functions, 91.91% branches. esp_lcd bridge through a native API stub: 100% lines/branches/functions. Each passes the harness's 71% gates.
- Native 64-bit object sizes: runtime 5456 bytes; raster 536 bytes including the 512-byte pixel buffer. These are host sizeof results, not physical MCU peak memory measurements.
- ESP-IDF v5.5.2, commit 30aaf64524299d3bde422ca9a2848090d1bc5d0f: full headless ESP32 build passed (166368-byte app image); full headless ESP32-S3 build passed (190256-byte app image). Both fit the default 1 MiB app partition. The bridge also compiles against the actual ESP-IDF headers.
- New implementation file whitespace checks pass. Unrelated pre-existing/concurrent workspace edits were left untouched. CI workflow is authored; remote Actions execution is not claimed.

Reproduction commands and SDK/board integration details are in [the firmware README](../../apps/companion-esp32/README.md). Local generated evidence is under .research/esp32-native; SDK/toolchains and build logs remain ignored workspace artifacts.

## Remaining #63 work

Physical board identity, controller/pinout and touch calibration; actual board drivers; on-device memory/performance and ergonomics; provisioning and protected credentials/time; authenticated WSS/pairing and session ordering/baselines/leases; live panel/asset transfer, persistence and interruption recovery; designer UI integration and richer widget/font/asset support. Downloaded content is not yet accepted by firmware: its C panel is embedded at build time. The firmware app has no recorder or transport and cannot arm live input.

No board was flashed, no physical screen was accessed, and no native network/game/backend qualification is claimed. All original hardware/live acceptance remains open. OTA and hardware expansion remain in #64/#65.
