# Generic ESP32 partial redraw — 2026-10-16

Problem: each press and release cleared and repainted the whole display, producing two visible full-screen sweeps on the SPI Sunton.

Implementation:
- Shared core computes changed visible widgets against the last draw snapshot, including feedback freshness transitions.
- A fixed 64×8 RGB565 tile (1024 bytes) composes background, text and all overlapping layers before each synchronous board write. No full framebuffer or allocation is used.
- Touch edges damage only the old/new active buttons. Unchanged frames write no pixels.
- Panel identity/hash, page, size and connection/arming heading changes force a full redraw. A failed flush cancels live input and forces a complete redraw on retry.
- Existing display/touch board APIs remain unchanged. RGB and SPI adapters consume the same bounded write callback.

Validation:
- GCC ASan/UBSan host suite passes at 480×320 and 800×480.
- Partial frames match reference full-render pixels on press/release, feedback change, stale transition, heading/page changes and overlapping widgets.
- Assertions restrict button writes to its bounds, verify unchanged frames perform zero writes, bound write payloads to 512 pixels, and test failure/recovery.
- Core coverage: 97.89% lines, 77.16% branches; raster: 100% lines, 87.36% branches.
- Sunton and Waveshare adapter fault suites pass.
- Sunton and Waveshare IDF v5.5.2 cross-builds pass. Both updated bundles were staged in the Windows mirror and native Cockpit rebuilt.

Flashed Sunton on COM6 using manual BOOT/RESET, verified all segment hashes and hard-reset the board. NVS was not erased, preserving saved pairing. Restarted native Cockpit; Sunton automatically reconnected with Panel 1 and reported live-armed / Panel connected · touch ready. Visual button-edge improvement remains for the user to assess.

Without a display TE/vsync input this does not promise atomic screen-wide updates; it removes the full-screen erase/repaint on button edges and limits remaining transitions to the changed region.
