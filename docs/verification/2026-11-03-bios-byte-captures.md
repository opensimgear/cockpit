# BIOS byte acquisition evidence

This milestone retains physical UDP acquisition evidence through BIOS parsing and
sparse decoded memory. It is a prerequisite for native per-port mapping evidence;
catalogue projections and mapping contributor integration remain open.

## Behavior

- Each payload byte retains its original receive `Instant`, nonwrapping raw
  report sequence and byte offset. Fragmented blocks, headers and buffered
  `0x55` runs preserve the original positions across reports.
- A counter observation uses the latest physical position of its two payload
  bytes. A later report that disambiguates a sync run cannot renew that lease.
  Decoder qualification epochs and the separately processed raw-report cursor
  retain their distinct meanings.
- Sparse memory stores bytes, validity and acquisition marks together. Capture
  pages use copy on write; a new snapshot cannot change an earlier snapshot's
  evidence. An unrelated write preserves old latched captures. An actual equal
  write updates captures; a duplicate BIOS counter does not install the write.
- A changed aircraft can carry only its complete candidate header, including
  the header's original marks. Raw acquisition replacement cannot carry bytes
  across acquisition identities. Loss, expiry and malformed framing withdraw
  validity before a new baseline can qualify.

## Bounds

There are 256 optional capture pages of 256 cells, covering the entire 65,536-byte
address space without a capture-slot exhaustion policy. Retained-byte accounting
counts raw memory, all allocated pages and Arc overhead in full, even when shared.
The native journal allows a 4 MiB snapshot/event and six events, with a 24 MiB
per-reader retained-payload budget and 64 readers. Envelopes and baselines have
their separate existing bounds. A compile-time check reserves 128 KiB beyond
the maximum capture-memory representation for bounded metadata.

The parser retains at most seven pending sync candidate marks, bounds a frame
to 131,072 wire bytes and 8,192 blocks, and stops while parsing the seventeenth
decoded event in one feed. Overflow discards the complete result and pending
parser state; a valid-looking event prefix is never published.

## Verification

The final isolated native suite passes: **1,868 passed, zero failed, 17 explicitly
ignored**, in 95.33 seconds with four workers. This includes parser fragmentation,
pending sync bytes, event limits, sparse capture memory, immutable snapshots,
duplicate/frozen replay, replacement, expiry and command-authority regressions.
Production compilation passes. Parent decoder coverage is 669/699 production
lines (95.71%). All **107 native module coverage gates** pass. The new BIOS parser
has 100% production line coverage, captured memory 98.44%, decoded state 91.06%
and decoded source 98.28%. Evidence hashes pin the isolated candidate sources.

The actual dev Cockpit ran the authored DCS BIOS/ownship fixture in 53.02 seconds.
It checked independent native/canonical/unbound inspection, profile processing,
pause/denial/frozen/respawn handling, independent BIOS gauge continuity while
ownship expires, and process-exit withdrawal. Cleanup errors are empty and the
original installation was restored. This uses authored packets and a fixture
process, with no physical outputs; it does not qualify actual simulator effects.

Local artifacts: `.research/reactive/bios-native-coverage/`,
`.research/reactive/bios-production-build.log`,
`.research/dcs-physics-native/1793670646552/evidence.json`.
The tested production executable SHA-256 is
`857f26eb6cba44ad087713269e616a1fd423a027764dba8aade4c488a9599384`.

Physical controller latency, direct destination delivery and real-simulator
effects remain unqualified in #199. Catalogue projection evidence and mapping
contributors are still follow-up implementation work in #192/#193.
