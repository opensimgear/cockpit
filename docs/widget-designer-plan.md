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
| W0 #113 | Strict versioned definitions, instances, layers, properties, ports, states, identity, capability contracts and LVGL feasibility | Software contracts implemented and checked; physical feasibility deferred |
| W1 #114 | Shared browser/Android composition renderer, input semantics, freshness and parity fixtures | Shared renderer and visual parity checked; live browser workflow and final Android build pending |
| W2 #115 | Pinned LVGL runtime, target compilation, board capability negotiation, existing USB/WSS delivery and same-firmware updates | Implemented; actual Cockpit WSS/C-engine acceptance and all six firmware CI jobs pass; hardware deferred |
| W3 #116 | Layer/state editor, immutable library revisions, reusable independent instances, complete illuminated-switch workflow | Actual Cockpit authoring/save/reload passes; export/import delivery acceptance pending |
| W4 #117 | Original control/instrument catalog, library bundles, publishing/reconciliation, example panel and runtime updates | Twelve families implemented and checked; complete-panel live workflow acceptance pending |
| W5 #118 | Running native dev Cockpit acceptance, automated/native/build checks, documentation and software parity evidence | Evidence and guidance in progress; final requirement audit pending |

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
Pure device state/enable-gate/indicator/gauge projection also passes 1,274
comparisons with the browser/Android shared model. ASan/UBSan and clock-boundary
checks pass, with 100% line/function and 88.97% branch coverage. These functions
do not own input or replace session admission; live sample mapping remains to be
connected.

The source-driven LVGL renderer now draws admitted definitions, dynamic samples
and inherited appearances directly. Its nine scenarios cover 12 instances,
including independently overridden buttons and an explicit three-port state
example. All 108 appearances match the actual shared WidgetView captured in the
running dev Cockpit, using the documented two-pixel geometry/ink and one-step
RGB565 solid-color tolerances. Evidence:
`.research/widget-source-browser-reference/source-parity.json` and
`.research/lvgl-native/source-*.png`. The comparator also rejects missing,
displaced and recolored negative controls.

Both LVGL harnesses and the 1,274 state cases pass ASan/UBSan again. Exact live
text matches 1,353 shared-code cases. Native renderer coverage is 100% lines and
functions, 85.61% branches. Queue draining fixes TinyTTF bitmap lifetime behind
alpha-layer tasks; source alpha uses one bounded strip at a time. A WSL Windows
mount I/O stall required a native-filesystem test snapshot; source hashes were
verified against this worktree and retained at
`.research/lvgl-native/widget-draw-source-hashes.json`. This does not enable
schema-3 assigned panels yet: full publication/port mapping, main-loop adoption,
capability negotiation and same-firmware live updates remain pending.
The actual native import/edit/preview/save/reload check passes again at
`.research/widgets-native-ui/1792627358657/`.

CI exposed a missing workspace package link in the state oracle job. The setup
fix at `00d3c3f` passes all six ESP32 jobs in
[CI run 39695083501](https://github.com/opensimgear/cockpit/actions/runs/39695083501).

W2 allows bounded runtime construction from the portable source. Extend the
existing authenticated canonical-document delivery and immutable candidate
snapshots to schema 3, keeping the source publication hash unchanged. This avoids
resurrecting the old branch's separate composite authoring format. Reuse current
asset/image-cache transfer and board fences; the generic A/B target-content
infrastructure remains available where prepared assets require it.

The remaining work must connect source admission and LVGL drawing to complete
panel snapshots and the extended typed input/feedback model, capability/resource
negotiation, main-loop adoption and same-firmware data updates. Rich S3 limits and
lower-memory Sunton limits must be explicit and independent of browser/Android
authoring. Preserve existing lease/baseline/cancellation and source-confirmation
rules. Full publishing, all-core-widget parity, Android software build checks and
assembled native Cockpit acceptance are still required; physical validation is
deferred only as stated above.

The firmware core now represents component bindings with independent typed
output slots and UTF-8 samples. Its existing queue carries boolean/numeric
momentary, toggle and selector changes; neutral baselines use each declared
fallback. Pointer cancellation, enable-gate expiry and navigation match 51 steps
and 24 ordered changes from the actual shared browser/Android controller.
Feedback admission uses two bounded passes with no full-runtime stack staging,
preserving atomic rejection, output identity and simulator provenance. Tests
cover duplicate/delta snapshots, invalid bindings, bipolar queue values, UTF-8
boundaries and precise per-port freshness damage. Core, wire/client, serial,
leases and image-cache regressions pass with fatal ASan/UBSan. Core coverage is
100% lines/functions and 78.86% branches; portable widget contracts are 100%
lines/functions and 75.90% branches. The native snapshot size is 17,456 bytes.
Full schema-3 document/asset admission and main-loop adoption are still pending.
The actual native Cockpit import/edit/preview/save/reload checks pass again at
`.research/widgets-native-ui/1792629243717/`. Both real LVGL sanitizer harnesses
pass against this model change; all 103 copied source/fixture files match the
worktree, recorded in `.research/lvgl-native/widget-draw-source-hashes.json`.

Source-renderer commit `e680b81` passes all six ESP32 software/cross-build jobs in
[CI run 39697413581](https://github.com/opensimgear/cockpit/actions/runs/39697413581).

Complete schema-3 candidate admission now verifies the authenticated source hash,
strict document shape, pinned/unused definitions, input/output contracts, logical
bindings, page items, groups, safe insets, viewports and asset identities. The
owned candidate can mix legacy widgets and portable components; it retains group
metadata and prepared image references. Failed candidates rewind arena counters
and preserve earlier admitted bytes. JSON heap/reply/node budgets are configurable
only without outstanding objects and report exact resource counts. Five source
cases (46 instances) and 4,239 shared-validator mutations pass fatal sanitizers;
admission coverage is 100% lines/functions and 86.35% branches. The catalog uses
35,777 native arena bytes. This is not MCU heap/stack qualification.

The shared 51-step/24-change controller trace now goes through this complete
publication decoder. Core coverage is 100% lines/functions and 79.24% branches;
wire/client/serial/lease/cache regressions also pass. Actual dev Cockpit
import/edit/pointer-preview/save/reload checks pass again at
`.research/widgets-native-ui/1792630087043/`. Live network adoption, bank fencing,
bitmap preparation, group drawing, board capability negotiation and the main
LVGL loop remain; the active firmware delivery path still accepts schema 2 only.
Typed-model commit `1dfc156` passes all six ESP32 jobs in
[CI run 39700777334](https://github.com/opensimgear/cockpit/actions/runs/39700777334).
Both real LVGL harnesses and 1,274 shared state cases also pass with the new
panel metadata ABI. All 107 native snapshot sources/fixtures match this worktree;
hashes and the complete admission log are retained in
`.research/esp32-panel-source-native/`.

Portable image layers now reuse authenticated image-chunk delivery and the
existing persistent cache, adding an optional layer identity. Host resolution is
bound to the exact instance properties and pinned revision. Firmware verifies
the echoed layer, prepares exact dimensions, deduplicates content/dimension
aliases, pins cache hits before allocating misses and preserves active-owner
pixels during failures/cancellation. The LVGL lookup is confined to prepared
pixels in its instance binding. All 116 native host tests pass, including pinned
artwork/independent instances and current-disarmed-session enforcement. The first
host-suite attempt hit sandbox loopback/credential-storage restrictions; the
authorized unrestricted retry passes. Device client/cache regressions pass with
96.53% line, 74.43% branch and 97.78% function coverage. Real LVGL sanitizer
coverage is 100% lines/functions and 86.42% branches for the renderer.

The updated native host was rebuilt and the actual dev Cockpit acceptance passes
at `.research/widgets-native-ui/1792630777228/`. Launch this app through
`pnpm tauri dev --no-watch --config .research/widgets-dev.json`; a direct Cargo
build/run missed the dev launch configuration and did not load the frontend.
No other worktree application was restarted. Complete admission commit `0de7a95`
passes [ESP32 CI run 39702180315](https://github.com/opensimgear/cockpit/actions/runs/39702180315).

Remaining integration must invoke this preparation on fenced source candidate
banks, select board-specific JSON/scene/LVGL budgets and connect the main drawing
loop. Also audit feedback wire limits: the host admits 120 UTF-16 units for units
and 256 for text, while the current device feedback path still has a 32-byte
unit check and admits at most 120 text units. Match the shared 120-unit display
projection without silently dropping valid host samples. The overall feature
and acceptance milestones remain unfinished; only physical validation is
deferred.

The feedback-limit mismatch is now corrected. Component text validates all 256
UTF-16 units admitted by the host before retaining the shared 120-unit display
prefix. Browser, Android and device projection all stop before splitting a
Unicode code point. Unit metadata admits the shared 120-unit bound and validates
UTF-8/blank content. Legacy fixed-widget text admission remains unchanged. The
complete-source trace includes a 120-unit Unicode unit; tests cover 256-unit
values, rejection at 257, invalid suffixes and surrogate-boundary display text.
All shared UI tests/type checks, device/core/network/cache regressions and real
LVGL sanitizer checks pass, including 1,357 exact shared text projections.
Portable text helpers have 100% line/function and 77.22% branch coverage. Actual
dev Cockpit acceptance passes at `.research/widgets-native-ui/1792631252789/`.

Portable scenes no longer carry the unused legacy 8 KiB string buffer. The
catalog's owned native candidate is now 27,585 bytes, down from 35,777. Schema 2
and schema 3 share strict source admission without changing canonical hashes;
the fixed-widget compatibility decoder keeps its own scratch. Six panel cases
(51 instances) and 4,244 shared-validator mutations pass sanitizers; admission
coverage is 100% lines/functions and 86.52% branches. Existing protocol/client,
lease, cache and export regressions also pass after the private scene refactor.

`network/scene_bank.c` supplies lazy caller-allocated A/B banks with immutable
active data, display-borrow fences and generation-bound preflight tickets. A
cancelled or rejected candidate retains its fence identity until the reader is
done. Failed allocations report the requested bank size and available capacity;
stale preflight completions and exhausted generations cannot activate data.
Native failure/lifetime checks pass with 100% line/function and 85.58% branch
coverage. This API owns data only and cannot grant input authority.

The 51 shared-controller steps now compare all 15 widget appearances as well as
their 24 input changes. This exposed a runtime adapter mismatch: input components
must show disabled while disarmed, while read-only/navigation components remain
available. The corrected adapter and independent output/provenance mapping pass
all 1,274 projection cases, with 100% line/function and 88.37% branch coverage.
The current shared renderer was captured again in the actual dev Cockpit at
`.research/widget-source-browser-reference/`. The 108 appearances also pass the
two-pixel/RGB565 comparison again. The live firmware still needs to
use these banks together with display-task preflight, board memory profiles and
the LVGL main loop before source assignment can be enabled. Feedback correction
`ee72739` passes [ESP32 CI run 39704009236](https://github.com/opensimgear/cockpit/actions/runs/39704009236).

Scene-bank commit `cbe6986` passes all six ESP32 jobs in
[CI run 39705147756](https://github.com/opensimgear/cockpit/actions/runs/39705147756).
The main firmware loop now calls the full LVGL panel compositor. It preserves
mixed source order and alpha, draws group labels with the same portable font as
browser/Android, and retains legacy widgets/status screens. Damage is polled
every tick, including freshness of independently bound component outputs. Only
a completed draw/present updates the successful-frame timestamp and publication
hash. Existing network render notifications and immutable panel fences remain.

Native sanitizer checks pass for the new compositor, legacy core/control traces,
1,357 exact text projections and 1,274 state cases. A narrow-region test exposed
that LVGL may pack many rows into a pixel buffer when damage is narrow; alpha
strips now split those clips explicitly. The no-PSRAM 32 KiB pool/four-row build
also passes the admitted catalog and fault suite, peaking at 13,312 pool bytes.
The rich-board pool reserves 96 KiB in PSRAM before LVGL initialization. MCU
cross-build size/stack evidence is still required; physical qualification remains
deferred. The application partition grows to 2 MiB and matching partition tables
are required for firmware upgrades; NVS offsets are unchanged.

The actual running dev Cockpit import/edit/pointer-preview/save/reload acceptance
passes at `.research/widgets-native-ui/1792632708790/`; the shared UI has zero
Svelte diagnostics and all 26 tests pass. The native whole-panel visual is
`.research/lvgl-native/panel-catalog.png`. The live network decoder still uses
schema 2: fenced source-bank adoption, candidate preflight, source capability
negotiation, remaining designer/catalog acceptance and browser/Android delivery
checks are still open. No milestone is claimed complete by this integration.

Renderer commit `dc9496e` passes portable, host-wire and Waveshare jobs in
[CI run 39706570399](https://github.com/opensimgear/cockpit/actions/runs/39706570399).
Waveshare's linked application is 0x18e050 bytes, with 22% app-partition headroom.
The Sunton/ESP32 jobs expose a 28,784-byte static DRAM overflow. The compact LVGL
pool and shared transport reply buffer now use checked lifetime reservations,
removing their 64 KiB of fixed arrays from compact-target BSS. Actual MCU rebuilds
must confirm the new fit; no runtime heap/stack or physical performance result is
inferred from that linker adjustment.

Rich-board replies now reserve 192 KiB in PSRAM and JSON uses its configurable
512 KiB/4,096-node PSRAM budget. Compact boards retain the existing 32 KiB reply
and JSON limits. Both USB and WSS use the same bounded reply workspace and keep
the existing frame/chunk protocol. Native tests pass for allocation failures and
retries in both ESP memory domains, shared source/bank/core regressions, settings,
and protocol/session/cache behavior. Serial checks verify 40 KiB reply assembly
and explicit rejection with the compact reply capacity.

The existing mobile workflow was dispatched on the widget branch. Shared checks
passed, but native builds found that the mobile lockfile omitted the shared
service's image decoder dependencies. Atomic fix `035b414` adds exactly those
eight locked packages without unrelated upgrades. The corrected Android build
is running in [mobile CI 39707088010](https://github.com/opensimgear/cockpit/actions/runs/39707088010).
Authentication qualification gates remain independent in #55/#56.

Memory correction `37adc97` passes all six ESP32 jobs in
[CI run 39707393926](https://github.com/opensimgear/cockpit/actions/runs/39707393926).
Sunton now links: application 0x18a930 bytes (23% app-partition headroom),
143,996 static DRAM bytes with 36,740 bytes left in that linker region, and
100,307 IRAM bytes. These are linker measurements, not free runtime heap or
physical timing. [Mobile CI 39707088010](https://github.com/opensimgear/cockpit/actions/runs/39707088010)
also passes shared checks and produces the `mobile-android` APK artifact and iOS
simulator bundle. Native authentication/physical qualification gates are retained.

The live firmware client now uses common schema-2/schema-3 source admission and
lazy fenced scene banks. It prepares cached images before a distinct disarmed
preflight phase, which the display task checks using a generation-bound ticket.
Late results cannot activate a cancelled/replaced candidate or clear another
reader's fence. Failed replacements preserve the prior scene disarmed with a
visible owned diagnostic/banner. Reconnect preserves local page selection only
by stable page ID within the same assigned instance/UI; page navigation cancels
input and reconnects for a fresh baseline when necessary.

The native client suite passes active-byte preservation, held-reader cancellation,
stale preflight completions, heartbeat liveness, failed budgets and stable-page
recovery. The same client binary also admits two portable compositions as data
(two and three layers/revisions), retaining logical IDs and existing ordered
input/baseline behavior. This is a protocol harness with a simulated host; actual
Cockpit USB/WSS software-target acceptance remains. Source profiles currently
bound rich arenas to 192 KiB/128 layers/32 instances and compact arenas to
16 KiB/24 layers/eight instances. Font caps are 128 and 48 respectively. Capability
advertising and Cockpit assignment enablement are still pending.

Current client sanitizer coverage is 96.66% lines, 96.23% functions and 73.92%
branches. The real LVGL compositor also passes font-budget/zero-write failures,
and actual dev Cockpit import/edit/pointer/save/reload acceptance passes again at
`.research/widgets-native-ui/1792633913735/`. Engine operations are versioned as
`2026.10.22.1`. Complete cross-platform acceptance and remaining authoring/catalog
requirements are still open; the goal remains active.

Candidate preflight keeps reporting the existing `checking-cache` preparation
phase while checking font/resource caches. The host therefore preserves its
initial feedback snapshot until the admitted runtime starts rendering. The
native host regression now explicitly covers this second preparation phase
after image download. Structured widget/definition/layer/field and budget
diagnostics are validated as bounded, assignment-bound operation metadata and
shown in Cockpit errors. All 116 native host tests and 12 message-presenter tests
pass; Cockpit and browser Svelte checks report zero errors/warnings. The updated
native dev host was rebuilt through the worktree's Tauri launch configuration.

The rebuilt actual Cockpit workflow passes at
`.research/widgets-native-ui/1792634219454/`. Source-integration CI exposed an
unnecessary renderer dependency in the generic platform header, breaking the
standalone storage harness. The display candidate API now has its own
`network/candidate.h`; storage, serial and full client/protocol regressions pass
again locally. The source-engine MCU run is
[39708656734](https://github.com/opensimgear/cockpit/actions/runs/39708656734);
the header correction requires a fresh run before claiming full CI success.

Header correction `f7d60aa` passes all six ESP32 jobs in
[CI run 39708911043](https://github.com/opensimgear/cockpit/actions/runs/39708911043).
Engine `2026.10.22.2` adds versioned renderer capability reports to authenticated
connections and nonce-bound USB status. Both memory profiles derive their
advertised and enforced scene, JSON, reply, font and LVGL limits from
`packages/companion-designer/assets/widget-profiles.json`. The generator also
checks the application/cache partition sizes. Text formatting explicitly reports
its four-digit precision and magnitude bound; this is a target limit, not a
restriction on portable authoring or browser/Android rendering.

The shared TypeScript/Rust validators accept only complete known profiles and
matching geometry/firmware identities. Reports grant no transport or input
authority. Disconnect/revocation clears authenticated reports, and conflicting
active reports produce an error instead of selecting the more permissive target.
The native host caches validated metadata for offline display without restoring
a session. Native firmware tests compare the actual C reports to the shared
manifest and exercise every profile-construction allocation failure in both
memory domains. Native and firmware alpha drawing now default to the same
four-row strips. Assignment compatibility still uses the legacy validator;
consuming these reports and actual Cockpit transport acceptance are next.

Capability metadata validation passes 119 service tests plus two ESP32 host
integration checks, 35 serial tests and 17 native host tests. The designer suite
passes with 100% line/function and 97.67% branch coverage for profile admission.
The rebuilt running dev Cockpit again passes catalog import, actual pointer
preview, immutable library editing and native save/reload at
`.research/widgets-native-ui/1792635916120/`. Windows socket/DPAPI tests require
the normal local execution context; their sandbox-only failures do not reproduce
there. Firmware cross-build validation of this metadata change is still pending.

Capability commit `70d4092` passes all six ESP32 jobs in
[CI run 39711314412](https://github.com/opensimgear/cockpit/actions/runs/39711314412).
Target checks now consume actual renderer reports in companion settings, direct
assignment, USB pairing, native profile authoring and authenticated session open.
Matching offline metadata is predictive only: a connected device without a
report cannot inherit a cached newer engine, and conflicting reports block new
sessions before any input lease is created. Legacy screens retain their existing
checks. Browser and Android companions without a bounded firmware report keep
their shared renderer capabilities.

The common target oracle has 36 cases covering compact/rich budgets, letterbox
and inset geometry, local touch areas, all authored states, pinned font glyphs,
prepared image dimensions/aliases, bindings and unsupported legacy operations.
TypeScript and Rust produce identical structured diagnostics without changing
source bytes. All 14 statically accepted panels also admit and render with the
real C/LVGL compositor in both the 96 KiB and 32 KiB pool builds. Seven glyph,
font-size or zero-area failures reject before board writes. Engine
`2026.10.22.3` adds the corresponding zero-area candidate preflight guard.
These are software checks; final device memory/cache availability is still
checked by fenced candidate preparation and no physical qualification is implied.

ESP32 assignments no longer disable grouping or block browser preview/export
in the designer. Incompatible target updates retain the prior assignment and
report a diagnostic; the authored panel can still be saved locally. The rebuilt
dev Cockpit passes catalog import, actual pointer preview, independent immutable
library revisions and native save/reload at
`.research/widgets-native-ui/1792636724827/`. Validation also passes 121 service
tests, two ESP32 host checks, 18 native host tests, eight passive profile-authoring
tests, 39 focused designer/settings/message tests, and zero Cockpit/browser
Svelte diagnostics. New compatibility code has 100% line/function and 94.29%
branch coverage; the C compositor has 100% line/function and 84.31% branch
coverage. Full actual-Cockpit transport/software-engine acceptance, remaining
catalog/designer criteria and final Android/release acceptance remain open.

Compatibility commit `bf78b07` passes all six ESP32 jobs in
[CI run 39712611017](https://github.com/opensimgear/cockpit/actions/runs/39712611017).
Actual Cockpit transport acceptance now passes at
`.research/widget-software-native/1792637446762/evidence.json`. An opt-in native
test uses the running dev Cockpit's normal IPC/publication/assignment operations
and authenticated certificate-verified WSS service. One unchanged C binary links
the actual firmware client, source banks, input controller and LVGL compositor;
only board/task/transport I/O and flash storage are supplied by software adapters.
Its PID and binary hash remain unchanged across both compositions and reconnect.

The first composition contains one reusable illuminated control; the second
contains two independently bound instances plus an original prepared image.
Both retain canonical source identity and complete frame presentation followed
by a neutral baseline before input. Actual software touch press/release reaches
the host in order and returns to unknown telemetry instead of inventing a
confirmed active state. Holding touch through replacement keeps the new panel
disarmed until release. A glyph-incompatible assignment is rejected while the
previous panel stays armed and assigned. The host streams 17,280 prepared pixel
bytes using 12 image requests; reconnect reuses one cached image with zero
misses/downloads and establishes a new session/baseline. Test credentials and
temporary publications are removed afterwards.

The test is `tests/ui/widget-software-native.mjs`; build its target using
`apps/companion-esp32/test/software_client.py` or the native LVGL suite, which now
also sanitizer-smokes the software target and records `software-client-smoke.json`.
This is software transport/render/input evidence. The adapter's bounded RAM image
cache does not qualify durable flash behavior, ESP-IDF task scheduling, physical
touch/DMA/stack budgets or Android native authentication. Those independent
hardware/native gates remain unchanged. Remaining milestone/catalog/designer
acceptance and final Android branch build still need a complete feature audit.

The visual widget inspector now exposes numeric property links for gauge
minimum/maximum, corner radius, stroke width and opacity. Paint/radius pickers
offer only properties whose declared bounds fit the field; linked number inputs
stay read-only until explicitly unlinked. Unlinking retains the resolved value,
and existing definition validation/undo/versioning handles the edit without
changing typed ports. Actual dev Cockpit acceptance at
`.research/widgets-native-ui/1792637663929/` verifies pinned exposed gauge ranges
and intentional unlinking, along with import, pointer preview and save/reload.
Cockpit/browser Svelte checks again have zero errors/warnings.

Numeric-property commit `f9216fd` passes all six ESP32 jobs in
[CI run 39713978949](https://github.com/opensimgear/cockpit/actions/runs/39713978949).
The original catalog now also includes a standalone typed text instrument and
an editable vector power icon. Its ring/stem illuminate only from fresh
game-confirmed simulator feedback. The complete twelve-family bundle is
`examples/companion/shared-systems-complete.bundle.json`; the established
ten-widget example and its archived publication/capability hashes are unchanged.

The complete catalog passes 121 native publication tests and two ESP32 host
checks, actual Cockpit import/edit/preview/save/reload at
`.research/widgets-native-ui/1792638131568/`, and 126 browser/LVGL appearance
comparisons over nine states. The visual verifier retains two pixels of context
outside each fractional widget crop, so its existing two-pixel geometry rule
also applies at the crop edge. Counted widget pixels, RGB565 color tolerance and
the 95% matching threshold are unchanged; missing, displaced and recolored
feature controls still reject. Rich/compact native compositor checks cover all
17 admitted cases of the expanded 40-case shared target oracle; seven unsupported
font/glyph/zero-area cases reject before drawing. The compact compositor peaks
at 17,248 bytes in its 32 KiB LVGL pool. Source admission covers seven documents,
63 widgets and 4,244 mutations; dynamic text checks retain 1,357 projections.
These are software measurements, without physical board or Android-device
qualification. Successful native UI acceptance removes its own unassigned test
panel after proving persistence, keeping repeated runs within draft/cache limits.
