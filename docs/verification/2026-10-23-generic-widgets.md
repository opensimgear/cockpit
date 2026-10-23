# Generic compiled widgets — feature #151

Scope: the generic source designer, off-device raster compiler and browser
companion. Toggle is the only shipped seed. ESP32 work remains in feature #161;
new ESP32 pairing is disabled on `main`.

## Compilation and sampling costs

Run `node packages/widget-compiler/scripts/measure-fixtures.mjs` to reproduce
artifact identities and resource accounting. The adjacent
[measurement data](2026-10-23-generic-widgets-costs.json) includes exact options,
capabilities, hashes, domains and resource fields.

The following observation used Windows x64, Node 24.20.0 and a Ryzen 7 7800X3D.
Each row clears the compiler cache, compiles at the source's native dimensions
with the browser v1 profile and audio disabled, then decodes 100 frames. Other
development tests were running on the same machine. Timings describe this run.
Byte counts and artifact identities follow deterministic compiler contracts.

| Source | Pixels | Motion | Frames | Unique tiles | Asset bytes | Manifest bytes | Resident allowance | Compile seconds | Mean decode ms |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Toggle | 240 × 144 | Off | 64 | 357 | 274,176 | 96,889 | 1,830,628 | 1.062 | 0.275 |
| Toggle | 240 × 144 | On | 320 | 561 | 430,848 | 255,744 | 3,369,216 | 4.984 | 0.319 |
| Linear RPM | 256 × 64 | Off | 1,032 | 178 | 136,704 | 323,212 | 2,499,888 | 16.075 | 0.153 |
| Radial RPM | 192 × 192 | Off | 776 | 1,322 | 1,015,296 | 594,027 | 7,626,924 | 12.165 | 0.321 |
| Three positions | 192 × 64 | Off | 3 | 46 | 35,328 | 10,407 | 283,164 | 0.039 | 0.093 |

The linear gauge has 129 samples over 0–8,000 RPM, with a conservative 0.9375
pixel half-step error over 240 pixels of travel. The radial gauge has 97 samples
over the same range, with a 1.7672 pixel half-step error at a 72 pixel radius over
270 degrees. These satisfy their respective 1 and 2 pixel limits. Threshold
guards use raw RPM: redline enters at 7,000 and clears at 6,800. They do not use
the selected visual sample. Both gauges have independent 500 ms freshness and
redline state, with explicit unknown/stale/disconnected presentation.

The gauge frame counts include every sample × four qualities × two redline
states. Increasing independent appearance dimensions increases that product.
Preflight rejects an unsupported sample error, frame count, asset budget or
active-plus-candidate allowance before publishing. Identical finished tiles
deduplicate across states. All sizes above include complete appearances;
authoring layers and fonts are absent from deployed artifacts.

Resident figures are the normative conservative allowances documented in the
[compiler](../../packages/widget-compiler/README.md), rather than JavaScript heap
measurements. Active plus candidate is twice each resident allowance. Panels
also check their aggregate manifest, shared assets and every widget surface.
Motion-off gauges still accept live values; they snap between sampled frames.

## Qualification procedure

`pnpm widgets:native:ui` uses the actual running dev Cockpit and a real browser
companion, with normal certificate verification. Its evidence includes native
source authoring and target-size Try, normal panel Save/assignment, pixel
comparisons, browser input and media, gauge stimuli, lifecycle cancellation,
cache and candidate replacement. PNG/JPEG/WebP imports exercise real WebView
WASM/CSP execution. Tests write screenshots and detailed traces under ignored
`.research/` directories. All six scenarios passed, including targeted reruns
after fixing the failures found during qualification.

The new gauge and three-position sources are generalization fixtures outside
the shipped catalogue. Synthetic feedback tests exercise browser dispatch and
presentation; they do not establish a real simulator or LAN latency result.
Native service tests separately check provenance and authority admission.

## Recorded results

| Check | Result |
| --- | --- |
| TypeScript, generated source/media corpus and Svelte | Passed; zero Svelte errors or warnings |
| Widget runtime | 32 tests; 100% lines / 97.32% branches / 99.60% functions |
| Deterministic compiler | 33 tests; 99.80% / 97.67% / 99.21% |
| Source store and editor history | 10 tests; 94.15% / 96.91% / 96.55% |
| Companion contracts/designer/UI and application helpers | All `pnpm companion:test` tests and 71% gates passed |
| Browser lifecycle, cache, feedback and candidate failure | 31 tests passed |
| Full native development workspace | 1,158 desktop tests passed, 8 ignored; 138 service tests passed |
| Clean selected feature snapshot | 136 service tests, desktop `cargo check --locked --tests`, and 12 cutover/library/MCP tests passed |

Native coverage exceeds 71% in every gated production module, including compiled
publication admission 87.68%, raster artifact admission 91.86%, behavior 93.39%,
source validation 93.23%, host 78.56%, library 99.43% and cutover 94.31%.
The clean snapshot excludes unrelated unfinished ESP32 changes present in the
development checkout; its two historical operation tests are absent. Exporting
and checking the selected index confirms the feature compiles without that work.

The full frontend run passed 1,131 of 1,132 tests while builds ran concurrently.
One existing dense-graph timing assertion took 2,068 ms against its 2,000 ms
limit. Its isolated file rerun passed all 35 tests, with that assertion at 988 ms;
the earlier complete run passed all 1,132 tests. No graph implementation changed.

The six live scenarios cover:

1. Native Toggle forms, history/cancellation, complete build identities, compiled
   target-size pixels and independent requested/confirmed state.
2. Normal panel forms, compile, Save and actual browser assignment. A real
   browser pointer reaches the exact native controller instance. Save waits for
   startup restoration to release the shared publication upload.
3. Four compiled browser fixtures, rapid reversal, pointer cancellation, real PCM
   audio, mute/no replay, unavailable audio APIs, cold/warm cache, and genuine
   visible → hidden → visible lifecycle with a fresh neutral baseline.
4. Native PNG/JPEG/WebP import, pinned WASM compilation and visible pixel checks
   under normal CSP.
5. A capsule built from blank through normal forms, exported through the UI and
   operated OFF/ON/OFF in the actual browser.
6. Published linear/radial gauges receiving synthetic session-bound wire stimuli:
   full sweeps, raw-value hysteresis, 500 ms expiry, quality recovery, live cue
   edges and silent snapshot restoration. Periodic samples keep the source fresh
   during pixel reads; deliberately stopping them proves expiry.

The four-fixture browser observation recorded 174 animation frames in 1.2 seconds,
p95 7.0 ms, maximum 7.1 ms, and no intervals above 33 ms. Authored 120 ms scalar
smoothing reached its final frame in 122.7–123.4 ms. Sampled raster decoding
accounted for 27.0 ms of sampled work across that measured interaction sequence.
Bidirectional WebSocket and HTTP observations contained zero graphics payloads
during steady interaction after activation. These are local observations, not
real simulator, network latency or release-wide performance guarantees.

Fault-injection tests also preserve the previous complete canvas while malformed
or unallocatable candidates fail. The old session closes and neither the old nor
candidate controls remain productive. Candidate surfaces are allocated and
painted before activation, then adopted without an extra pixel surface.

Detailed local evidence is in `generic-panel-builder-native/1792764090904`,
`generic-widget-browser-native/1792764104383`,
`generic-widget-blank-native/1792763647606`, and
`generic-widget-telemetry-native/1792763652455` beneath `.research/`.

## Development review data

At the user's request, the running development Cockpit contains four explicitly
imported user sources: `review-toggle` (Toggle with sound), `review-linear-rpm`
(Linear RPM), `review-radial-rpm` (Radial RPM), and `review-three-position`
(Three positions). Normal source import and panel forms created the saved
**Controls** and **Instruments** panels. Both panels and all four native library
records were verified after restarting Cockpit. Individual widget Try supports
independent feedback stimuli; gauges deliberately expire after 500 ms without
fresh samples. These user records do not change the sole shipped Toggle seed.
