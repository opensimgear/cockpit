# Shared widget software acceptance

Feature [#112](https://github.com/opensimgear/cockpit/issues/112) implements one
portable widget library and panel source for Cockpit preview, browser, Android
and ESP32. The current implementation is on `feat/widgets-112`; the software
validated production-source checkpoint is `ff00ac3`. Physical ESP32 and
Android-device validation was deferred
by the user on 22 October 2026. The qualified release matrix remains empty.

The [authoring guide](companion-guide.md#design-and-reuse-widgets) describes the
workflow. [Original starter panels](../examples/companion/README.md) include all
twelve families. [Implementation evidence](widget-designer-plan.md) records the
individual checkpoints and their limits.

## Requirement audit

| Milestone | Software implementation and checks | Outstanding acceptance |
| --- | --- | --- |
| W0 #113 | Strict schema 3 definitions/instances, immutable revisions, typed ports/properties, layers, state precedence, canonical source and separate capability identity. Schema 2 remains supported. Shared TypeScript/Rust/C fixtures validate admission and preserve source. Existing transport, cache, board drivers, scene ownership and session contracts are reused. | Physical Waveshare feasibility and separate Sunton measurements. |
| W1 #114 | Cockpit, browser and Android import the same `PanelCanvas`, `WidgetView`, controller, font metrics and state projection. Shared traces cover input cancellation and provenance/freshness. Browser SSR tests render all twelve families; actual native preview and live browser sessions pass. The final-source Android APK and iOS simulator builds pass. | Physical Android WebView/native lifecycle acceptance remains under #55. |
| W2 #115 | Pinned LVGL backend renders dynamic text, indicators, gauges, alpha, clipping and mixed legacy/components. Bounded source arenas, font pools, partial buffers, snapshots, generation/present fences and neutral-baseline admission are enforced. Authenticated WSS and nonce-bound USB carry explicit target reports. Both memory profiles pass actual native C/LVGL checks. One unchanged software-client binary accepts two full source compositions and reconnects through the real Cockpit host. All six firmware CI jobs pass on the current implementation commit. | Physical rendering, memory/flash/PSRAM, DMA/touch timing and board/transport qualification under #65/#76. |
| W3 #116 | Visual layer/state editor, geometry/style/font/image/property/port inspectors, undo/redo, snapping, resize/alignment, immutable library and intentional version adoption. Independent instance controls/feedback use existing profile bindings. Rich source survives authoring, import and incompatible-target saves. Actual Cockpit import/edit/reuse/preview/save/reload and UI library/panel export/import pass. | Device checks from W1/W2. No software authoring limitation is hidden by target simplification. |
| W4 #117 | Original illuminated button, toggle, selector, indicator, text/numeric instruments, linear/radial gauges, frame, divider, vector icon and navigation. Existing immutable publication, asset preparation, progress/cache, assignment and reconciliation paths handle updates. Source-preserving compatibility reports name unsupported fonts, budgets, geometry and overlapping touch areas. Live browser and C/LVGL consume the exact same edited multi-widget export. | Physical cross-platform traces. |
| W5 #118 | Software state/text/font/geometry/alpha/input oracles; actual Cockpit/browser/WSS/C-engine workflows; independent simultaneous browser sources, source-specific loss, neutral reconnect, held-touch replacement, invalid-assignment preservation and warm image reuse. Native fault-injection suites cover transport, bounded cache and cancellation. Guidance and original examples are checked in; final-source firmware and mobile builds pass. | Review and physical/native qualification evidence. Existing baseline repository failures are listed below. |

## Exact live-source evidence

The UI-exported panel has thirteen instances from the twelve-family catalog,
including two independent revisions of the illuminated control:

| Identity | Value |
| --- | --- |
| Source publication | `sha256:bd6f89ffb12193c7c005c391a87704e77e3e07f260faadbede4199f7dcb9e7f8` |
| Capability fingerprint | `sha256:0b2fb7f3a8f95a8e467566e372a03c7c46c01a56e13972afb48d5937a6b73a82` |
| C/LVGL software-client binary | `97be83ef94006d1232c88f1beee38ecc6167e4132b9b7f9ddf2479772c807034` |
| Firmware engine contract | `2026.10.22.3`, engine version 1 |
| Actual Cockpit UI export/import | `.research/widgets-native-ui/1792640440735/evidence.json` |
| Live browser companion | `.research/widget-browser-native/1792640473926/evidence.json` |
| Actual C/LVGL over host WSS | `.research/widget-software-native/1792640477640/evidence.json` |

The second C composition changes font, layout, decoration and prepared artwork
while retaining the capability fingerprint. The first publication is also
loaded by two independently paired browser processes. Closing one clears only
its source; the other stays armed and held. Reconnect starts a fresh neutral
session. Requested/local pointer state returns to unknown without inventing
confirmed simulator feedback. The native host rejects a deliberately overlapping
assignment and a missing glyph while retaining the previous armed assignment.
The edited control keeps its secondary caption above the main label and sits
inside the read-only section frame with a separate interactive hit area. Repeated
UI checks replace their own prior annotation instead of accumulating layers.
Live checks revoke their temporary clients and prune only newly created source
publications, retaining unrelated cache entries and asserting cleanup succeeds.
The UI also edits an existing selector's option caption, saves a new pinned
library revision and panel publication, and verifies capability identity and
bindings stay unchanged. Option-value changes still require reconciliation.

The C client links actual firmware source admission, network/client state
machine, input controller and LVGL compositor. Software adapters provide board
I/O, task/transport plumbing and bounded RAM image storage. Its unchanged PID
and binary hash prove data-driven updates; they do not measure physical ESP-IDF
scheduling, durable flash, touch, DMA, latency or Android authentication.

## Reproduction and tolerances

- Run `pnpm --filter @opensimgear/companion-designer coverage`, `pnpm browser:test`
  and `pnpm check`. The latest local results are 107 designer tests, 21 browser
  tests and zero Cockpit/browser Svelte diagnostics.
- Check deterministic fixtures with the `--check` mode of
  `scripts/widget-publication-fixtures.mjs`, `scripts/widget-profile-fixtures.mjs`,
  `scripts/widget-compatibility-fixtures.mjs` and `scripts/panel-source-fixtures.mjs`.
  The target oracle has 46 cases: 21 admit/render in both LVGL pool builds,
  seven fail glyph/font/zero-area preflight and two fail source touch-overlap
  admission. Other cases exercise explicit resource/feature rejections.
- Run the pinned native suite using `apps/companion-esp32/test/lvgl.py` and
  the source/state/input/network/serial tests used by ESP32 CI. Source admission
  covers seven panels, 63 widget instances and 4,244 mutations. Dynamic text has
  1,357 projections; state projection has 1,274 comparisons.
- Source visual parity compares 126 widget appearances across nine frames
  against the actual shared renderer in the running dev Cockpit. Its two-pixel
  geometry tolerance, one RGB565 channel-step color tolerance and 95% minimum
  significant-color footprint match include the original counted widget pixels.
  The verifier retains two pixels of surrounding context for fractional crop
  edges. Missing, displaced and recolored features are negative controls.
- With the real native dev Cockpit running, opt into
  `tests/ui/widgets-native.mjs` using `OSG_WIDGETS_NATIVE_UI=1`. Its evidence gives
  the actual exported file. Use that file as `OSG_WIDGETS_BUNDLE` for
  `tests/ui/widget-browser-native.mjs` with `OSG_WIDGETS_BROWSER_NATIVE=1`, and as
  `OSG_WIDGETS_SOFTWARE_BUNDLE` for `tests/ui/widget-software-native.mjs` with
  `OSG_WIDGETS_SOFTWARE_NATIVE=1`. The latter requires the built software-client
  executable and platform paths documented in the test. These tests use normal
  app handlers and native host operations; they do not enable mobile auth.

The latest native compositor peak is 17,248 bytes in both 96 KiB and 32 KiB LVGL
pool builds. These are native software pool measurements, not complete physical
board heap or stack measurements. Shared compatibility has 100% line/function
and 95.38% branch coverage; the compositor has 100% line/function and 84.31%
branch coverage. The native service suite passes 122 unit and two ESP32 host
integration tests.

## Build and qualification boundaries

All six ESP32 jobs pass for final production commit `ff00ac3` in
[run 39718306085](https://github.com/opensimgear/cockpit/actions/runs/39718306085).
Mobile checks, Android APK and iOS simulator builds pass on that same commit in
[run 39718306274](https://github.com/opensimgear/cockpit/actions/runs/39718306274).
The Android artifact is `mobile-android` (ID `12400910427`, 56,854,457 archive
bytes, digest `sha256:02a92736152c8e57522fe61ffbf8f17fd8885e355655c1230e9e56e2280d83b8`).
The iOS simulator artifact is `mobile-ios-simulator` (ID `12400380726`,
27,907,484 archive bytes). The independent PR mobile run
[39718311345](https://github.com/opensimgear/cockpit/actions/runs/39718311345)
also passes. Later documentation-only commits do not change these validated
production sources.
Debug APKs and iOS simulator archives do not qualify physical devices or signed
release installation/update.

General CI was already failing on starting commit `c144301`, as shown in
[baseline run 39678476707](https://github.com/opensimgear/cockpit/actions/runs/39678476707):
two game-setup worker tests report `Disconnected`, and the complete-profile-rig
test reports `Unknown persistent resource identity`. Those same failures remain
on this feature branch. The baseline's stale mapping-host lockfile is repaired;
locked local mapping-host tests compile and run with 110 passing and two
optional-driver qualification expectation failures in this Windows environment.
The local companion-platform build ran out of disk during compilation, so it
supplies no passing test result. Remaining platform validation uses CI.

Physical ESP32 and Android-device rows stay unchecked in #113–#118 and the
[release matrix](companion-release-matrix.md). No issue closure, native mobile
authentication enablement, game-output qualification or physical performance
claim follows from these software checks.
