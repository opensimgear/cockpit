# Desktop recovery record update timing

An existing live recovery record is updated with one prepared statement that
compares its exact revision and payload inside the writer transaction. The
initial insert uses the shared persistence transaction. The short read
preflight and the storage, layout and database epoch checks still run. The
transaction commits with SQLite FULL synchronization before dispatch. A test
commits external edits after the read preflight and confirms the writer rejects
both changed bytes and a changed revision while preserving the external record.
A retained tombstone can be written again at its current revision.

Two Windows dev runs per build used the same two authenticated WSS peers,
neutral preparation, idle view and open Profiles view. Baseline and candidate
executables included identical timing probes. The two baseline runs preceded
the candidate runs. All 808 baseline and 804 candidate keyboard edges reached
the private native receiver in order. Both candidate checkpoints were valid and
Stop left no owned keys or cleanup errors.

| Run | First idle / Profiles p95 | Sustained idle / Profiles p95 | Sustained record update p95, idle / Profiles |
| --- | ---: | ---: | ---: |
| Baseline 1 | 4.729 / 4.336 ms | 4.205 / 3.017 ms | 0.488 / 0.429 ms |
| Baseline 2 | 3.993 / 4.064 ms | 4.346 / 4.077 ms | 0.512 / 0.472 ms |
| Candidate 1 | 4.377 / 3.512 ms | 2.984 / 4.341 ms | 0.049 / 0.068 ms |
| Candidate 2 | 4.338 / 4.074 ms | 3.866 / 2.897 ms | 0.057 / 0.050 ms |
| Candidate without added probes | 4.034 / 3.736 ms | 3.853 / 3.048 ms | — |

End-to-end times run from WSS admission to `SendInput` return. Each first phase
had four edges. The record stage includes the epoch check and counts each paired
request once. Across the four sustained phases per build, p95 for the full intent
write was 1.633–1.841 ms on baseline and 1.189–1.376 ms on the candidate. Baseline
helper preparation stayed below 0.20 ms at p95. The largest first-phase candidate
sample spent 1.307 ms inside the Windows call itself. These short keyboard runs
met the strict 5 ms p95 and 10 ms p99 targets. Burst and mixed pointer timing
still require separate qualification.

The candidate without the added probes delivered all 403 expected keyboard
edges and proved clean Stop. It also passed the paired-key crash fixture. The
two keys shared one native request with distinct logical operation IDs. Their
durable held record survived interruption, ordinary restart required recovery,
and explicit recovery released both keys exactly once. Single-key interruption
also passed on the diagnostic candidate. Three paired-key diagnostic attempts
stopped before interruption because the process snapshot could not identify the
helper. This happened on both diagnostic builds and each attempt proved clean
Stop. The final verification used the candidate without the probes.

All 74 persistence and 205 desktop-output tests passed with one existing
persistence ignore. All 14 repository persistence coverage gates passed,
including 99.07% production lines for live evidence and 98.13% for the
persistence library module.

Local comparison evidence is under `.research/c193-reconnect/recovery-helper-trace/`
and `.research/c193-reconnect/guarded-recovery-update/`. Candidate SHA-256 is
`d835d211900b363b110a81591b5584acf2936c208769e0920f9c13253d37fe89`.
Evidence without the added probes is under
`.research/c193-reconnect/guarded-recovery-plain/`, with executable SHA-256
`0cdb1a51132dd7180237ba9a91cecf1fb658a12cca9e887935a0ccdcf24d1878`.
