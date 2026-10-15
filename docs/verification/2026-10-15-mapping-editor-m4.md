# M4: mapping editor and deterministic built-ins

Issue #30 adds editable drafts, logical target layouts, input learning, one-to-one presets, rule chains, modifier/layer gates and a paged raw/transformed/output inspector. Native output and visibility stay independently disabled by their qualification gates.

## Semantics

Draft saves use the existing bounded, validated profile store and never replace a running engine. Explicit preview start validates the candidate and assignments before releasing the previous run and establishing fresh baselines. Real live replacement still requires Stop and a new qualified output session. Advanced JSON is validated by the native compiler before becoming an editable draft.

Built-ins use integer arithmetic and explicit runtime time. Axis transforms apply dead zone, inversion, scale (1000 = unity), clamp and optional piecewise interpolation in that order. Merge subtracts the first unipolar axis from the second; split selects one bipolar half into a unipolar axis. Button-to-axis uses declared pressed/released values. Thresholds use separate press and release limits. Chords require all inputs. Gates provide modifiers/layers; disabling a gate removes its contribution and downstream timing, and a held momentary button must release before it can re-arm after a layer change. Toggles react to rising edges.

Single taps defer until the double-tap window expires; a second short tap cancels the single tap and produces a double tap. Hold starts at its threshold and ends on release. A pulse runs its full duration after release; repeat stops on release. A new macro press replaces that node's ordered sequence. Macros contain at most 16 steps, start at zero, end released, and last at most 60 seconds. Temporal nodes reserve from the shared 256-timer budget. Catch-up work is bounded even when a hidden intermediate rule would produce no output reports.

Buttons combine producer contributions with OR, so releasing one owner cannot release another hold. Multiple axis/hat producers require a complete priority list. The inspector shows each node, normalized inputs, native raw samples when available, transformed value, output, contribution, writer order and next deadline. Native raw snapshots are display observations and may be newer than the engine sample; they never feed mappings. Display polling is throttled to four updates per second and 32 nodes per page.

Learning starts only after mapping Stop. It observes the native report journal on the worker thread for up to 30 seconds, pins the source generation, ignores axis changes below 2048 normalized units, and presents simultaneous candidates explicitly. A learned control changes only the local draft assignment; Save remains explicit. Gaps, reconnects and timeout stop learning without output.

## Validation

Portable traces cover curve endpoints, threshold noise, chords, toggles, deferred tap/double-tap/hold, overlapping pulses, layer changes during held buttons, Stop during a macro, cancellation before advancing after source loss, and bounded repeat catch-up. Existing transport tests pass (148 tests). Frontend helper coverage is 100%; changed native modules exceed 70% coverage.

The running Windows Tauri WebView was exercised through its UI: a synthetic recording profile was created, a pulse was edited and saved with a 5000 microsecond duration, preview started, and the UI thread was deliberately busy for 150 milliseconds. The native worker still recorded the timed release. No JavaScript exceptions occurred and live-output controls remained disabled. Evidence is under `.research/controller-mapping-m0/inspect-editor-ui.mjs` and `cockpit-m4-editor.png`.

Real virtual-driver delivery under a busy UI remains conditional on #25/#27/#28 qualification; the recording proof does not qualify those backends. Keep this physical acceptance gap open.
