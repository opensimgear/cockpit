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
| W1 #114 | Shared browser/Android composition renderer, input semantics, freshness and parity fixtures | Pending |
| W2 #115 | Pinned LVGL runtime, target compilation, board capability negotiation, existing USB/WSS delivery and same-firmware updates | Pending |
| W3 #116 | Layer/state editor, immutable library revisions, reusable independent instances, complete illuminated-switch workflow | Pending |
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
