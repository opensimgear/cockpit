# Downloadable companion rendering — current feature plan

> Next-generation direction (23 October 2026): the
> [generic widget designer implementation plan](generic-widget-designer-plan.md)
> replaces the widget-authoring/runtime direction below with a capsule-only pilot
> and compiled raster visuals. Existing widgets need not be preserved. The
> source-layer renderer described here remains the current implementation until
> that plan's cutover; it is not the intended deployment contract for the new
> designer. Existing transport, identity, lifecycle and hardware evidence remain
> relevant.

[Feature #69](https://github.com/opensimgear/cockpit/issues/69), reconciled on
22 October 2026 with merged [#139](https://github.com/opensimgear/cockpit/pull/139)
and shared-widget feature [#112](https://github.com/opensimgear/cockpit/issues/112).

**Status: core software delivered; feature remains open for missing capabilities
and current-engine hardware qualification.** PR #77 is closed as superseded.
Its useful missing Waveshare framebuffer fix and fourteen original reports were
preserved on main in `ead7d32`. The [supersession audit](verification/companion-render-packages/2026-10-22-supersession.md)
distinguishes reused code, retired design and outstanding product requirements.

## Current implementation

Authors create one editable schema-3 widget library and panel source. Immutable
definitions, independent instances, typed ports and state rules serve Cockpit
preview, browser/Android's shared Svelte renderer and the ESP32 LVGL backend.
Schema-2 publications retain their existing identities and supported path.

Compatible layout, font, decoration and artwork updates use content publication
on the same installed compatible engine. New drawing/input operations or board
drivers may still require explicitly versioned firmware. Styling/caption changes
preserve capability identity; changed input behavior, option values, logical
ports or bindings require reconciliation.

The current pipeline reuses authenticated USB/WSS, installation approval,
source/session ownership, immutable snapshots, display fences, image-cache
delivery, progress and cancellation. Invalid target assignments retain editable
source and the valid running assignment. Reconnect always requires fresh
authority, released touch, a neutral baseline and fresh feedback.

The custom binary scene compiler/compositor, Q16.16 expression ABI, separate
render-prepare/read/open transport and old slot partition layouts from #77 are
retired implementation choices. Do not reintroduce them as prerequisites for
the shared source model. The portable two-slot content store and structural
inspector are already retained; their presence does not mean durable
complete-source storage is integrated with LVGL.

## Milestone reconciliation

| Milestone | Delivered / current replacement | Still required |
| --- | --- | --- |
| R0 [#70](https://github.com/opensimgear/cockpit/issues/70) — correctness and measured budgets | Waveshare coherence fix, three-fence/no-reuse regression; native board checks | Adapt bounded runtime diagnostics to LVGL, exact cancellation cause/time, loaded board/transport resource and timing measurements |
| R1 [#71](https://github.com/opensimgear/cockpit/issues/71) — shared contracts | Software outcome delivered by W0 [#113](https://github.com/opensimgear/cockpit/issues/113): strict schema 3, immutable identities, typed semantics, compatibility and cross-language oracles | Physical profile limits remain R0/R6; obsolete package ABI is retired |
| R2 [#72](https://github.com/opensimgear/cockpit/issues/72) — core rendering | Software outcome delivered by W2 [#115](https://github.com/opensimgear/cockpit/issues/115): bounded source admission, LVGL layers/fonts/gauges, damage, source banks and lifecycle | Authored local timelines/expressions move explicitly to R5; current-board performance remains R6 |
| R3 [#73](https://github.com/opensimgear/cockpit/issues/73) — durable source delivery | Live source publication and authenticated asset/cache delivery; portable content-store recovery primitives | Integrate power-safe persistence of a complete admitted source/asset closure, cold offline restoration, active/candidate pinning, layout migration and recovery evidence |
| R4 [#74](https://github.com/opensimgear/cockpit/issues/74) — designer/catalog | Software outcome delivered by W1/W3/W4 [#114](https://github.com/opensimgear/cockpit/issues/114)/[#116](https://github.com/opensimgear/cockpit/issues/116)/[#117](https://github.com/opensimgear/cockpit/issues/117): visual library/editor, twelve families, immutable revisions, preflight and publication/progress | Cross-platform physical traces remain W5/R6; no duplicate designer backlog |
| R5 [#75](https://github.com/opensimgear/cockpit/issues/75) — animations and advanced interactions | Momentary/toggle/selector and local navigation share existing state/controller contracts | Bounded shared local timelines and held animations, guard policy, XY/triangle grouped semantics and matching target implementations |
| R6 [#76](https://github.com/opensimgear/cockpit/issues/76) — physical qualification | Historical old-engine board evidence archived; current software checks/build history are available | Current LVGL firmware qualification on both boards over USB and Wi-Fi/WSS, visual/touch/resource/latency/soak/interruption evidence |

R1, R2 and R4 are completed **software outcomes**, with their superseded technical
requirements and relocated work named explicitly. W0–W5 remain open wherever
their acceptance mixes software with physical ESP32/Android evidence. Closing a
software milestone does not close #69/#112 or a physical qualification gate.

Milestone checklist:

- [ ] R0 #70 — current-engine diagnostics and measured board budgets.
- [x] R1 #71 — shared contracts and compatibility software (delivered by #139).
- [x] R2 #72 — bounded LVGL core rendering software (delivered by #139).
- [ ] R3 #73 — complete-source durable restoration and recovery.
- [x] R4 #74 — shared designer/catalog/publication software (delivered by #139).
- [ ] R5 #75 — bounded animations and advanced grouped interactions.
- [ ] R6 #76 — current LVGL physical qualification.

## Remaining software, in dependency order

1. **Measure and attribute before tuning (#70).** Port the bounded allocation-free
   diagnostics to the current renderer and transport. Record render/present and
   bus timing/bytes, touch contention/poll cadence, input queue and lease-age
   maxima, free/min/largest heap and task-stack minima. Latch exact cancellation
   cause/time before state cleanup. Keep snapshots redacted and separately bound
   to engine/source/board/session and stopped/live windows. Retain the existing
   71% per-module coverage gates and all timing limits.
2. **Complete durable source restoration (#73).** Extend the current source
   model and cache, rather than restoring the retired package protocol. Persist
   canonical source plus required asset identities transactionally, validate
   source/semantics/profile/hash before activation, and pin old/candidate data
   across interruption and readers. Respect the current larger LVGL app/image
   layout and preserve NVS/PHY during migration. A cold offline boot may display
   only verified content with unavailable feedback and inactive input; reconnect
   still requires a fresh session, neutral baseline and snapshot.
3. **Add bounded animations and advanced input (#75).** Define shared timeline
   start/repeat/cancel rules and expression/range/work limits before enabling
   authored effects. Budget old/new damage and hidden animation work; the
   historical narrow pulse strips are useful fixtures. Visual timers never
   create gameplay events. Add guard/unlock/expiry/fallback and complete XY/
   barycentric input groups across browser/controller/native host/C target.
   Projection, joint quantization (triangle sum 32767), reservation/sequence/
   retry and all-old/all-new mapping observations need independent vectors.
   Do not claim downstream OS/game atomicity.
4. **Integrate and regress.** Use the actual running dev Cockpit for authoring,
   publication, source-preserving rejection and same-binary update checks.
   Extend current source/state/input, storage fault and authenticated USB/WSS
   suites; do not build another widget schema, store or qualification system.

Current momentary/toggle/selector software must not be held back for unimplemented
guard/XY/triangle features. Each target advertises only what it actually admits;
browser/Android are not constrained by the smallest ESP32 profile.

## Remaining physical acceptance

These checks are deferred until the user triggers hardware validation. They
apply to the current LVGL engine, not the old #77 firmware:

- Qualify each board and transport separately: Sunton resistive touch/shared
  SPI/no-PSRAM and Waveshare GT911/DMA/PSRAM/scanout. Measure the preserved
  768,000-byte coherence copy under transport and flash load.
- Demonstrate new composition A → B, changed artwork/font/layout, identical
  before/after installed application hashes, NVS/PHY preservation and explicit
  engine/profile/publication/source/asset identities.
- Show cold install, warm-cache reconnect and a ten-minute runtime trace with
  zero graphics/layout/draw-command traffic during steady operation. Source
  and asset acquisition during activation is a separate phase.
- Run at least thirty minutes per board/transport under representative telemetry,
  touch and reconnect load, with no unintended disarms and comfortable margins
  to unchanged 250 ms pending-input, 600 ms render-health and 1,000 ms ACK limits.
  Percentiles alone are insufficient; record maxima, queue/heap/stack minima
  and exact fault attribution. Do not claim a frame-rate target before measuring.
- Inspect photographs/video for text/font, alpha/color order, clipping, stale
  trails, disjoint damage and requested versus confirmed state. Calibrate
  corners/center, hold/release/drag-out and noise; test Waveshare contact changes,
  unsupported multitouch and conservative presentation-failure latching.
- Perform controlled interruption tests against the durable storage actually
  integrated by R3: at least three cuts in each erase/write/verify/commit phase
  on each board, with complete old/new content or inactive diagnostics.
- Complete Android native device/storage/authentication/lifecycle parity under
  #55 and W1/W5. Build/preview or C software clients do not replace device checks.

Old D evidence includes a 648.046-second Waveshare live interval, but Sunton had
two cancellations and reached 247 ms pending input. Keep those observations and
their missing after-checks visible; do not use them as a pass for current LVGL.

## Completion rule

#69 is complete only when R0/R3/R5/R6's remaining acceptance is satisfied and the
completed R1/R2/R4 software remains validated through the shared pipeline.
#112 additionally retains its own W0–W5 and Android requirements. Independent
#55/#65/#76 device gates, #54/#66 host/game-output gates, and #68 signed upgrade/
rollback/trust qualification are not closed by this feature's software work.

## Evidence and retired material

- [Current shared-widget acceptance and exact identities](widget-designer-acceptance.md).
- [Preserved #77 audit and reports](verification/companion-render-packages/README.md).
- [Original R0–R6 issue requirements before reconciliation](verification/companion-render-packages/2026-10-22-original-issues.json).
- [Original custom-engine plan at its immutable commit](https://github.com/opensimgear/cockpit/blob/f2c9147b863a1aab7de1dd9026bf54e5430b16c4/docs/companion-render-packages.md).
- [Current ESP32 runtime and delivery](../apps/companion-esp32/README.md).

The eight unreferenced old `packages/companion-renderer/test/poc/*/*/preview.ppm`
files were removed from active main; they remain in Git history at `ead7d32`.
The old compiler/engine/protocol is retained through the closed PR and pinned
source references, rather than an orphan workspace package or duplicate runtime.
