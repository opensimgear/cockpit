# Controller latency investigation — 2026-11-02

The user reported noticeable controller-preview delay and said live input had not
yet been tested because profiles could not currently be edited. That exact
workflow is not qualified by these results. Low raw-input → destination and
preview latency remains an implementation requirement; the R0 budgets are ceilings,
not a reason to stop reducing avoidable work.

## Changes

Native controller journal commits now notify the profile worker's existing
epoch/unpark mechanism. The receiver no longer has to wait for its next five-ms
fallback to discover a report or loss. Speculative metadata clones drop the wake
callback. This is a routing wakeup, not an output-admission grant; the fallback
still exists for paths whose actual event/deadline migration is unfinished.

Wire publication previously encoded each payload three times and cloned it while
checking a worst-counter envelope. It now encodes once, checks exact envelope
overhead using a null payload and worst counters, and commits those validated
bytes through the same bounded journal path. Clock, generation, counters, frame,
batch, queue and subscriber limits remain enforced. New fixtures check escaped
metadata, exact byte boundaries, small custom budgets, rollback and overflow.

Controller preview states are frozen read-only views. Descriptors are copied and
validated at baseline boundaries; unchanged rows and arrays are shared rather than
deep-cloned for every report and listener. Changed values remain atomic and cannot
be mutated by synchronous or delayed consumers. The host and profile renderer props
accept read-only arrays. Physical values, normalization and declared ranges remain
unchanged.

When the last preview reader closes, its read model defers JSON encoding. Native
reports still enter the authoritative typed journal and wake routing. The next
subscription refreshes its snapshot under the same registry lock before opening
the reader; it cannot display an old cached value as a new baseline.

`controller_list.captureClock` exposes an explicitly labeled monotonic clock for
correlation. It does not renew sample evidence or advance the input cursor.

## Measurements and limits

| Measurement | Before | After |
| --- | ---: | ---: |
| Synthetic 137-control report admission, preview open, debug Rust p50 | 6.2–6.3 ms | 2.7–2.9 ms |
| Same admission, preview never opened, p50 | 0.25 ms | 0.25 ms |
| Same admission, preview opened then closed, p50 | Encoding previously continued | 0.26 ms |
| Reducer only, 137 unchanged values, two listeners, p50 | 0.773 ms | 0.015 ms |
| Reducer only, all values changing, two listeners, p50 | 0.885 ms | 0.042 ms |
| Real VKB Devices inspector, journal → DOM median | 15.96 ms | 8.65 ms |
| Real VKB Devices inspector, journal → DOM p95 | 24.96 ms | 17.55 ms |

The report-cost diagnostic uses 500 measured reports after warmup and includes
registry admission/publication CPU. It does not include OS/HID delivery, decoding,
virtual routing, IPC or painting. Run:

```text
cargo run --locked --manifest-path crates/controller-input/Cargo.toml --example report_cost
node scripts/reactive/controller-reducer-cost.mjs
```

The reducer diagnostic uses 4,000 warmed reports per scenario and bypasses generic
transport processing and DOM work. Its baseline is pinned to `a0bf5a3`.

The actual native Devices inspector observed VKBsim Gladiator EVO R with 137
controls. Two ten-second visible-window runs recorded 124 and 125 reports. The
clock bracket uncertainty was approximately ±4.7 ms before and ±4.5 ms after;
the estimated improvement should be read with that uncertainty. No continuity
errors were reported. The later frame-opportunity median was 14.35 ms after
versus 20.13 ms before; frame callbacks do not prove physical display latency.
Run `node scripts/reactive/controller-preview-latency.mjs` with the inspector open.
It rejects a run that changes source generation/clock or loses continuity.

These figures begin at journal commit, excluding earlier OS delivery, decoding
and registry-lock wait. They are neither physical-controller → photon latency nor
raw-input → virtual-device receipt. No deliberate physical sweep or active virtual
output was exercised. The reported controller-preview workflow, driver destination
receipt, production builds and high-rate end-to-end load still require qualification.
The Devices data must not substitute for those measurements.

Native observation JSON serialization still occurs under the capture registry lock
while a preview is active, though its cost is reduced. High-rate controller reports
also wake profile health for unassigned sources/inactive runs. Further producer,
read-model and routing scheduling work remains; this change does not finish R4/R7
or the full #188 acceptance.

## Verification

- Streams: 28 tests pass, with all production coverage floors above 71%.
- Controller crate: 125 tests pass, including closed-preview snapshot recovery,
  committed report/loss notifications, clone isolation and clock non-renewal.
- Focused preview/view tests: 25 pass; reducer coverage 99.59% lines, 96.43%
  branches and 100% functions.
- Isolated frontend suite: 1,666 pass. Isolated native library suite: 1,658 pass,
  zero fail, 12 existing opt-in checks ignored. The final idle-encoding change is
  additionally covered by the focused controller suite.
- Svelte check has zero errors/warnings, Nx plugin checks pass and both native
  executable and Vite production builds pass. Existing build advisories remain.
- Final running Windows cockpit checks pass for two shared physical-controller
  consumers, unsubscribe/reopen, preference changes and three document reloads.
  The tested executable SHA-256 is
  `baf9f4c12ac49c6dcdf77eaa8b0d16edfff70284a4b8eac8d9b1844d72300a29`;
  its isolated source tree is `1263ebbc9f6f4d59ea0415bffc474cb45459ade0`.

Local raw measurement artifacts are retained in ignored `.research/reactive/`.
The parent feature and hardware/simulator acceptance follow-up #199 remain open.
