# Ordered KeyHold journal writes

Cockpit can combine the validated receipt of one KeyHold press and the
recovery intent of the next press already queued in the same output job.
Both actions must be compiled KeyHolds. The second must have a current
armed producer and its own admissible native request. The coordinator
commits the first press's confirmed ownership and the second request's
possible holds together before sending the second request to the helper.
The helper still validates each request, deadline and scope independently.

Two successful acquisitions use three durable writes. A withdrawn
second edge or a projection that adds no new hold commits the
first receipt separately. A failed combined write sends no second
request and leaves the worker sequence usable for terminal Stop. A
deterministic SQLite fixture checks the journal from inside each native
request, including the first receipt and exact second intent.

## Running Windows Cockpit

Four alternating keyboard-only runs delivered all 1,616 marked edges
in order and proved clean Stop. Each phase used two authenticated WSS
sources and an exact foreground receiver. Profiles phases navigated
the actual Svelte editor. Timings run from WSS admission to successful
`SendInput` return.

| Run | Build | Steady idle p95 / p99 | Steady Profiles p95 / p99 | Journal writes / combined writes |
| --- | --- | ---: | ---: | ---: |
| A1 | Baseline | 14.12 / 15.45 ms | 13.46 / 15.20 ms | 632 / 0 |
| B1 | Ordered KeyHold | 12.33 / 17.53 ms | 9.97 / 14.89 ms | 543 / 89 |
| B2 | Ordered KeyHold | 10.46 / 14.70 ms | 5.64 / 8.92 ms | 585 / 47 |
| A2 | Baseline | 14.14 / 15.10 ms | 13.55 / 14.88 ms | 632 / 0 |

Steady p95 improved in both paired comparisons. The idle p99 rose in
the first comparison and fell in the second. The slower first dispatch
of the two sources was 9.05–10.47 ms across candidate phases, compared
with 12.28–14.49 ms across baseline phases. The strict desktop timing
and mixed pointer acceptance remain open. Physical preview and
raw-input-to-virtual-controller timing remain in #199.

The audit joined all 136 `JournalMerged` events to one physical write
on the exact helper channel, between the first helper ACK and the
second parent queue event. The first request had only its own intent
write. The merged write used the second request's sequence and was
followed by the first validated parent outcome before second dispatch.

The desktop-output suite passed 199 tests, the mapping-host suite passed
181, and the portable audit and recovery suite passed 34. The changed
Rust modules with executable changes measured 85.86–94.96% LLVM lines
and 85.00–100% functions. The audit enum addition was exercised in all
136 native merged writes. Branch instrumentation was unavailable.

Local traces, manifests, logs and coverage reports are under
`.research/c193-key-pair/`. The baseline executable SHA-256 is
`b1eb29dd1c65e128d4084d9bb9584e74b042c33b50c7727dea99f93ba4635966`.
The candidate SHA-256 is
`6c08a829e209a33ccba7faf975d3b41d116d889b53f97e0f3e297156a47fe387`.


The live worker-loss fixture passed on the candidate. Recover released the
held KeyA once, and re-Apply required neutral before the next press. The
exact receiver recorded down, up, down, up. The evidence SHA-256 is
`679e06e7eb6e7dad324dc852d6d95374a676bf5c26a84ef64ff5e7193e3a96c4`.

The default-concurrency Windows native run passed 2,485 tests and failed
one DCS capture-isolation test. The same case passed in isolation. A full
rerun with eight test threads passed 2,486 tests with 18 existing ignores.
