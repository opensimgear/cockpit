# Local reactive suite checkpoint, 2026-11-07

This checkpoint covers the production sources at `ad36fd97` and the test
fixture corrections in this commit. The working tree contains separate
profile editor changes. The frontend targets ran from an isolated export with
their own Git root and the installed workspace dependencies.

## Cockpit

- All `check`, `test` and `build` targets selected by the full Nx run passed
  across 19 projects with caching disabled. This includes the browser
  companion, TypeScript and Svelte checks, MCP frontend tests, changed-module
  coverage and production frontend builds.
- The first run exposed four stale UI fixtures. The corrected fixtures
  exercise committed App game and profile streams, native recovery readiness
  and disposal, the workspace identity projection, and the supported Assetto
  process monitor. All 20 focused cases passed on the final staged fixtures.
  Two Python targets passed once the existing Python runtime preceded the
  Windows Store alias on PATH.
- The instrumented Windows library suite passed 2,391 tests with 18 ignored.
  The binary was built from the same production sources as the current export.
  Its isolated build manifest requested only the Rust library artifact. The
  earlier measured changed-module coverage remains attached to the same
  native source and executable in the [discovery record](2026-11-07-game-discovery-page-isolation.md).
- The actual Windows dev Cockpit delivered all 65 authored vJoy edges and the
  independent Stop release to Windows Raw Input slot 1. One controller
  preview held a delivery and one MCP reader stayed unread for 1.92 s. The
  separate preview advanced. Devices Settings reported a busy slot before
  Stop and a free slot after reopen. The MCP Stop reply took 85.3 ms.
  Authored submission to Raw Input p50/p95/p99 measured 1.26/1.54/2.63 ms
  on the shared Windows performance counter. The 8 ms authored schedule
  had a measured submission interval p95 of 18.70 ms. This run did not
  measure physical HID capture, display presentation or simulator effects.

## Firmware

| Native host suite | Result |
| --- | --- |
| Firmware common `64025cce` | 10 CTest cases passed, including USB descriptors with the pinned TinyUSB source. Portable-core line coverage was 99.78%. |
| Device template `c5457884` | 24 CTest cases passed, including USB event delivery, metadata delivery and stream fairness. |
| Device gateway `e761e4d6` | Six CTest cases passed. The publication suite passed ASan, UBSan and its GCC coverage gates. |
| Pico Debug | The protocol-3 host test passed under ASan and UBSan. GCC measured 100% and 99.38% lines for the two debug modules. GCC's warning for the deliberately short invalid UID remained visible and the sanitizer run passed. |
| ESP32 companion | Serial, workspace, network, core, storage, Sunton and Waveshare host suites passed. Both board driver suites reported 100% line coverage. |

The raw logs and the vJoy trace are under
`.research/reactive/local-suite-ad36fd97/`. Linux desktop dependency
resolution stopped offline at `notify`, and macOS was unavailable on this
host. The final platform qualification, integrated load matrix and physical
checks in #199 remain open. CI remains deferred.
