# Windows companion reconnect and desktop pointer recheck

## Companion reconnect

The Windows offline-companion fixture now requires the observed online
source to match a current Hub assignment and publication before it
publishes a writable route. During concurrent unassignment, the
collector can still see the previous provider descriptor. In that case
it falls back to the paired metadata path with no writable authority.
This matches the production inventory's fail-closed behavior. The
change is confined to the native test fixture.

The isolated candidate at `87e25238` with the test patch applied passed
2,486 Windows native tests with zero failures and 18 existing ignores.
The formerly intermittent test passed in the full suite and in 20
consecutive isolated runs. The native test executable SHA-256 is
`2c4e44670542e3804024d8d73f09509470c377501c2c08b786a07b6b883bf67f`.
The suite log is at `.research/c193-reconnect/native-suite.log` and
has SHA-256 `90bd6e07649b268e4a6c52a46ce80fef416a3b946209d572e39e7548fcae87f7`.

The existing compiled-browser test also passed against the running
Windows dev Cockpit, executable SHA-256
`b1eb29dd1c65e128d4084d9bb9584e74b042c33b50c7727dea99f93ba4635966`.
The browser reloaded on the same applied owner, started from neutral,
and routed a new press to live feedback. The log is at
`.research/c193-reconnect/live.log`.

## Mixed keyboard and pointer

On the same Cockpit executable, the guarded fixture again found window
2755484 from PID 17540 under the pointer while the private receiver had
keyboard foreground. This is the [previously identified PickerHost
window](2026-11-09-desktop-pointer-target.md). The test stopped before any
native input submission. The wrapper verified clean ownership and
reported zero cleanup errors. The preflight evidence is in
`.research/reactive/r4-desktop-timing/mixed-recheck-3e7a338f7c/evidence.json`
with SHA-256 `6eb2b4192e84fa6fd1df7411752b2d1a3679d76c9cdd09d9c2a7d7bca3dae84b`.
Mixed pointer receipt needs an unobstructed desktop. The strict
dispatch targets remain open.

### Current audit build and early pointer check

The Windows dev build at `2f748f9d` opened its audit collector and ran
the same mixed fixture. The exact receiver again had keyboard foreground
while window 2755484 from PID 17540 covered the pointer target. A process
name check before launch had returned no PickerHost result. The native
hit test remained authoritative and the fixture stopped before submitting
input.

The fixture now checks the pointer target before capturing the application
scope, enabling desktop output or applying the profile. It still checks
again after Apply and after warmup. The running dev rerun reproduced the
obstruction at this earlier check. Its bridge and audit recorded zero
workload configure commands, Apply commands and native submissions. The
receiver exited normally and ownership cleanup was confirmed. All 25
portable latency tests passed.

The keyboard-only run on the same executable matched 404 marked
transitions across the idle and Svelte UI-load phases. Both phases
stopped cleanly.

| Phase | Matched samples | Capture-to-native p50 / p95 / p99 |
| --- | ---: | ---: |
| Idle first edges | 4 | 3.60 / 10.59 / 10.59 ms |
| Idle steady | 198 | 4.81 / 10.58 / 14.57 ms |
| UI-load first edges | 4 | 3.15 / 23.81 / 23.81 ms |
| UI-load steady | 198 | 4.76 / 10.27 / 14.12 ms |

The slowest UI-load first edge included 18.22 ms in the durable intent
operation. The slowest idle steady edge waited 16.03 ms between delivery
binding and request issue. The strict 5 ms p95 and 10 ms p99 targets
remain open, as does mixed pointer receipt.

The dev executable SHA-256 is
`e67217d25ed30ac1d3286c5d07595665a5e433976d8d4657bfdea5b4e19ab94c`.
The receiver SHA-256 is
`8178c417374c57c88a9c3362dba29969c739e6a5ce1cdc5c356ba12532154b68`.
Local evidence is under `.research/c193-reconnect/mixed-qualification-56876/`.

| Artifact | SHA-256 |
| --- | --- |
| `preflight-40612/evidence.json` | `60ac67f1cb205522b45aefad8d0ba0d654b11e05db8019de45f967450b1a4795` |
| `keys-37700/desktop-stage-analysis.json` | `d053590e1a8361ecc0ee40ace33841ecace6fcf6140884b5c046cb65a753d379` |

### Journal wait attribution

The [desktop latency analyzer](../../scripts/desktop-latency.py) now reports
capture to delivery binding, binding to request issue, and issue to native
submission return separately. It joins durable writes by exact channel and
request sequence. For a delivery that binds after the preceding request
issued, it intersects that delivery's wait with the preceding writes. It
reports the subset that starts after the previous worker acknowledgement
as receipt overlap. It rejects malformed journal timing intervals.

In the previous keyboard trace, 13 of 25 steady edges over 10 ms bound
after the preceding press issued and overlapped its final receipt write.
The other 12 already shared a journal write. This separates the potential
benefit of allowing a late queued edge to share a receipt from the cost
of writing the next acquisition intent.

The fresh running-Cockpit check used the same executable and receiver
and matched all 402 marked transitions. Both phases stopped cleanly.

| Steady phase | Capture-to-native p95 / p99 | Delivery binding to request p95 | Current intent write p95 | Edges overlapping the prior receipt / maximum overlap |
| --- | ---: | ---: | ---: | ---: |
| Idle | 10.40 / 13.65 ms | 5.17 ms | 4.24 ms | 11 / 3.61 ms |
| UI load | 10.79 / 13.93 ms | 5.38 ms | 4.14 ms | 14 / 3.90 ms |

All 36 portable desktop tests passed. The analyzer had 91% executable
line coverage. The refreshed audit report confirmed exact counts and
retained the strict 5 ms p95 and 10 ms p99 failure. The local report is
`.research/c193-reconnect/mixed-qualification-56876/attribution-32104/maintained-analysis.json`,
SHA-256 `7acff2f745f848dfd350551f64bd60d737c9f0db812acb47f25814b09a4148ea`.

### A press that arrives during the prior dispatch

The worker can take one additional FIFO head while the current job's last
single-key hold acquisition is in flight. It closes admission at the first
validated ACK and never waits for the next press. A current, already armed
press can share the prior receipt write with its own durable intent. Each
press still has its own source clock, dispatch check and native ACK.

Scheduler cancellation revokes the transferred press even when the producer
token stays the same. The coordinator checks the permit again after the
durable intent write. A skipped ACK for a revoked press cannot cancel a new
press queued after a fresh baseline. The scheduler's replay records the
single-head transfer in FIFO order.

Both Windows runs used two authenticated WSS sources, the same receiver and
the same Svelte navigation fixture. The baseline matched 394 marked edges
and the candidate matched 392, including the first edges. Both runs proved
clean Stop for both phases.

| Build | Measured phase | Matched | Admission to native p50 / p95 / p99 | Edges overlapping the preceding receipt | Merged acquisitions |
| --- | --- | ---: | ---: | ---: | ---: |
| Baseline | Idle | 198 | 4.74 / 11.34 / 14.21 ms | 10 | 44 |
| Candidate | Idle | 196 | 4.74 / 10.19 / 13.75 ms | 3 | 49 |
| Baseline | UI load | 196 | 4.82 / 10.59 / 14.73 ms | 13 | 43 |
| Candidate | UI load | 196 | 4.91 / 10.49 / 11.71 ms | 6 | 49 |

Six candidate merges in the loaded phase joined a press that bound after
the preceding request issued. The baseline had none. Current intent writes
still took 4.34 ms p95 at idle and 4.38 ms under load on the candidate.
The strict timing gate and mixed pointer qualification remain open.
Physical controller preview and virtual receipt require separate measurements.

The candidate executable SHA-256 is
`4c8c8944ad450b7e5c0a3372988e68f5413547aef624129fc2880d9b26534e2c`.
The baseline and receiver hashes are listed above. Local reports and
externally acknowledged checkpoints are under
`.research/c193-reconnect/late-press/{baseline-1,candidate-1}/`.

All 209 mapping, 183 mapping-host and 202 desktop-output tests passed
locally. The desktop-output suite used the bundled Python for its Windows
child-process check. Changed production modules had at least 87.69% line
coverage and 77.78% function coverage. The candidate also passed the running
helper-loss fixture, which recovered the exact held key and verified cleanup.
