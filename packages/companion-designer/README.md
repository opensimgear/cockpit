# Portable companion designer (C1)

This package owns the editable UI model and portable draft/publication format for Cockpit's Svelte designer and companion renderer. It has no network listener, controller output, driver, game, pairing or credential side effects. Machine-specific target assignments and host secrets are never part of this document.

## Format and renderer contract

DesignerDocument schemaVersion 2 wraps the unchanged C0 PortableUi schemaVersion 1. Its presentation adds widgets, groups, normalized page bounds, grid, viewport dimensions and normalized safe insets. Logical targets describe axis modes, button/hat capacities and stable input bindings; they identify portable layout roles rather than Windows vJoy slot numbers. C4 still needs native capacity preflight.

Widgets include momentary/toggle/pulse/repeating buttons, normalized selector values, hats, two-direction encoder button pulses, bipolar/unipolar sliders, XY pads and indicator/text/numeric/icon/gauge outputs. Axis centering/deadzone/inversion, fallback, pulse/repeat bounds and primitive arity are validated. Presentation is normalized into C0 pages for wire input; the renderer consumes the richer outer document. Safe insets are fractions, while viewport width/height are pixels.

normalizeToPortableUi derives C0 page items from presentation widgets. validateDesignerDocument rejects inconsistent imported page items, unknown properties, duplicate IDs, incompatible primitive references, out-of-page geometry, group cycles/references, unsafe strings/colors, incompatible target bindings, duplicate target output assignments and documents above 1 MiB. Groups are flat and page-local; nested/cyclic groups have no representation.

migratePortableUi explicitly imports valid legacy C0 documents, preserving logical control/output/page IDs and bindings. Bare legacy documents cannot import missing asset bytes. Missing assets are errors rather than silently discarded data.

## Editing and library

newDocument(title) seeds one button because C0 requires at least one logical input. widgetPresets and defaultWidgetConfig provide reusable starting controls. createDesignerHistory / applyDesignerCommand / undoDesigner / redoDesigner produce detached document states and cap history at 50 entries. Failed commands never replace current state. Move, resize, style and grouping retain stable IDs; widget duplication creates fresh primitive IDs and non-overlapping logical target bindings. Whole-draft duplication also remaps document/page/group/channel/asset/target references.

Commands cover widgets, pages, viewports/backgrounds, groups, grid snapping, alignment, multi-widget translation, logical targets/bindings and validated document replacement. Alignment uses the first selected widget as its anchor. Selections remain on one page; normalized geometry is checked transactionally.

createLibrary, saveDraft, openDraft, duplicateDraft, deleteDraft and async validateLibrary implement a bounded 20-draft library. Drafts are an array of {id,name,document,assets}. The app persists the returned library atomically; storage and UI selection are outside this package. User edits, published revisions, and machine assignments remain distinct records.

## Portable image bundles

exportBundle / importBundle use bundleVersion 1 with document and asset payloads. Payloads contain assetId and canonical base64 bytes; the document contains content digests, MIME and byte sizes. Only static PNG/JPEG/WebP are allowed. SVG, remote URLs, animated PNG/WebP, oversized files and decoded dimensions above 8192 per side / 16 megapixels are rejected.

Limits are 2 MiB per image, 8 MiB total asset bytes, 33,554,432 total decoded pixels (`DESIGNER_LIMITS.totalAssetPixels`), 1 MiB document and 13 MiB serialized bundle (including base64 expansion). Import checks the outer bound before JSON parsing and validates exact SHA-256/length/signature/dimensions against the manifest. The entire image set passes the decoded pixel budget before browser decoding begins. Asset bytes do not count against the logical document budget. The browser additionally decodes every imported/loaded/published image with createImageBitmap, checks decoded dimensions and closes its bitmap. Node-only validation provides bounded container preflight and digest checks; it is not evidence of an actual browser codec.

importAsset returns a detached updated document and payload. Button icons and page backgrounds reference validated manifest IDs. Deleting an asset still referenced by a widget/background fails document validation.

## Immutable publication and qualification boundary

publishDocument requires every input to have a logical target binding and every asset to be present and verified. It returns a detached deeply frozen revision with publicationHash over canonical full version 2 JSON, including behavior, layout and manifest digests; asset bytes are verified separately. capabilityFingerprint covers primitives, semantic widget configuration, logical target layouts and bindings. compareDesignerPublications distinguishes exact identity, styling changes and capability changes. Styling includes layout/labels/assets; pulse/repeat/toggle, selector values, axis policies and target binding changes require descriptor reconciliation.

C2 must bind sessions to this outer publicationHash while using normalized C0 primitives. Pairing, native transport, actual virtual output, mobile signing/lifecycle and hardware/game qualification remain separate. Publishing a local immutable revision never arms or connects a controller.

Local verification from this package: npm run check, npm test, npm run coverage. Node 24+ directly imports TypeScript; coverage requires at least 71% lines, branches and functions. The examples contain deterministic flight and racing bundles, and are regenerated with node examples/generate.mjs.

## Explicit schema 3 composite authoring foundation

`DesignerDocumentV3` and the APIs in `src/composite.ts` are a separate, opt-in authoring path. Existing schema 2 documents, validation, publication hashes, draft/library storage and the live app/runtime remain on schema 2. Importing this module does not migrate saved content or make the native service accept schema 3.

The source separates semantic owners and typed feedback contracts from visual component instances. Each owner claims its logical controls/outputs exactly once. Components reference immutable recipe IDs, integer versions and SHA-256 digests, declared target profiles/features, bounded scalar visual parameters and validated asset IDs. Recipes are declarative references; arbitrary JavaScript, callbacks and nested executable configuration have no document representation. Catalog resolution and verification of the actual recipe bytes remain compiler work. A source publication alone does not establish that a recipe can render on a device.

- `validateDesignerDocumentV3` validates detached strict JSON, ownership, feedback types/units/ranges/provenance, target bindings, component references and geometry. `normalizeCompositeToPortableUi` creates the single PortableUi 1 logical projection. Read-only visual aliases may repeat feedback references without adding logical placements. Input references stay within their owning component; a second component cannot claim the same owner.
- `migrateDesignerV2ToV3(document, resolveRecipe)` explicitly migrates momentary/toggle buttons and all legacy feedback widgets. The caller supplies real immutable catalog references. Existing primitive/widget/page/asset/target IDs, bindings and presentation data are retained; owner IDs are added deterministically. Unsupported pulse/repeat/selector/hat/encoder/slider/XY legacy interactions fail with the offending widget instead of disappearing or changing meaning. Legacy text uses a conservative 1024-byte source capacity rather than silently reducing the previous text contract to an embedded limit.
- `publishDesignerV3Document` verifies existing image payloads and returns a detached frozen source revision. Its full publication hash includes schema 3 and visual recipe references. `compositeSemanticData` defines fingerprint version 1; `compareDesignerV3Publications` classifies identical, styling and capability revisions. Semantic identity includes interaction ownership/behavior, atomic policies/transforms, fallbacks, typed feedback contracts, logical targets and bindings; layout, labels, artwork, recipe versions and visual parameters stay outside it.
- `checkCompositeTarget` reports component-specific declared-target/feature errors and provided text-capacity limits. With render ABI 1 it also diagnoses guard intervals outside 50–10000 ms, triangle quantization other than 32767 steps, signed Q16.16 range overflow/collapse, unit labels above 24 UTF-8 bytes and XY/triangle policies that require a future semantic-descriptor extension. This is source compatibility checking, not complete package admission: raster/storage/work budgets and verified catalog contents belong to the compiler and native authority.

Advanced guarded, XY and barycentric interactions can be represented explicitly in source. Initial ESP32 profiles remain qualified only for their advertised subset; accepting a source document never qualifies an interaction or admits an input session. A navigation owner has no gameplay controls and names an existing page; it creates no extra PortableUi control placement.

Verification: strict TypeScript passes and all 72 designer tests pass, including eleven dedicated composite tests. The new production module has 100% line, 97.25% branch and 98.53% function coverage. Tests cover legacy identity preservation, explicit unsupported migration, owner-ID collisions, repeated read-only feedback, immutable hashes, styling/semantic classification, target/lowering diagnostics malformed ownership/transform/source rejection, numeric/UTF-8 ABI boundaries and concurrent asset mutation. Shared asset verification snapshots the manifest and payloads before awaiting digest/browser decoding, so both schema 2 and schema 3 publications return exactly the bytes verified. Run the existing package check/test/coverage commands to reproduce them.
