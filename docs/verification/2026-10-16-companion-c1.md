# Companion C1 designer and portable panels — 16 October 2026

Issue [#39](https://github.com/opensimgear/cockpit/issues/39) implements the local
visual designer and arbitrary-document renderer for the companion feature
[#37](https://github.com/opensimgear/cockpit/issues/37). Cockpit and the separate
Companion application share the Svelte 5 renderer and TypeScript input semantics.
Android/iOS and another Windows PC remain delivery targets; this checkpoint does
not establish their native touch, safe-area, lifecycle or installation behavior.

## Delivered scope

- `packages/companion-designer` owns the version 2 outer document, validated C0
  logical controls, portable target layouts/bindings, editing/history, library,
  asset bundles, migration and immutable publication. Pages, normalized geometry,
  grid, groups, viewport dimensions and safe insets stay separate from machine
  assignments and credentials.
- `packages/companion-ui` owns the shared arbitrary-document Svelte canvas and
  recording-only input controller. It renders buttons, selectors, hats, encoders,
  sliders, XY axes and indicator/text/numeric/icon/gauge outputs. Pointer and
  keyboard ownership, axis fallback, latching, toggle/pulse/repeat behavior,
  cancellation and typed feedback presentation are explicit.
- Cockpit's Companion panels view integrates saved panels, templates, page/widget
  editing, drag/resize, grid/alignment/grouping, undo/redo, properties, logical
  binding, import/export, preview and publication. Changes disarm local preview.
- `src/lib/companion-library.ts` persists validated drafts and a separate
  publication archive in one atomic IndexedDB record. Save/load recompute each
  publication's content and capability hashes and validate its assets. Invalid
  data or a failed transaction leaves the previous committed snapshot intact.
  The local store is bounded to 64 MiB and 100 published revisions; the designer
  library is bounded to 20 drafts.

Designer schema 2 wraps C0 schema 1 through an explicit migration. Existing
logical IDs survive movement/restyling and migration; duplication remaps the
required IDs and references. Portable logical target descriptors contain no
Windows vJoy slot, controller ownership, pairing or credential information.

The full version 2 document determines the publication hash, including semantic
widget configuration and asset digests. Assets are verified separately against
their exact bytes. Published revisions are detached and deeply frozen. Semantic
input changes and target bindings affect the capability fingerprint and require
a new generation/reconciliation in the later native integration.

Assets are bounded static PNG/JPEG/WebP with size, signature, dimensions and
SHA-256 checks. Limits are 2 MiB per image, 8 MiB total assets, 1 MiB document and
13 MiB serialized bundle. Browser validation also decodes images with
`createImageBitmap` and closes the decoded bitmap. Node container/header tests
are separate from actual browser codec behavior.

Saving, importing, editing, previewing and publishing perform local document or
recording operations. This checkpoint adds no production LAN listener, pairing,
driver provisioning, native virtual report or game activation. Existing
unqualified host capability gates and optional defaults stay unchanged.

## Local automated validation

Checks ran locally with the Windows frontend toolchain and the owned C1 changes.
The coverage percentages below describe measurable TypeScript input/model/store
modules; they do not measure Svelte gesture delivery or native OS behavior.

| Check | Result |
| --- | --- |
| Designer model, assets, editing and publication tests | 57 pass; 99.66% lines, 96.58% branches, 98.60% functions |
| Shared input/feedback semantics tests | 16 pass; 100% lines, 96.35% branches, 95.83% functions |
| Atomic library/publication persistence tests | 12 pass; 100% lines, 96.74% branches, 100% functions |
| Existing C0 contracts/session tests | 64 pass |
| Existing frontend transport tests | 155 pass |
| Root and companion/package TypeScript/Svelte checks | Pass; zero errors/warnings |
| Cockpit and Companion production frontend builds | Pass |
| Native Windows Companion executable with embedded C1 assets | Pass; locked dependencies, `custom-protocol`, unsigned development build |
| Eight existing frontend coverage commands | Pass |
| Combined two-panel document/storage/bundle acceptance scenario | Pass |

The combined automated acceptance scenario uses two distinct UIs, a second page,
an image asset and representative controls. It saves and reopens the library,
duplicates a panel, exports/imports its bundle and verifies publication identity.
This provides document and persistence evidence without relying on the native
file chooser.

Focused regressions cover stable IDs and bindings, strict validation, bounded
asset bundles, immutable publications, failed writes, queued save ordering,
retained publication snapshots and corruption diagnostics. Input regressions
cover simultaneous button/XY ownership, nonzero fallback, latched/toggled state,
page/document/lifecycle cancellation, ordered pulses, bounded repeat timing and
feedback provenance/freshness. A new activation while a one-shot pulse is still
pending is explicitly rejected. Repeating inputs require a positive release
interval. Edit gestures snap when enabled and commit one history operation on
release; cancellation discards transient geometry.

Reproduction uses `pnpm check`, `pnpm companion:check`, `pnpm companion:test`,
`pnpm transport:test`, `pnpm ui:build`, `pnpm companion:ui:build` and the existing
eight coverage scripts in the root package. The shared packages also expose
their own `check` and `coverage` scripts. The native Companion build used
`cargo build --locked --features custom-protocol --bin osg-companion` from its
Tauri directory. Remote CI status is tracked separately;
these results do not claim a remote run has completed.

## Browser interaction and qualification boundary

Browser interaction exercised create/save/reopen for two templates, adding a
page and a button, changing a label through keyboard input, selecting phone
portrait preview and explicitly arming local preview. The final preview button
and hat check recorded four transitions and returned to zero active pointers.
This is browser UI evidence. It does not establish physical multitouch, native
phone/tablet safe areas or operation on a second Windows PC.

The native file chooser upload attempt stalled. No native file-picker
import/export round-trip pass is claimed. Automated bundle and persistence
round-trips passed independently, as recorded above.

The remaining C1 qualification checks are retained, unchecked, in
[#59](https://github.com/opensimgear/cockpit/issues/59):

- Real phone/tablet portrait and landscape safe areas, target sizing, simultaneous
  touches and native lifecycle cancellation.
- The shared panel on another Windows PC, including native touch/focus/lock/sleep
  behavior and realistic screen scaling.
- A native file-picker import/export round-trip, including a panel with an image,
  reopening the saved panel and verifying the same IDs, bindings and publication.

These checks are separate from the existing Android/iOS signing and trust,
second-PC networking and packaged mixed-peer qualification stories
[#55](https://github.com/opensimgear/cockpit/issues/55),
[#56](https://github.com/opensimgear/cockpit/issues/56),
[#57](https://github.com/opensimgear/cockpit/issues/57) and
[#58](https://github.com/opensimgear/cockpit/issues/58). Transferring a check does
not turn its unverified native behavior into a pass. The native listener and
authenticated source integration remain C2; virtual submission remains C4.
