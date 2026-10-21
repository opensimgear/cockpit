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
