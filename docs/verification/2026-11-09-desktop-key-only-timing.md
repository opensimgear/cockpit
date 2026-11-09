# Desktop keyboard timing with exact foreground proof

The mixed pointer run stopped at preflight again. PickerHost PID 17540
owned window 2755484 under the receiver's client area. Windows submitted
zero authored inputs, the receiver exited normally, and both cleanup
checks passed.

The `OSG_DESKTOP_LATENCY_KEYS_ONLY=1` diagnostic uses the same two native
WSS peers and the actual Svelte Profiles navigation. It builds a profile
with two KeyHold routes and no pointer bindings. The receive-only window
acknowledges a focus request without moving the cursor. The fixture checks
the exact foreground HWND, process identity and native application scope
before sending keys. It then requires exact ordered key receipt and clean
Stop. The mixed mode retains the separate pointer hit-test and marked
mouse-receipt checks.

The running dev Cockpit received all 198 steady idle key transitions and
196 steady UI-load transitions, plus four warmup transitions in each
phase. The UI-load phase recorded 49 native navigation changes. No
authored mouse movement was recorded. Both worker exits and the final
ownership check were clean. The offline native QPC analyzer validated
the trace and reported these timing misses.

| WSS admission to successful SendInput return | Samples | p50 / p95 / p99 |
| --- | ---: | ---: |
| Steady idle | 198 | 7.72 / 18.63 / 20.38 ms |
| Steady Profiles navigation | 196 | 7.57 / 17.71 / 19.39 ms |

The two first down edges were 15.46 and 24.88 ms in idle, and 7.81 and
16.85 ms during Profiles navigation. Those are two observed presses per
phase. The first-press and steady-state targets remain below 5 ms p95
and below 10 ms p99.

In steady idle, acquiring a key spent 5.91 ms p95 persisting the recovery
intent, with 4.82 ms p95 inside the evidence write. During Profiles
navigation those figures were 5.69 and 4.70 ms. From binding a key
delivery to issuing its request, p95 was 11.27 ms idle and 10.49 ms under
UI load. The second first press in idle waited 19.07 ms in that segment.
Worker dequeue to successful `SendInput` return stayed below 0.65 ms p99
in both steady phases. The parent serializes output and preserves exact
durable recovery evidence before key acquisition. That work and the wait
behind an earlier command account for the largest measured delays.

Run `r4-keys-71fdfa942b` used dev Cockpit source `42c71adb`, executable
SHA-256 `0f3a320ccb98dddc80fb995fdc2eef94e5245b3363ba79281a8e2273c701f58e`
and receiver SHA-256
`8178c417374c57c88a9c3362dba29969c739e6a5ce1cdc5c356ba12532154b68`.
Local evidence and the per-request stage breakdown are in
`.research/reactive/r4-desktop-timing/desktop-keys-71fdfa942b/`.
The evidence SHA-256 is
`8025f76100e519b2018372b48fd7ee129a29720f6fc225c72d5e7c405405b087`.
All 31 portable desktop tests passed locally. A separate instrumented
keyboard-only run also passed receipt and cleanup. Combined with the
portable tests, it covered 43 of 51 changed workload lines and all eight
changed foreground helper lines. Timing above comes from the uninstrumented
run.

Mixed pointer delivery, the desktop timing targets and abrupt worker
loss remain open in #193 and #198. Controller capture and preview timing
remain in #199.

## One live verification per journal commit

The desktop coordinator now calls `Evidence::write` directly after it
checks the journal's storage identity. The write verifies the live prior
record, then compares its exact revision and payload again inside the
durable transaction. The write still completes before the coordinator
issues a key acquisition to the native worker. Commands that do not
write a new intent retain their separate pre-dispatch verification.

The stale-evidence regression covers acquisition as well as release.
Each case rejects dispatch and preserves the external bytes exactly.
All 195 desktop-output tests and 29 module line-coverage gates passed.
The journal module measured 78.99% production lines and 77.27% LLVM
functions. Branch instrumentation was unavailable.

The uninstrumented comparison used the same two WSS peers and six-second
steady phases in four disposable Windows Cockpit runs. All 1,616 marked
key transitions arrived in order. Both phase workers stopped cleanly in
each run. The offline QPC analyzer validated the trace in every case.

| Run order | Build | Steady idle p95 / p99 | Steady Profiles p95 / p99 | Recovery intent p95 during Profiles |
| --- | --- | ---: | ---: | ---: |
| A1 | Baseline | 17.76 / 19.08 ms | 16.09 / 17.58 ms | 5.00 ms |
| B1 | Single verification | 15.91 / 18.47 ms | 14.52 / 15.40 ms | 4.51 ms |
| B2 | Single verification | 14.81 / 15.76 ms | 15.03 / 33.14 ms | 4.55 ms |
| A2 | Baseline | 16.88 / 20.55 ms | 16.84 / 20.44 ms | 5.29 ms |

The removed verification took 0.94–0.99 ms p95 in the baseline steady
phases. The candidate had lower steady p95 in both comparisons. First
presses and p99 varied. The strict desktop timing targets remain open.

The baseline executable SHA-256 is the value recorded above. The
candidate SHA-256 is
`127e911aef3329eb842336146071f633ca4f764a0cec9844d63a5bc643750cf9`.
The native manifest, audit files and stage summaries for all four runs
are in `.research/reactive/r4-desktop-timing/journal-ab-57742634/`.
