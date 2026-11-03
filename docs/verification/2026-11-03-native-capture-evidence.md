# Native simulator capture evidence — 2026-11-03

Prerequisite delivery for R3 #192 / R4 #193 / parent #188. Per-port mapping
integration, simulator effects and physical latency remain unqualified.

## Delivered behavior

Ownship receive time is captured immediately after UDP receive, before parsing
or waiting for state ownership. Accepted packets retain that `Instant` and a
host-owned nonwrapping capture counter, independent of protocol `u32` rollover.
Duplicates, retired sessions and malformed reports cannot renew either. Counter
exhaustion stays closed; capture/age survives snapshots, expiry and subscriptions.

Ownship now uses the existing typed native journal. JSON round trips would discard
the native `Instant`; the journal instead retains typed snapshots and defers JSON
presentation to consumers. Its existing external DTO remains unchanged. Limits
are 64 subscribers, 128 KiB per baseline/event, 32 pending events and 2 MiB queued
payload per reader. Conservative retained-byte accounting includes metadata,
strings and B-tree allocation allowances; unexpected structured values fail
closed. Slow readers receive explicit loss and must acquire a fresh subscription.

X-Plane retains a capture time and accepted acquisition counter with each dataref.
Unrelated guard/channel updates and selection remapping preserve a component's
evidence. Equal values in a new actual packet acquire new evidence. Empty input
does not advance it; exhaustion cannot wrap or restore known values.

The controller journal clock exposes its immutable identity/origin and checked
native `Instant` conversion without registry access. Conversion rejects pre-origin,
future and unrepresentable times and preserves the journal's microsecond/`+1`
convention. Registry session and clock identity agree. No rounded age or current
receipt time is substituted for acquisition time.

## Verification

- Full isolated native suite: **1,843 passed**, zero failures, 17 explicitly ignored.
- Focused ownship: **29 passed**. Focused X-Plane: **93 passed**, six explicit
  platform/helper cases ignored. Controller suite: **146 passed**.
- Controller stream/clock production lines: **100%**. New ownship retention
  module: **88.37%**; X-Plane session **99.16%**, snapshot **100%**.
- Production native executable builds. SHA-256:
  `29764f81dad342fb078c6a2f3ddb6a2e0a58f8b2d13803c5ee8a3fd33310abf9`.
- Actual Windows dev Cockpit: the private authored DCS process/BIOS/ownship
  fixture passes setup/repair, native/canonical/unbound inspection and identity
  processing, pause/active-pause/denial/frozen/stale withdrawal, advancing respawn
  baseline, mission stop and process exit. Cleanup reports no errors. This tests
  levels and acquisition behavior, not selective per-port token/timer recovery.
  Evidence: `.research/dcs-physics-native/1793667818053`.
- The actual X-Plane fixture is **not qualified**: its native WMI process-event
  query is denied with `0x80041003` before any RREF subscriptions start. The
  fixture now retains process/context diagnostics and reports the permission
  gate directly. No elevation was requested. User-operated monitoring permission
  and real X-Plane acceptance remain in #199.

The ordered ownship test stays within its declared queue bound; a separate test
forces overflow and fresh-reader recovery. Clock boundary tests use representable
Windows precision. The native fields remain present through typed subscription
clones while serialization omits them from the existing JSON DTO.

## Remaining scope

These stamps describe host acquisition, not a simulator's remote write time.
`ExportSnapshot` and game mapping still discard native per-port evidence and stamp
consumer admission; this delivery does not fix or qualify that boundary. BIOS
sparse byte/word witnesses, Assetto page evidence and its upstream notification
decision, captured-value conversions, contributor validation, scripts/timers and
selective family recovery remain open under the
[per-port contract](../reactive/native-capture-evidence.md). Physical preview and
raw-input-to-virtual latency still require measurements in #199.
