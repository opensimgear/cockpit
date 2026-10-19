# Native desktop admission latency — 19 October 2026

Production qualification remains disabled. This measures authenticated WSS
`SourceSink::changes` entry to the return of the actual successful Windows
`SendInput` call. It excludes physical capture, earlier WSS/network/decoding
work, and application consumption. It does not satisfy physical capture latency
acceptance for [#104](https://github.com/opensimgear/cockpit/issues/104).

The opt-in debug collector uses one host's QPC frequency in the parent and its
exact spawned workers. It joins the original admitted change, subscription and
queue domain to the converted typed frame, immutable effect observation,
immediate KeyHold delivery, private request, native return and both worker and
validated parent acknowledgements. Logical mapping timestamps are identity
fields only. The analyzer never subtracts them from QPC or Python clocks.
See Microsoft's [QPC guidance](https://learn.microsoft.com/en-us/windows/win32/sysinfo/acquiring-high-resolution-time-stamps).

The bounded writer reports queue/cap/clock/alias/I/O losses. Any missing or
ambiguous join, failed counter, incomplete checkpoint, missing worker footer or
unsuccessful exact worker exit invalidates the trace. A separate returned parent
checkpoint and successful worker exit are required because a final write timeout
can leave a stale optimistic control record on disk. The worker's diagnostic
exit follows ownership cleanup and ACK completion; input dispatch is unchanged.
Percentiles use integer nearest ranks and strict p95 < 5 ms / p99 < 10 ms gates.
Endpoint intervals of one QPC tick or less are rejected as ambiguous.

The first valid native run used development executable SHA-256
`4835eec35a3d17ebf6ea1d59c66e9b062280b503b86fec1117d2b74f38d9d3bf`.
Two real authenticated WSS peers drove separate KeyA/KeyB holds and explicit
raw-axis normalization into pointer X/Y. Each phase used a fresh native receiver
and explicit Apply, warmup, native begin/end markers, neutral cleanup and Stop.
The setup failure caused by the harness's incorrect axis-schema assertion is
retained separately; the actual descriptor is an integer range of -32768..32767.
The WSS normalized values are quantized by the existing native provider.

| Phase | Successful / expected key edges | p95 | p99 | Target |
| --- | ---: | ---: | ---: | --- |
| Idle baseline | 194 / 194 | 25.6031 ms | 32.5284 ms | Fail |
| Profiles UI baseline | 194 / 194 | 24.4899 ms | 27.4190 ms | Fail |
| Wake, idle | 194 / 194 | 24.7263 ms | 29.6341 ms | Fail |
| Wake, Profiles UI | 194 / 194 | 15.8776 ms | 20.4242 ms | Fail |
| Wake + redundant-write reduction, idle | 194 / 194 | 20.9529 ms | 28.7808 ms | Fail |
| Wake + redundant-write reduction, Profiles UI | 194 / 194 | 21.4010 ms | 25.1629 ms | Fail |

Both baseline traces are valid. All 1,540 workload batches were accepted, every
measured key edge reached the receiver in order, and all native gap, queue fault,
drop, collision, skip, no-op and unresolved counts were zero. Eight warmup key
edges and four later neutral cleanup releases are outside the measured interval.
There were 359 marked pointer movements. Requested input rate was 80 Hz per
peer; observed rate was 64.10–64.17 Hz. The actual Svelte Profiles refresh handler
performed 127 refreshes, 254 busy transitions and 23,440 DOM mutations over
6.3912 seconds (19.87 refreshes/s; configured 25 Hz).

The baseline's final returned and raw stopped checkpoints match at sequence
26,022 with zero collector faults or drops. Both receivers exited normally;
the native ledger is empty, no worker remains, and preferences were restored.
Private raw records and the failed setup are retained under
`.research/desktop-latency-native/`; `before-source-wake/` preserves the first
valid performance failure. No timeout or acceptance target was relaxed.

The final current build is SHA-256
`59426782dd543be864d1c5a3df8d0bfd29d6d37cb2e49c31449074872b63725c`.
Its [curated evidence](2026-10-19-desktop-latency-native.json) records another
388/388 successful key edges with zero loss, fault or unresolved outcomes. The
receiver observed 395 marked pointer movements. The actual UI handler performed
114 refreshes, 228 busy transitions and 21,079 DOM mutations. Both workers have
valid normal footers, exact successful exit proofs and clean Stop ACKs; the raw
and returned parent checkpoints agree at sequence 28,635 with no ignored tail.
Final ownership is empty, no worker remains, and preferences are restored.
Native sources and the harness match
[`4c2bf48`](https://github.com/opensimgear/cockpit/commit/4c2bf483de82c611c1fcf0b65072187aeda69d3e):
all 413 recorded native source hashes and the executable hash matched at commit.

Admitted companion changes now wake the native profile thread after the ordered
queue commits and its mutex is released. A notification epoch preserves work if
an internal IPC wait consumes the thread's unpark token. The existing 5/100 ms
fallback still checks timers, other devices and focus; shutdown also wakes the
thread. Median queue residence fell below one millisecond in the wake run, but
tail delays overlap earlier synchronous journal work. These are individual
controlled runs, not a statistical claim that every percentile improves.

Submission omits only the third journal write when its complete owner identity
and shared-control projection is byte-equivalent to the validated ACK snapshot.
Predispatch and post-ACK writes remain mandatory. Actual final KeyHold deliveries
retain three writes; the benchmark's pointer deliveries use two. Wheel/repeat
write elision has unit coverage only. Lifecycle operations retain their final
writes. Journal timestamps identify replacement as the largest ordinary stage
(approximately 1.16 ms p95 in the wake run); sync contributes tail latency.
Remaining latency acceptance is open and production stays gated.

Validation includes 43 desktop-output debug tests, 44 release tests run serially,
107 mapping-host tests, 14 native companion tests, four native wake tests, two
audit gateway tests, 16 analyzer fixtures and ten workload fixtures. All three
completed native workloads passed delivery and cleanup checks; each failed the
strict latency target. One earlier parallel release test encountered an
unexplained filesystem `AccessDenied` during temporary-file persistence. Its log
is retained; a focused invocation and the serial full suite passed without code
changes or production retries. The cause is not established. A separate host
fixture was corrected to await a readable inspector instead of interpreting its
explicit `busy` placeholder as a stopped route; a real stopped route still fails
immediately and a deterministic lock-contention regression covers the case.

The unchanged mapping-host coverage gate passes all 11 modules (minimum desktop
72.95%). The desktop coverage reader was corrected to exclude complete test-only
items rather than truncate at the first `cfg(test)` field. Actual coverage with
the repaired reader is coordinator 87.53%, worker 86.40%, reducer 92.34% and
keys/protocol 100%, above the unchanged 71% gate. All six parser regressions pass
and run with the 16 analyzer regressions in CI.

The preceding CI snapshot had 13/14 checks green and a timing-sensitive virtual
inspection fixture failure. The separate
[`b09601a` repair](https://github.com/opensimgear/cockpit/commit/b09601a)
uses a deterministic monotonic clock to prove that readiness is excluded while
query, parsing and teardown remain inside the unchanged 250 ms freshness bound.
A separate actual Python wire check remains. All 99 virtual-controller tests and
their unchanged coverage gates pass locally (inspection 83.33%). This repair does
not qualify virtual output or change the measured native-source snapshot above.
Full CI for the newly pushed commits was not yet established when this record
was prepared.

Reproduction requires the actual running native development Cockpit built with
`OSG_DEV_DESKTOP_INPUT=1`, an existing absolute `OSG_DESKTOP_AUDIT_DIR`, and a fresh
8–64 character alphanumeric/hyphen `OSG_DESKTOP_AUDIT_RUN` at process startup.
These diagnostic settings cannot be enabled by an IPC request. Use the same
directory/run for the opt-in workload with both `OSG_DESKTOP_LATENCY_NATIVE=1`
and `OSG_DESKTOP_NATIVE_INJECT=1`, then run:

```text
node --test tests/ui/desktop-latency-native.mjs
python scripts/desktop-latency.py --audit-dir <directory> --parent-report .research/desktop-latency-native/parent-checkpoint.json --phase idle:1:2:<actual-count> --phase ui-load:3:4:<actual-count> --output <report.json>
```

Use each phase's recorded sum of `expectedImmediateKeyChanges`. A fresh run
nonce/process is required for a repeated measured run. Keep other native input
tests and foreground-stealing tools paused during the workload. Scheduled
pointer/tap/repeat/cleanup work is excluded from immediate KeyHold percentiles;
this does not qualify those other paths or the installed/game/hardware matrix.
