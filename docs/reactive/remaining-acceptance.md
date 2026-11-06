# Remaining #188 acceptance

This ledger separates original software requirements from user-operated
qualification. It does not introduce another feature or waive an original gate.
The former 70% effort estimate is withdrawn: acceptance checkboxes are not
equally sized implementation tasks.

The original #189–198 acceptance list has 55 criteria, with 30 checked and 25
open. The open criteria include 12 software, qualification or governance
criteria and 13 requiring user-operated rig or simulator acceptance. Some open
criteria have delivered software and outstanding qualification.

#190 (R1, shared stream contracts) and #197 (R8, MCP workflow parity) are closed. Integrated qualification remains open in #198.

## Software and software qualification remain in the original issues

| Issue | Remaining boundary |
| --- | --- |
| #189 | Reconcile the complete acquisition/migration inventory and each disposition. The user approved one native shared-memory reader per active Assetto game; no general sampling exception follows. |
| #192 | Committed game ownership survives failed lookups and busy registry reads. Unstable Assetto page copies skip acquisition and preserve the original capture's age. The authored DCS fixture verifies independent BIOS and ownship withdrawal and recovery in the running Cockpit. Cross-adapter contention and quality qualification remain open. Windows denied the Assetto and X-Plane process notification prerequisite for the latest telemetry fixtures. |
| #193 | Worker-owned tap release, native high-resolution waits, parent/helper scope notifications and immutable queued admission proofs are delivered. Qualified native HID/internal desktop profiles now use retained wakes and deadlines, and the recent-application chooser also uses native events. The controlled two-window focus check passes after Windows is unlocked. Remaining work includes live simulator effects, broader focus and child-loss qualification, and first-dispatch and burst timing. Sustained latency qualification is interrupted by WSL foreground takeover. |
| #194 | The [read-model source matrix](read-model-sources.md) maps the native observation catalogue to committed producers and portal consumers. Complete per-provider background, cancellation and lock qualification. Activity's aggregate 750 ms metadata acquisition is replaced by four committed-source wake groups with passive caches. The denied-process-event fallback remains open. |
| #195 | Actual simulator feedback and separate native transport and board qualification remain. Listener accepts/server peers and native client reads use event I/O. Pending approval uses a host event. Active profile companion scheduling uses producer wakes and explicit deadlines. Unsupported adapters and physical companion firmware acquisition still need their original software disposition. The historical legacy mapping-route worker has no Cockpit caller and its commands are rejected. |
| #196 | Main navigation, two Settings tabs, repeated profile game-query handoff, preference reopen and Games detail reopen after a background commit are verified on native and browser-dev. Native dirty editor, controller metadata and companion listener checks passed. Complete the other nested lifetimes, background/reopen matrix and consistent component review. |
| #198 | Integrated software fault/load matrix, final applicable local suites and CI, final acquisition inventory and complete acceptance accounting. |

## R3 adapter evidence

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
Board compilation and physical transports remain unqualified; the recorded
ESP-IDF toolchain is unavailable locally. Pico Debug inventory is event-driven
with an independent watchdog renewal. The older Pico protocol-1 adapter and the
game's 100 ms process and telemetry maintenance remain original software work.

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

## User-operated qualification stays in #199

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
