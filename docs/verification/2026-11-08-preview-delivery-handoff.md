# Controller preview delivery handoff

The native and browser observation transports hand the shared reader their
validated, detached delivery. The shared reader still validates deliveries
from its ordinary transport callback and isolates mutable subscribers.
Native controller expansion checks the compact input and the restored frame
against their byte limits before acknowledging. An oversized expansion is
rejected and the input view loses its baseline.

## JavaScript delivery cost

Three alternating runs per version fed 137 controls through the native
transport and shared controller reducer. Two listeners received each result.
The benchmark excluded packet creation, IPC, DOM and display work. Each case
timed 4,000 deliveries after warmup and checked the exact axis sample and ACK
for every packet. The baseline used frontend commit `35328022`.

| Snapshot format | Baseline median of run p50 / p95 / p99 | Updated median of run p50 / p95 / p99 |
| --- | ---: | ---: |
| Compact steady reports | 567 / 687 / 868 µs | 359 / 443 / 622 µs |
| Full descriptor snapshots | 1,070 / 1,514 / 1,902 µs | 636 / 818 / 1,098 µs |

## Running Windows Cockpit

Two ten-second comparisons used the same native binary and the attached
stationary VKBsim Gladiator EVO R. The second comparison reversed the order.
The reports arrived about 80 ms apart. The diagnostic matched each observed
report to the Devices DOM counter. One sequence was absent from the second
updated trace.

| Run order | Frontend | Matched reports | Journal to DOM p50 / p95 / p99 | Clock bracket |
| --- | --- | ---: | ---: | ---: |
| 1 | Baseline | 125 | 8.24 / 14.46 / 16.30 ms | ±2.25 ms |
| 2 | Updated | 126 | 3.35 / 7.99 / 8.28 ms | ±2.35 ms |
| 3 | Updated | 125 | 3.13 / 7.57 / 9.78 ms | ±2.30 ms |
| 4 | Baseline | 125 | 3.10 / 8.83 / 9.22 ms | ±2.30 ms |

The first pair showed a difference that the reversed pair did not reproduce
outside clock uncertainty. These traces qualify the running preview path
and identify its stationary cadence. Physical motion, virtual-controller
receipt and display latency require the separate checks in #199.

Use `OSG_CONTROLLER_TRACE_DIRECTORY` with
`scripts/reactive/controller-preview-latency.mjs` to save each comparison
under a distinct directory. The local records for this run are in
`.research/reactive/preview-delivery-handoff/`.

The running checks use the native and browser gateway binaries identified
in the [idle trace matrix](2026-11-08-idle-transport-matrix.md).
Both clients passed the preview and visibility reader retirement checks.
The native output-free profile shared the controller capture, kept routing
when the preview closed and acquired a fresh baseline on profile replacement.

## Local checks

All 2,113 frontend and package tests, Svelte checking and the production
frontend build passed. The 52 focused stream and controller tests passed
with all configured stream coverage floors met. Across the four changed
modules, coverage was at least 97.89% lines, 87.83% branches and 86.36%
functions.

## Reuse frame byte counts on 10 November 2026

The observation transport now serializes each JSON frame once for
the wire budget check. Its UTF-8 byte length counts toward both the
frame and the complete envelope. The envelope accounting includes
extension fields, and the metadata limit remains independent. The
regression exercises exact acceptance and one-byte rejection for
both limits with multibyte characters and escaped JSON text.

Three alternating Node runs per version delivered 4,000 measured
reports per format through the native transport and controller reducer
with two listeners. Every report retained the expected value and ACK.

| Snapshot format | Baseline p50 / p95 / p99 | Updated p50 / p95 / p99 |
| --- | ---: | ---: |
| Compact reports | 359.5 / 451.0 / 625.8 µs | 294.2 / 411.6 / 496.6 µs |
| Full snapshots | 642.3 / 910.5 / 1,234.6 µs | 584.1 / 770.7 / 1,050.5 µs |

Each cell uses the median of the three run percentiles. The live
Windows comparisons used the same dev executable and the stationary
VKBsim Gladiator EVO R. The initial automatic selection picked the
VKB pedals for one run. The table uses the two comparable pairs,
including a repeat pinned to the Gladiator model.

| Run | Reports | Native read to DOM p50 / p95 / p99 | Clock bracket |
| --- | ---: | ---: | ---: |
| Updated 2 | 125 | 7.61 / 9.10 / 9.60 ms | ±2.55 ms |
| Baseline 2 | 125 | 7.66 / 9.47 / 9.74 ms | ±2.65 ms |
| Baseline 3 | 125 | 7.50 / 9.07 / 10.26 ms | ±2.65 ms |
| Updated 3 | 125 | 7.40 / 9.35 / 10.15 ms | ±2.60 ms |

The live differences fall inside clock uncertainty. Native WebView2
and browser-dev passed hidden preview retirement and reopened on
fresh baselines with zero `controller_inspect` queries. All 39
stream and 19 controller tests passed, along with the bridge tests,
Svelte checking and the production frontend build. The protocol
module measured 97.96% lines, 98.04% branches and 100% functions.

The raw traces are under
`.research/reactive/preview-delivery-handoff/validation-reuse/`.
The Node benchmark result `alternating-node.jsonl` has SHA-256
`6ec59d97c6af629e216749120a6efec496e7b0f7ac5785476f7254ef4429c16c`.
Physical sweep and display scanout remain in #199.
