# Shared DCS-BIOS decoding — 2026-11-02

This implements the shared decoded-source portion of #192/R3 and extends the R1
native channel foundation. It does not complete #188 or its integration milestone.

## Producer and projection boundary

Profiles and passive inspection now share one canonical decoder for matching
acquisition/header/freshness semantics. Selected output rows, catalogue aliases,
command ports and UI consumers do not duplicate that decoder. Different explicit
freshness/header contracts retain separate semantics while sharing native UDP
acquisition. Ephemeral test ports remain independent.

The source owns parsing, the memory validity bitmap, native timestamps, BIOS
counter/aircraft transitions and actual freshness deadlines. Consumers only read
committed snapshots and project them through their captured catalogue contracts.
Reads do not feed a parser, tick expiry or renew evidence. Immutable memory is
shared with `Arc`; source-native bytes, zero/false and output units stay unchanged.

The new native typed journal preserves those immutable allocations without JSON
encoding/decoding. It has atomic snapshot registration, ordered events, checked
counters, explicit generation replacement, cancellation and permanent reader loss
on overflow/close. Its retained-byte budgets are distinct from existing encoded
wire budgets; the JSON transport remains unchanged. A decoded reader has a 160 KiB
baseline cap, six pending-event cap and 864 KiB pending-payload cap, with at most
64 readers. Counts conservatively include shared memory allocations in full.
Each datagram is limited to 16 complete frame/reset events before immutable
memory snapshots are built. Excess fails the source explicitly instead of
allocating an unbounded decoded batch.

Native packet/BIOS gaps, malformed/reset data and changed aircraft invalidate
retained sparse memory. Identity plus consecutive counter progress establish a
baseline; frozen/duplicate reports cannot renew it or cause unchanged decoded
publications. Expiry retains original capture age, withdraws valid memory once,
and requires a new advancing baseline. Explicit blank/NONE identity publishes
stopped quality. Terminal failure closes subscribers; a fresh owner has a new
identity. Passive journal handles cannot retain the UDP socket after its last
lifecycle owner closes.

## Command admission

Output still requires captured catalogue, route, process/context and profile owner
authority. Source dispatch additionally requires a fresh observed baseline, intact
reader continuity and the current source epoch. It checks the expected aircraft
under the committed-state lock, so a same-catalogue variant cannot rearm through
an old tracker/context proof. Profile reads reject that mismatch immediately.

The processed raw cursor is separate from accepted evidence: duplicate packets
advance processing without renewing capture/freshness. Raw acquisition and decoded
commit are fenced through nonblocking command submission. Newly acquired but
uncommitted raw data rejects a command before submission; there is no hidden
receive pump, wait or ambiguous replay. R4 still needs to schedule such admission
from source events instead of the existing routing fallback.

## Remaining work and evidence limits

Profile scheduling, tracker/catalogue/process notifications, external read models,
other integrations, UI/MCP observation and complete-system qualification remain
open. Mapping frames retain their existing ordered admission clock; per-value
native provenance and independent engine freshness remain R4 work. The legacy
pure decoder helpers remain for authored unit injection, not production acquisition.
No compatibility/schema-version layer was introduced. User-operated simulator and
hardware acceptance stays in #199, first in progress.

## Verification

- Full isolated native suite: 1,712 pass, zero fail, 12 existing opt-in checks
  ignored. Focused instrumented integration runs: 166 pass, one existing opt-in
  decoder check ignored. Coverage-harness tests: 20 pass.
- Stream conformance: all 37 tests pass. New native journal production coverage
  is 99.57% and its receive module 96.59%. Decoded metadata/specification is
  80.56%, pure state 90.37%, lifecycle/dispatch owner 98.21%, catalogue projection
  100%, and raw transport client 99.21%. Other changed native consumers range
  from 84.01% to 97.78%; all coverage gates pass, including profile read/delivery
  fragments. Tests include exact-variant authority with an unchanged tracker
  proof, acquired-but-uncommitted command fencing, packet limits with no partial
  publication, shared memory identity, counter/epoch exhaustion and prompt close.
- Native executable builds. The running Windows cockpit fixture passes managed
  setup/repair, simultaneous passive inspection and profile processing, native
  zero/false values, pause/permission/frozen/stale withdrawal and recovery,
  respawn, mission stop and process exit in 53.81 seconds. Cleanup reports zero
  errors, restores original installations/settings and adds no scan root.
- Final executable SHA-256:
  `273b74b3c53b9a57cca69c14d1c35a33520dbcad5bd0cb1bade6f4040e577dd9`.
  Isolated build tree: `95ddd99be1da19ae3f978144f7ffc9c9d14ebd98`.
  Fixture completed `2026-11-02T18:10:52.668Z`; local evidence:
  `.research/dcs-physics-native/1793642998860/evidence.json`.

The fixture authors its process and UDP telemetry in the actual native cockpit,
with no physical outputs. It does not qualify actual DCS, physical capture-to-
destination/display latency, production load, full-app idle or the combined soak.
Raw artifacts remain in ignored `.research/reactive/r3-shared-*` directories.
The preceding receive-owner commit `c1dec15` passed GitHub CI.
