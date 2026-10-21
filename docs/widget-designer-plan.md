# Shared widget designer — feature #112

Implementation branch: `feat/widgets-112`. Source worktree: `cockpit-widgets`.

The accepted requirement is a single editable widget library and panel source for
Cockpit preview, browser, Android and ESP32. Browser and Android extend the same
Svelte renderer. LVGL is the ESP32 backend, not an alternative source format.
Original illuminated controls and instruments take visual inspiration from the
GameGlass reference; no external artwork or Star Citizen provider is included.

## Implementation checklist

| Story | Required software delivery | Status |
| --- | --- | --- |
| W0 #113 | Strict versioned definitions, instances, layers, properties, ports, states, identity, capability contracts and LVGL feasibility | In progress |
| W1 #114 | Shared browser/Android composition renderer, input semantics, freshness and parity fixtures | Renderer implemented; qualification pending |
| W2 #115 | Pinned LVGL runtime, target compilation, board capability negotiation, existing USB/WSS delivery and same-firmware updates | Drawing feasibility implemented; integration pending |
| W3 #116 | Layer/state editor, immutable library revisions, reusable independent instances, complete illuminated-switch workflow | Authoring implemented; device workflow pending |
| W4 #117 | Original control/instrument catalog, library bundles, publishing/reconciliation, example panel and runtime updates | Pending |
| W5 #118 | Running native dev Cockpit acceptance, automated/native/build checks, documentation and software parity evidence | Pending |

Physical ESP32 and Android-device validation is deferred at the user's explicit
request on 22 October 2026. It must remain identified as deferred evidence, and
must not be replaced by simulated measurements or used to claim qualified native
authentication, real hardware performance or game-output acceptance. Existing
#55, #65 and #76 qualification gates remain independent.

## Contracts and backlog reconciliation

Extend `packages/companion-designer` and its publication pipeline rather than
creating a parallel store, transport or profile. Existing schema 2 publications
retain their validators and hashes. Rich compositions require an explicit newer
document schema; immutable definitions are included in its publication identity.
Stable input/output IDs and interaction semantics determine capability identity;
changing fonts, images, labels, layer order or styling does not change bindings.

Feature #69's compositor-first backend is superseded by LVGL for this feature.
Reuse the established board drivers, image cache, immutable snapshots, fences,
download/assignment and session ownership. Reconcile #74 catalog integration and
#75 interaction semantics with this common source model. No platform silently
drops widgets and browser/Android are not limited to the smallest board.

Every meaningful implementation checkpoint records its actual checks below.
Completion requires all software rows, integration and regression evidence; this
file is a tracker, not evidence that unchecked work exists.

## Evidence

- Worktree created from main `c144301`; locked dependencies installed offline.
- Shared widget source contract added with strict bounded validation, typed
  property/port references, immutable library revisions, semantic projection,
  explicit state precedence and feature/touch-budget diagnostics.
- `pnpm --filter @opensimgear/companion-designer check` passes.
- Designer coverage suite: 88 tests pass; widget-model coverage is 100% lines,
  97.55% branches and 97.78% functions. No UI, native LVGL or hardware evidence
  is implied by this contract checkpoint.
- Source-document checkpoint: explicit schema 3 definitions and component
  instances, independent primitive/channel ownership, undo/duplication, pinned
  revision adoption, ten original editable catalog recipes, bundle round-trip
  identity and styling-versus-semantic fingerprints. Existing schema 2
  publication behavior remains unchanged. Native admission and runtime delivery
  integration remain pending.
- Updated designer checks and coverage pass: 97 tests; 97.50% aggregate lines,
  96.27% branches and 96.93% functions.
- Native publication admission independently validates schema 3 definitions,
  properties, layers, states, ports, navigation and numeric contracts. Exact
  TypeScript-generated publication/capability fixtures are consumed by Rust;
  artwork changes retain capability identity and interaction changes do not.
- `cargo test --manifest-path crates/companion-service/Cargo.toml --lib --offline`
  passes all 114 tests with the local socket/protected-storage permissions needed
  by the existing native transport suite. Physical-device acceptance is deferred.
- Prior render-package branch inspected: reuse candidates include the existing
  bounded two-slot `render/content_store` and authenticated render-delivery path.
  Its custom scene compositor must be adapted behind LVGL rather than retained
  as a second new source/widget model. Integration is still pending.
- Shared browser/Android SVG composition renderer now uses the same typed state,
  layer/property projection, momentary/toggle/selector ownership and cancellation.
  Navigation remains local; confirmed indicators require fresh simulator samples.
  Missing telemetry does not suppress local pressed appearance. Designer and UI
  coverage suites pass: 104 and 24 tests respectively, with all coverage gates.
- Native dev Cockpit is running from this isolated worktree at port 1422. The
  native navigation allowlist now checks the configured exact loopback origin;
  the targeted native origin test passes. Packaged origin rules are preserved.
- Actual native UI acceptance (`OSG_WIDGETS_NATIVE_UI=1 node --test
  tests/ui/widgets-native.mjs`) passes. It imports all ten catalog widgets,
  exercises real pointer capture and release, edits and saves a library revision,
  verifies independent pinned versions 1 and 5, saves the panel through native
  publication admission and reloads both panel and library. Local screenshots
  and evidence: `.research/widgets-native-ui/1792622346932/`. This proves native
  authoring/preview and persistence, not ESP32 or Android-device qualification.
- Extended native authoring acceptance also verifies state-specific text,
  resetting overrides, edited hit-area geometry and restored hit areas. The
  inspector now exposes selector options/fallback, feedback port ranges, numeric
  display precision, property bounds, artwork imports and layer alignment.
  Native evidence: `.research/widgets-native-ui/1792622887004/`; desktop/browser
  Svelte checks pass with zero diagnostics.
- LVGL 9.4.0 is pinned by commit and archive digest. Its synchronous adapter
  writes partial RGB565 strips through the existing board interface and fences
  presentation outside session/input ownership. Native sanitizers render eight
  state appearances from the actual shared catalog source. This feasibility
  harness deliberately uses a built-in font and pre-resolved state fixtures:
  portable font metrics, device-side state execution, compiled content store,
  authenticated USB/WSS delivery and main-loop adoption are still required.
  The generic firmware still calls the existing raster main loop at this point.
- `python3 apps/companion-esp32/test/lvgl.py` passes with upstream LVGL itself
  instrumented by address/undefined-behavior sanitizers. Adapter coverage is
  100% lines, 71.64% branches and 100% functions. Checks include scaled ARGB
  images (RGB565 corner quantization tolerance of one channel step), clipping,
  unknown/stale/pressed/active appearances, bounded 8-row writes, injected write
  and present failures, and full-redraw recovery. The eight PNGs and coverage
  report are retained in `.research/lvgl-native/` and configured as CI artifacts.
  MCU cross-build results for the new component remain pending.
- The Sunton and Waveshare physical-board configurations successfully compile
  the LVGL component and adapter at `f5eaf07` in [CI run 39689943931](https://github.com/opensimgear/cockpit/actions/runs/39689943931).
  Host-wire tests also pass there. Headless jobs exposed a pre-existing 2 MiB
  default conflicting with the 3.9 MiB partition table; common defaults now use
  4 MiB while board overrides retain their own sizes. The portable export budget
  test now creates nonempty pages so it reaches the intended budget gate, and
  compares generated C text independently of Windows checkout line endings.
- Reused the allocation-free A/B content store and target-binary structural
  inspector from render-package commit `f2c9147`; the old custom compositor was
  excluded. Native SHA-256/ASan/UBSan tests pass all 318 modeled power cuts and
  immutable-reader checks with maximum reads/writes of 2048 bytes. Store
  coverage: 100% lines/functions, 72.44% branches; inspector: 100% lines/functions,
  94.92% branches. Partition binding, source/semantic admission, compiler and
  live delivery remain pending integration. These tests do not write hardware.
- The entire ESP32 workflow at `cc0ea17` passes [CI run 39690518696](https://github.com/opensimgear/cockpit/actions/runs/39690518696):
  portable render/network/storage tests, both headless MCU targets, Sunton,
  Waveshare and host-wire. This is cross-build/software evidence, not hardware.
- Portable sans/mono fonts now share pinned TTF files, integer glyph advances
  and baseline metrics between WidgetView and LVGL. Browser and Android frontend
  bundles contain both fonts and their OFL notices; firmware font data retains
  the corresponding notices. Native font ownership is confined to the display
  task with eight face/size slots, unretained glyph bitmaps and strict UTF-8/text
  limits. Missing glyphs are rejected by the native font API, not silently lost.
- LVGL TinyTTF exposed an upstream 64-bit private-cache alignment fault under
  UBSan. A declared, checksum-verified pointer-alignment patch fixes it without
  suppressing checks or changing metrics. The harness now treats UBSan findings
  as terminal failures. Native renderer/font tests pass: 100% lines/functions;
  71.64% adapter and 82.46% font branches. Shared UI coverage passes 26 tests.
- Actual native Cockpit font-load/authoring evidence is retained at
  `.research/widgets-native-ui/1792624359614/`. Production Cockpit, browser and
  Android frontend builds pass. The reference capture uses the actual WidgetView
  rendered in the running Cockpit WebView at 800×480, independently of the C
  adapter. Twelve text layers match bounds/ink placement within the documented
  two-physical-pixel rasterizer tolerance. Files:
  `.research/widget-browser-reference/typography-parity.json` and the browser and
  LVGL `active.png` images. This is a typography fixture, not full cross-platform
  or live-session acceptance.

## Remaining runtime integration

Device-side definition admission now has an independently tested bounded C
decoder. It matches all 5,965 shared-validator mutations and all resolved layers
of 15 source/instance examples across eight states. ASan/UBSan pass, with 100%
line/function and 89.30% branch coverage. Failed candidates preserve already
admitted bytes; JSON may be destroyed after admission. Native data for 52 layers
occupies 34,753 bytes. This is software evidence, not a board budget or live
delivery claim. See `apps/companion-esp32/render/WIDGET_SOURCE.md`.
The actual dev Cockpit authoring/persistence check passes again with evidence at
`.research/widgets-native-ui/1792625493148/`. Font commit `0ae2fdb` now passes
all six ESP32 jobs, including both physical-board cross-builds, in
[CI run 39692895632](https://github.com/opensimgear/cockpit/actions/runs/39692895632).

W2 allows bounded runtime construction from the portable source. Extend the
existing authenticated canonical-document delivery and immutable candidate
snapshots to schema 3, keeping the source publication hash unchanged. This avoids
resurrecting the old branch's separate composite authoring format. Reuse current
asset/image-cache transfer and board fences; the generic A/B target-content
infrastructure remains available where prepared assets require it.

The remaining work must implement device-side layer/state/property admission,
typed component inputs/feedback, dynamic LVGL construction, capability/resource
negotiation, main-loop adoption and same-firmware data updates. Rich S3 limits and
lower-memory Sunton limits must be explicit and independent of browser/Android
authoring. Preserve existing lease/baseline/cancellation and source-confirmation
rules. Full publishing, all-core-widget parity, Android software build checks and
assembled native Cockpit acceptance are still required; physical validation is
deferred only as stated above.
