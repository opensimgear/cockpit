# Acquisition and read-model migration inventory

Initial source audit, 2026-11-02. R0 is in progress. `refresh-sites.json` is the
machine-readable list of timer sites produced by the audit script; it is an
inventory, not an allowlist or proof that every timer is an acquisition loop.
See [native timing dispositions](native-timing-dispositions.md) for the classified native sites.

Direct USB update, 2026-11-04: input acquisition and committed configuration
metadata now use native CDC streams. The historical `module_usb.rs` two-second
`recv_timeout` entry is retired; source metadata wakes the parked worker. The
remaining finite reconnect waits belong to explicit configuration reconciliation,
not periodic state acquisition. See [USB metadata](usb-metadata.md) and its dated
verification.

CAN panel inputs use timestamped module events forwarded by gateways that
advertise panel streaming. Cockpit profiles, previews and MCP share the
gateway capture. Profile port discovery reads the validated module profile
without requesting input state. [CAN verification](https://github.com/opensimgear/cockpit/issues/191#issuecomment-6627877630)
records the software evidence. Physical module and gateway acceptance remains
in [#199](https://github.com/opensimgear/cockpit/issues/199).

## R2 native input transport gate

This disposition covers input acquisition. Physical latency, held-input
and loss qualification remain in [#199](https://github.com/opensimgear/cockpit/issues/199).

| Transport | Current acquisition owner | Gate disposition |
| --- | --- | --- |
| Windows host controllers | The Raw Input worker blocks on Windows messages and its shutdown event. Startup and device-change notifications own enumeration. Profiles, preview and MCP consume the committed report journal. | The production report and enumeration loops have no periodic state query. |
| Direct USB modules | The blocking CDC duplex owner receives unsolicited input frames and separates them from configuration replies. The module capture feeds profile admission and passive readers. | The production input path has no periodic state query. Explicit configuration commands use their own reply deadlines. |
| CAN panel input | The gateway forwards module panel-stream frames. The native serial owner demultiplexes events and replies, and profile and preview consumers subscribe to the committed gateway capture. | The production input path has no periodic state query. The polled CAN fixture compiles only in tests. Gateways without panel streaming are excluded from profile and preview input admission. |
| Linux and macOS host controllers | The shared polling worker runs only from the explicit qualification probe. Ordinary Cockpit startup does not start native capture on these platforms. | Acquisition remains unaccepted until report and topology event adapters replace the probe polling and pass platform qualification. |

Gateway catalogue and temporary publication checks have a separate scope.
The remaining CAN publication fallback discovers firmware ownership and layout
changes. It does not read panel values or feed the input journal.

Qualified Windows controller/internal profiles now service routing from retained
HID report watches, committed resource-token changes and actual deadlines.
Optional virtual output keeps its independent renewal deadline; pending ordered
reports request an immediate drain. The 5 ms adapter fallback remains for games,
desktop output, panels, companions, visibility, publication and physical actions,
and for hosts without the required native watches.

Desktop tap, repeat, moving-pointer and isolated reducer deadlines now join the
native host's deadline selection. Its parent and worker still retain the 5 ms
focus/lease service requirement. Native dispatch cost and notification of focus
discontinuities remain open; exposing deadlines alone does not qualify timing.

November 4 audit update. Workspace discovery now enumerates serial ports on
native arrival/removal notifications. Changes in the serial topology cache,
including enumeration failure and recovery, wake the aggregate resource worker.
Unchanged enumeration results leave it parked. USB fleet identity and capability
changes also wake the worker, which projects cached USB state without starting
another device probe. The aggregate worker parks when sources have no pending
work. Missing endpoint capabilities retain bounded retry delays. Catalogue
failures, catalogues without an active file watch, and publication evidence
expiry schedule cache projection separately.
Host-controller metadata wakes its own resource worker on commits and first
readiness.

Companion pairing, assignment and transport readiness wake resource projection
from the native host. An accepted input baseline makes the transport visible
to profile inventory. Ordinary input values do not drive inventory updates.
STM32 gateway cache commits wake inventory for connection, module identity and
capability changes. Input cursors, output receipts and timestamp-only commits
stay off the inventory path. A fresh timestamp after expired evidence wakes
inventory so the new proof can be used.
Game library commits, setup and process context changes, and current vehicle
changes wake profile inventory. Watched SDK and DCS catalogue changes also
invalidate the profile inventory's catalogue cache by source generation.

The game-context owner parks after its initial pass when the library has no
configured integration. A committed library replacement wakes the owner after
the new catalogue is visible. The independent vehicle telemetry worker also
parks while it owns no reader. Accepted reader admission wakes it even before
the first packet; detached reads retain maintenance until their result is
accepted or retired. Admitted readers still use the separate 100 ms process
qualification pass. Watched Assetto readers drain only on capture notifications.
Their first native page timestamp determines the exact local freshness deadline,
which expires the decoder without another read from the source. Configured
integrations still use the 500 ms
process pass. For a watched DCS installation, native changes to the selected
export, BIOS, catalogue and aircraft files trigger setup reinspection. The
Saved Games root watch also observes direct DCS profile directory creation,
removal and rename. Startup and native setup, removal and recovery operations
publish the ledger state. Safety-sensitive commands still read durable evidence.
The two-second pass remains for scopes where native watches could not be
installed. Assetto
reinspects the selected executable on native file changes. The Rally and
Competizione watches include the possible sibling runtime before it exists.
If a native watch cannot be registered, that Assetto selection retains the
existing two-second check.
X-Plane setup readiness comes from committed local settings and executable
selection. The context owner watches each selected installation's
`Resources/plugins/DataRefs.txt` with an exact native file filter. Changes
invalidate that X-Plane version's cached catalogue and wake context projection.
Explicit catalogue reads can still check the current file. The vehicle capture
worker and explicit launch and setup checks are separate.

| Provider | External source still requiring native invalidation |
| --- | --- |
| DCS | Setup and discovery scopes without native watch coverage. |

| Surface / owner | Current boundary | Required replacement and qualification |
| --- | --- | --- |
| App/workspace/gateway | App.svelte 3 s workspace refresh | Gateway worker publishes committed status/module changes; platform serial arrival/removal notifications invalidate inventory |
| Controller input | controller-input capture + stream journal | Windows report receive publishes batches; replace Linux/macOS 4 ms backend polling with platform report streams; share journal wakeups and descriptor cache |
| Controller/device visibility | HostControllers, DeviceVisibility, visibility coordinator | Platform device/process notifications plus acknowledged visibility mutations; quiet controls remain connected |
| USB/CAN/module inputs | Direct CDC input stream and advertised CAN panel stream with boot/config fences | Verify physical burst continuity, disconnect and rebaseline. Explicit diagnostic journal reads are separate from native acquisition. |
| Resource/system inventory | Device/process/commit invalidation with retries for unresolved resources and one-shot evidence expiry | Native workers publish committed snapshots. System inventory pages read the cache. Explicit Refresh inspects optional drivers and external virtual-device state. |
| Profile library/editor/resources/ports | ProfilesPanel and editor previews | Durable document/resource commits and capture topics; revision-pinned pages, query replacement disposal |
| Runtime/activation/progress | profile_session_host health drain; activation windows 150/200 ms | Source/completion wakeups and next authored deadline; publish health/progress commits; independent Stop, compensation and heartbeat |
| Automatic selection/recovery/learning | UI status loops and native game/process tracking | Process notifications, explicit recovery/learning transitions and captured input; no authority in passive subscribers |
| Debug/topology/live/path/virtual previews | UI 250 ms–2 s reads | Subscribe to shared native snapshots and reports; virtual preview observes acknowledged output, never drives runtime |
| DCS-BIOS / managed DCS export | Socket reports plus command-pumped reads | One blocking socket producer publishes source-native batches, context and quality deadlines; separately confirm simulator effects |
| X-Plane | xplane bounded datagram poll/resubscription | Blocking socket receive; protocol subscription renewal remains timed; source expiry distinct from connection |
| Shared-memory telemetry | games/telemetry/windows_source double-copy sample | One native sampler per active game is approved at the shared-memory boundary. Downstream consumers subscribe to committed captures |
| Games/rules/catalogues/setup | game_get_state, GameProfileRule, jobs | Publish process/context, committed rules, catalogue replacement and background job completion; explicit rescan remains a command |
| Discovery/artwork/metadata | Async workers, UI completion refresh | Worker progress/completion publishes revision, rejects old query/job generations; bounded immutable page reads |
| Companion presence/pairing/listeners | UI roster/listener subscriptions; OS-backed listener and peer I/O readiness, exact Hub/connection deadlines and direct cancellation | Native client reads also use readiness, scoped cancellation and absolute operation deadlines. Pending native pairing now receives a terminal host event on the original socket, with exact offer expiry; manual/serial status requests remain available. See [pairing events](../verification/2026-11-05-pairing-events.md) and [native client](../verification/2026-11-05-native-client-event-io.md), [listener](../verification/2026-11-05-companion-listener-readiness.md) and [peer I/O verification](../verification/2026-11-05-companion-peer-io.md). |
| Panels/designer/publication | Browser display reports follow publication, page and size changes; authenticated assignment and feedback streams | Preserve exact publication generation and native renderer ownership. Finish cross-client qualification. |
| Browser companion connection | Assignment and transport events trigger reconnect; transient failures own one retry deadline | Lease renewal and release watchdogs remain timed. Qualify background behavior on each supported client. |
| Serial/ESP32 firmware | Serial workers + status/control request cadence | Explicit producer/firmware protocol stream, bounded backpressure, publication replacement and held-release evidence per board |
| Settings/preferences/dependencies/backups | UI preferences and AI settings use native observations | Publish remaining dependency/backup worker progress. AI settings reads status after a private invalidation from listener or credential changes |
| Activity/debug/notifications | ActivityPanel/DebugPanel 3 s | Append/clear/settings commits publish pinned revisions; notification dismissal remains a real deadline |
| MCP observation/admin | Gateway commands and HTTP sessions | Shared topic registry/snapshot boundary, negotiated resource notifications and bounded cursor fallback; observe never renews authority |
| Native/browser-dev transports | gateway.ts IPC; native-gateway-bridge HTTP | Same typed envelopes, atomic subscription boundary, cancellation and overflow, including transport restart |
| Desktop/virtual/logic workers | Independent bounded pipes, host polls completions | Completion/loss wakeups; do not perform pipe I/O in capture subscriber callbacks |

The inventory includes hidden activation windows, offscreen designers and browser
workers. Timer sites must be classified during cutover: acquisition/read-model
refresh, source/protocol cadence, independent lease, deadline, debounce or animation.
No blanket timer ban: request timeouts, UI debounces, owned renewal and authored
mapping behavior remain legitimate when they do not refresh application state.

The currently supported telemetry adapters are Assetto Corsa, Assetto Corsa
Competizione, Assetto Corsa EVO, Assetto Corsa Rally, DCS-BIOS, managed DCS ownship,
and X-Plane 11/12 (see ../telemetry.md). The user approved one native
shared-memory sampler per active Assetto game for all four dialects. The sampler
owns the mapping and publishes detected physics packet and graphics/static page
changes with the original packet identity, mapping generation and capture times. Inspector,
profile and MCP consumers share committed updates and have independent delivery
limits. The exception applies to acquisition only. Live qualification remains
separate for each simulator.

The reader targets a 2 ms sample period. A page overwritten between reads
cannot be recovered. A Windows process-event lease now fences the selected
mapping family and ends the reader on loss of exclusivity or notification
continuity. Manual setup retains a single preflight process inventory. The
global process observer and live simulator qualification remain open.
Physical controller acquisition is outside this exception.

| Assetto timing owner | Disposition | Verification |
| --- | --- | --- |
| Shared capture worker, 2 ms sampling and 50 ms read-error retry | Approved native mapping acquisition and bounded recovery. A failed process lease retires the worker. | Ordered and latest-reader fixtures, mapping generation tests and failed-lease replacement regression |
| Manual setup verification | Journal publication, cancellation or the existing verification deadline wakes the probe. | Native tests cover cancellation and outcome handling. Live permission denial and Stop passed. Authorized simulator delivery remains in #199. |
| Global game activation observer, 500 ms catalogue process scan | Replace with a process-event owner for registered executable identities and initial inventory. | Verify launch, competing processes, process replacement, missed notifications and idle behavior |

[Approval record](https://github.com/opensimgear/cockpit/issues/189#issuecomment-6640472899)

DCS export's existing up-to-20 Hz frame callback is source-defined cadence. Its
one-second freshness lease expires on the original deadline.

Preserve controller journal session/cursor/gap semantics, batch size 4,096,
non-consuming multi-reader access, exact numeric domains and complete baselines.
Preserve companion authenticated publication/owner fences, native subscription
limits and release/backpressure behavior. Inventory/storage/discovery must not
block active output dispatch, revocation, Stop or lease maintenance.

Run `node scripts/reactive/audit.mjs --committed` to inventory the checked-out
commits across Cockpit and the sibling firmware repositories. Omit `--committed`
to include worktree changes. Use `--check` with the same mode to verify the ledger.
The scanner includes all tracked production source trees and sibling firmware
repositories. It also records native deadline waits such as `park_timeout` and
`sleep_until`. The generated list includes legitimate timing and some inline
test code, so each entry still needs semantic review. At final R9 there
must be an owner and explicit disposition for every remaining production site.
