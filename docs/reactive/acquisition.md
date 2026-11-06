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

Qualified Windows controller and internal profiles service routing from retained
HID report watches, committed resource-token changes and actual deadlines.
Pending virtual reports request an immediate drain. A terminal feeder reply or
closed reply pipe wakes the profile host, and output leases retain their own
renewal deadline. Qualified game and desktop adapters also use retained wakes.
Mixed adapter combinations and missing watches can still require bounded service.

Desktop tap, repeat, pointer motion and reducer deadlines enter native deadline
selection. Foreground, desktop and session changes notify the parent and helper
scope owners. The helper also wakes for native commands and release deadlines.
First-dispatch, burst timing and broader adapter combinations remain open.

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
accepted or retired. Watched Assetto and X-Plane readers use the exact process
monitor already owned by the capture source. Process loss withdraws the cached
view even during a detached read. Assetto readers drain on capture notifications.
Their first native page timestamp determines the exact local freshness deadline, which
expires the decoder without another read from the source. Other admitted
readers still use the separate 100 ms process qualification pass. The pass
leaves unopened readers and open retries with the context owner. A wake with
no candidates skips installation validation and catalogue reads. Before and
after detached capture work, qualification checks the installation fields
directly against a catalogue snapshot taken with the metadata error check.
The shared Windows process subscription wakes game context on lifecycle events. A live
subscription lets the context worker reuse a successful batched inventory within
the same library and event revisions. When X-Plane is the only configured
integration, the context owner waits for notifications or a pending vehicle
reader retry. X-Plane SDK changes use their own file watch. The 500 ms pass
remains when process coverage is unavailable or an
inventory read fails. Each game's installation and runtime checks still apply.
For DCS, native changes to the selected export, BIOS, catalogue and aircraft
files trigger setup reinspection. The Saved Games root watch observes direct
DCS profile directory creation, removal and rename. A committed setup change
wakes the context owner to replace watches when the selected folder changes.
With both watches and live process notifications, the context owner parks
between events and pending retries. Safety-sensitive commands still read
durable evidence.
X-Plane passive inspection schedules explicit demand at its lease deadline.
Closing an observation wakes the vehicle worker to release its channels even
when the native capture has no updates.
DCS ownship acquisition retries at its own deadline, even when no BIOS packets
arrive. The retry does not renew the separate process qualification.
Watched DCS readers use an interruptible wait on the exact selected Windows
process handle. When the required watches are available, capture, retry, and
process exit each have independent wakes without a vehicle liveness timer.
The two-second pass remains for scopes where native watches could not be
installed. Assetto
reinspects the selected executable on native file changes. The Rally and
Competizione watches include the possible sibling runtime before it exists.
With live process notifications and a working Assetto file watch, the context
owner can park between events. The two-second deadline remains when a watch
cannot be registered or a watched reinspection is waiting for setup admission.
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
| App/workspace/gateway | Workspace publishes committed cached state; platform serial arrival/removal notifications invalidate inventory | Qualify per-provider background delivery, especially external device changes |
| Controller input | controller-input capture + stream journal | Windows report receive publishes batches; replace Linux/macOS 4 ms backend polling with platform report streams; share journal wakeups and descriptor cache |
| Controller/device visibility | HostControllers, DeviceVisibility, visibility coordinator | Platform device/process notifications plus acknowledged visibility mutations; quiet controls remain connected |
| USB/CAN/module inputs | Direct CDC input stream and advertised CAN panel stream with boot/config fences | Verify physical burst continuity, disconnect and rebaseline. Explicit diagnostic journal reads are separate from native acquisition. |
| Resource/system inventory | Device/process/commit invalidation with retries for unresolved resources and one-shot evidence expiry | Native workers publish committed snapshots. System inventory pages read the cache. Explicit Refresh inspects optional drivers and external virtual-device state. |
| Profile library/editor/resources/ports | ProfilesPanel and editor previews | Durable document/resource commits and capture topics; revision-pinned pages, query replacement disposal |
| Runtime/activation/progress | Native profile and activation read models publish committed health and progress; the activation window uses its scoped subscription | Verify cancellation, compensation and independent heartbeat under mixed faults and load |
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
| Settings/preferences/dependencies/backups | Committed preferences, Activity retention, and backup state use observations. AI settings reads status after a private invalidation from listener or credential changes. | Verify remaining dependency progress and reopen behavior. |
| Activity/debug/notifications | Activity commits and Debug's scoped topics invalidate current reads; notification dismissal has a local deadline | Qualify background changes and nested lifetimes |
| MCP observation/admin | Gateway commands and HTTP sessions | Shared topic registry/snapshot boundary, negotiated resource notifications and bounded cursor fallback; observe never renews authority |
| Native/browser-dev transports | gateway.ts IPC; native-gateway-bridge HTTP | Same typed envelopes, atomic subscription boundary, cancellation and overflow, including transport restart |
| Desktop/virtual/logic workers | Independent bounded pipes, host polls completions | Completion/loss wakeups; do not perform pipe I/O in capture subscriber callbacks |

## Unresolved acquisition boundaries

| Boundary | Current behavior | Remaining gate |
| --- | --- | --- |
| Legacy Pico protocol 1 | A disconnected `LegacyPicoService` parks. Claim expiry and unowned inactivity have their own local deadlines. A connected legacy board still uses a two-second `status` snapshot. Pico Debug selects a different service with sequenced inventory events. | Establish a legacy firmware event path or record an explicit sampling decision. |
| Windows process observation | Native process notifications are preferred. Global game activation parks when the catalogue has no installed game with process monitoring support and wakes on a library commit. The 500 ms discovery fallback applies when a monitorable installation exists and event coverage is unavailable. Scoped integration setup reports denied access and can request the explicit temporary monitoring helper. | Qualify consent and exact-process notification lifetime. Keep fallback evidence separate from native event coverage. |
| File and provider watches | Unavailable watches and detached reads retain scoped maintenance. DCS and Assetto setup scopes can retain two-second checks. Other admitted vehicle readers can retain 100 ms process qualification. | Verify watch coverage and retire or explicitly disposition each remaining fallback. |
| Linux and macOS controller input | The qualification probe uses a polling worker. Normal Cockpit startup leaves native controller capture disabled on these platforms. | Verify platform report and topology adapters before enabling capture. |
| ESP32 companion touch | The common touch task checks both boards every 20 ms. Sunton exposes touch IRQ on GPIO36. Waveshare wires GT911 INT to GPIO4, currently configured as input with interrupts disabled. | Wake the task from the board interrupt and recheck the input after arming to cover a contact already in progress. Drain and clear GT911 ready reports. For Sunton, verify release, debounce and held-motion behavior before disposition of any sampling during contact. |
| ESP32 companion display | Candidate readiness, feedback, touch changes and screen state notify the display task. It waits for visible feedback expiry or a failed-render retry deadline when needed. | Verify board builds, idle behavior and recovery on Sunton and Waveshare hardware. |
| ESP32 USB console | Provisioning drains nonblocking stdin. Direct requests retry empty reads and use an absolute response deadline. | Route UART or USB receive readiness to one bounded decoder. Let pending requests wait on bytes, cancellation or their deadline. Preserve exact link and reply identity through reconnect. |

The recorded approval applies to the Assetto shared-memory reader. The boundaries above remain open under the original acquisition gate.

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
| Global game activation observer | Native process events and initial inventory own the registered executable view. Windows access denial retains the 500 ms discovery fallback. | Authored start and exit passed through the fallback. Native consent, event continuity and live simulator qualification remain open. |

[Approval record](https://github.com/opensimgear/cockpit/issues/189#issuecomment-6640472899)

DCS export's existing up-to-20 Hz frame callback is source-defined cadence. Its
one-second freshness lease expires on the original deadline.

Preserve controller journal session/cursor/gap semantics, batch size 4,096,
non-consuming multi-reader access, exact numeric domains and complete baselines.
Preserve companion authenticated publication/owner fences, native subscription
limits and release/backpressure behavior. Inventory/storage/discovery must not
block active output dispatch, revocation, Stop or lease maintenance.

Run `node scripts/reactive/audit.mjs --committed --output=.research/reactive/current-refresh-sites.json` to inventory the checked-out
commits across Cockpit and the sibling firmware repositories. Omit `--committed`
to include worktree changes. Create the output directory first. Reuse the same output path and add `--check` to verify the generated snapshot. The checked-in ledger is the pinned baseline for `--check-dispositions`.
The scanner includes all tracked production source trees and sibling firmware
repositories. It also records native deadline waits such as `park_timeout` and
`sleep_until`. The generated list includes legitimate timing and some inline
test code, so each entry still needs semantic review. At final R9 there
must be an owner and explicit disposition for every remaining production site.

### November 6 committed inventory comparison

The committed scan found 262 timing sites at Cockpit `04dec909`,
device-gateway `e761e4d`, device-template `c545788` and firmware-common
`64025cc`. The batched Git reader produced the same bytes as the per-file
reader. The 247-entry pinned baseline and its dispositions remain available
for tracking retired sites.

| Count increase by file | Current owner and purpose |
| --- | --- |
| Browser companion connection worker (+1) | Retry deadline after connection failure. Close cancels it. |
| Controller observation UI (+2) | Scheduler declaration and frame callback for dirty preview presentation. |
| Desktop audit capacity (+7) | One production condition-variable wait for a barrier's remaining budget. Six waits belong to inline regressions. |
| Desktop focus lifetime and native owner (+3) | Bounded waits for owner retirement, initialization and posted shutdown. |
| Desktop worker serve and wait (+3) | Response writer shutdown bound and interruptible wait until an authored deadline. One wait is an inline regression. |
| Mapping-host desktop worker (+1) | Condition-variable wait for worker completion up to the caller's deadline. |
| Profile command worker, gateway and exact process exit (+17) | Inline regressions bound fixture coordination and exit observation. |
| Controller report-cost example (+1) | Authored pacing in the optional report benchmark. |

The comparison counts source sites, including test waits and callback
declarations. The unresolved acquisition table tracks the firmware, process
and platform boundaries that still require changes or qualification.
