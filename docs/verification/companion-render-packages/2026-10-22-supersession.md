# PR #77 supersession audit — 22 October 2026

PR [#77](https://github.com/opensimgear/cockpit/pull/77), head
`f2c9147b863a1aab7de1dd9026bf54e5430b16c4`, was compared with main at
`b95bb6d525f72e2de18105f0389f06f71f062678`, including merged
[#139](https://github.com/opensimgear/cockpit/pull/139)
(`ac24fc28bd3bb6378d8e040c9fdd27aa14448999`). The comparison covers the
337 files changed by #77 from its merge base, their current equivalents and the
original hardware reports. The shared schema-3 widget source and LVGL backend
supersede the old custom scene compiler/interpreter. Closing #77 does not
complete feature #69 or qualify the new firmware on hardware.

## Preserved implementation

The useful missing production fix is Waveshare framebuffer coherence. Main still
swapped its two partial-update buffers without repairing the recycled frame.
The fix copies the just-presented 768,000-byte image to the recycled buffer only
after all three existing VSYNC waits succeed. It needs no additional framebuffer,
keeps the failure latch and deadlines, and applies to LVGL's partial redraws too.

The original regression now checks twelve alternating distant updates against
both complete framebuffers, immediate reuse of caller scratch, an unchanged
presentation, draining queued VSYNCs, submission failure and failure at each of
the three fences. It asserts no writes/recycling during submission or waiting,
and no recycling after a failed presentation. The [original implementation
record](../2026-10-16-waveshare-framebuffer-coherence.md) is retained as historical
evidence; the full-copy bandwidth/latency still needs current-board measurement.

## Disposition of the remaining work

| Area in #77 | Current LVGL implementation / disposition | Remaining owner |
| --- | --- | --- |
| Bounded two-slot content store, pinned readers, hash validation, commit-last journal and power-cut tests | `render/content_store.{c,h}` are byte-identical to #77. `render/package.{c,h}` retains the structural validator (only its explanatory comment differs). Already preserved; native recovery tests cover 318 interruptions. | #73 for integration/physical recovery |
| Downloadable new compositions, fonts/artwork, compatibility and semantics | Shared schema-3 definitions/instances, source admission, target reports, portable fonts, editor/library, existing authenticated USB/WSS and image-cache delivery supersede the old binary-package model. Live same-binary source replacement is covered by #139. | #112–#118; physical checks #65/#76 |
| Actual flash adapter and durable complete package/source restoration | #77's `content_store_esp`, raw-slot partitions and render-prepare/read/open protocol are absent from the running LVGL path. Main persists image pixels, but reacquires document/session state from the host after reconnect. Portable store tests are not proof of integrated durable source restoration or cold offline panel boot. Preserve this as an explicit gap; adapt to shared source if required, with fresh admission and no restored authority. Do not copy the old partitions over LVGL's larger application/image-cache layout. | #73, #76 |
| Timing, transport byte counters, heap/stack minima, queue/lease peaks and baseline capture | #77's allocation-free `core/diagnostics`, `tools/baseline.py` and redacted `tools/render-proof.py` are not in main. Current operation progress/errors do not replace these measurements. Preserve their implementation at the immutable source links below; adapt instrumentation to LVGL before reusing the capture protocol. Exact cancellation cause/time and loaded input-priority margins remain needed. | #70, #73, #76 |
| Local timelines, expressions and C/D held pulse strips | The old binary scene evaluator is intentionally superseded. The new shared model has state-driven visuals but no equivalent authored local timeline/expression contract. Retain bounded periodic/held-animation work as a follow-up, including damage budgets and proof that timers cannot emit gameplay input. #77 did not finish XY/triangle/guarded-group semantics either. | #75, common source model #112 |
| Original composite recipes, artwork/font subsets, exact binary raster goldens | Shared definitions/catalog/portable fonts and current source/state/text/visual oracles replace the old authoring/compiler ABI. Old A/B/C/D sources, previews, licenses and goldens remain reproducible at the pinned commit; they are reference fixtures, not admitted current source. LVGL parity uses documented rasterizer tolerances rather than claiming the old bit-exact renderer result. | Current [widget acceptance](../../widget-designer-acceptance.md) |
| UART ownership, zero-damage presentation, immutable snapshots and session fences | Main has a persistent fragmented-record decoder, source banks/generation fences and no-damage render handling. Keep the current implementations; reapplying the old engine/client would regress the newer source and lifecycle paths. | Existing network/serial/LVGL regressions; #76 hardware |
| One-time engine installation/readback and host package persistence | Main has its current setup-preserving firmware installer and publication/cache persistence. The old installer/demo are branch-specific references; their hashes and NVS/PHY observations are preserved below. They do not validate the new partition layout or engine. | #65/#68/#76 |

The old runtime is not merged wholesale: its ABI/profile tables, native host
render-package modules and fixed-point scene engine describe a different source
and transport contract. Their tests remain useful design references, not missing
LVGL features merely because the file names differ.

## Historical evidence retained on main

All fourteen original files under this directory (excluding this audit) were
copied unchanged from #77's exact head. They include the foundation/runtime
records, stopped-input baseline, software/build captures, A/B, failed C,
D, warm-cache and corrected Waveshare repeat JSON reports. The
[current feature plan](../../companion-render-packages.md) replaces obsolete
implementation guidance; the [original plan](https://github.com/opensimgear/cockpit/blob/f2c9147b863a1aab7de1dd9026bf54e5430b16c4/docs/companion-render-packages.md)
and original issue requirements remain archived.
Original report commands, branch links, local evidence paths and firmware
versions describe that old engine; they are not instructions or acceptance
claims for current main.

- [A/B proof](2026-10-17-ab-proof.json): new downloaded compositions with matching
  before/after application readbacks on both boards.
- [C failure](2026-10-17-c-animation-proof.json) and [D result](2026-10-17-d-animation-proof.json):
  retain both failures and improvement. D's narrow pulse strips reduce periodic
  redraw tiles 6× on Sunton and 13× on Waveshare. Waveshare stayed live for
  648.046 seconds; Sunton recorded two cancellations and reached 247 ms pending
  input against its unchanged 250 ms limit. Successful input ACK maxima were
  125 ms and 21 ms respectively. The generic cancellation status does not prove
  a cause. Sunton's A/B after-readback predates C/D and is not a D after-check.
- [Warm-cache proof](2026-10-17-warm-proof.json) and
  [corrected Waveshare repeat](2026-10-17-waveshare-repeat-proof.json): retain the
  original rejection/recovery observations alongside the improved repeat.
- [Stopped-input baselines](milestone-0-baseline.md): historical resource/render
  observations, not loaded-input or LVGL headroom measurements.

These runs used synthetic feedback and recording-host input. They do not prove
simulator feedback, successful virtual output or in-game behavior. Their old
firmware hashes cannot qualify the merged LVGL build.

## Open acceptance after closing #77

Keep #70/#73/#75/#76 open. Re-run the current LVGL engine on both physical boards,
with unchanged-firmware source swaps and before/after readbacks, warm-cache byte
accounting, sustained input and exact cancellation attribution. Measure the
Waveshare full-frame copy under transport/flash load and Sunton shared-SPI touch
contention. Retain the 250 ms pending-input, 600 ms render-health and 1,000 ms ACK
limits; collect comfortable worst-case margins rather than only percentiles.

Complete USB and Wi-Fi/WSS board/transport soaks, calibrated touch, photographs
and visual checks, heap/stack/PSRAM measurements, and physical interruption
recovery for the storage path actually shipped. The original power-cut matrix
calls for at least three repetitions at each erase/write/verify/commit phase.
Deferred physical tests remain deferred; no new device qualification is claimed.

## Immutable source references

All references pin the old head so closing the PR does not depend on its branch name:

- [Complete old source, fixtures and licenses](https://github.com/opensimgear/cockpit/tree/f2c9147b863a1aab7de1dd9026bf54e5430b16c4).
- [Diagnostics implementation](https://github.com/opensimgear/cockpit/blob/f2c9147b863a1aab7de1dd9026bf54e5430b16c4/apps/companion-esp32/core/diagnostics.c),
  [baseline collector](https://github.com/opensimgear/cockpit/blob/f2c9147b863a1aab7de1dd9026bf54e5430b16c4/apps/companion-esp32/tools/baseline.py),
  [proof reporter](https://github.com/opensimgear/cockpit/blob/f2c9147b863a1aab7de1dd9026bf54e5430b16c4/apps/companion-esp32/tools/render-proof.py).
- [ESP flash adapter](https://github.com/opensimgear/cockpit/blob/f2c9147b863a1aab7de1dd9026bf54e5430b16c4/apps/companion-esp32/render/content_store_esp.c),
  [authenticated delivery](https://github.com/opensimgear/cockpit/blob/f2c9147b863a1aab7de1dd9026bf54e5430b16c4/crates/companion-service/src/hub_render.rs),
  [animation engine](https://github.com/opensimgear/cockpit/blob/f2c9147b863a1aab7de1dd9026bf54e5430b16c4/apps/companion-esp32/render/engine.c).

## Verification of this preservation

- Waveshare adapter: warnings-as-errors, ASan/UBSan, 100% line/function and
  86.52% branch coverage; all three timeout/no-reuse cases pass. The same new
  test fails against main's original swap-only board at the first complete
  framebuffer comparison (negative control).
- Portable core/raster/bridge, Sunton, content-store and structural-package
  sanitizer regressions pass their existing coverage gates.
- Network/client/input/serial/image-cache sanitizer regressions pass all
  existing coverage gates. Actual pinned LVGL sanitizer suites pass both 96 KiB
  and 32 KiB pools, 1,357 text projections, target oracle, whole-panel compositor
  and firmware software-client smoke checks. The identical tracked source plus
  the preserved board/test changes was also tested on Linux storage; the initial
  snapshot needed its profile-assets fixture added before the full suite passed.
- Shared designer coverage and all 21 browser tests pass.
- Actual running dev Cockpit widget acceptance passes import, pointer handling,
  immutable revisions, selector caption identity, persistence and exact UI
  export/import. Evidence: `.research/widgets-native-ui/1792676020896`.

The historical ESP-IDF SDK locations are unavailable here and Docker is not
currently running, so new pinned-IDF board cross-builds were not performed in
this preservation pass. Existing #139 cross-builds predate this board fix.
Software tests do not exercise physical DMA/VSYNC or qualify full-copy timing
on an actual Waveshare.
