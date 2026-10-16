# Shared companion panel UI

The C1 renderer consumes a validated `DesignerDocument` from `@opensimgear/companion-designer`. Cockpit's designer and the standalone Companion use the same Svelte canvas and recording-only input controller. No renderer callback talks to a game, native driver, file system or network.

```svelte
<PanelCanvas document={draft} pageId={page} mode="edit"
  selectedIds={selection} onselect={(ids) => selection = ids}
  onlayout={(id, bounds) => moveInDesignerModel(id, bounds)} />

<PanelCanvas document={draft} mode="preview" assets={verifiedAssetUrls}
  feedback={samplesByChannelId} oncontroller={(controller) => preview = controller} />
<!-- Explicitly call preview.arm(); no preview automatically arms. -->
```

The optional `controller` prop lets a parent own a controller created with `createPreviewController(document)`. `oncontroller` receives the owned controller on mount. `subscribe` provides detached values/events; `onchange` records ordered primitive changes. The owning app must disarm on native lifecycle invalidation as well as the canvas's built-in DOM blur, hidden-page and pagehide handling.

`PanelCanvas` renders normalized widget layouts inside each page viewport's safe inset region and preserves its aspect ratio. It supports group outlines, validated local raster page backgrounds/button icons, all C1 controls, and typed text/numeric/icon/indicator/gauge feedback. Asset values are `assetId → data:image/png|jpeg|webp;base64,...`; remote URLs and SVG are rejected. Asset bytes should first pass the designer bundle's hash/image validation.

Edit overlays consume input, provide shift multi-selection, pointer dragging and grid-step arrow moves through callbacks. Pointer drag and selected-widget resize gestures honor snapping and commit once on release; cancellation discards transient bounds. The parent owns persistence, history, binding, inspector and publication. Preview changes never edit the document. Sliders and XY controls respect unipolar/bipolar ranges, deadzone, inversion, spring return or latching and each primitive's declared fallback. Selectors publish scalar option values. Relative encoders publish distinct clockwise/counterclockwise button pulses. Repeat/pulse timing is deterministic and bounded; a delayed clock tick disarms rather than replaying missed input. A second one-shot pulse activation while the first is pending is explicitly rejected rather than coalesced; repeating inputs require a positive release interval.

Pointer cancellation/capture loss releases that pointer's controls without disturbing another pointer. Stop, page/mode changes, document replacement, unmount and lifecycle loss return every control—including latched/toggled values and detached pulses—to declared fallback and discard timers. Keyboard auto-repeat cannot retrigger a momentary/toggle/pulse button. Keyboard widget blur cancels held input; normal axis key release respects latching.

Feedback samples use the C0 typed `FeedbackValue` plus local `receivedAtMs`, indexed by channel ID. The canvas shows the supplied provenance/source and age, labels aging/stale/unavailable samples, and does not manufacture game confirmation from a local preview value. The preview has no real simulator feedback source.

Run `pnpm --filter @opensimgear/companion-ui check` and `coverage`. Tests cover behavior, ordering, timer gaps, per-pointer ownership, nonzero fallback, latched state, lifecycle resets, feedback typing/freshness and raster URL boundaries. Native/mobile touch, suspend/resume, remote delivery and latency remain separate qualification work.
