# Remaining #188 acceptance

This ledger separates original software requirements from user-operated
qualification. It does not introduce another feature or waive an original gate.
The former 70% effort estimate is withdrawn: acceptance checkboxes are not
equally sized implementation tasks.

The original #189–198 acceptance list has 55 criteria, with 38 checked and 17
open. Six of the open criteria were transferred to #199 for user-operated
qualification. Some remaining criteria have delivered software and outstanding
integrated or platform qualification.

#190 (R1, shared stream contracts) and #197 (R8, MCP workflow parity) are closed. Integrated qualification remains open in #198.

## Software and software qualification remain in the original issues

| Issue | Remaining boundary |
| --- | --- |
| #189 | The [current timing matrix](current-timing/README.md) accounts for the scanned sites and documents acquisition inside hardware and platform loops. DCS and Assetto setup [withdraw readiness when file watch registration fails and recover from a fresh baseline](../verification/2026-11-08-setup-watch-recovery.md). Passive vehicle admission requires a live exact process watch and a capture notification. Installation and setup commits wake scope revalidation independently of the catalogue worker. DCS alias and X-Plane installed aircraft catalogues use [scoped native file notifications](../verification/2026-11-08-vehicle-catalogue-wakes.md). Remaining gates are decisions for active acquisition fallbacks and qualification against the R0 budgets. The user approved one native shared-memory reader per active Assetto game. The approved [Windows process discovery boundary](../verification/2026-11-07-game-process-discovery-preference.md) uses a Settings opt-in and one shared one-second check when events are off or denied. |
| #192 | Passive and active telemetry retain their exact readers during transient process inventory failures. A [late catalogue refresh](../verification/2026-11-07-slow-vehicle-catalogue.md) also preserves the admitted passive reader and its existing capture. Active proof expiry withdraws samples and requires a fresh baseline. Unstable Assetto page copies preserve the original capture's age. The authored DCS fixture verifies independent BIOS and ownship withdrawal and recovery in the running Cockpit. The [DCS backpressure fixture](../verification/2026-11-07-dcs-telemetry-backpressure.md) verifies BIOS expiry alongside fresh ownship captures and a stalled reader on both transports. Contention across the other integrations and actual simulator quality qualification remain open. Windows denied the Assetto and X-Plane process notification prerequisite for the latest telemetry fixtures. |
| #193 | Worker-owned tap release, native high-resolution waits, parent/helper scope notifications and immutable queued admission proofs are delivered. Qualified native HID/internal desktop profiles now use retained wakes and deadlines, and the recent-application chooser also uses native events. Authored DCS profiles with [explicit](../verification/2026-11-08-mixed-game-desktop-wakes.md) and [selected-game](../verification/2026-11-08-selected-game-desktop-wakes.md) desktop scopes run without the 5 ms service interval while their watches and process proof are healthy. The controlled two-window focus check passes after Windows is unlocked. Remaining work includes live simulator effects, broader focus and child-loss qualification, and first-dispatch and burst timing. Sustained latency qualification is interrupted by WSL foreground takeover. |
| #194 | The [read-model source matrix](read-model-sources.md) and [running delivery checks](../verification/2026-11-08-r5-read-model-delivery.md) verify discovery, process, filesystem, settings and Activity delivery on native Windows and Edge browser-dev. The process fixture uses the approved Windows fallback. Granted-event consent and watcher lifetime remain in #199. Native macOS auto-start delivery remains open. |
| #195 | Actual simulator feedback and separate native transport and board qualification remain. Listener accepts/server peers and native client reads use event I/O. Pending approval uses a host event. Active profile companion scheduling uses producer wakes and explicit deadlines. Unsupported adapters and physical companion firmware acquisition still need their original software disposition. The historical legacy mapping-route worker has no Cockpit caller and its commands are rejected. |
| #196 | Main navigation, Activity retention, repeated profile game-query handoff, preference reopen and Games detail reopen after a background commit are verified on native and browser-dev. [Debug telemetry](../verification/2026-11-07-telemetry-reopen-parity.md) follows visible game renames and shows current metadata and authored DCS samples after reopening on both transports. Native General discards desktop status replies after hiding and shows background tray and progress commits on reopen. Browser-dev shows the desktop-only controls as unavailable and follows the saved progress value. Native AI settings retires its status reader while hidden and reconciles a background credential revocation on reopen. Browser-dev shows the desktop-only AI settings boundary without a status request. Native dirty editor, controller metadata and companion listener checks passed. Companion library reopen passed on native and across separate browser-dev tabs. The [applied virtual preview](../verification/2026-11-07-virtual-preview-reopen.md) mounts hidden without subscribing and shows the latest routed sample on each reveal on both transports. [Debug desktop readiness](../verification/2026-11-07-debug-desktop-readiness-details.md) now opens only when the details are expanded and retires on collapse, hide and navigation. Native and browser-dev verified exact collapse retirement. Other nested lifetime and background/reopen checks remain open. |
| #198 | The [30-minute authored Windows soak](../verification/2026-11-07-dcs-panel-vjoy.md#30-minute-desktop-soak), [software burst and fault scenarios](../verification/2026-11-07-stream-fault-scenarios.md), and [terminal observation reopen](../verification/2026-11-08-terminal-observation-reopen.md) passed. Physical unplug and replug remain in #199. All eight top-level views passed [60-second native and browser-dev idle traces](../verification/2026-11-08-idle-transport-matrix.md) with zero gateway requests. The broader integrated matrix, full timing budget, applicable suites and CI, and final acquisition inventory remain open. |

## R7 UI consistency

[Debug recovery reader lifetime](../verification/2026-11-08-companion-transport-and-timing-audit.md)
passed three open/retire cycles on both transports. Each current baseline
caused exactly one status read and closed readers stayed quiet. Recovery
presentation during an actual active-profile fault remains open.

[Navigation and profile progress](../verification/2026-11-08-r7-navigation-and-profile-progress.md)
passed on the current native and browser-dev builds. The profile editor
reopened on the committed rename, applied a saved range edit under a new
owner, and reopened on a stopped runtime after an independent Stop on
Overview. Both transports observed the exact progress outcomes and expiry
without status refreshes. The full nested-surface matrix remains open.

[Settings Devices and Backups](../verification/2026-11-08-settings-reader-lifetimes.md)
retire their readers while the document is hidden. Devices reopens on
current desktop, driver, inventory and visibility baselines on native and
browser-dev. Native Backups follows the current catalogue on reopen and
recovers a queued restore after the observation broker has stopped.

[Companion listener settings](../verification/2026-11-08-companion-listener-visibility.md)
retire the listener reader when hidden. Both transports verified a fresh
baseline and one status read on reveal after a listener commit in a private
Cockpit session.

The [Companions roster](../verification/2026-11-08-companion-roster-visibility.md)
retires its passive status demand while hidden and discards delayed replies.
Both clients reopened on the committed listener state and offered pairing.

The [Companion panel editor](../verification/2026-11-08-panel-assignment-reopen.md)
reconciles committed assignments on both clients and retains local checkbox
edits across reveal. Save merges those edits into a fresh host snapshot.

The [controller detail visibility card](../verification/2026-11-08-controller-visibility-reader-lifetime.md)
retires both of its readers on document hide or card exit. Native and
browser-dev reopened on the preference committed while hidden. The
surrounding Devices inventory reader retained its own lifetime.

The [physical controller preview](../verification/2026-11-08-controller-preview-visibility.md)
releases input demand and cancels its queued presentation frame while hidden.
Native and browser-dev reopen on a fresh input baseline and preserve the
controller detail draft. Real VKB preview cadence remains in #199.

[Direct USB details](../verification/2026-11-08-usb-details-visibility.md)
retire the exact input and metadata readers while the document is hidden.
The card reopens on current stream baselines and preserves its local setup draft.

The Games executable field follows committed changes for the selected
installation and preserves a local edit until commit or Escape. The running
native and browser-dev fixtures observed a visible replacement, retained a
draft across a second external change, restored the current path on Escape,
and reopened on a third change. Clean blur sent no mutation.

Matching profiles stops candidate reads while hidden. Native and browser-dev
both showed the latest committed preference after reveal and after reopening
Games. Each opening made one candidate read and kept the old choice disabled
until the current snapshot arrived.

[Native driver setup](../verification/2026-11-07-optional-driver-observation.md#driver-setup-across-settings-navigation)
retains the pending operation across Settings navigation, including when a driver
file appears before the installer returns. Completion reaches global notifications.
Native and browser-dev show the latest observed driver metadata on reopen.

The companion and integration port sections share the same tile, filter and
pager. The tile owns spacing, typography and status styles. Companion authoring
controls retain concise labels, icons and accessible remove actions. The
running Windows Cockpit passed the companion input/output tile and paging
fixture on November 7. Ten focused port UI tests and the full companion
coverage command also passed. The WebView loaded the development frontend,
which contained unrelated local profile edits. The component review used
committed main. The physical VKB and broader background/reopen gates remain open.

The DCS aircraft editor follows provider settings and changes to JSON in the
selected DCS-BIOS catalogue directory through native file notifications. The
prior desktop build reproduced a stale port list after an in-place module edit.
Desktop and browser-dev fixtures verify path changes, in-place edits and alias
remaps while the profile remains open, then another edit while it is closed.
The open draft retains its unsaved rename and layout. Reopening shows the
current ports, and the saved profile and assignments retain their original
values. The [vehicle conflict preview](../verification/2026-11-08-vehicle-conflict-visibility.md)
also stops compiling while the document is hidden and checks the current
draft on reveal. The fixtures use private databases and authored catalogue files.
The Windows suite passed 2,393 tests with 18 default ignores. The four
configured native coverage gates passed. The additional DCS context watcher
measured 81.13% production line coverage. All 19 local Nx projects passed their
applicable check, test and build targets.

## R3 adapter evidence

[Active telemetry proof continuity](../verification/2026-11-08-active-telemetry-proof-continuity.md)
withdraws game branches when a successful process or catalogue capture starts
after the previous proof expires. Recovery retains the exact reader and takes
a fresh baseline. A delayed successful capture cannot conceal an expired proof.

[X-Plane owner setup](../verification/2026-11-08-xplane-owner-scope.md)
uses a lock per exact process. A slow catalogue open can no longer hold
another X-Plane process's acquisition. Same-process setup still shares its
owner or rejects a competing configuration.
[Assetto owner setup](../verification/2026-11-08-assetto-owner-scope.md)
locks each variant independently and still shares the reader within that
variant. End-to-end simulator qualification remains open.

[X-Plane inspection demand](../verification/2026-11-08-xplane-inspection-demand.md)
is validated under the shared tracker lock. The capture worker applies the
native channel selection outside that lock, including while the source is
quiet.

[Passive telemetry capture](../verification/2026-11-08-vehicle-capture-workers.md)
now handles native reads and freshness deadlines independently per integration.
A blocked reader can no longer delay another integration's capture.
[Passive reader startup](../verification/2026-11-08-vehicle-reader-startup.md)
runs native open and initial watch registration on separate workers by
integration. Simulator acceptance remains in #199.

[Timed DCS reads under a held receipt](../verification/2026-11-07-dcs-telemetry-backpressure.md#observation-driven-read-timing)
matched every authored value on native and browser-dev. [Watched catalogue reuse](../verification/2026-11-07-dcs-telemetry-backpressure.md#reuse-the-watched-dcs-catalogue)
removed the request-time rebuild for unchanged catalogues. Both clients
showed edited and restored descriptors, and the slowest inspection request
fell below 23 ms in the comparison runs. Browser-dev still recorded one
123 ms sample from UDP to readable state. The [expanded trace](../verification/2026-11-07-dcs-telemetry-backpressure.md#native-publication-spacing-with-stale-bios)
located gaps before native publication. Stale BIOS projection now reuses
prepared channel IDs while ownship continues. [Exact file notifications](../verification/2026-11-07-dcs-telemetry-backpressure.md#exact-file-notifications-for-the-passive-reader)
replace recurring content loads while a passive reader has a healthy watch.
Both clients retained the reader through an unrelated edit and withdrew on
a selected edit during quiet UDP. Other integrations and rig timing remain open.

The adapter evidence item is checked. Each supported adapter has automated
coverage and running Cockpit evidence. The dated runs below used authored
publishers. Real simulator behavior remains open in #199 for each simulator.

| Adapter | Automated evidence | Running Cockpit evidence | Real simulator |
| --- | --- | --- | --- |
| DCS-BIOS | Native transport, catalogue and capture tests | Counter-only renewals, independent expiry and recovery, and a stalled observer in the [running fixture](https://github.com/opensimgear/cockpit/issues/192#issuecomment-6685710159) | Open |
| DCS managed ownship | Native parser, source and session tests | Selected and unbound channels, pause, export denial, frozen clock, respawn and exit in the same fixture | Open |
| Assetto Corsa | Windows source tests cover Corsa mapping, process identity and stable copies | Corsa entry in the [four-variant authored run](../verification/2026-11-03-native-port-admission.md), including processing, frozen expiry and recovery | Open |
| Assetto Corsa Competizione | Windows source tests cover Competizione mapping, process identity and stable copies | Competizione entry in the same authored run | Open |
| Assetto Corsa EVO | Windows source tests cover EVO mapping, process identity and stable copies | EVO entry in the same authored run | Open |
| Assetto Corsa Rally | Windows source tests cover Rally mapping, process identity and stable copies | Rally entry in the same authored run | Open |
| X-Plane 11 | [RREF leases](../verification/2026-11-02-xplane-subscription-leases.md) and [retained scheduling](../verification/2026-11-06-xplane-profile-service.md) tests | Version 11 passed authored RREF delivery, partial expiry and recovery. The [current SDK watcher](https://github.com/opensimgear/cockpit/issues/192#issuecomment-6685794927) passed without telemetry. | Open |
| X-Plane 12 | Same RREF and retained scheduling tests | Version 12 passed the authored RREF and current SDK watcher checks separately. | Open |

Later X-Plane and Assetto telemetry runs stopped at Windows process-notification
access denial. The earlier runs establish adapter evidence for the tested builds.
The process-monitor consent and lifetime qualification remain in #199.

The passive vehicle reader retains its current process generation when a
successful projection misses the two-second cache deadline. It discards the
late projection and keeps the previous capture's original timestamp. The
X-Plane fixture confirms that the next capture advances on the same reader.
Process expiry, confirmed replacement and reader failure still withdraw
their affected reader. The rebuilt Cockpit's authored DCS fixture also kept
the profile owner while an independent reader received 32 notifications
during one withheld delivery. These checks cover separate reader boundaries.
The full native suite passed 2,375 tests with 18 existing ignores. The
vehicle polling module reached 95.73% line and 100% function coverage.
Process inventory contention and real simulator qualification remain open.

Passive telemetry now distinguishes an unavailable process lookup from a
confirmed exit. A lookup error preserves the reader for the same installation
and setup under its existing process evidence. It cannot extend the lease or
publish a newly opened reader. Setup replacement, confirmed exit and expiry
still withdraw the affected reader. This change covers the passive reader.
Active game-context and command validation have their own proof requirements.
The native suite passed 2,378 tests with 18 default ignores. Both pinned
DCS archive smoke tests passed separately. Production line coverage was
91.30% for the context runtime, 98.62% for vehicle observation, and 94.94%
for vehicle polling using the setup coverage gate's counting rules. The
supplementary LLVM report measured 65.85% functions for the context runtime.
The rebuilt Cockpit passed the authored DCS fixture, including process exit.

The Windows process inventory now reports a competing process separately from
an unavailable lookup. The passive reader withdraws when Windows confirms
ambiguity, even if the previously selected process is still alive. The
preceding dev build reproduced the regression with two authored DCS processes
and kept known airspeed while the context reported ambiguity. The corrected
build withdrew both ownship and BIOS values and resumed after the second
process exited. No simulator or physical inputs were used. The native suite
passed 2,379 tests with 18 default ignores. The changed production modules
passed their line coverage gates.

### Active process inventory contention

An exact watched inventory row remains usable when an unrelated row cannot
be inspected. An unavailable lookup on the selected row cannot renew its
process proof. The original 100 ms proof expiry still withdraws telemetry.
The retained reader resumes with a baseline after a successful lookup.
Confirmed process exit or ambiguity still withdraws the context. The Games
view reports uncertainty without issuing a new session. Strict command
admission requires an independent successful lookup.

The isolated Windows suite passed 2,387 tests with 18 default ignores. Both
pinned DCS-BIOS archive checks passed separately. The repository's production
line counter measured 99.32% for the context broker, 91.40% for the context
runtime, 100% for the process cache, 95.71% for preparation validation,
85.23% for session reads and 98.97% for session proof validation. LLVM's
supplementary function count for the full context runtime was 66.28%.

The first suite run caught a changed error message for confirmed process loss.
The passing rerun preserves the existing message. The default dev build
ran out of disk creating the static library. The running fixture used
an uninstrumented dev executable linked from the same Rust sources with the
isolated manifest set to build only the Rust library format needed by the
dev executable. The fixture passed competing-process withdrawal, independent
BIOS and ownship recovery, and process exit with no cleanup errors.
Executable SHA-256 was
`4AE8A5A13979A5265DBCCC82BC92EEB1C5CD3CE8760C276626F5B21C716D8FCA`.

[Watched process inventory retries](../verification/2026-11-08-context-inventory-scope.md)
preserve definitive rows and retry only unavailable rows within the exact
scope. A superseded read is discarded before projection.
[Background context qualification](../verification/2026-11-08-context-projection.md)
runs executable checks, process liveness and catalogue lookup independently
by integration. One blocked check cannot stop another integration from
publishing. [Setup watch owners](../verification/2026-11-08-setup-watch-owners.md)
now handle native watch registration, background setup and aircraft catalogue
baselines independently by integration. Provider changes and explicit setup
commands still require exclusive admission.
[Assetto fingerprint slots and setup job readiness](../verification/2026-11-08-assetto-fingerprint-lanes.md)
keep executable checks independent across variants and withhold cached
source proof only for the provider with an active setup job.

### Queued DCS commands during an unavailable lookup

Every queued DCS command still requires a successful exact process check.
An unavailable lookup rejects the command and cancels the lane. The host
discards work queued behind it and reports the rejection once. An effect
submitted while that cancellation is pending cannot reopen the lane.
Later effects use a new lane and must pass the same strict validation.
An owner retired during a lookup still takes the terminal path.

The Windows native suite passed 2,390 tests with 18 default ignores. Both
pinned DCS-BIOS archive checks passed separately. Native regressions
injected lookup unavailability and covered lane cancellation, racing
submission, owner retirement and BIOS telemetry on the retained reader.
Production line coverage was 86.55% for the command worker, 71.19% for
session delivery, 85.29% for session reads and 91.50% for the context runtime.
The test executable retained coverage instrumentation. The linker retry
omitted the optional symbol database after the disk filled.

The authored DCS dev smoke passed competing-process withdrawal, independent
BIOS and ownship recovery, and process exit with no cleanup errors. It used
real local UDP and a fixture process. The isolated dev executable used only
the Rust library format. Its SHA-256 was
`1BC4039FB1B88EC72D7DF26DBAFDFAE0EB748CD3DEDB45FDD1C51138B9D5241F`.
Unavailable command proof was injected in the native suite. Simulator
command effects remain in #199.

The active companion feedback publisher now emits an initial snapshot and
per-channel changes/independent freshness renewals rather than full snapshots
every 34 ms. One latest pending projection and exact acknowledgements bound
slow delivery. [Qualification and limits](../verification/2026-11-05-feedback-publication.md)
record the remaining complete transport and physical acceptance matrices.
The quiet feedback criterion in #195 is checked. Two independently bound
channels stayed fresh through five quiet updates over 1,116 ms in the running
browser fixture. The browser regression verifies that a quiet channel expires
while its peer stays fresh and that replaying a delta cannot extend freshness.
Native projection tests separately verify that an expired sample is published
as stale and cannot renew itself.

The slow feedback criterion in #195 is checked. The browser fixture holds
one feedback acknowledgement across multiple renewals and an input lease.
The browser still sends input and the exact profile owner and armed peer
remain live. Windows Raw Input sees independent vJoy transitions before
feedback resumes. The browser receives a current snapshot after release.
Native worker and service tests cover blocked validation, coalescing for
a slow peer, and bounded latest state. Sustained mixed-consumer load remains
part of #198.

The lifecycle criterion in #195 is checked. The real browser exercised its
visibility handler with a controlled hidden state and released the held input
while the assigned session remained armed. The affected feedback returned to
neutral and the profile owner survived. Explicit navigation disconnected the
peer and reopening required a fresh neutral baseline. Replacing and restoring
the publication on the paired installation verified each exact hash and all
typed neutral controls. Browser and native regressions separately verify that
retired feedback acknowledgements and stale assignment receipts cannot modify
the replacement.

CAN publication ownership and gateway inventory use sequenced gateway
invalidations. Setup and lease deadlines are scheduled independently. Physical
layout, inventory and CAN route acceptance remain in #199.

ESP32 Wi-Fi and direct USB pairing now use terminal approval subscriptions
instead of 400 ms status requests. The software receiver/bridge tests and actual
Cockpit protocol checks are documented in [ESP32 pairing verification](../verification/2026-11-05-esp32-pairing-events.md).
Current ESP32 direct USB firmware streams status changes. The operator chose
to [require that capability](../verification/2026-11-08-esp32-usb-status-watch-required.md).
The host uses small liveness messages and requests a full snapshot for
discovery and identity-bound recovery. Physical transport qualification
remains in #199.
Pico Debug inventory is event-driven
with an independent watchdog renewal. Cockpit retires the older Pico protocol-1
adapter by operator decision. The game's 100 ms process and telemetry
maintenance remain original software work.

The mixed-stream recovery criterion in #193 is checked. The reducer tests
cover loss before timers, fresh baselines without replay and generation
fencing. Native input overflow recovery and stale script completion checks
supplement the authored running-Cockpit fixtures. See
[baseline seeding](../verification/2026-11-03-source-scoped-baselines.md),
[native capture admission](../verification/2026-11-03-native-port-admission.md)
and the native overflow regression in commit `246237e2`.

The stalled-consumer criterion in #193 is checked. The active host fixture
holds the gateway sessions lock and stalls the simulator command worker
while a guarded DCS branch is active. Virtual lease renewal, a mapped button
report, Stop, slot release and restoration complete while both consumers
are still blocked. Busy publication snapshots reject affected commands and
cancel their queued work. Confirmed loss revokes affected command authority
immediately and retries the exact reader's inventory update if the registry
is busy. Existing native queue saturation tests verify explicit failure and
cancellation. The rebuilt Cockpit passes MCP and Windows vJoy burst checks
with a stalled preview. Live simulator effects and physical latency remain
in #199.

On November 6, the instrumented Raw Input test delivered all 65 authored vJoy
button transitions and the Stop release to slot 1 in the running Cockpit
(SHA-256 `DBD39F4B3572446E3DE7CFFF7EF88C217D3ACAC3C6E64AA175C36708E6FCC0AA`).
The slow physical preview subscriber held its first delivery and a separate
preview advanced. Submission intervals were 11.55/15.27/15.53 ms
(p50/p95/p99), Windows receiver intervals were 11.77/15.17/15.85 ms, and
gateway round trips were 1.86/2.17/3.01 ms. The authored interval target was
8 ms. The fixture measures cadence on each clock separately and cannot
establish input-to-output latency. Rebuild the Raw Input probe for these
fields. The logs and timing samples are in
`.research/reactive/current-vjoy-timing/`.

The separate controller registry diagnostic
(`cargo run --manifest-path crates/controller-input/Cargo.toml --example report_cost -- --paced`)
preserved 300 ordered synthetic encoder-button transitions at each of 124.97
and 997.28 reports/s. The faster run carried 37 reports from a concurrent
125 Hz source and recovered a neutral baseline after overrun. The registry
diagnostic and the vJoy fixture cover different stages. The combined
physical and software load gates remain open.

The native dual-rate fixture now routes both sources to one virtual-controller
reducer while a preview reader holds its first delivery. One run captured 256
fast reports at 999.32 Hz and 32 peer reports at 125.14 Hz. The reducer
produced all 288 expected button states in order. After a journal overflow it
required fresh baselines. The fixture then disconnected the fast source,
checked that its button released while the peer stayed held, and confirmed
that the replacement generation could not reuse the retired route token.

A separate run against the current dev Cockpit delivered 65 authored vJoy
edges and the Stop release to Windows Raw Input while one preview and one MCP
reader were stalled. The independent preview advanced and Devices Settings
showed the slot free after Stop. Submission-to-receipt p50/p95/p99 were
1.38/1.99/2.43 ms. The output fixture uses debug inputs and a separate pacing
schedule. Physical unplug and end-to-end dual-rate output timing remain open.
The full native suite passed 2,374 tests with 18 existing ignores. Input
routing reached 87.78% line and 90.00% function coverage. The local vJoy
trace is `vjoy-raw-burst-result.json` under
`.research/reactive/held-launch-schedule/dual-rate-routing-1794022400331/`.

The running-Cockpit Stop and recovery criterion in #193 is checked. The
compiled browser companion fixture explicitly disconnects while its input
and feedback are held. The affected samples withdraw under the same profile
owner while an independent authored source keeps a Windows vJoy button held.
The reconnected browser must supply a new neutral baseline and then a new
press. Windows Raw Input records the independent button's authored transitions
and release from Stop, with no release during the browser outage. One local
run observed browser loss and withdrawal after 52 ms and Stop plus the
committed slot release after 345 ms. These are fixture observations, separate
from raw input or physical end-to-end latency.

The [prepared output renewal](../verification/2026-11-08-prepared-output-heartbeat.md)
uses the feeder's existing deadline and fault notifications during
activation. The Windows combined vJoy and neutral desktop fixtures passed.

## Integrated Windows checks on November 6

The isolated native candidate passed 2,375 library tests serially with Windows
access and 18 explicit ignores. The earlier sandboxed parallel run aborted
with `STATUS_INVALID_HANDLE`. The live MCP suite and both controller and
shared-stream coverage gates passed. See [#198's run record](https://github.com/opensimgear/cockpit/issues/198#issuecomment-6689971815).

The running dev executable with SHA-256
`6817D4EFA657AB08E5D3BF8FF79527F7DA46AC0FE3B7E1139AB8D26391BF0692`
delivered 65 authored vJoy button transitions and the Stop release to Windows
Raw Input slot 1. A physical preview held its initial receipt while a second
preview advanced. An MCP reader went unread for 1.77 seconds, and a separate
MCP client's Stop request returned in 64.7 ms. Submission intervals were 5.55/18.54/20.98 ms
(p50/p95/p99) against an 8 ms target. Receiver intervals were
5.42/18.58/21.15 ms. These are cadence observations on separate clocks.

In a separate run, terminating the exact fixture-owned feeder faulted the
profile and freed slot 1 after three authored transitions and a release.
The delayed physical preview fixture independently resumed with an absolute
snapshot at revision 8231 after holding revision 8221. The independent
preview received ten further reports during the hold. Local artifacts are
`vjoy-raw-burst-result.json`, `vjoy-feeder-loss-result.json`,
`controller-latest-state-native.json` and `native-suite-after-clean.log`
under `.research/reactive/`. Integrated load, physical latency and actual
simulator confirmation remain open.

### Authored vJoy submission-to-receipt timing

The November 6 fixture now records the Node submission timestamp and the
Windows Raw Input receiver timestamp on the same performance counter. It
checks the receiver's absolute start against the launch bracket before
joining events by exact slot and ordered button transition. Submission
cadence, gateway round trips and receiver cadence remain separate. [libuv Windows clock](https://github.com/libuv/libuv/blob/v1.52.1/src/win/util.c) · [Windows performance counters](https://learn.microsoft.com/en-us/windows/win32/sysinfo/acquiring-high-resolution-time-stamps)

The running dev Cockpit (SHA-256
`63F25B8DDEEDEB71F22B47A074D35ABE4F1E92E602038C5307B36AF5B7D39779`)
delivered 195 authored edges across three independently applied 65-edge
bursts. Each run held a preview receipt and stalled an MCP reader. Pooled
submission-to-receipt p50/p95/p99 were 1.25/1.55/7.99 ms. The first edge of
each burst took 8.27/7.93/7.99 ms. The separately verified Stop releases
are excluded from these percentiles.

The original probe signaled readiness before its foreground-process lookup
and message-pump setup. Moving the signal after that work, with the same
running Cockpit and stalled consumers, reduced the first edges to
1.80/1.99/2.14 ms. The three corrected 65-edge runs had pooled p50/p95/p99
of 1.26/1.53/2.14 ms and a maximum of 2.27 ms. This isolates the prior
first-edge outlier to probe startup. The authored interval target was
8 ms, with measured submission p95 intervals between 15.16 and 19.72 ms
across the corrected runs.

The measurement starts just before the test submits an authored debug
input over the WebView bridge and ends at the Raw Input callback. Physical
HID acquisition and display presentation are separate measurements. Raw
samples are in `.research/reactive/vjoy-qpc-timing/burst-{1,2,3}.json`
and `.research/reactive/vjoy-ready-timing/burst-{1,2,3}.json`.

### Two authored sources to the same Windows vJoy slot

Three isolated running-Cockpit runs preserved 193 fast and 25 peer button
transitions per run. One preview held its first delivery, the other advanced
by 14 reports, and an MCP reader went unread for 1.63–1.68 seconds. The
independent MCP reader observed an active value. Stop released both buttons
after 41.93–50.20 ms at Windows Raw Input, and the slot was free on Settings
reopen. The largest observed debug input queue depth was one frame.

| Source | Measured submission rate | Largest per-run submission-to-receipt upper bound p50 / p95 / p99 |
| --- | --- | --- |
| Fast, 1 ms target | 663.21–698.94 Hz | 1.46 / 1.82 / 2.58 ms |
| Peer, 8 ms target | 125.07–126.05 Hz | 1.54 / 2.33 / 5.13 ms |

The test brackets the WebView clock before and after each burst. The
combined bracket widths were 0.85–0.91 ms. The development executable from
`3592e502` has SHA-256
`16781f2732f42e6d689ec9c740489c8d08a8bbb5cd13f5c78b5a3db575802ac5`.
Use `OSG_VJOY_DUAL_RATE=1` with the existing vJoy fixture and rebuild
`RawInputProbe.cs` for the two-button receiver. The original single-source
burst also passed with the rebuilt probe. The local records are in
`.research/reactive/dual-rate-live/`.

Overlapping WebView calls reached approximately 1 kHz twice. A third attempt
arrived out of order and Cockpit rejected a stale sequence. The committed
fixture awaits acknowledgement on each source independently. The native
journal and reducer regression separately passed at 1,001.17 Hz and
125.12 Hz. It preserved all 288 expected reports before overflow, then
verified fresh baselines and fenced the replaced source.
Stable desktop WebView 1 kHz and the 30-minute rig soak remain open.
The [combined desktop fault run](../verification/2026-11-07-dcs-panel-vjoy.md)
passed with authored DCS telemetry, a compiled browser panel and real vJoy.
The follow-up retired the actual Cockpit telemetry inspector and both test
readers. The same profile then sent fresh panel feedback while Windows
received both vJoy transitions. This satisfies the closed-preview criterion
in #193. Actual simulator effects remain in #199.

### Ordered 125 Hz and 1 kHz on browser-dev

Three browser-dev runs sent two independent ordered debug sources through
the production loopback gateway and the native service into one Windows
vJoy slot. The fast source sustained 992.24–1,000.97 Hz and the peer
125.01–125.24 Hz. They overlapped for at least 191.63 ms. Windows Raw Input
received all 654 authored transitions and six Stop releases. One browser
preview acknowledgement stayed pending while the independent preview advanced
by 12–13 reports. Settings showed the slot busy, then free on reopen.

The largest per-run p50/p95/p99 for loopback HTTP submission to Windows Raw
Input were 0.93/1.19/2.02 ms for the fast source and 1.08/1.30/4.03 ms for
the peer. Both clocks use the Windows performance counter and the fixture
checks the receiver launch bracket. The largest reported debug input queue
depth was one frame. Both Stop releases arrived within 1.29 ms of command
submission. The desktop parity run also passed at 697.93 Hz and 124.80 Hz
with the held MCP reader.

Use `OSG_VJOY_BROWSER_URL` with the loopback dev URL and the same Raw Input
probe. The browser test requires each measured rate within 5% of target and
at least 180 ms of overlap. The fixture used a dedicated HTTP keep-alive
agent. Earlier fetch-based attempts retained every edge at lower rates.
The native gateway executable from `3592e502` has SHA-256
`33256e5ce2f3fefaf3b7997dca9bddf22d25ed9bc029a6c1b5230c1168a41d47`.
The raw records are `browser-final-{1,2,3}.json` in
`.research/reactive/dual-rate-live/`. This run covers authored browser-dev
input and an independent stalled browser observer. Physical acquisition,
simulator confirmation and combined load remain separate gates.

### November 7 single-source comparisons

The November 7 comparison used the same dev executable for both 65-edge
bursts (SHA-256 `EF370AE5C5FC7AF25BA877B822195DBBF0882DC1A9BFEF21C987202EFBED1623`).
Both runs delivered every authored edge and the separate Stop release to
Windows Raw Input slot 1.

| Readers | Submission-to-receipt p50 / p95 / p99 |
| --- | --- |
| Regular | 1.27 / 1.59 / 8.34 ms |
| One held preview receipt and an unread MCP reader | 1.21 / 1.84 / 8.73 ms |

The independent preview advanced from report 42 to 59 with no errors during
the hold. The MCP reader went unread for 1.81 s and its Stop request took
63.7 ms. These two runs show continued delivery under the tested load.
The fixture also passed directly after startup once it waited for the
compatible free slot to appear. The local traces are under
`.research/reactive/held-launch-schedule/controller-latency-{current,quiet,startup}/`.

A follow-up enabled the stalled preview, unread MCP subscription and Devices
Settings inventory checks in the same run. Windows Raw Input received all 65
authored transitions and the Stop release. The independent preview advanced,
the MCP reader held unread for 1.91 seconds, and Devices Settings showed the
slot busy before Stop and free on reopen. Submission-to-receipt p50/p95/p99
were 1.20/1.43/2.84 ms and the MCP Stop reply took 86.4 ms. The 8 ms authored
schedule is a target. These measurements are from a development executable
built with working-tree edits, and physical acquisition remains in #199.

The serial Windows library suite on that working tree finished with 2,379
passes, one failure and 18 ignores. The failing topology fixture includes a
local `hold` case that the compiled rule enum rejects. The browser companion
suite (75 tests), MCP frontend suite (27 tests), companion coverage suite with
loopback access, stream and controller coverage, and `pnpm check` passed.
Final clean-candidate and platform suite qualification remains open.

An archive of committed `33ebae88` passed the locked Windows native library
suite serially with 2,377 passes, zero failures and 18 ignores. The topology
test passed on that revision. ESP32 host suites on the same archive passed for
serial, workspace, network, core and storage. The Sunton and Waveshare adapter
fault-injection suites passed with 100% line coverage each. These suites use
host mocks. Hardware timing, the final integrated soak and other platform
suites still require evidence.

Both physical board profiles cross-built locally from the clean `e7f592db`
archive using ESP-IDF v5.5.2. The generated configurations enable the UART
console. Each application fits its 2 MiB partition with about 21% free.

| Board profile | MCU | Application bytes | Application SHA-256 |
| --- | --- | ---: | --- |
| `sunton-3248s035r` | ESP32 | 1,651,120 | `6ba929647f496d678986950d0295f169aec333c6ac6f7877d4d37872d2a90cce` |
| `waveshare-lcd7-s3` | ESP32-S3 | 1,665,776 | `d5b84bced1db29e240d0abbd639ef06b4ed26713626329b7448f3d4b9e2c8ea0` |

The local `.research/reactive/esp32-board-builds/` directory holds the build
logs, generated settings, flash manifests and all three images named by each
manifest. No device was flashed. Boot, transport, touch and timing checks
remain in #199.

The USB status change in `dfb6bfbb` also cross-builds on both boards with
ESP-IDF v5.5.2 and the UART console enabled. The application images each leave
21% of the smallest app partition free.

| Board profile | Application bytes | Application SHA-256 |
| --- | ---: | --- |
| `sunton-3248s035r` | 1,652,224 | `3e93879c01e2b09b364d3cdc549f606000b9f8a944bfacf30beb19ad9432a59e` |
| `waveshare-lcd7-s3` | 1,667,040 | `4bf35961e3a1d09f08984956c63ba9f8aed6de86580f6dfa5bf4d6bb50a8cf8b` |

The local `.research/reactive/esp32-usb-status-builds/` folder holds the
build logs, generated configurations, flash manifests and installer bundles.
Both bundles pass Cockpit's local manifest and native installer validators.
They report firmware version `2026.10.22.7`. Use the source commit and image
hash to distinguish them from the earlier builds carrying the same version.

The same three images per board were bundled under
`.research/reactive/esp32-board-builds/installer-bundles/` with their build
commit, board identity, offsets and segment hashes. Both version `2026.10.22.7`
bundles passed Cockpit's local manifest validator and native installer image
validator. The bundles are local qualification artifacts.

A separate development Cockpit built from clean `554a6611` embedded both
bundles. Its running native installer gateway reported both board profiles
available at `2026.10.22.7`, and the read-only check observed no flash state
change. This exercised development bundle discovery and version reporting.

## November 6 VKB Devices preview checkpoint

The running Windows dev Cockpit displayed the attached VKBsim Gladiator EVO R
with 137 controls. Two ten-second passive runs observed 125 consecutive
reports each. The controller was left stationary. Native report intervals
were about 80 ms median in both runs, with no skipped report sequences.

| Trace | Journal to DOM p50 / p95 / p99 | Journal to next frame opportunity p95 | Clock bracket |
| --- | --- | --- | --- |
| Normal | 10.78 / 12.77 / 14.22 ms | 18.98 ms | ±2.25 ms |
| IPC tracing enabled | 10.19 / 13.11 / 13.71 ms | 18.25 ms | ±2.50 ms |

The native build used the production recovery changes in `7f3beaf1`.
Its SHA-256 was `436A8B8971E031BF6911211D37D83EE0495BF5BF754A3FC5A40DCBA555E608E3`.
The IPC raw log is `.research/reactive/controller-preview-latency-ipc.json`.
The November 8 comparison replaced the earlier normal run's fixed-path
raw file. These figures
start at the journal commit and end at the Devices inspector. Profile-inspector
axis timing during a deliberate sweep, physical display latency and physical
input-to-virtual receipt remain in #199.

The [November 8 delivery comparison](../verification/2026-11-08-preview-delivery-handoff.md)
isolated a reduction in JavaScript processing cost for native reports. The
reversed live run did not resolve a journal-to-DOM difference outside its
clock bracket. Its separate trace directories preserve each comparison.

## User-operated qualification stays in #199

The [shared controller capture check](../verification/2026-11-07-shared-controller-routing.md)
is accepted in #191. The running Cockpit verified controller preview and
applied routing, and the authored native USB fixture verified exact capture
sharing and delivery after the preview closed. The running app had no USB
module endpoint. The real board, controller sweep, unplug and latency
checks remain operator acceptance in #199.

The 13 original criteria requiring physical devices, actual simulators, rig
baseline/budgets or the combined 30-minute soak remain externally unqualified.
#199 groups these scenarios and must stay first in the Progress lane. Windows
consent and helper-lifetime evidence are also recorded there. Moving an external
step does not move its missing software prerequisite out of the original issue.

Controller preview timing and raw-input-to-virtual timing are separate checks.
The preview backlog fix is delivered, but it does not establish physical
end-to-end latency. The native worker now releases taps independently of parent journal persistence.
Its latest measured native-call span is 1.5959 ms for an authored 1 ms dwell.
First-dispatch, queued burst and focus/timer precision remain software timing
work under #193; these observations do not qualify physical input latency.

## Passive telemetry authority evidence

`game_telemetry_api.rs` only validates bounded filters, reads prepared captures
and acquires passive inspection demand. It does not apply a profile or dispatch
simulator control commands. X-Plane telemetry subscription requests are read
acquisition, distinct from control effects.

`telemetry_inspection_is_bounded_passive_and_available_without_a_profile` tests
read-only availability and rejects activation fields.
`game_telemetry_inspection_bypasses_admission_and_never_changes_native_control_epoch`
holds both control admission locks, exercises inspection and a scoped observer,
checks the unchanged control epoch and empty adapter set, and rejects a retired
observer. Earlier authored DCS running-Cockpit evidence is recorded in
`docs/verification/2026-11-03-native-port-admission.md`; it is not actual simulator
effect qualification. Two new inspection probes on 2026-11-05 found an empty game
catalogue and did not qualify a live simulator stream. Acceptance of this
authority invariant rests on the focused native tests, code review and earlier
authored running-Cockpit evidence; actual simulator delivery remains unchecked.

CI remains deferred until feature implementation is complete, as requested.
