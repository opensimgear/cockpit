# Assetto native page acquisition evidence

Prerequisite for #188/#192/#193 native per-port mapping evidence. This covers
Assetto Corsa, Competizione, EVO and Rally through their common native reader.
It does not resolve the upstream shared-memory notification boundary or qualify
real simulators and hardware; those requirements remain open.

## Behavior and bounds

The read owner records the original host `Instant` immediately before each
physics, graphics and static page copy. A checked, nonwrapping host sequence
identifies each page read; the owner UUID and checked mapping generation are
distinct from the selected process PID/creation identity and protocol counters.
Remapping advances the native generation while keeping the host sequence.

The existing double-copy stability check accepts only matching bounded byte
pages and carries the acquisition records of that exact second copy. Retries
never stamp earlier bytes with later times. This remains a best-effort stability
check, with no seqlock or upstream multi-page atomicity claim. Each page is at
most 4,096 bytes; two copies and at most three attempts are retained sequentially.
The session holds only bounded current/published capture sets and an observation
fence, with no historical registry.

Decoded values and their committed three-page capture set share a snapshot.
The original page times govern native freshness; consumer receipt time, rounded
age, synthetic fixture clocks and repeated reads cannot renew that proof.
An unchanged physics packet retains the earlier published capture set and still
expires. Equal times with distinct capture sequences are allowed; regressions,
future/expired evidence, reused baseline captures and same-owner generation
rollback fail closed. Reset withdraws values while preserving replay fences.
Session epoch exhaustion permanently withdraws authority.

Authored JSON/pure fixtures explicitly have no native capture set. They retain
their test clock and cannot masquerade as the native acquisition owner. A live
source cannot switch to fixture timing within the same source identity.

The unused BIOS `feed_captured` helper and its receipt-clock correlation tests
were removed: production BIOS readers already use the canonical marked decoder.
Native BIOS regression tests remain in the shared decoder and projection paths.

## Verification

The isolated full native suite passes: **1,876 passed, zero failed, 17 explicitly
ignored**, in 98.50 seconds with four workers. Production compilation and all
**108 native module coverage gates** pass. The new capture module has 91.95%
production line coverage and the extracted session 94.49%; the changed DCS
projection parent has 95.10%. Evidence hashes pin the isolated tested sources.
The new opt-in fixture
`OSG_ASSETTO_CAPTURE_NATIVE=1 node --test tests/ui/assetto-capture-native.mjs`
uses private authored publishers for all four dialects, input-only identity
processing and no physical outputs. It checks frozen expiry and recovery, removes
its temporary profiles, and restores original installations through supported
library APIs. A baseline run against the prior production executable passes all
four dialects in 72.69 seconds. The final run against the new executable also
passes all four dialects in **101.47 seconds**.

The tested production executable SHA-256 is
`d4210b6c4f072c6a31171f76ea505d3940a338039b1c9fe81f53df7b839d0a54`.
Local artifacts: `.research/reactive/assetto-capture-native-coverage/`,
`.research/reactive/assetto-capture-production.log` and
`.research/reactive/assetto-capture-final-fixture.log`. The fixture evidence is
`.research/assetto-capture-native/1793672492214/evidence.json`; all four original
installation lists were restored and cleanup errors are empty.

Page/field contributor selection, `ExportSnapshot` captured-value propagation,
canonical dependency unions, independent per-port recovery and reducer/ticket
integration remain implementation work. This milestone does not qualify physical
preview/raw-to-virtual latency or actual simulator effects; user acceptance stays
in #199.
