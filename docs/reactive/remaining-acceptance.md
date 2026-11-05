# Remaining #188 acceptance

This ledger separates original software requirements from user-operated
qualification. It does not introduce another feature or waive an original gate.
The former 70% effort estimate is withdrawn: acceptance checkboxes are not
equally sized implementation tasks.

The original #189–198 acceptance list has 55 criteria. This reconciliation accepts
the passive telemetry authority criterion in #192, bringing the checked count
from 12 to 13. There are 42 open criteria: 29 software/qualification/governance
criteria and 13 requiring user-operated rig/simulator acceptance. An unchecked criterion can contain
delivered software plus outstanding qualification; it is not necessarily missing
implementation.

#190 (R1, shared stream contracts) is completed and closed. The remaining
milestones retain their open acceptance boundaries; project statuses for #197
and #198 are In progress because implementation/qualification is underway.

## Software and software qualification remain in the original issues

| Issue | Remaining boundary |
| --- | --- |
| #189 | Reconcile the complete acquisition/migration inventory and each disposition. The user approved one native shared-memory reader per active Assetto game; no general sampling exception follows. |
| #192 | Current integration lifecycle/quality, contention and adapter qualification, including truthful reporting of unavailable Windows notification consent and simulators. |
| #193 | Worker-owned tap release, native high-resolution waits, parent/helper scope notifications and immutable queued admission proofs are delivered. Qualified native HID/internal desktop profiles now use retained wakes and deadlines; the recent-application chooser also uses native events. The controlled two-window focus check passes after Windows is unlocked. Remaining adapter combinations, broader focus/child-loss qualification, first-dispatch/burst timing, mixed-stream fault and affected-owner cleanup matrices, and independent dispatch under stalled consumers remain. Sustained latency qualification is interrupted by WSL foreground takeover. |
| #194 | Complete producer/read-model and cancellation/lock matrices. Activity's aggregate 750 ms metadata acquisition is replaced by four committed-source wake groups with passive caches; see the Activity source matrix and qualification record. Wider application coverage and the denied-process-event fallback remain explicit. |
| #195 | Companion publication/reconnect/backgrounding, per-widget expiry and slow-consumer qualification. Listener accepts/server peers and native client reads use event I/O; pending approval uses a host event. Active profile companion scheduling uses producer wakes and explicit deadlines; unsupported adapters and physical companion firmware acquisition still need their original software disposition. The historical legacy mapping-route worker has no Cockpit caller and its commands are rejected. |
| #196 | Nested navigation/query/editor lifetime and background/reopen matrices; consistent component review. |
| #197 | Complete manual/MCP state/error/cancel/recovery parity, sustained socket backpressure and schema/documentation/negotiation inventory. |
| #198 | Integrated software fault/load matrix, final applicable local suites and CI, final acquisition inventory and complete acceptance accounting. |

The active companion feedback publisher now emits an initial snapshot and
per-channel changes/independent freshness renewals rather than full snapshots
every 34 ms. One latest pending projection and exact acknowledgements bound
slow delivery. [Qualification and limits](../verification/2026-11-05-feedback-publication.md)
record the remaining complete transport and physical acceptance matrices.

CAN publication ownership uses sequenced gateway invalidations and exact lease
deadlines. General gateway inventory still uses its snapshot cadence. Physical
layout and CAN route acceptance remain in #199.

ESP32 Wi-Fi and direct USB pairing now use terminal approval subscriptions
instead of 400 ms status requests. The software receiver/bridge tests and actual
Cockpit protocol checks are documented in [ESP32 pairing verification](../verification/2026-11-05-esp32-pairing-events.md).
Board compilation and physical transports remain unqualified; the recorded
ESP-IDF toolchain is unavailable locally. General gateway inventory and the
game's 100 ms process and telemetry maintenance remain original software work.

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
