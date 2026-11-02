# Managed DCS ownship producer — 2026-11-02

This is the initial managed-ownship producer portion of #192/R3 and #188.
It does not complete the integration or routing migration.

## Implementation

One lifecycle owner binds the loopback UDP receiver. A dedicated current-thread
reactor waits for a datagram, cancellation, or the freshness deadline authored by
the last accepted report. There is no idle refresh timer. Ordered datagrams enter
a bounded typed journal at acquisition, independently of inspector/profile reads.
Source-native values and declared units/ranges remain unchanged.

Cloned views retain committed state/history, not the socket. A slow BIOS read can
retain a view of the same live ownship producer instead of freezing a copied
snapshot. Reading that view performs no receive I/O. Packet gaps replace the
generation and require an advancing baseline; replays do not extend freshness.
Pause, permission, frozen model time, context and malformed-packet transitions
withdraw ownship values independently of valid BIOS gauges.

Every local producer has a unique identity, separate from the Lua mission and
epoch. A local receiver replacement therefore requires a new profile baseline
even if mission, epoch and packet sequence match the old receiver. Unrelated BIOS
resources retain their generations and command authority.

Cancellation wakes the reactor directly and does not require a UDP wake packet.
Publication failures withdraw the cache, close the journal and terminate receive
ownership. The existing tracker retries a failed ownship owner after its explicit
error delay while retaining the independent BIOS source. Passive snapshots project
expired evidence as stale. A subscription at an already expired live baseline
waits for the native deadline commit, bounded to one second, and fails with an
explicit timeout if the owner cannot run; it cannot invent a renewed baseline.

## Qualification boundaries

DCS-BIOS acquisition, tracker/context/process discovery, other integrations,
external observation read models and event-driven profile routing remain separate
unfinished work. Existing tracker refresh and error-retry scheduling still exist.
The existing retired-mission replay guard retains only the last 16 session IDs;
arbitrarily old mission replay is not qualified by this change.
The typed ownship journal is not yet the frontend/MCP telemetry transport. No
synthetic fixture counts as actual simulator behavior, physical input/output, or
raw-input-to-destination latency qualification. User-operated acceptance remains
in #199, first in the progress lane.

## Verification

- The complete isolated native suite passes: 1,668 tests, zero failures and 12
  existing opt-in checks ignored. The 20 focused ownship tests include ordered
  acquisition without consumer pumps, sequence wrap/gaps, replay non-renewal,
  malformed/oversized frames, slow-reader overflow, socket release after failed
  publication and prompt shutdown during continuous traffic.
- Instrumented production line coverage: main ownship module 100%, parser 93.42%,
  session 99.29%, receive owner 94.36%. Changed tracker/profile consumer modules
  range from 84% to 99.01%, above the 71% gate. Profile coverage includes the
  `include!` read/prepare/delivery fragments.
- All 20 coverage-harness checks pass. The native executable builds successfully.
  The preceding controller latency commit `a64b332` passed GitHub CI.
- The native fixture now uses current installation registration/selection APIs.
  It selects only its private installation for removal and checks that original
  installations, name and selected installation are restored exactly.
- The running dev cockpit initially had an invalid profile-library document and
  was read-only, also blocking user editing. The supported `profile_reset`
  operation repaired that dev configuration, transactionally archiving its prior
  document in `recovery/explicit-reset`. Read-only changed from true to false and
  diagnostics cleared. This is a local configuration repair, not a schema
  compatibility change.
- The tested native executable SHA-256 is
  `299d9c902fc498efc066d3e0e038e89ab7351e43eae7d18e0111804dfbcdb7eb`;
  its isolated build tree is `d81c91b7d1d1baee646e0bd792debc5afb763df3`.
- Running Windows cockpit fixture passes all phases in 31.13 seconds: managed
  setup/repair, simultaneous independent inspection and selected native/canonical
  profile processing, zero/false preservation, pause/active-pause/permission/frozen
  time withdrawal and rearm, stale ownship with live BIOS gauges, respawn baseline,
  mission stop and process exit. Final cleanup reports zero errors, exact original
  installation restoration and no added scan root. Evidence completed at
  `2026-11-02T16:33:40.786Z` in local ignored
  `.research/dcs-physics-native/1793637189660/evidence.json`.

The last fixture uses an authored simulator process and authored UDP telemetry in
the actual native cockpit, with no physical outputs. It does not qualify real DCS.
