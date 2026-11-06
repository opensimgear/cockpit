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
Sequenced gateway invalidations wake ownership and layout readback. The
temporary CAN publication actor uses those invalidations and explicit lease
and cleanup deadlines. Its former 20 ms State/Inspect loop is retired.

Qualified Windows controller and internal profiles service routing from retained
HID report watches, committed resource-token changes and actual deadlines.
Pending virtual reports request an immediate drain. A terminal feeder reply or
closed reply pipe wakes the profile host, and output leases retain their own
renewal deadline. Qualified game and desktop adapters also use retained wakes.
Mixed adapter combinations and missing watches can still require bounded service.

A pending Windows game launch with notified HID or native panel inputs and
an optional neutral virtual feeder can wait for retained wakes, feeder renewal,
or launch expiry. Native panels require a bound baseline and a registered host
wake. A pre-Start desktop owner has no registration to renew and can also
park. Physical action workers renew their own neutral leases and notify the
host on loss. Companion, visibility, and publication work keeps bounded service.
The hold stays disarmed until Start.

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
| Automatic selection/recovery/learning | Committed context and control wakes, captured input, registry release and retry deadlines | Qualify recovery and learning through context loss and reconnect. Passive subscribers have no authority. |
| Debug/topology/live/path/virtual previews | Shared observation snapshots and report updates | Verify nested lifetimes and current snapshots on reopen. Virtual preview observes acknowledged output. |
| DCS-BIOS / managed DCS export | Native UDP readiness publishes source captures. Freshness and source recovery have independent deadlines. | Qualify simulator effects, exact process watch lifetime and uncovered setup/vehicle scopes. |
| X-Plane | Shared RREF owner receives datagrams on socket readiness. Timed subscription renewal and component freshness have separate deadlines. | Qualify selected version, process consent and simulator expiry/recovery. |
| Shared-memory telemetry | games/telemetry/windows_source double-copy sample | One native sampler per active game is approved at the shared-memory boundary. Downstream consumers subscribe to committed captures |
| Games/rules/catalogues/setup | game_get_state, GameProfileRule, jobs | Publish process/context, committed rules, catalogue replacement and background job completion; explicit rescan remains a command |
| Discovery/artwork/metadata | Async workers, UI completion refresh | Worker progress/completion publishes revision, rejects old query/job generations; bounded immutable page reads |
| Companion presence/pairing/listeners | UI roster/listener subscriptions; OS-backed listener and peer I/O readiness, exact Hub/connection deadlines and direct cancellation | Native client reads also use readiness, scoped cancellation and absolute operation deadlines. Pending native pairing now receives a terminal host event on the original socket, with exact offer expiry; manual/serial status requests remain available. See [pairing events](../verification/2026-11-05-pairing-events.md) and [native client](../verification/2026-11-05-native-client-event-io.md), [listener](../verification/2026-11-05-companion-listener-readiness.md) and [peer I/O verification](../verification/2026-11-05-companion-peer-io.md). |
| Panels/designer/publication | Browser display reports follow publication, page and size changes; authenticated assignment and feedback streams | Preserve exact publication generation and native renderer ownership. Finish cross-client qualification. |
| Browser companion connection | Assignment and transport events trigger reconnect; transient failures own one retry deadline | Lease renewal and release watchdogs remain timed. Qualify background behavior on each supported client. |
| Serial and ESP32 firmware | Direct module USB input and metadata use CDC streams. Pico Debug requires CDC3 with inventory events. ESP32 host provisioning and companion transport have separate readiness and lease owners. | Qualify publication replacement, bounded backpressure and held release on each board. Remaining touch and console acquisition are listed below. |
| Settings/preferences/dependencies/backups | Committed preferences, Activity retention, and backup state use observations. AI settings reads status after a private invalidation from listener or credential changes. | Verify remaining dependency progress and reopen behavior. |
| Activity/debug/notifications | Activity commits and Debug's scoped topics invalidate current reads; notification dismissal has a local deadline | Qualify background changes and nested lifetimes |
| MCP observation/admin | Gateway commands and HTTP sessions | Shared topic registry/snapshot boundary, negotiated resource notifications and bounded cursor fallback; observe never renews authority |
| Native/browser-dev transports | gateway.ts IPC; native-gateway-bridge HTTP | Same typed envelopes, atomic subscription boundary, cancellation and overflow, including transport restart |
| Desktop/virtual/logic workers | Independent bounded pipes with completion and loss wakes. Authored actions and output leases retain their own deadlines. | Qualify mixed adapters and faults under load. Capture subscribers do not own output pipe I/O. |

## Unresolved acquisition boundaries

| Boundary | Current behavior | Remaining gate |
| --- | --- | --- |
| Windows process observation | Native process notifications are preferred. Global game activation parks when the catalogue has no installed game with process monitoring support and wakes on a library commit. After a live catalogue watcher exits, the owner attempts one replacement per library revision. The 500 ms discovery fallback applies when a monitorable installation exists and event coverage is unavailable. Initial access denial does not restart the watcher. Scoped integration setup reports denied access and can request the explicit temporary monitoring helper. | The Windows regression checks the recovery limit and its reset on a library commit. Qualify consent and exact-process notification lifetime. Keep fallback evidence separate from native event coverage. |
| File and provider watches | Unavailable watches and detached reads retain scoped maintenance. DCS and Assetto setup scopes can retain two-second checks. Other admitted vehicle readers can retain 100 ms process qualification. | Verify watch coverage and retire or explicitly disposition each remaining fallback. |
| Linux and macOS controller input | The qualification probe uses a polling worker. Normal Cockpit startup leaves native controller capture disabled on these platforms. | Verify platform report and topology adapters before enabling capture. |
| ESP32 companion touch | Waveshare GT911 INT on GPIO4 wakes the touch task. One 20 ms follow-up covers an early interrupt. Held contact, faults and interrupt-registration failure retain the 20 ms cadence. A quiet 200 ms read renews the existing touch safety lease. Sunton continues its 20 ms reads. | Qualify IRQ delivery, release and fault timing on Waveshare. Verify a safe Sunton event path and explicitly disposition remaining touch sampling. |
| ESP32 companion display | Candidate readiness, feedback, touch changes and screen state notify the display task. It waits for visible feedback expiry or a failed-render retry deadline when needed. | Verify board builds, idle behavior and recovery on Sunton and Waveshare hardware. |
| ESP32 USB console | On the supported UART console boards, the driver event queue wakes provisioning and retains one unread notification. Direct requests wait for UART readiness with their original deadline. The network task owns both reads and shares one bounded decoder. | Qualify command cancellation, reconnect and physical USB on both boards. Non-UART console builds still use the maintenance wake and have no board qualification. |

The pinned ESP-IDF console forwards readiness registration to its primary
device. The UART driver signals receive readiness on interrupt and checks
already buffered bytes during registration. [Console VFS](https://github.com/espressif/esp-idf/blob/v5.5.2/components/esp_vfs_console/vfs_console.c) · [UART VFS](https://github.com/espressif/esp-idf/blob/v5.5.2/components/esp_driver_uart/src/uart_vfs.c)

The provisioning wake uses the UART driver's bounded event queue. It carries
readiness only. The driver ring and the network task retain byte ownership.
The network task parks while disconnected or faulted on the UART boards.
Pending pairing waits for approval, transport termination, UART activity
or its existing offer expiry. Panel preparation wakes when the renderer
finishes the current candidate, on UART activity, or at the existing heartbeat
deadline. Panel error also waits for UART activity or its heartbeat deadline.
Assignment waiting wakes on UART or Wi-Fi stream notifications and uses the
earlier of the host ping and the Wi-Fi stream reconnect deadlines. A legacy
direct link without assignment push retains its existing reauthentication
deadline. Live UART builds wake on queued input, changes in touch or render
readiness, faults, and UART or Wi-Fi events. The wait also covers the earliest
heartbeat, feedback reconnect, pending batch and input safety expiry. The
10 ms batch spacing still applies when edges are queued. Pairing startup and
authentication wait until their request deadline. Opening waits for the exact
renderer borrow to be released when the inactive scene bank is busy. Non-UART
console acquisition retains the 10 ms maintenance wake.

The Wi-Fi feedback worker parks while its one raw frame is held by the network
task. Consumption or cancellation releases that handoff. The separate socket
readiness wait still has a 500 ms cancellation bound.

The pinned USB Serial/JTAG VFS supports read readiness once its buffered
driver is installed. A non-UART console bridge would need to install that
driver and coordinate readiness with the existing direct-request reader.
This remains unqualified. See the [USB Serial/JTAG VFS source](https://github.com/espressif/esp-idf/blob/v5.5.2/components/esp_driver_usb_serial_jtag/src/usb_serial_jtag_vfs.c).
[ESP-IDF UART event queue](https://docs.espressif.com/projects/esp-idf/en/v5.5.2/esp32/api-reference/peripherals/uart.html)

Sunton uses GPIO36 for PENIRQ. ESP32 documents restrictions on interrupt
use of GPIO36 with ADC or Wi-Fi/Bluetooth sleep. The existing Sunton reader
stays active pending a verified event path. The GT911 follow-up covers the
documented driver behavior where INT can precede a ready report.
[ESP32 GPIO restrictions](https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-reference/peripherals/gpio.html) · [Linux Goodix driver](https://github.com/torvalds/linux/blob/master/drivers/input/touchscreen/goodix.c)

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

### Native event and firmware waits

At Cockpit `d44ed47f` and the same three firmware revisions, the expanded scan
found 344 sites. The previous scanner found 259 at these revisions. The 247
historical dispositions still check against the pinned ledger independently.
Current scans include native scheduler calls, Windows wait handles, socket
deadlines, FreeRTOS waits and TinyUSB task calls. The Pico Debug firmware
source is included even though the gateway repository stores it under
`simulation/pico`. Fixture directories remain excluded.

| Owner | Disposition of newly inventoried calls |
| --- | --- |
| Native scheduler and producer-owned caches | Activity, device inventory, profile read models, companion status, and USB/CAN readers wake on source changes. Explicit deadlines cover retained evidence, request budgets, persistence retry and cleanup. Indefinite waits park the actor. |
| Game and profile service scheduling | Game read models wake on producer commits and expire captures at their retained deadlines. Global process and context workers can select the 500 ms discovery fallback. The setup and vehicle fallbacks remain in the unresolved table. Profile routing chooses its nearest actual deadline or the service bound when a source has no usable watch. |
| Native companion peers and local gateway | Peer presence uses a direct wake and a reconnect deadline. Pairing waits for the offer's expiry or its terminal event. Windows serial and socket waits select I/O readiness or the current operation deadline. Local gateway socket timeouts bound individual request reads and writes. |
| Desktop dispatch and process handles | The dispatch worker waits on command, scope and deadline handles. The audit collector bounds capacity waits by the caller's deadline. Zero-time process handle checks validate identity or exit during an owned operation. The long-running X-Plane process waits also select shutdown. |
| ESP32 | UART provisioning blocks on a driver queue. Display notification waits also handle explicit expiry and render retry. Touch and connected network maintenance retain the cadences described above. |
| STM32 module, STM32 gateway and Pico Debug | TinyUSB service runs inside the device loop and owns USB progress. Pico Debug also advances virtual lifecycle and publishes queued inventory. |

The protocol-1 firmware cannot publish inventory events. Cockpit now requires
protocol 3 and parks automatic connection after identifying protocol 1.

### Native readiness primitives

At Cockpit `41e4080e` with the same firmware revisions, the scan includes
348 sites after adding the four waits below. These waits were absent from
the earlier 344-site scan. The historical ledger still checks independently.

| Site | Owner and wake |
| --- | --- |
| `crates/controller-input/src/windows/worker.rs:47` | Windows Raw Input blocks indefinitely on the message queue or the worker's shutdown event. It does not periodically query controller state. |
| `crates/companion-service/src/server/listener.rs:30` | Mio wakes on incoming sockets, committed hub deadline changes, peer completion, Stop, or the next hub expiry. |
| `crates/companion-service/src/server/socket_io.rs:56` | Mio wakes the peer on read/write readiness, cancellation, or the current operation deadline. |
| `apps/companion-esp32/network/serial_wire.c:98` | The network task drains existing USB console bytes, then waits for UART readiness or the original direct-request deadline. An interrupt resumes the same wait budget. |

### Protocol retirement and launch scheduler comparison

The committed scan at Cockpit `9d11e267`, with the same firmware revisions,
found 338 sites. The ten removals from the 348-site readiness scan all belong
to the retired protocol-1 Pico service and workspace, including their fixtures.
Automatic connection parks on the upgrade error until the endpoint changes or
an operator reconnects. The current protocol-3 board still needs physical
qualification in #199.

Qualified held launches now wait on existing native watches, an optional
neutral vJoy feeder renewal, or the exact launch expiry. The source scan still
finds the fallback primitive because holds with other adapters retain the 5 ms
service bound. This is an acquisition boundary to resolve or disposition under
#189. The 247 historical timing entries still have exactly one disposition each.
