# Remaining #188 acceptance

This ledger separates original software requirements from user-operated
qualification. It does not introduce another feature or waive an original gate.
The former 70% effort estimate is withdrawn: acceptance checkboxes are not
equally sized implementation tasks.

The original #189–198 acceptance list has 55 criteria, with 27 checked and 28
open. The open criteria include 15 software, qualification or governance
criteria and 13 requiring user-operated rig or simulator acceptance. Some open
criteria have delivered software and outstanding qualification.

#190 (R1, shared stream contracts) is completed and closed. The remaining
milestones retain their open acceptance boundaries; project statuses for #197
and #198 are In progress because implementation/qualification is underway.

## Software and software qualification remain in the original issues

| Issue | Remaining boundary |
| --- | --- |
| #189 | Reconcile the complete acquisition/migration inventory and each disposition. The user approved one native shared-memory reader per active Assetto game; no general sampling exception follows. |
| #192 | Committed game ownership survives failed lookups and busy registry reads. Unstable Assetto page copies skip acquisition and preserve the original capture's age. The authored DCS fixture verifies independent BIOS and ownship withdrawal and recovery in the running Cockpit. The broader integration contention and adapter criteria remain open. Windows denied the Assetto process notification prerequisite for live fixture setup. |
| #193 | Worker-owned tap release, native high-resolution waits, parent/helper scope notifications and immutable queued admission proofs are delivered. Qualified native HID/internal desktop profiles now use retained wakes and deadlines, and the recent-application chooser also uses native events. The controlled two-window focus check passes after Windows is unlocked. Remaining work includes live simulator effects, broader focus and child-loss qualification, and first-dispatch and burst timing. Sustained latency qualification is interrupted by WSL foreground takeover. |
| #194 | The [read-model source matrix](read-model-sources.md) maps the native observation catalogue to committed producers and portal consumers. Complete per-provider background, cancellation and lock qualification. Activity's aggregate 750 ms metadata acquisition is replaced by four committed-source wake groups with passive caches. The denied-process-event fallback remains open. |
| #195 | Actual simulator feedback and separate native transport and board qualification remain. Listener accepts/server peers and native client reads use event I/O. Pending approval uses a host event. Active profile companion scheduling uses producer wakes and explicit deadlines. Unsupported adapters and physical companion firmware acquisition still need their original software disposition. The historical legacy mapping-route worker has no Cockpit caller and its commands are rejected. |
| #196 | Main navigation, two Settings tabs, repeated profile game-query handoff, preference reopen and Games detail reopen after a background commit are verified on native and browser-dev. Native dirty editor, controller metadata and companion listener checks passed. Complete the other nested lifetimes, background/reopen matrix and consistent component review. |
| #197 | Complete manual/MCP state/error/cancel/recovery parity and schema/documentation/negotiation inventory. The desktop output status snapshot is available through MCP. Slow-observer isolation passed in the running app and in bounded native stress fixtures. Sustained socket backpressure remains part of broader load qualification. |
| #198 | Integrated software fault/load matrix, final applicable local suites and CI, final acquisition inventory and complete acceptance accounting. |

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
