# Shared X-Plane acquisition and process notification fence — 2026-11-02

This is an R3 implementation milestone for #188/#192. It does not finish R3,
the UI/read-model migration, event-driven routing or simulator acceptance.

## Behavior

Profiles and passive inspectors with the same captured X-Plane process and setup
share one acquisition actor, socket, subscription union and typed native state.
Consumers read committed snapshots; reads no longer receive UDP, renew protocol
subscriptions or advance capture evidence. Selection changes preserve other
readers' subscriptions and native timestamps. Different captured setups cannot
silently establish competing acquisition for the same process.

The actor receives UDP independently of rendering and schedules actual source
expiry, simulator-clock progress expiry, request pacing and protocol renewal
deadlines. A snapshot includes producer, epoch, capture sequence/time, native
number/boolean values and individual sample capture times. Zero and false remain
known values. Local projection limits inspector work to that reader's selection.
Mapping admission remains a distinct monotonic clock; explicit native capture
and routing admission separation is still required in R4.

Reader count, selected paths, protocol requests, frames and bytes are bounded.
Slow-reader loss retires that reader and releases its lease. Stop/revocation is
independently schedulable; paced unsubscribe cleanup runs outside caller/registry
locks. Fixed listen ports can briefly reject replacement during real cleanup;
the default ephemeral port avoids that reuse boundary.

## Windows permission and lifecycle

An exact retained native process handle detects selected-process exit. WMI
process-start notifications fence competing X-Plane processes. Subscription and
queued-start validation precede readiness; callback overflow, upstream dropped
events, provider failure and monitor loss revoke authority. There is no fallback
process polling inside this fence.

On this machine, ordinary medium-integrity Windows StartTrace subscription
returns `WBEM_E_ACCESS_DENIED (0x80041003)`. Direct capability checking is cached.
Passive status reads never elevate. The telemetry inspector exposes an explicit
Allow action and Cancel/Stop controls, with typed MCP equivalents
`game_process_monitor_enable` and `game_process_monitor_cancel`. Requests identify
only a registered game; callers cannot choose a PID, executable or command.

Allow can launch only the current application executable as a short-lived Windows
permission helper. The bounded, local-only named pipe uses a protected user/SYSTEM
DACL, random nonce, process-handle/image/user checks and creation-epoch checks in
both directions. Cancellation, pipe closure, parent/game exit and startup timeout
revoke the request. A pending Windows consent dialog cannot be dismissed by the
app; late consent cannot configure a canceled request. Only one OS consent launch
can remain in flight. No installed service or compatibility schema was added.

Actual consent, elevated event delivery and helper/COM cleanup under OS stalls
remain **unqualified**. Software tests exercise native IPC, real process handles,
legitimate in-memory WMI payloads and injected watcher/launcher boundaries; those
tests do not substitute for real elevated delivery. #199 tracks that user-operated
acceptance separately and remains first in progress.

## Verification

- Full instrumented isolated native suite: 1,799 passed, zero failed, 17 explicit
  opt-in/helper checks ignored; 95.52 seconds, four test workers.
- All 17 focused production modules meet the 71% line floor. Shared owner 95.92%,
  registry/client 89.47%, actor worker 89.27%, permissions 79.69%, MCP dispatcher
  75.87%, process facade
  85.94%, Windows worker 97.24%, callback sink 99.09%, bridge 84.44%, protocol
  96.06%, security 81.22%, native wait 78.85%, launcher 93.94%.
- Existing game/session/vehicle consumers and gateway pass local coverage.
- Coverage harness/parser regressions: 20 passed. The broader MCP dispatcher now
  has a permanent coverage gate, including typed permission operations, operation
  replay and guarded gateway/USB configuration validation.
- Frontend source suite: 1,538 passed before the final bounded telemetry fix;
  subsequent focused telemetry/UI tests: 19 passed. Telemetry model coverage:
  96.77% lines, 89.66% branches, 92.68% functions. Svelte check: zero errors/warnings.
- Native production executable builds. SHA-256:
  `b6731dc718853825d65e47cd4efb6c96902c7e889744c854cd425273c6748c02`.
- Actual newly built Windows Cockpit, 20:34 UTC: the real 33-game library reaches
  the telemetry inspector and process-monitor controls. Explicit Allow with no
  observed game rejects before consent; Cancel succeeds; forged PID input fails.
  No UAC, game launch, simulator command or physical output was requested.
- Widget/package regression groups: 134 passed. Vite production build passes;
  the existing large-chunk advisory remains.

Running-app verification exposed the telemetry client's obsolete seven-game
reply limit. The bounded limit now matches the registered library's 512-game
capacity; a 33-game acceptance and 513-game rejection regression cover it.

Local raw logs, source stamps, coverage and native UI artifacts are retained in
ignored `.research/reactive/r3-xplane-owner-*` and
`.research/reactive/r3-xplane-monitor-native-evidence.json`.

## Open qualification

Global process/context/catalogue observation and existing telemetry UI refresh
still need their R5/R7 cutover. Other integration adapters, ordered R4 journal
consumption, full pause/replay/restart/soak cases and real X-Plane/Windows consent
acceptance remain open. Prior authored X-Plane 11/12 fixture evidence belongs to
the preceding subscription-lease milestone; it does not qualify this new helper.

Controller preview latency remains a separate open requirement. Synthetic stream
dispatch profiling identifies duplicate native/shared validation but does not
measure the reported profile-editor preview or raw-input-to-virtual receipt.
Existing Devices-inspector figures cannot stand in for either measurement.
